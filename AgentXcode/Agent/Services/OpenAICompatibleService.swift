import AgentLLM
import AgentAudit
@preconcurrency import Foundation
import AgentTools

/// Unified service for OpenAI and Hugging Face Inference API.
/// Both use the OpenAI chat completions format with SSE streaming.
/// Generate a 9-char alphanumeric tool call ID compatible with all providers (including Mistral).
private func shortToolId() -> String {
    let chars = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"
    return String((0..<9).map { _ in chars.randomElement()! })
}

/// Sanitize an existing tool call ID to 9 alphanumeric chars.
private func sanitizeToolId(_ id: String) -> String {
    let clean = String(id.unicodeScalars.filter { CharacterSet.alphanumerics.contains($0) })
    if clean.count >= 9 { return String(clean.prefix(9)) }
    // Pad if too short
    return clean + shortToolId().prefix(9 - clean.count)
}

@MainActor
final class OpenAICompatibleService {
    let apiKey: String
    let model: String
    let baseURL: URL
    let supportsVision: Bool
    let provider: APIProvider
    var temperature: Double = 0.2
    var compactTools: Bool = false
    /// Key name for the messages array in the request body.
    /// OpenAI uses "messages", LM Studio Native uses "input".
    let messagesKey: String

    let historyContext: String
    let userHome: String
    let userName: String
    let projectFolder: String
    /// Max output tokens. 0 = omit (let provider decide).
    let maxTokens: Int

    // MARK: - Rate Limiting
    /// Per-provider last request timestamp for rate limiting.
    private static var lastRequestTime: [APIProvider: CFAbsoluteTime] = [:]
    /// Minimum seconds between requests per provider. Empty = no throttle.
    private static let rateLimitSeconds: [APIProvider: Double] = [:]
    /// Dynamic backoff from Retry-After header (overrides static limit until it expires).
    private static var retryAfterUntil: [APIProvider: CFAbsoluteTime] = [:]

    init(apiKey: String, model: String, baseURL: String, supportsVision: Bool = false, historyContext: String = "", projectFolder: String = "", provider: APIProvider, messagesKey: String = "messages", maxTokens: Int = 0) {
        self.apiKey = apiKey
        self.model = model
        self.baseURL = URL(string: baseURL) ?? URL(filePath: "/")
        self.supportsVision = supportsVision
        self.provider = provider
        self.messagesKey = messagesKey
        self.maxTokens = maxTokens
        self.historyContext = historyContext
        self.userHome = FileManager.default.homeDirectoryForCurrentUser.path
        self.userName = NSUserName()
        self.projectFolder = projectFolder
    }

    /// LM Studio models use compact prompt to fit small context windows
    private var isLMStudio: Bool { provider == .lmStudio }

    var overrideSystemPrompt: String?

    var systemPrompt: String {
        if let override = overrideSystemPrompt { return override }
        if isLMStudio {
            return AgentTools.compactSystemPrompt(userName: userName, userHome: userHome, projectFolder: projectFolder)
        }
        var prompt = SystemPromptService.shared.prompt(for: provider, userName: userName, userHome: userHome, projectFolder: projectFolder)
        if !projectFolder.isEmpty {
            prompt = "CURRENT PROJECT FOLDER: \(projectFolder)\nAlways cd to this directory before running any shell commands. Use it as the default for all file operations. You may go outside it when needed.\n\n" + prompt
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
        let groups = isLMStudio ? Tool.codingGroups : activeGroups
        return AgentTools.ollamaTools(for: provider, activeGroups: groups, compact: compact, projectFolder: projectFolder)
    }

    /// Prepend project folder to the last user message (only on first message).
    private func withFolderPrefix(_ messages: [[String: Any]]) -> [[String: Any]] {
        // Skip if folder is in system prompt already (iteration 1) or short prompt (iteration 2+)
        guard !projectFolder.isEmpty, messages.count <= 1 else { return messages }
        let prefix = "PROJECT FOLDER: \(projectFolder)\n"
        var result = messages
        for i in stride(from: result.count - 1, through: 0, by: -1) {
            guard result[i]["role"] as? String == "user" else { continue }
            if let text = result[i]["content"] as? String {
                result[i]["content"] = prefix + text
            } else if var blocks = result[i]["content"] as? [[String: Any]],
                      let first = blocks.first, first["type"] as? String == "text",
                      let existing = first["text"] as? String {
                blocks[0]["text"] = prefix + existing
                result[i]["content"] = blocks
            }
            break
        }
        return result
    }

    /// Short system prompt for subsequent iterations — LLM already has the full prompt cached.
    private var shortSystemPrompt: String {
        var prompt = "Continue the task. Use tools as needed. ALWAYS call task_complete when finished."
        if !projectFolder.isEmpty {
            prompt += "\nPROJECT FOLDER: \(projectFolder)"
        }
        prompt += MemoryStore.shared.contextBlock
        return prompt
    }

    /// All tools every iteration — compact descriptions in coding mode.
    func toolsForIteration(_ messages: [[String: Any]], activeGroups: Set<String>? = nil) -> [[String: Any]] {
        // Tools are byte-stable across iterations now (the old condensed/full _tool-name swap
        // was removed in 2.38.0 because canonical names no longer carry the suffix).
        return tools(activeGroups: activeGroups, compact: compactTools)
    }

    /// Set to true when a tool call fails — next turn sends full _tool names, then resets.
    var needsFullToolNames: Bool = false

    /// Convert Claude-format messages to OpenAI chat messages.
    /// Always sends full system prompt — OpenAI-compatible APIs don't cache.
    private func convertMessages(_ messages: [[String: Any]]) -> [[String: Any]] {
        let prompt = systemPrompt
        var chatMessages: [[String: Any]] = [
            ["role": "system", "content": prompt]
        ]
        // Track tool call ID → name mapping for tool response "name" field (required by Mistral)
        var toolIdToName: [String: String] = [:]

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
                            let sanitizedId = sanitizeToolId(toolUseId)
                            var toolMsg: [String: Any] = [
                                "role": "tool",
                                "tool_call_id": sanitizedId,
                                "content": content
                            ]
                            // Add "name" field — required by Mistral and Gemini
                            let toolName = toolIdToName[toolUseId] ?? toolIdToName[sanitizedId] ?? (block["name"] as? String) ?? "tool"
                            toolMsg["name"] = toolName
                            chatMessages.append(toolMsg)
                        }
                    } else {
                        // Content blocks (text + images) — use OpenAI multipart content
                        var contentParts: [[String: Any]] = []
                        let imageBlocks = blocks.filter { $0["type"] as? String == "image" }
                        AuditLog.log(.api, "convertMessages: \(blocks.count) blocks, \(imageBlocks.count) images, supportsVision=\(supportsVision)")
                        for block in blocks {
                            if block["type"] as? String == "text",
                               let t = block["text"] as? String {
                                contentParts.append(["type": "text", "text": t])
                            } else if supportsVision,
                                      block["type"] as? String == "image",
                                      let source = block["source"] as? [String: Any],
                                      let base64 = source["data"] as? String {
                                let mediaType = source["media_type"] as? String ?? "image/png"
                                AuditLog.log(.api, "convertMessages: adding image \(base64.count) chars, mediaType=\(mediaType)")
                                contentParts.append([
                                    "type": "image_url",
                                    "image_url": [
                                        "url": "data:\(mediaType);base64,\(base64)"
                                    ] as [String: Any]
                                ])
                            }
                        }
                        if contentParts.isEmpty {
                            contentParts.append(["type": "text", "text": "(image was attached but this model does not support vision)"])
                        }
                        chatMessages.append(["role": "user", "content": contentParts])
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
                            let rawId = block["id"] as? String ?? shortToolId()
                            let callId = sanitizeToolId(rawId)
                            let name = block["name"] as? String ?? ""
                            // Track ID → name for tool response messages
                            toolIdToName[rawId] = name
                            toolIdToName[callId] = name
                            let input = block["input"] as? [String: Any] ?? [:]
                            // OpenAI expects arguments as a JSON string
                            let argsString: String
                            if let data = try? JSONSerialization.data(withJSONObject: input),
                               let str = String(data: data, encoding: .utf8) {
                                argsString = str
                            } else {
                                argsString = "{}"
                            }
                            var tc: [String: Any] = [
                                "id": callId,
                                "type": "function",
                                "function": [
                                    "name": name,
                                    "arguments": argsString
                                ] as [String: Any]
                            ]
                            // Gemini thought_signature — echo back for tool call round-trips
                            if let sig = block["thought_signature"] as? String {
                                tc["thought_signature"] = sig
                            }
                            toolCalls.append(tc)
                        }
                    }

                    var assistantMsg: [String: Any] = ["role": "assistant"]
                    if !textParts.isEmpty { assistantMsg["content"] = textParts }
                    // Gemini thought_signature — echo back on assistant message and each tool_call
                    if !toolCalls.isEmpty {
                        var sig: String?
                        for block in blocks {
                            if let s = block["thought_signature"] as? String { sig = s; break }
                        }
                        if let sig {
                            // Echo on each tool_call as extra_content.google.thought_signature
                            let extraContent: [String: Any] = ["google": ["thought_signature": sig]]
                            for i in toolCalls.indices {
                                toolCalls[i]["extra_content"] = extraContent
                            }
                            // Also echo at the message level
                            assistantMsg["extra_content"] = extraContent
                        }
                        assistantMsg["tool_calls"] = toolCalls
                    }
                    chatMessages.append(assistantMsg)
                }
            }
        }
        // Mistral requires strict tool message ordering:
        // 1. tool messages must follow an assistant message with tool_calls
        // 2. number of tool responses must equal number of tool_calls
        if provider == .mistral || provider == .codestral || provider == .vibe {
            var cleaned: [[String: Any]] = []
            var i = 0
            while i < chatMessages.count {
                let msg = chatMessages[i]
                let role = msg["role"] as? String ?? ""

                if role == "assistant", let calls = msg["tool_calls"] as? [[String: Any]], !calls.isEmpty {
                    let expectedCount = calls.count
                    let callIds = Set(calls.compactMap { $0["id"] as? String })
                    // Collect following tool messages
                    var toolMsgs: [[String: Any]] = []
                    var j = i + 1
                    while j < chatMessages.count, chatMessages[j]["role"] as? String == "tool" {
                        toolMsgs.append(chatMessages[j])
                        j += 1
                    }
                    if toolMsgs.count == expectedCount {
                        // Counts match — keep assistant + all tool messages
                        cleaned.append(msg)
                        cleaned.append(contentsOf: toolMsgs)
                    } else {
                        // Mismatch — only keep tool messages that match a call ID, pad missing ones
                        cleaned.append(msg)
                        var usedIds = Set<String>()
                        for tm in toolMsgs {
                            if let tid = tm["tool_call_id"] as? String, callIds.contains(tid) {
                                cleaned.append(tm)
                                usedIds.insert(tid)
                            }
                        }
                        // Pad any missing tool responses so counts match
                        let callNameMap = Dictionary(uniqueKeysWithValues: calls.compactMap { c -> (String, String)? in
                            guard let id = c["id"] as? String,
                                  let fn = c["function"] as? [String: Any],
                                  let name = fn["name"] as? String else { return nil }
                            return (id, name)
                        })
                        for cid in callIds where !usedIds.contains(cid) {
                            var pad: [String: Any] = [
                                "role": "tool",
                                "tool_call_id": cid,
                                "content": "(no result)"
                            ]
                            if let name = callNameMap[cid] { pad["name"] = name }
                            cleaned.append(pad)
                        }
                    }
                    i = j
                } else if role == "tool" {
                    // Orphaned tool message — drop it
                    i += 1
                } else {
                    cleaned.append(msg)
                    i += 1
                }
            }
            return cleaned
        }

        return chatMessages
    }

    /// Convert OpenAI-format messages to LM Studio Native "input" format.
    /// Each item needs "type": "text" (or "image") plus "role" and "text".
    private func convertToNativeInput(_ openAIMessages: [[String: Any]]) -> [[String: Any]] {
        return openAIMessages.compactMap { msg -> [String: Any]? in
            let role = msg["role"] as? String ?? "user"
            if role == "tool" { return nil }
            if let text = msg["content"] as? String {
                return ["type": "text", "content": text]
            } else if let parts = msg["content"] as? [[String: Any]] {
                let combined = parts.compactMap { $0["text"] as? String }.joined(separator: "\n")
                if !combined.isEmpty {
                    return ["type": "text", "content": combined]
                }
            }
            if role == "assistant" {
                return ["type": "text", "content": "(tool call)"]
            }
            return nil
        }
    }

    /// Build the messages payload for the request body, converting format if needed.
    private func buildMessagesPayload(_ messages: [[String: Any]]) -> Any {
        let chatMessages = convertMessages(messages)
        if messagesKey == "input" {
            return convertToNativeInput(chatMessages)
        }
        return chatMessages
    }

    // MARK: - Non-Streaming

    /// LM Studio Native (/api/v1/chat) doesn't support tools or max_tokens
    private var isNativeFormat: Bool { messagesKey == "input" }

    /// Wait if needed to respect per-provider rate limits and Retry-After backoff.
    private func enforceRateLimit() async {
        let now = CFAbsoluteTimeGetCurrent()
        // Honor Retry-After from a previous 429
        if let until = Self.retryAfterUntil[provider], now < until {
            let wait = until - now
            try? await Task.sleep(for: .seconds(wait))
        }
        // Enforce minimum gap between requests
        if let minGap = Self.rateLimitSeconds[provider],
           let last = Self.lastRequestTime[provider] {
            let elapsed = CFAbsoluteTimeGetCurrent() - last
            if elapsed < minGap {
                let wait = minGap - elapsed
                try? await Task.sleep(for: .seconds(wait))
            }
        }
        Self.lastRequestTime[provider] = CFAbsoluteTimeGetCurrent()
    }

    /// Record a Retry-After value from a 429 response.
    private static func recordRetryAfter(_ seconds: Double, for provider: APIProvider) {
        retryAfterUntil[provider] = CFAbsoluteTimeGetCurrent() + seconds
    }

    func send(messages: [[String: Any]], activeGroups: Set<String>? = nil) async throws -> (content: [[String: Any]], stopReason: String, inputTokens: Int, outputTokens: Int) {
        await enforceRateLimit()
        let payload = buildMessagesPayload(messages)

        var body: [String: Any] = [
            "model": model,
            "temperature": temperature,
            messagesKey: payload,
            "stream": false
        ]
        if !isNativeFormat {
            if maxTokens > 0 { body["max_tokens"] = maxTokens }
            let toolDefs = toolsForIteration(messages, activeGroups: activeGroups)
            if !toolDefs.isEmpty {
                body["tools"] = toolDefs
                body["tool_choice"] = "auto"
                // Mistral: disable parallel tool calls — our loop handles one at a time
                if provider == .mistral || provider == .codestral || provider == .vibe {
                    body["parallel_tool_calls"] = false
                }
            }
        }

        let bodyData = try JSONSerialization.data(withJSONObject: body)
        return try await Self.performRequest(bodyData: bodyData, apiKey: apiKey, url: baseURL)
    }

    // MARK: - Streaming

    func sendStreaming(
        messages: [[String: Any]],
        activeGroups: Set<String>? = nil,
        onTextDelta: @escaping @Sendable (String) -> Void
    ) async throws -> (content: [[String: Any]], stopReason: String, inputTokens: Int, outputTokens: Int) {
        await enforceRateLimit()
        let payload = buildMessagesPayload(messages)

        var body: [String: Any] = [
            "model": model,
            "temperature": temperature,
            messagesKey: payload,
            "stream": true
        ]
        if !isNativeFormat {
            if maxTokens > 0 { body["max_tokens"] = maxTokens }
            let toolDefs = toolsForIteration(messages, activeGroups: activeGroups)
            if !toolDefs.isEmpty {
                body["tools"] = toolDefs
                body["tool_choice"] = "auto"
                if provider == .mistral || provider == .codestral || provider == .vibe {
                    body["parallel_tool_calls"] = false
                }
            }
        }

        let bodyData = try JSONSerialization.data(withJSONObject: body)
        return try await Self.performStreamingRequest(
            bodyData: bodyData,
            apiKey: apiKey,
            url: baseURL,
            onTextDelta: onTextDelta
        )
    }

    // MARK: - Non-Streaming Request

    nonisolated private static func performRequest(
        bodyData: Data, apiKey: String, url: URL
    ) async throws -> (content: [[String: Any]], stopReason: String, inputTokens: Int, outputTokens: Int) {
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.setValue("Bearer \(apiKey)", forHTTPHeaderField: "Authorization")
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

        guard let json = try JSONSerialization.jsonObject(with: data) as? [String: Any],
              let choices = json["choices"] as? [[String: Any]],
              let firstChoice = choices.first,
              let message = firstChoice["message"] as? [String: Any] else {
            throw AgentError.invalidResponse
        }

        let finishReason = firstChoice["finish_reason"] as? String ?? "stop"

        // Convert to Claude-compatible content blocks
        var contentBlocks: [[String: Any]] = []
        var parsedToolFromText = false

        if let text = message["content"] as? String, !text.isEmpty {
            // Check for DeepSeek text-based tool calls before treating as plain text
            if let deepSeekCalls = OllamaService.extractDeepSeekToolCalls(from: text) {
                for call in deepSeekCalls {
                    contentBlocks.append([
                        "type": "tool_use",
                        "id": shortToolId(),
                        "name": call.name,
                        "input": call.input
                    ])
                }
                parsedToolFromText = true
            } else if let dsmlCalls = OllamaService.extractDSMLToolCalls(from: text) {
                for call in dsmlCalls {
                    contentBlocks.append([
                        "type": "tool_use",
                        "id": shortToolId(),
                        "name": call.name,
                        "input": call.input
                    ])
                }
                parsedToolFromText = true
            } else if let (toolName, _, parsed) = OllamaService.extractFirstToolCall(from: text) {
                contentBlocks.append([
                    "type": "tool_use",
                    "id": shortToolId(),
                    "name": toolName,
                    "input": parsed
                ])
                parsedToolFromText = true
            } else {
                // Strip vLLM/Qwen special tokens
                var cleaned = text
                    .replacingOccurrences(of: "<\\|im_(?:start|end)\\|>", with: "", options: .regularExpression)
                    .trimmingCharacters(in: .whitespacesAndNewlines)
                // If native tool_calls also present, skip text that is raw JSON
                let hasNativeTools = message["tool_calls"] != nil
                if hasNativeTools && (cleaned.hasPrefix("{\"name\"") || cleaned.hasPrefix("[{\"name\"")) {
                    cleaned = ""
                }
                if !cleaned.isEmpty {
                    contentBlocks.append(["type": "text", "text": cleaned])
                }
            }
        }

        if let toolCalls = message["tool_calls"] as? [[String: Any]] {
            for call in toolCalls {
                guard let function = call["function"] as? [String: Any],
                      let name = function["name"] as? String else { continue }

                let callId = sanitizeToolId(call["id"] as? String ?? shortToolId())

                // OpenAI: arguments is a JSON string
                let input: [String: Any]
                if let argsString = function["arguments"] as? String,
                   let parsed = try? JSONSerialization.jsonObject(with: Data(argsString.utf8)) as? [String: Any] {
                    input = parsed
                } else if let args = function["arguments"] as? [String: Any] {
                    input = args
                } else {
                    let funcName = function["name"] as? String ?? "unknown"
                    AuditLog.log(.api, "[OpenAIService] Failed to parse tool args for \(funcName): \(String(describing: function["arguments"]).prefix(200))")
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

        let hasToolCalls = (message["tool_calls"] != nil) || parsedToolFromText
        let stopReason = hasToolCalls ? "tool_use" : (finishReason == "tool_calls" ? "tool_use" : (finishReason == "length" ? "max_tokens" : "end_turn"))
        let usage = json["usage"] as? [String: Any]
        let inTok = usage?["prompt_tokens"] as? Int ?? 0
        let outTok = usage?["completion_tokens"] as? Int ?? 0
        return (contentBlocks, stopReason, inTok, outTok)
    }

    // MARK: - Streaming Request (SSE)

    nonisolated private static func performStreamingRequest(
        bodyData: Data, apiKey: String, url: URL,
        onTextDelta: @escaping @Sendable (String) -> Void
    ) async throws -> (content: [[String: Any]], stopReason: String, inputTokens: Int, outputTokens: Int) {
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.setValue("Bearer \(apiKey)", forHTTPHeaderField: "Authorization")
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
        var finishReason = "stop"
        var streamInputTokens = 0
        var streamOutputTokens = 0

        // Accumulate streamed tool calls: index -> (id, name, arguments)
        var toolCallAccum: [Int: (id: String, name: String, arguments: String)] = [:]
        // Gemini thought_signature — on the message/delta level, echoed back on assistant message
        var thoughtSignature: String?

        // Buffer text line-by-line so we can suppress raw JSON tool calls
        // that vLLM/Qwen outputs as text content instead of native tool_calls
        var lineBuffer = ""

        /// Check if a line is a raw JSON tool call (e.g. {"name": "...", "arguments": {...}})
        func isToolCallJSON(_ line: String) -> Bool {
            let trimmed = line.trimmingCharacters(in: .whitespacesAndNewlines)
            guard trimmed.hasPrefix("{"), trimmed.contains("\"name\""),
                  trimmed.contains("\"arguments\"") else { return false }
            // Verify it actually parses as JSON with name + arguments keys
            if let data = trimmed.data(using: .utf8),
               let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
               obj["name"] is String, obj["arguments"] != nil {
                return true
            }
            return false
        }

        /// Flush the line buffer — forward to UI if it's not a tool call JSON
        func flushLineBuffer() {
            guard !lineBuffer.isEmpty else { return }
            if !isToolCallJSON(lineBuffer) {
                onTextDelta(lineBuffer)
            }
            lineBuffer = ""
        }

        // OpenAI SSE format: lines prefixed with "data: "
        for try await line in bytes.lines {
            // Skip empty lines and SSE comments
            guard line.hasPrefix("data: ") else { continue }
            let payload = String(line.dropFirst(6))

            // End of stream
            if payload == "[DONE]" { break }

            guard let data = payload.data(using: .utf8),
                  let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { continue }

            // Extract usage if present (final chunk)
            if let usage = json["usage"] as? [String: Any] {
                streamInputTokens = usage["prompt_tokens"] as? Int ?? streamInputTokens
                streamOutputTokens = usage["completion_tokens"] as? Int ?? streamOutputTokens
            }

            // LM Studio Native: top-level "content" field without "choices"
            if let nativeContent = json["content"] as? String, json["choices"] == nil {
                fullText += nativeContent
                onTextDelta(nativeContent)
                continue
            }

            guard let choices = json["choices"] as? [[String: Any]],
                  let firstChoice = choices.first else { continue }

            // Check finish reason
            if let fr = firstChoice["finish_reason"] as? String {
                finishReason = fr
            }

            guard let delta = firstChoice["delta"] as? [String: Any] else { continue }

            // Gemini thought_signature — nested under extra_content.google.thought_signature
            if let extra = delta["extra_content"] as? [String: Any],
               let google = extra["google"] as? [String: Any],
               let sig = google["thought_signature"] as? String {
                thoughtSignature = sig
            }
            // Also check top-level (future-proofing)
            if thoughtSignature == nil, let sig = delta["thought_signature"] as? String {
                thoughtSignature = sig
            }

            // Tool call deltas (streamed incrementally)
            if let toolCalls = delta["tool_calls"] as? [[String: Any]] {
                for tc in toolCalls {
                    let index = tc["index"] as? Int ?? 0

                    if toolCallAccum[index] == nil {
                        toolCallAccum[index] = (id: "", name: "", arguments: "")
                    }

                    if let id = tc["id"] as? String {
                        toolCallAccum[index]?.id = id
                    }
                    if let function = tc["function"] as? [String: Any] {
                        if let name = function["name"] as? String {
                            toolCallAccum[index]?.name += name
                        }
                        if let args = function["arguments"] as? String {
                            toolCallAccum[index]?.arguments += args
                        }
                    }
                    // Gemini: extra_content.google.thought_signature per tool_call
                    if let extra = tc["extra_content"] as? [String: Any],
                       let google = extra["google"] as? [String: Any],
                       let sig = google["thought_signature"] as? String {
                        thoughtSignature = sig
                    }
                }
            }

            // Text content delta — buffer by newlines to detect and suppress
            // raw JSON tool calls that vLLM/Qwen outputs as text
            if let content = delta["content"] as? String, !content.isEmpty {
                // Strip special tokens
                let cleaned = content
                    .replacingOccurrences(of: "<|im_start|>", with: "")
                    .replacingOccurrences(of: "<|im_end|>", with: "")
                fullText += cleaned

                // Suppress all text when native tool calls are being streamed
                if !toolCallAccum.isEmpty { continue }

                // Stream text directly (like Claude) — only buffer potential JSON tool calls
                if lineBuffer.hasPrefix("{") || cleaned.trimmingCharacters(in: .whitespacesAndNewlines).hasPrefix("{") {
                    // Buffering a potential JSON tool call line — accumulate until newline
                    for ch in cleaned {
                        if ch == "\n" {
                            let suppressed = isToolCallJSON(lineBuffer)
                            flushLineBuffer()
                            if !suppressed { onTextDelta("\n") }
                        } else {
                            lineBuffer.append(ch)
                        }
                    }
                } else {
                    onTextDelta(cleaned)
                }
            }
        }
        // Flush any remaining buffered text
        flushLineBuffer()

        // Build Claude-compatible content blocks
        var contentBlocks: [[String: Any]] = []
        var parsedToolFromText = false

        // Convert accumulated native tool calls first
        for index in toolCallAccum.keys.sorted() {
            guard let tc = toolCallAccum[index], !tc.name.isEmpty else { continue }
            let callId = tc.id.isEmpty ? shortToolId() : sanitizeToolId(tc.id)

            let input: [String: Any]
            if let parsed = try? JSONSerialization.jsonObject(with: Data(tc.arguments.utf8)) as? [String: Any] {
                input = parsed
            } else {
                AuditLog.log(.api, "[OpenAIService] Failed to parse streamed tool args for \(tc.name): \(tc.arguments.prefix(200))")
                input = [:]
            }

            var toolBlock: [String: Any] = [
                "type": "tool_use",
                "id": callId,
                "name": tc.name,
                "input": input
            ]
            // Gemini thought_signature — store for echoing back
            if let sig = thoughtSignature {
                toolBlock["thought_signature"] = sig
            }
            contentBlocks.append(toolBlock)
        }

        // If no native tool calls, check text for DeepSeek-style tool calls
        if contentBlocks.isEmpty && !fullText.isEmpty {
            if let deepSeekCalls = OllamaService.extractDeepSeekToolCalls(from: fullText) {
                for call in deepSeekCalls {
                    contentBlocks.append([
                        "type": "tool_use",
                        "id": shortToolId(),
                        "name": call.name,
                        "input": call.input
                    ])
                }
                parsedToolFromText = true
            } else if let dsmlCalls = OllamaService.extractDSMLToolCalls(from: fullText) {
                for call in dsmlCalls {
                    contentBlocks.append([
                        "type": "tool_use",
                        "id": shortToolId(),
                        "name": call.name,
                        "input": call.input
                    ])
                }
                parsedToolFromText = true
            } else if let (toolName, _, parsed) = OllamaService.extractFirstToolCall(from: fullText) {
                contentBlocks.append([
                    "type": "tool_use",
                    "id": shortToolId(),
                    "name": toolName,
                    "input": parsed
                ])
                parsedToolFromText = true
            }
        }

        // Add text if no tool calls were found from it
        // Strip vLLM/Qwen special tokens that leak through as text content
        if !parsedToolFromText && !fullText.isEmpty {
            var cleaned = fullText
            // Remove <|im_start|>, <|im_end|>, and similar special tokens
            cleaned = cleaned.replacingOccurrences(of: "<\\|im_(?:start|end)\\|>", with: "", options: .regularExpression)
            // If native tool calls exist, discard text that is just raw JSON tool call output
            if !toolCallAccum.isEmpty {
                let trimmed = cleaned.trimmingCharacters(in: .whitespacesAndNewlines)
                // Skip text that looks like raw tool call JSON the model leaked
                if trimmed.isEmpty || trimmed.hasPrefix("{\"name\"") || trimmed.hasPrefix("[{\"name\"") {
                    cleaned = ""
                }
            }
            if !cleaned.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                contentBlocks.insert(["type": "text", "text": cleaned], at: 0)
            }
        }

        if contentBlocks.isEmpty {
            contentBlocks.append(["type": "text", "text": "(no response)"])
        }

        let hasToolCalls = !toolCallAccum.isEmpty || parsedToolFromText
        let stopReason = hasToolCalls ? "tool_use" : (finishReason == "tool_calls" ? "tool_use" : (finishReason == "length" ? "max_tokens" : "end_turn"))
        return (contentBlocks, stopReason, streamInputTokens, streamOutputTokens)
    }
}
