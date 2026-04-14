@preconcurrency import Foundation
import AgentTools
import AgentLLM

// MARK: - Sub-Agent Spawning

/// Represents an isolated sub-agent execution with its own message history.
@MainActor
final class SubAgent: Identifiable {
    let id = UUID()
    let name: String
    let prompt: String
    let projectFolder: String
    var toolGroups: Set<String>? // nil = default (Core+Work+Code)
    var maxIterations: Int = 15
    var status: Status = .running
    var result: String = ""
    var task: Task<String, Never>?
    var inputTokens: Int = 0
    var outputTokens: Int = 0
    let startTime = Date()
    /// Mailbox for inter-agent messages. Parent can inject messages mid-task.
    var mailbox: [String] = []

    enum Status: String {
        case running, completed, failed
    }

    init(name: String, prompt: String, projectFolder: String) {
        self.name = name
        self.prompt = prompt
        self.projectFolder = projectFolder
    }

    var duration: TimeInterval {
        Date().timeIntervalSince(startTime)
    }

    /// XML notification for parent context.
    var notification: String {
        """
        <task-notification>
          <task-id>\(id.uuidString.prefix(8))</task-id>
          <name>\(name)</name>
          <status>\(status.rawValue)</status>
          <result>\(LogLimits.trim(result, cap: LogLimits.summaryChars))</result>
          <usage>
            <input_tokens>\(inputTokens)</input_tokens>
            <output_tokens>\(outputTokens)</output_tokens>
            <duration_ms>\(Int(duration * 1000))</duration_ms>
          </usage>
        </task-notification>
        """
    }
}

extension AgentViewModel {

    /// Maximum concurrent sub-agents per task.
    static let maxSubAgents = 3

    /// Active sub-agents for the current task.
    var activeSubAgents: [SubAgent] {
        subAgents.filter { $0.status == .running }
    }

    /// Spawn an isolated sub-agent runs concurrently with parent task.
    func spawnSubAgent(name: String, prompt: String, toolGroups: Set<String>? = nil, maxIterations: Int = 15) -> String {
        guard activeSubAgents.count < Self.maxSubAgents else {
            return "Error: Maximum \(Self.maxSubAgents) concurrent sub-agents reached. Wait for one to complete."
        }

        let agent = SubAgent(name: name, prompt: prompt, projectFolder: projectFolder)
        agent.toolGroups = toolGroups
        agent.maxIterations = maxIterations
        subAgents.append(agent)
        appendLog("🔀 Sub-agent '\(name)' spawned [\(agent.id.uuidString.prefix(8))]")
        flushLog()

        agent.task = Task { [weak self] in
            guard let self else { return "Error: parent deallocated" }
            let result = await self.executeSubAgent(agent)
            return result
        }

        return
            "Sub-agent '\(name)' spawned "
            + "(id: \(agent.id.uuidString.prefix(8))). "
            + "You will receive a <task-notification> when it completes."
    }

    /// Execute a sub-agent's task in isolation using the current provider/model
    private func executeSubAgent(_ agent: SubAgent) async -> String {
        let provider = selectedProvider
        let modelName = globalModelForProvider(provider)
        let mt = maxTokens

        // Build a minimal service for this sub-agent
        let historyContext = "" // Sub-agents start with clean context
        let claude: ClaudeService?
        if provider == .claude {
            claude = ClaudeService(
                apiKey: apiKey,
                model: selectedModel,
                historyContext: historyContext,
                projectFolder: agent.projectFolder,
                maxTokens: mt
            )
        } else if provider == .lmStudio && lmStudioProtocol == .anthropic {
            claude = ClaudeService(
                apiKey: lmStudioAPIKey,
                model: lmStudioModel,
                historyContext: historyContext,
                projectFolder: agent.projectFolder,
                baseURL: lmStudioEndpoint,
                maxTokens: mt
            )
        } else {
            claude = nil
        }
        let openAICompatible: OpenAICompatibleService?
        switch provider {
        case .claude, .ollama, .localOllama, .foundationModel:
            openAICompatible = nil
        case .lmStudio where lmStudioProtocol == .anthropic:
            openAICompatible = nil
        case .vLLM:
            openAICompatible = OpenAICompatibleService(
                apiKey: apiKeyForProvider(provider), model: modelName,
                baseURL: vLLMEndpoint, historyContext: historyContext,
                projectFolder: agent.projectFolder, provider: provider,
                maxTokens: mt
            )
        default:
            let url = chatURLForProvider(provider)
            openAICompatible = url.isEmpty ? nil : OpenAICompatibleService(
                apiKey: apiKeyForProvider(provider), model: modelName,
                baseURL: url, historyContext: historyContext,
                projectFolder: agent.projectFolder, provider: provider,
                maxTokens: mt
            )
        }
        let ollama: OllamaService?
        switch provider {
        case .ollama:
            ollama = OllamaService(
                apiKey: ollamaAPIKey, model: ollamaModel,
                endpoint: ollamaEndpoint, historyContext: historyContext,
                projectFolder: agent.projectFolder, provider: .ollama
            )
        case .localOllama:
            ollama = OllamaService(
                apiKey: "", model: localOllamaModel,
                endpoint: localOllamaEndpoint, historyContext: historyContext,
                projectFolder: agent.projectFolder, provider: .localOllama,
                contextSize: localOllamaContextSize
            )
        default:
            ollama = nil
        }

        // Set temperature
        claude?.temperature = temperatureForProvider(.claude)
        ollama?.temperature = temperatureForProvider(provider)
        openAICompatible?.temperature = temperatureForProvider(provider)

        // Sub-agent tool groups
        let activeGroups: Set<String> = agent.toolGroups ?? [Tool.Group.core, Tool.Group.work, Tool.Group.code]

        var messages: [[String: Any]] = [
            ["role": "user", "content": agent.prompt]
        ]

        var iterations = 0
        let maxIterations = agent.maxIterations
        var finalResult = ""

        while !Task.isCancelled && iterations < maxIterations {
            iterations += 1

            do {
                let response: (content: [[String: Any]], stopReason: String, inputTokens: Int, outputTokens: Int)
                if let claude {
                    response = try await claude.sendStreaming(messages: messages, activeGroups: activeGroups) { _ in }
                } else if let openAICompatible {
                    let r = try await openAICompatible.sendStreaming(messages: messages, activeGroups: activeGroups) { _ in }
                    response = (r.content, r.stopReason, r.inputTokens, r.outputTokens)
                } else if let ollama {
                    let r = try await ollama.sendStreaming(messages: messages, activeGroups: activeGroups) { _ in }
                    response = (r.content, r.stopReason, r.inputTokens, r.outputTokens)
                } else {
                    agent.status = .failed
                    agent.result = "No LLM service available"
                    return agent.notification
                }

                agent.inputTokens += response.inputTokens
                agent.outputTokens += response.outputTokens

                var toolResults: [[String: Any]] = []
                var hasToolUse = false

                for block in response.content {
                    guard let type = block["type"] as? String else { continue }
                    if type == "text", let text = block["text"] as? String {
                        finalResult = text
                    } else if type == "tool_use" {
                        hasToolUse = true
                        guard let toolId = block["id"] as? String,
                              var name = block["name"] as? String,
                              var input = block["input"] as? [String: Any] else { continue }

                        (name, input) = Self.expandConsolidatedTool(name: name, input: input)

                        if name == "task_complete" {
                            finalResult = input["summary"] as? String ?? finalResult
                            break
                        }

                        // Execute tool (sub-agent shares parent's dispatch)
                        let ctx = ToolContext(
                            toolId: toolId,
                            projectFolder: agent.projectFolder,
                            selectedProvider: selectedProvider,
                            tavilyAPIKey: tavilyAPIKey
                        )
                        var results: [[String: Any]] = []
                        _ = await dispatchTool(name: name, input: input, ctx: ctx, toolResults: &results)
                        toolResults.append(contentsOf: results)
                    }
                }

                let assistantContent: Any = response.content.isEmpty ? "Continuing." as Any : response.content as Any
                messages.append(["role": "assistant", "content": assistantContent])

                // Check mailbox for messages from parent/other agents
                if !agent.mailbox.isEmpty {
                    let incoming = agent.mailbox.joined(separator: "\n")
                    agent.mailbox.removeAll()
                    toolResults.append([
                        "type": "tool_result",
                        "tool_use_id": "agent_message",
                        "content": "<message from coordinator>\n\(incoming)\n</message>"
                    ])
                }

                if hasToolUse && !toolResults.isEmpty {
                    let capped = Self.truncateToolResults(toolResults)
                    messages.append(["role": "user", "content": capped])
                } else if !hasToolUse {
                    break // Text-only response = done
                }

            } catch {
                agent.status = .failed
                agent.result = "Error: \(error.localizedDescription)"
                appendLog("🔀 Sub-agent '\(agent.name)' failed: \(error.localizedDescription)")
                flushLog()
                return agent.notification
            }
        }

        agent.status = .completed
        agent.result = LogLimits.trim(finalResult, cap: LogLimits.summaryChars)
        appendLog(
            "🔀 Sub-agent '\(agent.name)' completed "
                + "(\(agent.inputTokens + agent.outputTokens) tokens, "
                + "\(String(format: "%.1f", agent.duration))s)"
        )
        flushLog()
        return agent.notification
    }

    /// Send a message to a running sub-agent by name. Returns status.
    func sendMessageToAgent(name: String, message: String) -> String {
        guard let agent = subAgents.first(where: { $0.name == name && $0.status == .running }) else {
            // Try by ID prefix
            if let agent = subAgents.first(where: { $0.id.uuidString.hasPrefix(name) && $0.status == .running }) {
                agent.mailbox.append(message)
                return "Message delivered to '\(agent.name)'."
            }
            return "Error: No running sub-agent named '\(name)'. Active agents: \(activeSubAgents.map(\.name).joined(separator: ", "))"
        }
        agent.mailbox.append(message)
        return "Message delivered to '\(agent.name)'."
    }

    /// Collect notifications from completed sub-agents and clear them.
    func collectSubAgentNotifications() -> [String] {
        let completed = subAgents.filter { $0.status != .running }
        let notifications = completed.map(\.notification)
        subAgents.removeAll { $0.status != .running }
        return notifications
    }
}
