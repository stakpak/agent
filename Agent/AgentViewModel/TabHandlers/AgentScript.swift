            if tab.isMainTab {
                // ── Main tab: spawn a separate background tab so main tab stays free ──
                // Dedup: close any existing background tab for this script before spawning a fresh one.
                if let existing = scriptTabs.first(where: { $0.scriptName == scriptName && !$0.isMainTab && $0.id != tab.id }) {
                    closeScriptTab(id: existing.id)
                }
                let spawnedTab = openScriptTab(scriptName: scriptName, selectTab: false)
                spawnedTab.projectFolder = tab.projectFolder
                spawnedTab.isRunning = true
                spawnedTab.appendLog("🦾 Spawned from \(tab.scriptName)")
                spawnedTab.flush()
                RecentAgentsService.shared.recordRun(agentName: scriptName, arguments: arguments, prompt: "run \(scriptName) \(arguments)")

                let stderrCapture = scriptCaptureStderr
                Task { [weak self, weak spawnedTab] in
                    guard let self, let spawnedTab else { return }

                    if await Self.offMain({ [ss = self.scriptService] in !ss.isDylibCurrent(name: scriptName) }) {
                        await MainActor.run {
                            spawnedTab.appendLog("🦾 Compiling: \(scriptName)")
                            spawnedTab.flush()
                        }
                        let compileResult = await Self.executeTCC(command: compileCmd)
                        if compileResult.status != 0 {
                            await MainActor.run {
                                spawnedTab.appendLog("❌ Compile failed (exit code: \(compileResult.status))")
                                spawnedTab.appendOutput(compileResult.output)
                                spawnedTab.isRunning = false
                                spawnedTab.exitCode = compileResult.status
                                spawnedTab.flush()
                            }
                            RecentAgentsService.shared.updateStatus(agentName: scriptName, arguments: arguments, status: .failed)
                            return
                        }
                    }

                    await MainActor.run {
                        spawnedTab.appendLog("🦾 Running: \(scriptName)")
                        spawnedTab.flush()
                    }

                    let cancelFlag = spawnedTab._cancelFlag
                    let runResult = await self.scriptService.loadAndRunScriptViaProcess(
                        name: scriptName,
                        arguments: arguments,
                        projectFolder: spawnedTab.projectFolder,
                        captureStderr: stderrCapture,
                        isCancelled: { cancelFlag.value }
                    ) { [weak spawnedTab] chunk in
                        Task { @MainActor in
                            spawnedTab?.appendOutput(chunk)
                        }
                    }

                    let isUsageOutput = runResult.output.trimmingCharacters(in: .whitespacesAndNewlines).hasPrefix("Usage:")
                    let statusNote = runResult.status == 0 ? "completed" : (isUsageOutput ? "usage" : "exit code: \(runResult.status)")
                    let wasCancelled = await MainActor.run { spawnedTab.isCancelled } || runResult.status == 15

                    await MainActor.run {
                        spawnedTab.isRunning = false
                        spawnedTab.exitCode = runResult.status
                        spawnedTab.appendLog("\(scriptName) \(statusNote)")
                        spawnedTab.flush()
                    }

                    if wasCancelled {
                        RecentAgentsService.shared.updateStatus(agentName: scriptName, arguments: arguments, status: .cancelled)
                    } else if isUsageOutput || runResult.status != 0 {
                        RecentAgentsService.shared.updateStatus(agentName: scriptName, arguments: arguments, status: .failed)
                    } else {
                        RecentAgentsService.shared.updateStatus(agentName: scriptName, arguments: arguments, status: .success)
                    }
                }

                tab.appendLog("🚀 Started '\(scriptName)' in background tab")
                tab.flush()
                let toolOutput = "🚀 Started '\(scriptName)' in background script tab '\(scriptName)'. Output streams to that tab — switch to it to monitor progress. The current task continues."
                return TabToolResult(
                    toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": toolOutput],
                    isComplete: false
                )
            } else {
                // ── Script tab (non-main): run on the same tab ──
                tab.isRunning = true
                tab.appendLog("🦾 Running: \(scriptName)")
                tab.flush()
                RecentAgentsService.shared.recordRun(agentName: scriptName, arguments: arguments, prompt: "run \(scriptName) \(arguments)")

                // Compile if needed
                if await Self.offMain({ [ss = scriptService] in !ss.isDylibCurrent(name: scriptName) }) {
                    tab.appendLog("🦾 Compiling: \(scriptName)")
                    tab.flush()
                    let compileResult = await Self.executeTCC(command: compileCmd)
                    if compileResult.status != 0 {
                        tab.appendLog("❌ Compile failed (exit code: \(compileResult.status))")
                        tab.appendOutput(compileResult.output)
                        tab.isRunning = false
                        tab.exitCode = compileResult.status
                        tab.flush()
                        RecentAgentsService.shared.updateStatus(agentName: scriptName, arguments: arguments, status: .failed)
                        let errOutput = "❌ Compile failed: \(compileResult.output)"
                        return TabToolResult(
                            toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": errOutput],
                            isComplete: false
                        )
                    }
                }

                let cancelFlag = tab._cancelFlag
                let runResult = await scriptService.loadAndRunScriptViaProcess(
                    name: scriptName,
                    arguments: arguments,
                    projectFolder: tab.projectFolder,
                    captureStderr: scriptCaptureStderr,
                    isCancelled: { cancelFlag.value }
                ) { [weak tab] chunk in
                    Task { @MainActor in
                        tab?.appendOutput(chunk)
                    }
                }

                let isUsageOutput = runResult.output.trimmingCharacters(in: .whitespacesAndNewlines).hasPrefix("Usage:")
                let statusNote = runResult.status == 0 ? "completed" : (isUsageOutput ? "usage" : "exit code: \(runResult.status)")
                let wasCancelled = tab.isCancelled || runResult.status == 15

                tab.isRunning = false
                tab.exitCode = runResult.status
                tab.appendLog("\(scriptName) \(statusNote)")
                tab.flush()

                if wasCancelled {
                    RecentAgentsService.shared.updateStatus(agentName: scriptName, arguments: arguments, status: .cancelled)
                } else if isUsageOutput || runResult.status != 0 {
                    RecentAgentsService.shared.updateStatus(agentName: scriptName, arguments: arguments, status: .failed)
                } else {
                    RecentAgentsService.shared.updateStatus(agentName: scriptName, arguments: arguments, status: .success)
                }

                return TabToolResult(
                    toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": runResult.output],
                    isComplete: false
                )
            }