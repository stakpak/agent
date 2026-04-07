import AgentLLM
@preconcurrency import Foundation
import AgentTools
import AgentAudit

@MainActor
final class OllamaService {
    let apiKey: String
    let model: String
    let baseURL: URL
    let supportsVision: Bool
    let provider: APIProvider
    var temperature: Double = 0.2
    var compactTools: Bool = false
    /// Context window size for local Ollama. 0 = let model decide.
    let contextSize: Int

    var onStreamText: (@MainActor @Sendable (String) -> Void)?

    let historyContext: String
    let userHome: String
    let userName: String
    let projectFolder: String

    init(
        apiKey: String, model: String, endpoint: String,
        supportsVision: Bool = false, historyContext: String = "",
        projectFolder: String = "", provider: APIProvider = .ollama,
        contextSize: Int = 0
    ) {
        self.apiKey = apiKey
        self.model = model
        let effectiveEndpoint = endpoint.isEmpty ? "http://localhost:11434/api/chat" : endpoint
        self.baseURL = URL(string: effectiveEndpoint) ?? URL(filePath: "/")
        self.supportsVision = supportsVision
        self.provider = provider
        self.contextSize = contextSize
        self.historyContext = historyContext
        self.userHome = FileManager.default.homeDirectoryForCurrentUser.path
        self.userName = NSUserName()
        self.projectFolder = projectFolder
    }

    var overrideSystemPrompt: String?

    var systemPrompt: String {
        if let override = overrideSystemPrompt { return override }
        var prompt = SystemPromptService.shared.prompt(for: provider, userName: userName, userHome: userHome, projectFolder: projectFolder)
        if !projectFolder.isEmpty {
            prompt =
                "CURRENT PROJECT FOLDER: \(projectFolder)\n"
                    + "Always cd to this directory before running any "
                    + "shell commands. Use it as the default for all file "
                    + "operations. You may go outside it when needed.\n\n" +
                prompt
        }
        if supportsVision {
            prompt += "\nYou have VISION. When images are attached, you can see and analyze them."
        }
        if !historyContext.isEmpty {
            prompt += historyContext
        }
        prompt += MemoryStore.shared.contextBlock
        return prompt
    }

    func tools(activeGroups: Set<String>? = nil, compact: Bool = false) -> [[String: Any]] {
        AgentTools.ollamaTools(
            for: provider, activeGroups: activeGroups,
            compact: compact, projectFolder: projectFolder
        )
    }

    /// Set to true when a tool call fails — next turn sends full _tool names, then resets.
    var needsFullToolNames: Bool = false

    /// Prepend project folder to the last user message so it's always visible in context.
    private func withFolderPrefix(_ messages: [[String: Any]]) -> [[String: Any]] {
        guard !projectFolder.isEmpty else { return messages }
        let prefix = "PROJECT FOLDER: \(projectFolder)\n"
        var result = messages
        for i in stride(from: result.count - 1, through: 0, by: -1) {
            guard result[i]["role"] as? String == "user" else { continue }
            if let text = result[i]["content"] as? String {
                result[i]["content"] = prefix + text
            } else if var blocks = result[i]["content"] as? [[String: Any]],
                      let first = blocks.first, first["type"] as? String == "text",
                      let existing = first["text"] as? String
            {
                blocks[0]["text"] = prefix + existing
                result[i]["content"] = blocks
            }
            break
        }
        return result
    }

    /// Send messages via OpenAI-compatible chat completions API.
    /// Translates response into the same format as ClaudeService for the task loop.
    func send(
        messages: [[String: Any]],
        activeGroups: Set<String>? = nil
    ) async throws
        -> (content: [[String: Any]], stopReason: String, inputTokens: Int, outputTokens: Int)
    {
        // Convert Claude-format messages to OpenAI-format
        var chatMessages: [[String: Any]] = [
            ["role": "system", "content": systemPrompt]
        ]

        for msg in withFolderPrefix(messages) {
            guard let role = msg["role"] as? String else { continue }

            if role == "user" {
                if let text = msg["content"] as? String {
                    chatMessages.append(["role": "user", "content": text])
                } else if let blocks = msg["content"] as? [[String: Any]] {
                    // Could be tool_result blocks or content blocks with images
                    let isToolResults = blocks.first?["type"] as? String == "tool_result"
                    if isToolResults {
                        for block in blocks {
                            guard let toolUseId = block["tool_use_id"] as? String,
                                  let content = block["content"] as? String else { continue }
                            chatMessages.append([
                                "role": "tool",
                                "tool_call_id": toolUseId,
                                "content": content
                            ])
                        }
                    } else {
                        // Content blocks (text + images)
                        var text = ""
                        var images: [String] = []
                        for block in blocks {
                            if block["type"] as? String == "text",
                               let t = block["text"] as? String
                            {
                                text += t
                            } else if block["type"] as? String == "image",
                                      let source = block["source"] as? [String: Any],
                                      let base64 = source["data"] as? String
                            {
                                images.append(base64)
                            }
                        }
                        if !text.isEmpty || !images.isEmpty {
                            var msg: [String: Any] = ["role": "user", "content": text.isEmpty ? "Describe the attached image(s)." : text]
                            if !images.isEmpty {
                                msg["images"] = images
                                AuditLog.log(.api, "[OllamaService] Sending \(images.count) image(s), sizes: \(images.map(\.count))")
                            }
                            chatMessages.append(msg)
                        }
                    }
                }
            } else if role == "assistant" {
                if let text = msg["content"] as? String {
                    chatMessages.append(["role": "assistant", "content": text])
                } else if let blocks = msg["content"] as? [[String: Any]] {
                    var textParts = ""
                    var toolCalls: [[String: Any]] = []

                    for block in blocks {
                        let blockType = block["type"] as? String
                        if blockType == "text", let t = block["text"] as? String {
                            textParts += t
                        } else if blockType == "tool_use" {
                            let callId = block["id"] as? String ?? UUID().uuidString
                            let name = block["name"] as? String ?? ""
                            let input = block["input"] as? [String: Any] ?? [:]
                            // Ollama native API expects arguments as a dict, not a JSON string
                            toolCalls.append([
                                "id": callId,
                                "type": "function",
                                "function": [
                                    "name": name,
                                    "arguments": input
                                ] as [String: Any]
                            ])
                        }
                    }

                    var assistantMsg: [String: Any] = ["role": "assistant"]
                    if !textParts.isEmpty { assistantMsg["content"] = textParts }
                    if !toolCalls.isEmpty { assistantMsg["tool_calls"] = toolCalls }
                    chatMessages.append(assistantMsg)
                }
            }
        }

        // Ollama requires the last message to be user or tool — strip trailing assistant messages
        while chatMessages.last?["role"] as? String == "assistant" {
            chatMessages.removeLast()
        }

        var body: [String: Any] = [
            "model": model,
            "messages": chatMessages,
            "tools": tools(activeGroups: activeGroups, compact: compactTools),
            "stream": false,
            // Keep the model resident in VRAM for 30 min after each call so the KV cache
            // survives between Agent's loop iterations. Default is 5 min, which drops
            // the cache during any user pause and forces a full prefill on resume.
            "keep_alive": "30m"
        ]

        // Only send options for local Ollama; cloud providers manage their own context limits
        if provider == .localOllama {
            var opts: [String: Any] = ["temperature": temperature]
            if contextSize > 0 {
                opts["num_ctx"] = contextSize
                opts["num_predict"] = max(2048, contextSize / 4)
            }
            body["options"] = opts
        }

        // .sortedKeys for byte-stable JSON. Even on local Ollama (where the
        // KV cache hits via slot reuse rather than prefix matching), keeping
        // the bytes deterministic costs nothing and makes request bodies
        // diffable for debugging cache issues.
        let bodyData = try JSONSerialization.data(withJSONObject: body, options: [.sortedKeys])
        return try await Self.performRequest(
            bodyData: bodyData,
            apiKey: apiKey,
            url: baseURL
        )
    }


    // MARK: - Streaming

    func sendStreaming(
        messages: [[String: Any]],
        activeGroups: Set<String>? = nil,
        onTextDelta: @escaping @Sendable (String) -> Void
    ) async throws -> (content: [[String: Any]], stopReason: String, inputTokens: Int, outputTokens: Int) {
        // Convert Claude-format messages to OpenAI-format (same as send())
        var chatMessages: [[String: Any]] = [
            ["role": "system", "content": systemPrompt]
        ]

        for msg in withFolderPrefix(messages) {
            guard let role = msg["role"] as? String else { continue }

            if role == "user" {
                if let text = msg["content"] as? String {
                    chatMessages.append(["role": "user", "content": text])
                } else if let blocks = msg["content"] as? [[String: Any]] {
                    let isToolResults = blocks.first?["type"] as? String == "tool_result"
                    if isToolResults {
                        for block in blocks {
                            guard let toolUseId = block["tool_use_id"] as? String,
                                  let content = block["content"] as? String else { continue }
                            chatMessages.append([
                                "role": "tool",
                                "tool_call_id": toolUseId,
                                "content": content
                            ])
                        }
                    } else {
                        var text = ""
                        var images: [String] = []
                        for block in blocks {
                            if block["type"] as? String == "text",
                               let t = block["text"] as? String
                            {
                                text += t
                            } else if block["type"] as? String == "image",
                                      let source = block["source"] as? [String: Any],
                                      let base64 = source["data"] as? String
                            {
                                images.append(base64)
                            }
                        }
                        if !text.isEmpty || !images.isEmpty {
                            var msg: [String: Any] = ["role": "user", "content": text.isEmpty ? "Describe the attached image(s)." : text]
                            if !images.isEmpty {
                                msg["images"] = images
                            }
                            chatMessages.append(msg)
                        }
                    }
                }
            } else if role == "assistant" {
                if let text = msg["content"] as? String {
                    chatMessages.append(["role": "assistant", "content": text])
                } else if let blocks = msg["content"] as? [[String: Any]] {
                    var textParts = ""
                    var toolCalls: [[String: Any]] = []

                    for block in blocks {
                        let blockType = block["type"] as? String
                        if blockType == "text", let t = block["text"] as? String {
                            textParts += t
                        } else if blockType == "tool_use" {
                            let callId = block["id"] as? String ?? UUID().uuidString
                            let name = block["name"] as? String ?? ""
                            let input = block["input"] as? [String: Any] ?? [:]
                            toolCalls.append([
                                "id": callId,
                                "type": "function",
                                "function": [
                                    "name": name,
                                    "arguments": input
                                ] as [String: Any]
                            ])
                        }
                    }

                    var assistantMsg: [String: Any] = ["role": "assistant"]
                    if !textParts.isEmpty { assistantMsg["content"] = textParts }
                    if !toolCalls.isEmpty { assistantMsg["tool_calls"] = toolCalls }
                    chatMessages.append(assistantMsg)
                }
            }
        }

        // Ollama requires the last message to be user or tool — strip trailing assistant messages
        while chatMessages.last?["role"] as? String == "assistant" {
            chatMessages.removeLast()
        }

        var body: [String: Any] = [
            "model": model,
            "messages": chatMessages,
            "tools": tools(activeGroups: activeGroups, compact: compactTools),
            "stream": true,
            // Keep model + KV cache resident for 30 min so the loop's stable prefix
            // (system prompt + tools + earlier history) gets reused across iterations.
            "keep_alive": "30m"
        ]

        // Only send options for local Ollama; cloud providers manage their own context limits
        if provider == .localOllama {
            var opts: [String: Any] = ["temperature": temperature]
            if contextSize > 0 {
                opts["num_ctx"] = contextSize
                opts["num_predict"] = max(2048, contextSize / 4)
            }
            body["options"] = opts
        }

        // .sortedKeys for byte-stable prefix caching — see send() for rationale.
        let bodyData = try JSONSerialization.data(withJSONObject: body, options: [.sortedKeys])
        return try await Self.performStreamingRequest(
            bodyData: bodyData,
            apiKey: apiKey,
            url: baseURL,
            onTextDelta: onTextDelta
        )
    }

    /// Network I/O off main thread. Parses Ollama native response into Claude-compatible format.
    nonisolated private static func performRequest(
        bodyData: Data, apiKey: String, url: URL
    ) async throws -> (content: [[String: Any]], stopReason: String, inputTokens: Int, outputTokens: Int) {
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "content-type")
        if !apiKey.isEmpty {
            request.setValue("Bearer \(apiKey)", forHTTPHeaderField: "Authorization")
        }
        request.httpBody = bodyData
        request.timeoutInterval = llmAPITimeout

        let (data, response) = try await URLSession.shared.data(for: request)

        guard let httpResponse = response as? HTTPURLResponse else {
            throw AgentError.invalidResponse
        }

        guard httpResponse.statusCode == 200 else {
            let errorBody = String(data: data, encoding: .utf8) ?? "Unknown error"
            throw AgentError.apiError(statusCode: httpResponse.statusCode, message: errorBody)
        }

        guard let json = try JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            throw AgentError.invalidResponse
        }

        // Ollama native format: { "message": {...}, "done": true }
        guard let message = json["message"] as? [String: Any] else {
            throw AgentError.invalidResponse
        }

        let done = json["done"] as? Bool ?? true
        let promptEval = json["prompt_eval_count"] as? Int ?? 0
        let evalCount = json["eval_count"] as? Int ?? 0

        // Convert to Claude-compatible content blocks
        var contentBlocks: [[String: Any]] = []
        var parsedToolFromText = false

        if let text = message["content"] as? String, !text.isEmpty {
            // Check for DeepSeek V3.1 tool calls with special token markers
            if let deepSeekCalls = Self.extractDeepSeekToolCalls(from: text) {
                let normalized = text.replacingOccurrences(of: "｜", with: "|").replacingOccurrences(of: "▁", with: "_")
                if let markerStart = normalized.range(of: "<|tool_calls_begin|>") ?? normalized.range(of: "tool_calls_begin") {
                    let beforeText = String(text[text.startIndex..<markerStart.lowerBound]).trimmingCharacters(in: .whitespacesAndNewlines)
                    if !beforeText.isEmpty {
                        contentBlocks.append(["type": "text", "text": beforeText])
                    }
                }
                for call in deepSeekCalls {
                    contentBlocks.append([
                        "type": "tool_use",
                        "id": UUID().uuidString,
                        "name": call.name,
                        "input": call.input
                    ])
                }
                parsedToolFromText = true
            }
            // Check for DeepSeek V3.2 DSML-style tool calls (<function_calls><invoke name="...">)
            else if let dsmlCalls = Self.extractDSMLToolCalls(from: text) {
                // Extract text before the first <function_calls> or <invoke tag
                let cleaned = text.replacingOccurrences(of: "｜DSML｜", with: "").replacingOccurrences(of: "|DSML|", with: "")
                if let markerStart = cleaned.range(of: "<function_calls>") ?? cleaned.range(of: "<invoke") {
                    let beforeText = String(cleaned[cleaned.startIndex..<markerStart.lowerBound])
                        .trimmingCharacters(in: .whitespacesAndNewlines)
                    if !beforeText.isEmpty {
                        contentBlocks.append(["type": "text", "text": beforeText])
                    }
                }
                for call in dsmlCalls {
                    contentBlocks.append([
                        "type": "tool_use",
                        "id": UUID().uuidString,
                        "name": call.name,
                        "input": call.input
                    ])
                }
                parsedToolFromText = true
            }
            // Check if model wrote a tool call as plain text (common with Ollama models)
            else if let (toolName, nameRange, parsed) = Self.extractFirstToolCall(from: text) {
                let beforeText = String(text[..<nameRange.lowerBound]).trimmingCharacters(in: .whitespacesAndNewlines)
                if !beforeText.isEmpty {
                    contentBlocks.append(["type": "text", "text": beforeText])
                }

                contentBlocks.append([
                    "type": "tool_use",
                    "id": UUID().uuidString,
                    "name": toolName,
                    "input": parsed
                ])
                parsedToolFromText = true
            } else {
                contentBlocks.append(["type": "text", "text": text])
            }
        }

        if let toolCalls = message["tool_calls"] as? [[String: Any]] {
            for call in toolCalls {
                guard let function = call["function"] as? [String: Any],
                      let name = function["name"] as? String else { continue }

                let callId = call["id"] as? String ?? UUID().uuidString

                // Ollama native: arguments is a dict, not a JSON string
                let input: [String: Any]
                if let args = function["arguments"] as? [String: Any] {
                    input = args
                } else if let argsString = function["arguments"] as? String,
                          let parsed = try? JSONSerialization.jsonObject(with: Data(argsString.utf8)) as? [String: Any]
                {
                    input = parsed
                } else {
                    let funcName = function["name"] as? String ?? "unknown"
                    AuditLog.log(
                        .api,
                        "[OllamaService] Failed to parse tool args for "
                            + "\(funcName): "
                            + "\(String(describing: function["arguments"]).prefix(200))"
                    )
                    input = [:]
                }

                contentBlocks.append([
                    "type": "tool_use",
                    "id": callId,
                    "name": name,
                    "input": input
                ])
            }
        }

        if contentBlocks.isEmpty {
            contentBlocks.append(["type": "text", "text": "(no response)"])
        }

        // Determine stop reason from tool calls presence
        let hasToolCalls = message["tool_calls"] != nil || parsedToolFromText
        let stopReason = hasToolCalls ? "tool_use" : (done ? "end_turn" : "end_turn")

        return (contentBlocks, stopReason, promptEval, evalCount)
    }

    nonisolated private static func performStreamingRequest(
        bodyData: Data, apiKey: String, url: URL,
        onTextDelta: @escaping @Sendable (String) -> Void
    ) async throws -> (content: [[String: Any]], stopReason: String, inputTokens: Int, outputTokens: Int) {
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "content-type")
        if !apiKey.isEmpty {
            request.setValue("Bearer \(apiKey)", forHTTPHeaderField: "Authorization")
        }
        request.httpBody = bodyData
        request.timeoutInterval = llmAPITimeout

        let (bytes, response) = try await URLSession.shared.bytes(for: request)

        guard let httpResponse = response as? HTTPURLResponse else {
            throw AgentError.invalidResponse
        }

        guard httpResponse.statusCode == 200 else {
            var errorData = Data()
            for try await byte in bytes {
                errorData.append(byte)
            }
            let errorBody = String(data: errorData, encoding: .utf8) ?? "Unknown error"
            throw AgentError.apiError(statusCode: httpResponse.statusCode, message: errorBody)
        }

        var fullText = ""
        var contentBlocks: [[String: Any]] = []
        let stopReason = "end_turn"
        var insideToolCall = false
        var pendingBuffer = "" // Buffer text that might be the start of a tool call
        var repetitionCount = 0
        var lastSegment = ""
        var streamInputTokens = 0
        var streamOutputTokens = 0

        // Ollama streaming returns NDJSON: one JSON object per line
        for try await line in bytes.lines {
            guard !line.isEmpty,
                  let data = line.data(using: .utf8),
                  let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else
            {
                continue
            }

            // Each line has: {"message": {"content": "...", "role": "assistant"}, "done": false}
            if let message = json["message"] as? [String: Any],
               let content = message["content"] as? String,
               !content.isEmpty
            {
                fullText += content

                // Repetition detection: if the model keeps emitting the same ~50 char segment, bail out
                if fullText.count > 200 {
                    let recentEnd = fullText.endIndex
                    let recentStart = fullText.index(recentEnd, offsetBy: -min(60, fullText.count))
                    let segment = String(fullText[recentStart..<recentEnd]).trimmingCharacters(in: .whitespacesAndNewlines)
                    if segment.count > 20 && segment == lastSegment {
                        repetitionCount += 1
                        if repetitionCount >= 3 {
                            // Truncate the repeated text from fullText
                            break
                        }
                    } else {
                        lastSegment = segment
                        repetitionCount = 0
                    }
                }

                if insideToolCall { continue }

                pendingBuffer += content

                // Check if the buffer contains a confirmed tool call start
                let check = pendingBuffer.replacingOccurrences(of: "｜", with: "|").replacingOccurrences(of: "▁", with: "_")

                // Detect XML-style markers (V3.2 DSML, V3.1 special tokens)
                var detectedMarker: String? = nil
                for marker in ["<|tool_calls_begin|>", "<function_calls>", "<invoke "] {
                    if check.contains(marker) { detectedMarker = marker; break }
                }

                // Detect plain-text tool calls (V3.1 style: tool_name{...} or tool_name {"..."})
                if detectedMarker == nil {
                    for toolName in AgentTools.toolNames {
                        if let range = check.range(of: toolName) {
                            // Verify it's followed by { or whitespace+{ (not just a substring in prose)
                            let after = check[range.upperBound...]
                            let trimmed = after.drop(while: { $0 == " " || $0 == "\n" })
                            if trimmed.first == "{" {
                                detectedMarker = toolName
                                break
                            }
                        }
                    }
                }

                if let marker = detectedMarker {
                    insideToolCall = true
                    // Flush any text before the marker
                    if let range = check.range(of: marker) {
                        let before = String(pendingBuffer[pendingBuffer.startIndex..<range.lowerBound])
                            .trimmingCharacters(in: .whitespacesAndNewlines)
                        if !before.isEmpty { onTextDelta(before) }
                    }
                    pendingBuffer = ""
                    continue
                }

                // If buffer might be the start of a tag, hold it back
                if check.contains("<") {
                    continue // Keep buffering — might be XML tag
                }

                // If buffer ends with (or contains) a known tool name, hold it back
                // because the '{' with arguments may arrive in the next chunk.
                // But don't hold forever — flush if buffer grows too large without a '{'.
                if pendingBuffer.count < 300 {
                    var containsToolName = false
                    for toolName in AgentTools.toolNames {
                        if check.contains(toolName) {
                            containsToolName = true
                            break
                        }
                    }
                    if containsToolName {
                        continue // Keep buffering — waiting for '{'
                    }
                }

                // No tag detected — flush the buffer
                onTextDelta(pendingBuffer)
                pendingBuffer = ""
            }

            // Check for tool calls in streaming response
            if let message = json["message"] as? [String: Any],
               let toolCalls = message["tool_calls"] as? [[String: Any]]
            {
                for toolCall in toolCalls {
                    if let function = toolCall["function"] as? [String: Any],
                       let name = function["name"] as? String
                    {
                        let id = toolCall["id"] as? String ?? "call_\(UUID().uuidString.prefix(8).lowercased())"
                        let input = function["arguments"] as? [String: Any] ?? [:]
                        contentBlocks.append([
                            "type": "tool_use",
                            "id": id,
                            "name": name,
                            "input": input
                        ])
                    }
                }
            }

            // Final message has done: true — extract token counts
            if let done = json["done"] as? Bool, done {
                streamInputTokens = json["prompt_eval_count"] as? Int ?? 0
                streamOutputTokens = json["eval_count"] as? Int ?? 0
                break
            }
        }

        // Flush any remaining buffered text (no tool call detected)
        if !pendingBuffer.isEmpty && !insideToolCall {
            onTextDelta(pendingBuffer)
        }

        // Only parse tool calls from text if no native tool_calls were found
        var parsedToolFromText = false
        if contentBlocks.isEmpty, let deepSeekCalls = Self.extractDeepSeekToolCalls(from: fullText) {
            for call in deepSeekCalls {
                contentBlocks.append([
                    "type": "tool_use",
                    "id": UUID().uuidString,
                    "name": call.name,
                    "input": call.input
                ])
            }
            parsedToolFromText = true
        }
        // Check for DeepSeek V3.2 DSML format
        if contentBlocks.isEmpty, let dsmlCalls = Self.extractDSMLToolCalls(from: fullText) {
            for call in dsmlCalls {
                contentBlocks.append([
                    "type": "tool_use",
                    "id": UUID().uuidString,
                    "name": call.name,
                    "input": call.input
                ])
            }
            parsedToolFromText = true
        }
        if contentBlocks.isEmpty, let (toolName, nameRange, parsed) = Self.extractFirstToolCall(from: fullText) {
            let beforeText = String(fullText[..<nameRange.lowerBound]).trimmingCharacters(in: .whitespacesAndNewlines)
            if !beforeText.isEmpty {
                contentBlocks.append(["type": "text", "text": beforeText])
            }

            contentBlocks.append([
                "type": "tool_use",
                "id": UUID().uuidString,
                "name": toolName,
                "input": parsed
            ])
            parsedToolFromText = true
        }

        // If no tool calls found, return text
        if !parsedToolFromText && contentBlocks.isEmpty && !fullText.isEmpty {
            contentBlocks.append(["type": "text", "text": fullText])
        }

        return (contentBlocks, stopReason, streamInputTokens, streamOutputTokens)
    }

    /// Find the earliest tool call by position in the text, parse its JSON args.
    /// Returns (toolName, rangeOfName, parsedArgs) or nil.
    nonisolated static func extractFirstToolCall(from text: String) -> (String, Range<String.Index>, [String: Any])? {
        let toolNames = AgentTools.toolNames

        var earliest: (String, Range<String.Index>, [String: Any])? = nil

        for toolName in toolNames {
            guard let nameRange = text.range(of: toolName) else { continue }
            // Only consider if this is earlier than what we found so far
            if let existing = earliest, nameRange.lowerBound >= existing.1.lowerBound { continue }
            // Skip garbage between tool name and '{' (LLMs sometimes emit junk)
            let remainder = text[nameRange.upperBound...]
            guard let braceIdx = remainder.firstIndex(of: "{") else { continue }
            // Only allow up to 20 garbage chars between name and '{'
            guard text.distance(from: nameRange.upperBound, to: braceIdx) <= 20 else { continue }
            let afterName = String(remainder[braceIdx...])
            // Extract just the first balanced JSON object to avoid garbage trailing braces
            guard let json = Self.extractFirstJSON(from: afterName) else { continue }
            earliest = (toolName, nameRange, json)
        }

        return earliest
    }

    /// Parse DeepSeek-style tool calls from text using special token markers.
    /// V3.1 format: <｜tool▁call▁begin｜>function_name<｜tool▁sep｜>{"arg":"val"}<｜tool▁call▁end｜>
    /// Also handles: <｜tool▁call▁begin｜>{"name":"...","parameters":{...}}<｜tool▁call▁end｜>
    /// Unicode variants with fullwidth ｜ and half-width | are both supported.
    nonisolated static func extractDeepSeekToolCalls(from text: String) -> [(name: String, input: [String: Any])]? {
        // Normalize: DeepSeek uses fullwidth ｜ (U+FF5C) and ▁ (U+2581) in tokens
        let normalized = text
            .replacingOccurrences(of: "｜", with: "|")
            .replacingOccurrences(of: "▁", with: "_")
            .replacingOccurrences(of: "\u{2581}", with: "_")

        guard normalized.contains("tool_calls_begin") || normalized.contains("tool_call_begin") else {
            return nil
        }

        var results: [(String, [String: Any])] = []
        var searchRange = normalized.startIndex..<normalized.endIndex

        while let beginRange = normalized.range(of: "tool_call_begin", range: searchRange) {
            let afterBegin = beginRange.upperBound
            guard let endRange = normalized.range(of: "tool_call_end", range: afterBegin..<normalized.endIndex) else { break }

            let rawContent = String(normalized[afterBegin..<endRange.lowerBound])
                .replacingOccurrences(of: "<|", with: "")
                .replacingOccurrences(of: "|>", with: "")
                .trimmingCharacters(in: .whitespacesAndNewlines)

            // V3.1 format: function_name\ntool_sep\n{"arg":"val"}
            // After stripping <| and |>, tool_sep appears as plain text
            if rawContent.contains("tool_sep") {
                let parts = rawContent.components(separatedBy: "tool_sep")
                let funcName = parts[0].trimmingCharacters(in: .whitespacesAndNewlines)
                let argsText = parts.dropFirst().joined(separator: "tool_sep").trimmingCharacters(in: .whitespacesAndNewlines)
                if !funcName.isEmpty {
                    var params: [String: Any] = [:]
                    if let data = argsText.data(using: .utf8),
                       let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
                    {
                        params = json
                    }
                    results.append((funcName, params))
                }
            }
            // Legacy format: {"name": "...", "parameters": {...}}
            else if let data = rawContent.data(using: .utf8),
                    let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
                    let name = json["name"] as? String
            {
                let params = json["parameters"] as? [String: Any]
                    ?? json["arguments"] as? [String: Any]
                    ?? [:]
                results.append((name, params))
            }

            searchRange = endRange.upperBound..<normalized.endIndex
        }

        return results.isEmpty ? nil : results
    }

    /// Parse DeepSeek V3.2 DSML-style tool calls from text.
    /// V3.2 emits: <｜DSML｜function_calls><｜DSML｜invoke name="tool">
    ///   <｜DSML｜parameter name="key" string="true">value</｜DSML｜parameter>
    /// </｜DSML｜invoke></｜DSML｜function_calls>
    /// Ollama strips the ｜DSML｜ tokens, leaving plain XML-like tags.
    nonisolated static func extractDSMLToolCalls(from text: String) -> [(name: String, input: [String: Any])]? {
        // Strip any remaining DSML tokens
        let cleaned = text
            .replacingOccurrences(of: "｜DSML｜", with: "")
            .replacingOccurrences(of: "|DSML|", with: "")

        guard cleaned.contains("<function_calls>") || cleaned.contains("<invoke") else {
            return nil
        }

        var results: [(String, [String: Any])] = []

        // Find each <invoke name="...">...</invoke> block
        let invokePattern = #"<invoke\s+name="([^"]+)">(.*?)</invoke>"#
        guard let invokeRegex = try? NSRegularExpression(pattern: invokePattern, options: [.dotMatchesLineSeparators]) else {
            return nil
        }

        let nsText = cleaned as NSString
        let matches = invokeRegex.matches(in: cleaned, range: NSRange(location: 0, length: nsText.length))

        for match in matches {
            guard match.numberOfRanges >= 3 else { continue }
            let name = nsText.substring(with: match.range(at: 1))
            let body = nsText.substring(with: match.range(at: 2))

            // Parse <parameter name="key" string="true|false">value</parameter>
            var params: [String: Any] = [:]
            let paramPattern = #"<parameter\s+name="([^"]+)"\s+string="(true|false)">(.*?)</parameter>"#
            if let paramRegex = try? NSRegularExpression(pattern: paramPattern, options: [.dotMatchesLineSeparators]) {
                let nsBody = body as NSString
                let paramMatches = paramRegex.matches(in: body, range: NSRange(location: 0, length: nsBody.length))
                for pm in paramMatches {
                    guard pm.numberOfRanges >= 4 else { continue }
                    let key = nsBody.substring(with: pm.range(at: 1))
                    let isString = nsBody.substring(with: pm.range(at: 2)) == "true"
                    let value = nsBody.substring(with: pm.range(at: 3))

                    if isString {
                        params[key] = value
                    } else {
                        // Try to parse as JSON value (number, bool, object, array)
                        if let data = value.data(using: .utf8),
                           let parsed = try? JSONSerialization.jsonObject(with: data)
                        {
                            params[key] = parsed
                        } else {
                            params[key] = value
                        }
                    }
                }
            }

            // If no parameters found but body has JSON, try parsing it
            if params.isEmpty {
                let trimmedBody = body.trimmingCharacters(in: .whitespacesAndNewlines)
                if trimmedBody.hasPrefix("{"),
                   let data = trimmedBody.data(using: .utf8),
                   let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
                {
                    params = json
                }
            }

            results.append((name, params))
        }

        return results.isEmpty ? nil : results
    }

    /// Extract the first balanced JSON object from a string, ignoring trailing garbage.
    nonisolated static func extractFirstJSON(from text: String) -> [String: Any]? {
        var depth = 0
        var inString = false
        var escape = false
        var endIndex: String.Index?

        for i in text.indices {
            let c = text[i]
            if escape { escape = false; continue }
            if c == "\\" && inString { escape = true; continue }
            if c == "\"" { inString.toggle(); continue }
            if inString { continue }
            if c == "{" { depth += 1 }
            else if c == "}" {
                depth -= 1
                if depth == 0 { endIndex = text.index(after: i); break }
            }
        }

        guard let end = endIndex else { return nil }
        let jsonString = String(text[text.startIndex..<end])
        guard let data = jsonString.data(using: .utf8),
              let parsed = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { return nil }
        return parsed
    }
}
