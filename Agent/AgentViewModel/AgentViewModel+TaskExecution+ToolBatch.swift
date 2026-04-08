
@preconcurrency import Foundation
import AgentTools
import AgentMCP
import AgentD1F
import AgentSwift
import Cocoa

// MARK: - Task Execution — Tool Batch Dispatch + Vision Verification

extension AgentViewModel {

    /// Executes a set of pending tool calls from a single LLM turn.
    ///
    /// Consecutive read-only tools are partitioned into parallel batches that
    /// pre-execute common shell reads off the main actor via a TaskGroup; the
    /// results are stashed into `Self.precomputedResults` so the subsequent
    /// `dispatchTool` calls return instantly. Write/mutating tools serialize.
    /// Appends the tool results to `toolResults` via inout.
    func executePendingToolBatches(
        pendingTools: [(toolId: String, name: String, input: [String: Any])],
        toolResults: inout [[String: Any]]
    ) async {
        if !pendingTools.isEmpty {
            let maxConcurrency = 10
            // Partition into batches: consecutive read-only = parallel batch, write = serial batch
            var batches: [(parallel: Bool, tools: [(toolId: String, name: String, input: [String: Any])])] = []
            for tool in pendingTools {
                let isReadOnly = Self.readOnlyTools.contains(tool.name)
                if isReadOnly, let last = batches.last, last.parallel {
                    batches[batches.count - 1].tools.append(tool)
                } else {
                    batches.append((parallel: isReadOnly, tools: [tool]))
                }
            }

            for batch in batches {
                if batch.parallel && batch.tools.count > 1 {
                    // Parallel batch: pre-execute shell tools off MainActor
                    let shellTools: Set<String> = [
                        "read_file",
                        "list_files",
                        "search_files",
                        "read_dir",
                        "git_status",
                        "git_diff",
                        "git_log",
                        "git_diff_patch"
                    ]
                    let shellBatch = batch.tools.filter { shellTools.contains($0.name) }
                    if shellBatch.count > 1 {
                        let capturedPF = projectFolder
                        let cmds = shellBatch.map { (
                            $0.toolId,
                            Self.buildReadOnlyCommand(name: $0.name, input: $0.input, projectFolder: capturedPF)
                        ) }
                        var preResults: [String: String] = [:]
                        await withTaskGroup(of: (String, String).self) { group in
                            for (i, (id, cmd)) in cmds.enumerated() where i < maxConcurrency {
                                let cid = id; let ccmd = cmd
                                let workDir = capturedPF.isEmpty ? NSHomeDirectory() : capturedPF
                                group.addTask {
                                    guard !ccmd.isEmpty else { return (cid, "") }
                                    let pipe = Pipe(); let p = Process()
                                    p.executableURL = URL(fileURLWithPath: "/bin/zsh")
                                    p.arguments = ["-c", ccmd]
                                    p.currentDirectoryURL = URL(fileURLWithPath: workDir)
                                    var env = ProcessInfo.processInfo.environment
                                    env["HOME"] = NSHomeDirectory()
                                    // Match the AGENT_PROJECT_FOLDER contract used by every other
                                    // shell-execution path (executeTCC, UserService, HelperService).
                                    env["AGENT_PROJECT_FOLDER"] = workDir
                                    env["PATH"] = "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin:" +
                                        (env["PATH"] ?? "")
                                    p.environment = env; p.standardOutput = pipe; p.standardError = pipe
                                    try? p.run(); p.waitUntilExit()
                                    return (
                                        cid,
                                        String(data: pipe.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8) ?? ""
                                    )
                                }
                            }
                            for await (id, result) in group { preResults[id] = result }
                        }
                        Self.precomputedResults = preResults
                    }
                    for tool in batch.tools {
                        let ctx = ToolContext(
                            toolId: tool.toolId,
                            projectFolder: projectFolder,
                            selectedProvider: selectedProvider,
                            tavilyAPIKey: tavilyAPIKey
                        )
                        _ = await dispatchTool(name: tool.name, input: tool.input, ctx: ctx, toolResults: &toolResults)
                    }
                    Self.precomputedResults = nil
                } else {
                    // Serial batch: execute one by one
                    for tool in batch.tools {
                        let ctx = ToolContext(
                            toolId: tool.toolId,
                            projectFolder: projectFolder,
                            selectedProvider: selectedProvider,
                            tavilyAPIKey: tavilyAPIKey
                        )
                        _ = await dispatchTool(name: tool.name, input: tool.input, ctx: ctx, toolResults: &toolResults)
                    }
                }
            }
        }
    }

    /// Vision verification: auto-screenshot after UI actions so the LLM can see the result.
    /// OPT-IN via `visionAutoScreenshotEnabled` (Settings → Vision Auto-Screenshot).
    /// Default OFF because it (1) hogs the main thread on every UI iteration,
    /// (2) bloats every prompt with a base64 image even for non-vision models,
    /// and (3) the next accessibility(find_element) query usually tells the LLM
    /// what happened just as well, without the screenshot cost.
    func runVisionAutoScreenshotIfNeeded(
        pendingTools: [(toolId: String, name: String, input: [String: Any])],
        isVision: Bool,
        toolResults: inout [[String: Any]]
    ) async {
        if visionAutoScreenshotEnabled && isVision && !pendingTools.isEmpty {
            let uiActions: Set<String> = [
                "ax_click",
                "ax_click_element",
                "ax_perform_action",
                "ax_type_text",
                "ax_type_into_element",
                "ax_open_app",
                "ax_scroll",
                "ax_drag",
                "click",
                "click_element",
                "perform_action",
                "type_text",
                "open_app",
                "web_click",
                "web_type",
                "web_navigate"
            ]
            let hadUIAction = pendingTools.contains { uiActions.contains($0.name) }
            if hadUIAction {
                let screenshotResult = await Self.captureVerificationScreenshot()
                if let imageData = screenshotResult {
                    // Append screenshot as image content block to tool results
                    toolResults.append([
                        "type": "tool_result",
                        "tool_use_id": "vision_verify",
                        "content": [
                            ["type": "text", "text": "[Auto-screenshot after UI action — verify the action succeeded]"],
                            ["type": "image", "source": ["type": "base64", "media_type": "image/png", "data": imageData]]
                        ]
                    ])
                    appendLog("📸 Vision: auto-screenshot for verification")
                }
            }
        }
    }
}
