import Foundation
import AppKit
import AgentTools

extension AgentViewModel {
    // MARK: - Registration

    func registerDaemon() {
        let msg = helperService.registerHelper()
        appendLog(msg)
    }

    func registerAgent() {
        let msg = userService.registerUser()
        appendLog(msg)
    }

    func unregisterDaemon() {
        helperService.shutdownDaemon()
        daemonPingOK = false
        appendLog("⚙️ Helper daemon unregistered.")
    }

    func unregisterAgent() {
        userService.shutdownAgent()
        userPingOK = false
        appendLog("⚙️ User agent unregistered.")
    }

    func testConnection() {
        appendLog("🔌 Testing connections...")
        Task {
            var userOK = await userService.ping()
            userPingOK = userOK
            appendLog("⚙️ User agent: \(userOK ? "ping OK" : "no response")")
            var daemonOK = await helperService.ping()
            daemonPingOK = daemonOK
            appendLog("⚙️ Launch Daemon: \(daemonOK ? "ping OK" : "no response")")
            if !userOK {
                appendLog("🔄 User agent: mending...")
                _ = userService.restartAgent()
                try? await Task.sleep(nanoseconds: 1_000_000_000)
                userOK = await userService.ping()
                userPingOK = userOK
                appendLog("⚙️ User agent: \(userOK ? "mended — ping OK" : "still NOT responding")")
            }
            if !daemonOK {
                appendLog("🔄 Launch Daemon: mending...")
                _ = helperService.restartDaemon()
                try? await Task.sleep(nanoseconds: 1_000_000_000)
                daemonOK = await helperService.ping()
                daemonPingOK = daemonOK
                appendLog("⚙️ Launch Daemon: \(daemonOK ? "mended — ping OK" : "still NOT responding")")
            }
            if !userOK || !daemonOK {
                appendLog("⚠️ Click Register to restart services")
            }
        }
    }

    // MARK: - Run / Stop

    func run() {
        let task = taskInput.trimmingCharacters(in: .whitespaces)
        guard !task.isEmpty else { return }

        // Handle /clear commands
        if task.lowercased().hasPrefix("/clear") {
            taskInput = ""
            let arg = task.dropFirst(6).trimmingCharacters(in: .whitespaces).lowercased()
            switch arg {
            case "", "log":
                clearSelectedLog()
            case "all":
                clearAll()
            case "llm":
                rawLLMOutput = ""
                if let selId = selectedTabId, let tab = tab(for: selId) {
                    tab.rawLLMOutput = ""
                }
                appendLog("🧹 LLM output cleared.")
                flushLog()
            case "history":
                promptHistory.removeAll()
                UserDefaults.standard.removeObject(forKey: "agentPromptHistory")
                if let selId = selectedTabId, let tab = tab(for: selId) {
                    tab.promptHistory.removeAll()
                }
                appendLog("🧹 Prompt history cleared.")
                flushLog()
            case "tasks":
                history.clearAll()
                appendLog("🧹 Task history cleared.")
                flushLog()
            case "tokens":
                taskInputTokens = 0; taskOutputTokens = 0
                sessionInputTokens = 0; sessionOutputTokens = 0
                appendLog("🧹 Token counts reset.")
                flushLog()
            default:
                appendLog("Usage: /clear [all|log|llm|history|tasks|tokens]")
                flushLog()
            }
            return
        }

        // Handle /memory command — show, edit, or clear memory
        if task.lowercased().hasPrefix("/memory") {
            taskInput = ""
            let arg = task.dropFirst(7).trimmingCharacters(in: .whitespaces)
            if arg.isEmpty || arg.lowercased() == "show" {
                let content = MemoryStore.shared.content
                appendLog("📝 Memory:\n\(content.isEmpty ? "(empty)" : content)")
            } else if arg.lowercased() == "clear" {
                MemoryStore.shared.write("")
                appendLog("📝 Memory cleared.")
            } else if arg.lowercased() == "edit" {
                // Open the memory file in default editor
                let url = URL(fileURLWithPath: NSHomeDirectory() + "/Documents/AgentScript/memory.md")
                NSWorkspace.shared.open(url)
                appendLog("📝 Opened memory.md in editor.")
            } else {
                // Anything else: append to memory
                MemoryStore.shared.append(arg)
                appendLog("📝 Added to memory: \(arg)")
            }
            flushLog()
            return
        }

        // Switch to appropriate LLM tab: current LLM tab, parent LLM tab if on child, or main tab
        ensureLLMTabSelected()

        promptHistory.append(task)
        UserDefaults.standard.set(promptHistory, forKey: "agentPromptHistory")
        // Sync to selected tab so arrow keys work — seed from viewModel if tab is empty
        if let selectedId = selectedTabId,
           let tab = tab(for: selectedId)
        {
            if tab.promptHistory.isEmpty && !promptHistory.isEmpty {
                tab.promptHistory = promptHistory
            } else {
                tab.addToHistory(task)
            }
        }
        historyIndex = -1
        savedInput = ""
        taskInput = ""

        // Queue if already running
        if isRunning {
            mainTaskQueue.append(task)
            appendLog("📋 Queued (\(mainTaskQueue.count)): \(task)")
            flushLog()
            return
        }

        startMainTask(task)
    }

    /// Start executing a task on the main tab. If a previous task is still draining
    /// (retry loop or in-flight HTTP), waits for it to fully terminate first — otherwise
    /// both loops write to the same activityLog producing garbled output.
    private func startMainTask(_ task: String) {
        let previousTask = runningTask
        runningTask = Task {
            // Drain any previous main task before starting this one. cancel()
            // is idempotent — stop() may have already called it, this just
            // ensures it. Then await the previous task's value so we know it
            // has fully exited its loop, including any in-flight HTTP request
            // and any catch-block log lines.
            if let previous = previousTask {
                previous.cancel()
                _ = await previous.value
            }
            // Reset cancellation flag AFTER the previous task has fully exited.
            // Setting it before would let the previous task think it's no
            // longer cancelled if it polls vm.isCancelled (some loops do).
            isCancelled = false
            currentTaskPrompt = task
            ChatHistoryStore.shared.startNewTask(prompt: task)

            await executeTask(task)
            // When done, run next queued task
            if !mainTaskQueue.isEmpty && !isCancelled {
                let next = mainTaskQueue.removeFirst()
                startMainTask(next)
            }
        }
    }

    /// Navigate prompt history. direction: -1 = older (up arrow), 1 = newer (down arrow)
    func navigatePromptHistory(direction: Int) {
        guard !currentTabPromptHistory.isEmpty else { return }

        if historyIndex == -1 {
            // Starting to browse — save current input
            savedInput = taskInput
            if direction == -1 {
                historyIndex = currentTabPromptHistory.count - 1
            } else {
                return // already at the beginning, nothing newer
            }
        } else {
            historyIndex += direction
        }

        if historyIndex < 0 {
            // Went past the oldest — restore saved input
            historyIndex = -1
            taskInput = savedInput
            return
        }

        if historyIndex >= currentTabPromptHistory.count {
            // Back to current input
            historyIndex = -1
            taskInput = savedInput
            return
        }

        taskInput = currentTabPromptHistory[historyIndex]
    }

    func stop(silent: Bool = false) {
        let queueCount = mainTaskQueue.count
        mainTaskQueue.removeAll()
        isCancelled = true
        runningTask?.cancel()
        runningTask = nil
        helperService.cancel()
        helperService.onOutput = nil
        // Don't cancel userService — tabs may be using it for concurrent operations
        userService.onOutput = nil
        // Stop progress updates
        stopProgressUpdates()
        if !silent {
            if queueCount > 0 {
                appendLog("🚫 Cancelled. \(queueCount) queued task(s) cleared.")
            } else {
                appendLog("🚫 Cancelled.")
            }
        }
        flushLog()
        persistLogNow()
        // End the current task in chat history
        ChatHistoryStore.shared.endCurrentTask(cancelled: !silent)
        isRunning = false
        isThinking = false
        currentTaskPrompt = ""
        currentAppleAIPrompt = ""
        userServiceActive = false
        rootServiceActive = false
        userWasActive = false
        rootWasActive = false
    }

    /// Stop everything — main task AND all script tabs.
    func stopAll() {
        stop()
        for tab in scriptTabs {
            // Stop LLM tasks and clear queues
            if tab.isLLMRunning || !tab.taskQueue.isEmpty {
                stopTabTask(tab: tab)
            }
            // Cancel running scripts
            if tab.isRunning {
                tab.isCancelled = true
                tab.cancelHandler?()
                tab.isRunning = false
            }
            tab.logFlushTask?.cancel()
            tab.llmStreamFlushTask?.cancel()
            tab.flush()
        }
        persistScriptTabs()
    }

}
