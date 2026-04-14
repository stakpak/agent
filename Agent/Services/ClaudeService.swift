import AgentLLM
import AgentAudit
@preconcurrency import Foundation
import AgentTools

@MainActor
final class ClaudeService {
    let apiKey: String
    let model: String
    let endpointURL: URL

    private static let defaultBaseURL = URL(string: "https://api.anthropic.com/v
    private static let apiVersion = "2023-06-01"
    private let isLocalEndpoint: Bool

    // MARK: - Rate Limit Tracking Anthropic 429/529 → capture Retry-After, pad
    private static var retryAfterUntil: CFAbsoluteTime = 0

    /// Wait if needed to respect Retry-After backoff from a previous 429/529.
    private static func enforceRateLimit() async {
        let now = CFAbsoluteTimeGetCurrent()
        if retryAfterUntil > now {
            let wait = retryAfterUntil - now
            try? await Task.sleep(for: .seconds(wait))
        }
    }

    /// Record Retry-After from 429/529.
    static func recordRetryAfter(_ seconds: Double) {
        retryAfterUntil = CFAbsoluteTimeGetCurrent() + seconds
    }

    /// Parse Retry-After header.
    nonisolated static func parseRetryAfter(_ headerValue: String?) -> Double {
        guard let v = headerValue?.trimmingCharacters(in: .whitespaces),
              !v.isEmpty,
              let seconds = Double(v) else { return 0 }
        return min(seconds, 300)
    }

    let historyContext: String
    let userHome: String
    let userName: String
    let projectFolder: String
    /// Max output tokens. 0 = use default
    let maxTokens: Int

    init(
        apiKey: String,
        model: String,
        historyContext: String = "",
        projectFolder: String = "",
        baseURL: String? = nil,
        maxTokens: Int = 0
    ) {
        self.apiKey = apiKey
        self.model = model
        self.endpointURL = baseURL.flatMap { URL(string: $0) } ?? Self.defaultBaseURL
        self.isLocalEndpoint = baseURL != nil
        self.maxTokens = maxTokens
        self.historyContext = historyContext
        self.userHome = FileManager.default.homeDirectoryForCurrentUser.path
        self.userName = NSUserName()
        self.projectFolder = projectFolder
    }

    /// When set, overrides the full system prompt
    var overrideSystemPrompt: String?

    var systemPrompt: String {
        if let override = overrideSystemPrompt { return override }
        if isLocalEndpoint {
            // Local Claude-protocol endpoints
            return SystemPromptService.wrapWithRules(
                AgentTools.compactSystemPrompt(userName: userName, userHome: userHome, projectFolder: projectFolder)
            )
        }
        var prompt = SystemPromptService.shared.prompt(for: .claude, userName: userName, userHome: userHome, projectFolder: projectFolder)
        if !projectFolder.isEmpty {
            prompt =
                "CURRENT PROJECT FOLDER: \(projectFolder)\n"
                    + "Always cd to this directory before running any "
                    + "shell commands. Use it as the default for all file "
                    + "operations. You may go outside it when needed.\n\n" +
                prompt
        }
        if !historyContext.isEmpty {
            prompt += historyContext
        }
        prompt += MemoryStore.shared.contextBlock
        return prompt
    }

    func tools(activeGroups: Set<String>? = nil, compact: Bool = false) -> [[String: Any]] {
        // No mode-based narrowing — every user-enabled tool flows through. Loca
        var t = AgentTools.claudeFormat(activeGroups: activeGroups, compact: compact, projectFolder: projectFolder)
        // Only add native web_search for real Anthropic API
        if !isLocalEndpoint {
            t.removeAll { ($0["name"] as? String) == "web_search" }
            t.append([
                "type": "web_search_20250305",
                "name": "web_search"
            ])
        }
        return t
    }

    /// Prepend project folder to the last user message so it's always visible i
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

    var temperature: Double = 0.2
    var compactTools: Bool = false

    func send(
        messages: [[String: Any]],
        activeGroups: Set<String>? = nil
    ) async throws
        -> (content: [[String: Any]], stopReason: String, inputTokens: Int, outputTokens: Int)
    {
        guard isLocalEndpoint || !apiKey.isEmpty else { throw AgentError.noAPIKey }
        await Self.enforceRateLimit()

        // Use structured system prompt with cache_control for prompt caching
        let systemBlock: Any = isLocalEndpoint ? systemPrompt : [
            ["type": "text", "text": systemPrompt, "cache_control": ["type": "ephemeral"]]
        ]

        var body: [String: Any] = [
            "model": model,
            "max_tokens": maxTokens > 0 ? maxTokens : 16384,
            "temperature": temperature,
            "system": systemBlock,
            "messages": withFolderPrefix(messages)
        ]
        // Only include tools for real Anthropic API
        if !isLocalEndpoint {
            var toolDefs = tools(activeGroups: activeGroups, compact: compactTools)
            // Mark last tool with cache_control for prompt caching
            if !toolDefs.isEmpty {
                toolDefs[toolDefs.count - 1]["cache_control"] = ["type": "ephemeral"]
            }
            body["tools"] = toolDefs
        }

        // Serialize on main actor, then offload network I/O + parsing. .sortedK
        let bodyData = try JSONSerialization.data(withJSONObject: body, options: [.sortedKeys])
        return try await Self.performRequest(
            bodyData: bodyData,
            apiKey: apiKey,
            apiVersion: Self.apiVersion,
            url: endpointURL
        )
    }

    /// Network I/O and response parsing off the main thread
    nonisolated private static func performRequest(
        bodyData: Data, apiKey: String, apiVersion: String, url: URL
    ) async throws -> (content: [[String: Any]], stopReason: String, inputTokens: Int, outputTokens: Int) {
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue(apiKey, forHTTPHeaderField: "x-api-key")
        request.setValue(apiVersion, forHTTPHeaderField: "anthropic-version")
        request.setValue("prompt-caching-2024-07-31", forHTTPHeaderField: "anthropic-beta")
        request.setValue("application/json", forHTTPHeaderField: "content-type")
        request.httpBody = bodyData
        request.timeoutInterval = llmAPITimeout

        let (data, response) = try await URLSession.shared.data(for: request)

        guard let httpResponse = response as? HTTPURLResponse else {
            throw AgentError.invalidResponse
        }

        guard httpResponse.statusCode == 200 else {
            // 429 = rate limit, 529 = Anthropic "Overloaded".
            if httpResponse.statusCode == 429 || httpResponse.statusCode == 529 {
                let header = httpResponse.value(forHTTPHeaderField: "Retry-After")
                let parsed = Self.parseRetryAfter(header)
                let waitSeconds = parsed > 0 ? parsed : 30
                await MainActor.run {
                    Self.recordRetryAfter(waitSeconds)
                }
            }
            let errorBody = String(data: data, encoding: .utf8) ?? "Unknown error"
            throw AgentError.apiError(statusCode: httpResponse.statusCode, message: errorBody)
        }

        guard let json = try JSONSerialization.jsonObject(with: data) as? [String: Any],
              let content = json["content"] as? [[String: Any]],
              let stopReason = json["stop_reason"] as? String else
        {
            throw AgentError.invalidResponse
        }

        let usage = json["usage"] as? [String: Any]
        let inputTokens = usage?["input_tokens"] as? Int ?? 0
        let outputTokens = usage?["output_tokens"] as? Int ?? 0

        return (content, stopReason, inputTokens, outputTokens)
    }

    // MARK: - Streaming

    func sendStreaming(
        messages: [[String: Any]],
        activeGroups: Set<String>? = nil,
        onTextDelta: @escaping @Sendable (String) -> Void
    ) async throws -> (content: [[String: Any]], stopReason: String, inputTokens: Int, outputTokens: Int) {
        guard isLocalEndpoint || !apiKey.isEmpty else { throw AgentError.noAPIKey }
        await Self.enforceRateLimit()

        let systemBlock: Any = isLocalEndpoint ? systemPrompt : [
            ["type": "text", "text": systemPrompt, "cache_control": ["type": "ephemeral"]]
        ]

        var body: [String: Any] = [
            "model": model,
            "max_tokens": maxTokens > 0 ? maxTokens : 16384,
            "system": systemBlock,
            "messages": withFolderPrefix(messages),
            "stream": true
        ]
        if !isLocalEndpoint {
            var toolDefs = tools(activeGroups: activeGroups, compact: compactTools)
            if !toolDefs.isEmpty {
                toolDefs[toolDefs.count - 1]["cache_control"] = ["type": "ephemeral"]
            }
            body["tools"] = toolDefs
        }

        // .sortedKeys for byte-stable prefix caching — see send() for rationale
        let bodyData = try JSONSerialization.data(withJSONObject: body, options: [.sortedKeys])
        return try await Self.performStreamingRequest(
            bodyData: bodyData,
            apiKey: apiKey,
            apiVersion: Self.apiVersion,
            url: endpointURL,
            onTextDelta: onTextDelta
        )
    }

    nonisolated private static func performStreamingRequest(
        bodyData: Data, apiKey: String, apiVersion: String, url: URL,
        onTextDelta: @escaping @Sendable (String) -> Void
    ) async throws -> (content: [[String: Any]], stopReason: String, inputTokens: Int, outputTokens: Int) {
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue(apiKey, forHTTPHeaderField: "x-api-key")
        request.setValue(apiVersion, forHTTPHeaderField: "anthropic-version")
        request.setValue("prompt-caching-2024-07-31", forHTTPHeaderField: "anthropic-beta")
        request.setValue("application/json", forHTTPHeaderField: "content-type")
        request.httpBody = bodyData
        request.timeoutInterval = llmAPITimeout

        let (bytes, response) = try await URLSession.shared.bytes(for: request)

        guard let httpResponse = response as? HTTPURLResponse else {
            throw AgentError.invalidResponse
        }

        guard httpResponse.statusCode == 200 else {
            // 429/529 Retry-After capture — see performRequest for rationale.
            if httpResponse.statusCode == 429 || httpResponse.statusCode == 529 {
                let header = httpResponse.value(forHTTPHeaderField: "Retry-After")
                let parsed = Self.parseRetryAfter(header)
                let waitSeconds = parsed > 0 ? parsed : 30
                await MainActor.run {
                    Self.recordRetryAfter(waitSeconds)
                }
            }
            var errorData = Data()
            for try await byte in bytes {
                errorData.append(byte)
            }
            let errorBody = String(data: errorData, encoding: .utf8) ?? "Unknown error"
            throw AgentError.apiError(statusCode: httpResponse.statusCode, message: errorBody)
        }

        var contentBlocks: [[String: Any]] = []
        var currentTextBlock = ""
        var currentToolId = ""
        var currentToolName = ""
        var currentToolJson = ""
        var stopReason = ""
        var inToolUse = false
        var inServerToolUse = false
        var pendingServerResult: [String: Any]?
        var inputTokens = 0
        var outputTokens = 0

        for try await line in bytes.lines {
            guard line.hasPrefix("data: ") else { continue }
            let jsonStr = String(line.dropFirst(6))
            guard let data = jsonStr.data(using: .utf8),
                  let event = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
                  let type = event["type"] as? String else { continue }

            switch type {
            case "message_start":
                if let message = event["message"] as? [String: Any],
                   let usage = message["usage"] as? [String: Any]
                {
                    inputTokens = usage["input_tokens"] as? Int ?? 0
                    // Track prompt cache metrics
                    let cacheRead = usage["cache_read_input_tokens"] as? Int ?? 0
                    let cacheCreation = usage["cache_creation_input_tokens"] as? Int ?? 0
                    if cacheRead > 0 || cacheCreation > 0 {
                        Task { @MainActor in
                            TokenUsageStore.shared.recordCacheMetrics(read: cacheRead, creation: cacheCreation)
                        }
                    }
                }

            case "content_block_start":
                if let block = event["content_block"] as? [String: Any],
                   let blockType = block["type"] as? String
                {
                    if blockType == "text" {
                        currentTextBlock = ""
                        inToolUse = false
                        inServerToolUse = false
                    } else if blockType == "tool_use" {
                        currentToolId = block["id"] as? String ?? ""
                        currentToolName = block["name"] as? String ?? ""
                        currentToolJson = ""
                        inToolUse = true
                        inServerToolUse = false
                    } else if blockType == "server_tool_use" {
                        currentToolId = block["id"] as? String ?? ""
                        currentToolName = block["name"] as? String ?? ""
                        currentToolJson = ""
                        inToolUse = true
                        inServerToolUse = true
                    } else if blockType == "web_search_tool_result" {
                        pendingServerResult = block
                    }
                }

            case "content_block_delta":
                if let delta = event["delta"] as? [String: Any],
                   let deltaType = delta["type"] as? String
                {
                    if deltaType == "text_delta", let text = delta["text"] as? String {
                        currentTextBlock += text
                        onTextDelta(text)
                    } else if deltaType == "input_json_delta", let json = delta["partial_json"] as? String {
                        currentToolJson += json
                    }
                }

            case "content_block_stop":
                if inToolUse {
                    let input: [String: Any]
                    if let parsed = try? JSONSerialization.jsonObject(with: Data(currentToolJson.utf8)) as? [String: Any] {
                        input = parsed
                    } else {
                        AuditLog.log(
                            .api,
                            "[ClaudeService] Failed to parse tool args for \(currentToolName): \(currentToolJson.prefix(200))"
                        )
                        input = [:]
                    }
                    let blockType = inServerToolUse ? "server_tool_use" : "tool_use"
                    contentBlocks.append([
                        "type": blockType,
                        "id": currentToolId,
                        "name": currentToolName,
                        "input": input
                    ])
                    currentToolName = ""
                    currentToolId = ""
                    currentToolJson = ""
                    inToolUse = false
                    inServerToolUse = false
                } else if let result = pendingServerResult {
                    contentBlocks.append(result)
                    pendingServerResult = nil
                } else if !currentTextBlock.isEmpty {
                    contentBlocks.append(["type": "text", "text": currentTextBlock])
                    currentTextBlock = ""
                }

            case "message_delta":
                if let delta = event["delta"] as? [String: Any],
                   let reason = delta["stop_reason"] as? String
                {
                    stopReason = reason
                }
                if let usage = event["usage"] as? [String: Any] {
                    outputTokens = usage["output_tokens"] as? Int ?? outputTokens
                }

            default:
                break
            }
        }

        return (contentBlocks, stopReason, inputTokens, outputTokens)
    }
}
