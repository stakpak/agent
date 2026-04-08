@preconcurrency import Foundation
import AppKit
import Speech

extension AgentViewModel {
    // MARK: - Tool Step Recording

    /// Record a tool step starting. Returns the step ID for later completion.
    @discardableResult
    func recordToolStep(name: String, detail: String) -> UUID {
        let step = ToolStep(name: name, detail: detail, startTime: Date())
        toolSteps.append(step)
        return step.id
    }

    /// Mark a tool step as completed.
    func completeToolStep(id: UUID, status: ToolStep.Status = .success) {
        if let idx = toolSteps.firstIndex(where: { $0.id == id }) {
            toolSteps[idx].duration = Date().timeIntervalSince(toolSteps[idx].startTime)
            toolSteps[idx].status = status
        }
    }

    // MARK: - Agent Failure Notification

    /// Call when an agent fails — triggers the remove-from-menu alert.
    /// Finds the most recent matching entry by name and stores its UUID for exact removal.
    func notifyAgentFailed(name: String, arguments: String) {
        if let entry = RecentAgentsService.shared.entries.first(where: { $0.agentName == name && $0.arguments == arguments })
            ?? RecentAgentsService.shared.entries.first(where: { $0.agentName == name })
        {
            failedAgentName = name
            failedAgentId = entry.id
            showFailedAgentAlert = true
        }
    }

    // MARK: - Service Group Sync

    /// Keep service tool groups and individual tools in sync with userEnabled/rootEnabled.
    func syncServicesGroup() {
        let prefs = ToolPreferencesService.shared
        // Sync User Agent group
        let agentGroupOn = prefs.isGroupEnabled(Tool.Group.user)
        if userEnabled != agentGroupOn { prefs.toggleGroup(Tool.Group.user) }
        // Sync individual tool — re-enable if service turned on
        if userEnabled && !prefs.isEnabled(selectedProvider, "execute_agent_command") {
            prefs.toggle(selectedProvider, "execute_agent_command")
        }
        // Sync Launch Daemon group
        let daemonGroupOn = prefs.isGroupEnabled(Tool.Group.root)
        if rootEnabled != daemonGroupOn { prefs.toggleGroup(Tool.Group.root) }
        // Sync individual tool — re-enable if service turned on
        if rootEnabled && !prefs.isEnabled(selectedProvider, "execute_daemon_command") {
            prefs.toggle(selectedProvider, "execute_daemon_command")
        }
    }

    // MARK: - Small computed convenience

    var daemonReady: Bool { helperService.helperReady }
    var agentReady: Bool { userService.userReady }
    var hasAttachments: Bool { !attachedImages.isEmpty }

    var selectedOllamaSupportsVision: Bool {
        ollamaModels.first(where: { $0.name == ollamaModel })?.supportsVision ?? false
    }

    var selectedLocalOllamaSupportsVision: Bool {
        localOllamaModels.first(where: { $0.name == localOllamaModel })?.supportsVision ?? false
    }

    /// Check if speech recognition is authorized
    var isSpeechRecognitionAuthorized: Bool {
        SFSpeechRecognizer.authorizationStatus() == .authorized
    }

    // MARK: - Ollama Pre-warm

    /// Send a tiny request to Ollama to load the model into memory.
    /// This eliminates the 5-15s cold-start delay on the first task.
    func preWarmOllama() async {
        let provider = selectedProvider
        guard provider == .ollama || provider == .localOllama else { return }
        let endpoint = provider == .ollama ? ollamaEndpoint : localOllamaEndpoint
        let model = provider == .ollama ? ollamaModel : localOllamaModel
        guard !model.isEmpty else { return }

        let chatURL = endpoint.isEmpty ? "http://localhost:11434/api/chat" : endpoint
        guard let url = URL(string: chatURL) else { return }

        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.timeoutInterval = 30
        let body: [String: Any] = [
            "model": model,
            "messages": [["role": "user", "content": "hi"]],
            "stream": false,
            "options": ["num_predict": 1] // Generate just 1 token — enough to load model
        ]
        request.httpBody = try? JSONSerialization.data(withJSONObject: body)

        do {
            let _ = try await URLSession.shared.data(for: request)
            appendLog("⚙️ Ollama: \(model) pre-warmed")
        } catch {
            // Silent fail — model will load on first task instead
        }
    }
}
