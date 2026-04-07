
@preconcurrency import Foundation
import AgentTools
import AgentMCP
import AgentD1F
import AgentSwift
import AgentAccess
import Cocoa




// MARK: - Native Tool Handler (Apple AI)

extension AgentViewModel {


    // MARK: - Native Tool Handler (Apple AI)

    /// Executes a tool call from Apple AI's Foundation Models native tool system.
    /// Routes to the same execution logic as TaskExecution tool handlers.
    func executeNativeTool(_ rawName: String, input rawInput: sending [String: Any]) async -> String {
        // Expand consolidated CRUDL tools into legacy tool names
        let (name, input) = Self.expandConsolidatedTool(name: rawName, input: rawInput)
        let pf = projectFolder
        NativeToolContext.toolCallCount += 1

        // Prefix-matched tools
        if let result = await handleWebTool(name: name, input: input) { return result }
        if let result = await handleSeleniumTool(name: name, input: input) { return result }
        // Saved-script CRUDL tools (list/save/delete/run for AppleScript and JXA).
        // expandConsolidatedTool maps applescript(action:list) → list_apple_scripts (etc.),
        // and those leaf names live in handleSavedScriptTool, not the main switch below.
        let savedScriptNames: Set<String> = [
            "list_apple_scripts", "run_apple_script", "save_apple_script", "delete_apple_script",
            "list_javascript", "run_javascript", "save_javascript", "delete_javascript",
        ]
        if savedScriptNames.contains(name) {
            return await handleSavedScriptTool(name: name, input: input)
        }
        // ax_ accessibility tools — already expanded, handle directly via the accessibility switch below
        // (expandConsolidatedTool maps accessibility(action:X) → ax_X, so ax_ names arrive here already expanded)

        switch name {
        // Shell commands
        case "execute_agent_command", "run_shell_script":
            let command = input["command"] as? String ?? ""
            if let suggestion = Self.suggestTool(command) { return suggestion }
            if let pathErr = Self.preflightCommand(command) { return pathErr }
            appendLog("🔧 $ \(Self.collapseHeredocs(command))")
            flushLog()
            if Self.needsTCCPermissions(command) {
                let result = await Self.executeTCCStreaming(command: command, workingDirectory: pf) { [weak self] chunk in
                    Task { @MainActor in self?.appendRawOutput(chunk) }
                }
                if result.status > 0 { appendLog("exit code: \(result.status)") }
                flushLog()
                return result.output.isEmpty ? "(no output, exit \(result.status))" : result.output
            }
            let result = await executeViaUserAgent(command: command, workingDirectory: pf)
            // Auto-detect "command not found" and respond with whereis lookup
            if result.status != 0 && result.output.contains("command not found") {
                let tool = command.trimmingCharacters(in: .whitespaces).components(separatedBy: " ").first ?? ""
                if !tool.isEmpty {
                    let lookup = await executeViaUserAgent(command: "/usr/bin/whereis \(tool) 2>/dev/null; which \(tool) 2>/dev/null; ls /opt/homebrew/bin/\(tool) /usr/local/bin/\(tool) 2>/dev/null")
                    let paths = lookup.output.trimmingCharacters(in: .whitespacesAndNewlines)
                    appendLog("🔍 whereis \(tool): \(paths.isEmpty ? "not found" : paths)")
                    return "command not found: \(tool)\nwhereis results:\n\(paths.isEmpty ? "Not installed on this system." : paths)\nUse the full path to run it, or ask the user to install it."
                }
            }
            return result.output.isEmpty ? "(no output, exit \(result.status))" : result.output
        case "execute_daemon_command":
            let command = input["command"] as? String ?? ""
            appendLog("🔴 # \(Self.collapseHeredocs(command))")
            flushLog()
            let result = await helperService.execute(command: command, workingDirectory: pf)
            if result.status > 0 { appendLog("exit code: \(result.status)") }
            flushLog()
            return result.output.isEmpty ? "(no output, exit \(result.status))" : result.output
        // AppleScript (NSAppleScript in-process with TCC)
        case "run_applescript":
            let source = (input["source"] as? String ?? "")
            let result = await Self.offMain { () -> (String, Bool) in
                var err: NSDictionary?
                guard let script = NSAppleScript(source: source) else { return ("Error", false) }
                let out = script.executeAndReturnError(&err)
                if let e = err { return ("AppleScript error: \(e)", false) }
                return (out.stringValue ?? "(no output)", true)
            }
            if result.1 {
                let autoName = Self.autoScriptName(from: source)
                let _ = await Self.offMain { [ss = scriptService] in ss.saveAppleScript(name: autoName, source: source) }
            }
            return result.0
        // osascript (runs osascript CLI in-process with TCC)
        case "run_osascript":
            let script = input["script"] as? String ?? input["command"] as? String ?? ""
            let escaped = script.replacingOccurrences(of: "'", with: "'\\''")
            let command = "osascript -e '\(escaped)'"
            let result = await Self.executeTCCStreaming(command: command) { _ in }
            if result.status == 0 {
                let _ = scriptService.saveAppleScript(name: Self.autoScriptName(from: script), source: script)
            }
            return result.output.isEmpty ? "(no output, exit \(result.status))" : result.output
        // JavaScript for Automation (JXA via osascript -l JavaScript)
        case "execute_javascript":
            let script = input["source"] as? String ?? input["script"] as? String ?? ""
            let escaped = script.replacingOccurrences(of: "'", with: "'\\''")
            let command = "osascript -l JavaScript -e '\(escaped)'"
            let result = await Self.executeTCCStreaming(command: command) { _ in }
            if result.status == 0 {
                let _ = scriptService.saveJavaScript(name: Self.autoScriptName(from: script), source: script)
            }
            return result.output.isEmpty ? "(no output, exit \(result.status))" : result.output
        // Script management
        case "list_agents":
            let scripts = await Self.offMain { [ss = scriptService] in ss.listScripts() }
            return scripts.isEmpty ? "No scripts found" : scripts.map { "\($0.name) (\($0.size) bytes)" }.joined(separator: "\n")
        case "run_agent":
            let scriptName = input["name"] as? String ?? ""
            let arguments = input["arguments"] as? String ?? ""
            guard let cmd = await Self.offMain({ [ss = scriptService] in ss.compileCommand(name: scriptName) }) else {
                return "Error: script '\(scriptName)' not found"
            }
            RecentAgentsService.shared.recordRun(agentName: scriptName, arguments: arguments, prompt: "run \(scriptName) \(arguments)")
            // Compile first
            let compileResult = await Self.executeTCC(command: cmd)
            guard compileResult.status == 0 else {
                RecentAgentsService.shared.updateStatus(agentName: scriptName, arguments: arguments, status: .failed)
                return "Build failed:\n\(compileResult.output)"
            }
            // Run the compiled dylib in-process via dlopen
            let result = await scriptService.loadAndRunScript(name: scriptName, arguments: arguments)
            let isUsage = result.output.trimmingCharacters(in: .whitespacesAndNewlines).hasPrefix("Usage:")
            if isUsage || result.status != 0 {
                RecentAgentsService.shared.updateStatus(agentName: scriptName, arguments: arguments, status: .failed)
            } else {
                RecentAgentsService.shared.updateStatus(agentName: scriptName, arguments: arguments, status: .success)
            }
            return result.output.isEmpty ? "(no output, exit \(result.status))" : result.output
        case "read_agent":
            let readName = input["name"] as? String ?? ""
            return await Self.offMain { [ss = scriptService] in ss.readScript(name: readName) ?? "Not found" }
        case "create_agent", "update_agent":
            let createName = input["name"] as? String ?? ""
            let createContent = input["content"] as? String ?? ""
            return await Self.offMain { [ss = scriptService] in ss.createScript(name: createName, content: createContent) }
        case "delete_agent":
            let deleteName = input["name"] as? String ?? ""
            return await Self.offMain { [ss = scriptService] in ss.deleteScript(name: deleteName) }
        case "combine_agents":
            let sourceA = input["source_a"] as? String ?? ""
            let sourceB = input["source_b"] as? String ?? ""
            let target = input["target"] as? String ?? ""
            guard let contentA = await Self.offMain({ [ss = scriptService] in ss.readScript(name: sourceA) }) else { return "Error: script '\(sourceA)' not found." }
            guard let contentB = await Self.offMain({ [ss = scriptService] in ss.readScript(name: sourceB) }) else { return "Error: script '\(sourceB)' not found." }
            let merged = Self.combineScriptSources(contentA: contentA, contentB: contentB, sourceA: sourceA, sourceB: sourceB)
            if await Self.offMain({ [ss = scriptService] in ss.readScript(name: target) }) != nil {
                return await Self.offMain { [ss = scriptService] in ss.updateScript(name: target, content: merged) }
            } else {
                return await Self.offMain { [ss = scriptService] in ss.createScript(name: target, content: merged) }
            }
        // File operations
        case "read_file":
            let path = input["file_path"] as? String ?? ""
            guard !path.isEmpty else {
                return "Error: file_path is required for read_file. Pass an absolute path like file_path:\"/Users/...\". Use file_manager(action:\"list\", path:...) to see what files exist if you don't know the path."
            }
            // Delegate to CodingService.readFile which returns line-numbered output
            // and gives a clear 'file not found' error with a list-files suggestion
            // when the path is wrong. Honors offset+limit (1-based offset).
            let offset = input["offset"] as? Int
            let limit = input["limit"] as? Int
            return await Self.offMain {
                CodingService.readFile(path: path, offset: offset, limit: limit)
            }
        case "write_file":
            let path = input["file_path"] as? String ?? ""
            let content = input["content"] as? String ?? ""
            // Back up before overwriting
            let tabID = selectedTabId ?? Self.mainTabID
            FileBackupService.shared.backup(filePath: path, tabID: tabID)
            let url = URL(fileURLWithPath: path)
            try? FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
            do { try content.write(to: url, atomically: true, encoding: .utf8); return "Wrote \(path)" }
            catch { return "Error: \(error.localizedDescription)" }
        // MARK: edit_file — delegate to CodingService.editFile (d1f-powered with
        // line-ending normalization, fuzzy whitespace match, context disambiguation,
        // and round-trip verification). The duplicate edit_file logic that lived
        // here had none of those safeguards and was the source of most "old_string
        // not found" errors when the LLM had a slightly-stale snapshot of the file.
        case "edit_file":
            let path = input["file_path"] as? String ?? ""
            guard !path.isEmpty else { return "Error: file_path is required for edit_file" }
            let old = input["old_string"] as? String ?? ""
            let new = input["new_string"] as? String ?? ""
            let replaceAll = input["replace_all"] as? Bool ?? false
            let context = input["context"] as? String
            // Back up before editing so undo_edit can restore
            let tabID = selectedTabId ?? Self.mainTabID
            FileBackupService.shared.backup(filePath: path, tabID: tabID)
            return await Self.offMain {
                CodingService.editFile(path: path, oldString: old, newString: new, replaceAll: replaceAll, context: context)
            }
        // MARK: create_diff
        case "create_diff":
            var source = input["source"] as? String ?? ""
            let destination = input["destination"] as? String ?? ""
            if let fp = input["file_path"] as? String, !fp.isEmpty {
                let expanded = (fp as NSString).expandingTildeInPath
                if let data = FileManager.default.contents(atPath: expanded),
                   let text = String(data: data, encoding: .utf8) {
                    source = text
                }
            }
            let diff = MultiLineDiff.createDiff(source: source, destination: destination, includeMetadata: true)
            let d1f = MultiLineDiff.displayDiff(diff: diff, source: source, format: .ai)
            let diffId = DiffStore.shared.store(diff: diff, source: source)
            return "diff_id: \(diffId.uuidString)\n\n\(d1f)"
        // MARK: apply_diff
        case "apply_diff":
            let path = input["file_path"] as? String ?? ""
            let diffIdStr = input["diff_id"] as? String ?? ""
            let asciiDiff = input["diff"] as? String ?? ""
            let expanded = (path as NSString).expandingTildeInPath
            guard let data = FileManager.default.contents(atPath: expanded),
                  let source = String(data: data, encoding: .utf8) else { return "Error: cannot read \(path)" }
            do {
                let patched: String
                if let uuid = UUID(uuidString: diffIdStr),
                   let stored = DiffStore.shared.retrieve(uuid) {
                    patched = try MultiLineDiff.applyDiff(to: source, diff: stored.diff)
                } else if !asciiDiff.isEmpty {
                    patched = try MultiLineDiff.applyASCIIDiff(to: source, asciiDiff: asciiDiff)
                } else {
                    throw DiffError.invalidDiff
                }
                try patched.write(to: URL(fileURLWithPath: expanded), atomically: true, encoding: .utf8)
                let verifyDiff = MultiLineDiff.createAndDisplayDiff(source: source, destination: patched, format: .ai)
                return "Applied diff to \(path)\n\n\(verifyDiff)"
            } catch {
                return "Error applying diff: \(error.localizedDescription)"
            }
        // List/search files (via User LaunchAgent - no TCC required)
        case "list_files":
            let rawPat = input["pattern"] as? String ?? "*.swift"
            // Reject wildcard-only patterns — too broad, suggest specific extension
            if rawPat == "*" || rawPat == "*.*" {
                return "Error: pattern '*' is too broad. Use a file extension like '*.swift', '*.json', '*.py', or '*.txt'. Example: list_files(pattern: \"*.swift\")"
            }
            let pat = CodingService.shellEscape(rawPat)
            let rawDir = input["path"] as? String ?? pf
            let displayDir = CodingService.trimHome(rawDir)
            let result = await executeViaUserAgent(command: "find . -maxdepth 8 -type f -name \(pat) ! -path '*/.*' ! -path '*/.build/*' ! -path '*/.git/*' ! -path '*/.swiftpm/*' ! -name '.DS_Store' ! -name '*.xcuserstate' 2>/dev/null | sed 's|^\\./||' | sort | head -100", workingDirectory: rawDir, silent: true)
            let raw = result.output.trimmingCharacters(in: .whitespacesAndNewlines)
            return raw.isEmpty ? "No files found" : "[project folder: \(displayDir)] paths are relative to project folder\n\(CodingService.formatFileTree(raw))"
        case "search_files":
            let pat = CodingService.shellEscape(input["pattern"] as? String ?? "")
            let rawDir = input["path"] as? String ?? pf
            let displayDir = CodingService.trimHome(rawDir)
            let escapedDir = CodingService.shellEscape(rawDir)
            let result = await executeViaUserAgent(command: "grep -rn \(pat) \(escapedDir) 2>/dev/null | head -50")
            return result.output.isEmpty ? "No matches" : "[project folder: \(displayDir)] paths are relative to project folder\n\(result.output)"
        case "read_dir":
            let rawDir = input["path"] as? String ?? pf
            let displayDir = CodingService.trimHome(rawDir)
            let detail = (input["detail"] as? String ?? "slim") == "more"
            let cmd = detail
                ? "ls -la . 2>/dev/null"
                : "find . -maxdepth 1 -not -name '.*' 2>/dev/null | sed 's|^\\./||' | sort"
            let result = await executeViaUserAgent(command: cmd, workingDirectory: rawDir, silent: !detail)
            let raw = result.output.trimmingCharacters(in: .whitespacesAndNewlines)
            return raw.isEmpty ? "Directory not found or empty" : "[project folder: \(displayDir)]\n\(raw)"
        case "mkdir":
            let rawPath = input["path"] as? String ?? ""
            guard !rawPath.isEmpty else { return "Error: path is required" }
            let stripped = rawPath.hasPrefix("./") ? String(rawPath.dropFirst(2)) : rawPath
            let resolved = stripped.hasPrefix("/") || stripped.hasPrefix("~")
                ? (stripped as NSString).expandingTildeInPath
                : (pf as NSString).appendingPathComponent(stripped)
            let escaped = CodingService.shellEscape(resolved)
            let result = await executeViaUserAgent(command: "mkdir -p \(escaped) && echo 'Created: \(resolved)'")
            let out = result.output.trimmingCharacters(in: .whitespacesAndNewlines)
            if out.hasPrefix("Created:") {
                projectFolder = resolved
                return "\(out)\nProject folder set to: \(resolved)"
            }
            return out.isEmpty ? "Error creating directory" : out
        case "if_to_switch":
            let filePath = input["file_path"] as? String ?? ""
            return await Self.offMain { CodingService.convertIfToSwitch(path: filePath) }
        case "extract_function":
            let filePath = input["file_path"] as? String ?? ""
            let funcName = input["function_name"] as? String ?? ""
            let newFile = input["new_file"] as? String ?? ""
            return await Self.offMain { CodingService.extractFunctionToFile(sourcePath: filePath, functionName: funcName, newFileName: newFile) }
        // Tool discovery
        case "list_tools":
            let prefs = ToolPreferencesService.shared
            let enabledTools = AgentTools.tools(for: selectedProvider)
                .filter { prefs.isEnabled(selectedProvider, $0.name) }
                .sorted { $0.name < $1.name }
            let builtIn = enabledTools.map { tool -> String in
                if let actionProp = tool.properties["action"],
                   let desc = actionProp["description"] as? String {
                    return "\(tool.name) (actions: \(desc))"
                }
                return tool.name
            }
            let mcp = MCPService.shared
            let mcpTools = mcp.discoveredTools
                .filter { mcp.isToolEnabled(serverName: $0.serverName, toolName: $0.name) }
                .sorted { $0.name < $1.name }
                .map { "mcp_\($0.serverName)_\($0.name)" }
            let all = builtIn + (mcpTools.isEmpty ? [] : ["--- MCP Tools ---"] + mcpTools)
            return all.joined(separator: "\n")
        // Memory tool — persistent typed memories the LLM reads at task start
        case "memory":
            let action = input["action"] as? String ?? "read"
            switch action {
            case "read":
                let content = MemoryStore.shared.content
                return content.isEmpty ? "Memory is empty. User can add preferences here." : content
            case "write":
                let text = input["text"] as? String ?? ""
                MemoryStore.shared.write(text)
                return "Memory updated."
            case "append":
                let text = input["text"] as? String ?? ""
                MemoryStore.shared.append(text)
                return "Added to memory."
            case "clear":
                MemoryStore.shared.write("")
                return "Memory cleared."
            case "list":
                let manifest = MemoryStore.shared.manifest()
                return manifest.isEmpty ? "No memories stored." : manifest
            case "save":
                let id = input["id"] as? String ?? "untitled"
                let name = input["name"] as? String ?? id
                let desc = input["description"] as? String ?? ""
                let typeStr = input["type"] as? String ?? "user"
                let type = MemoryType(rawValue: typeStr) ?? .user
                let text = input["text"] as? String ?? ""
                let entry = MemoryEntry(id: id, name: name, description: desc, type: type, content: text)
                MemoryStore.shared.save(entry)
                MemoryStore.shared.rebuildIndex()
                return "Saved memory '\(name)' [\(type.rawValue)]."
            case "load":
                let id = input["id"] as? String ?? ""
                if let entry = MemoryStore.shared.load(id: id) {
                    return "[\(entry.type.rawValue)] \(entry.name)\n\(entry.content)"
                }
                return "Memory '\(id)' not found."
            case "delete":
                let id = input["id"] as? String ?? ""
                MemoryStore.shared.delete(id: id)
                MemoryStore.shared.rebuildIndex()
                return "Deleted memory '\(id)'."
            default:
                return "Unknown memory action. Use: read, write, append, clear, list, save, load, delete."
            }
        // Skills — reusable prompt templates
        case "invoke_skill":
            let action = input["action"] as? String ?? "invoke"
            switch action {
            case "list":
                return SkillsService.shared.manifest()
            case "invoke":
                let name = input["name"] as? String ?? ""
                guard let skill = SkillsService.shared.load(name: name) else {
                    return "Skill '\(name)' not found. Use action=list to see available skills."
                }
                return "SKILL PROMPT [\(skill.name)]:\n\(skill.content)"
            case "save":
                let id = input["id"] as? String ?? input["name"] as? String ?? "untitled"
                let name = input["name"] as? String ?? id
                let desc = input["description"] as? String ?? ""
                let whenToUse = input["when_to_use"] as? String ?? ""
                let content = input["content"] as? String ?? ""
                let skill = Skill(id: id, name: name, description: desc, whenToUse: whenToUse, content: content)
                SkillsService.shared.save(skill)
                return "Saved skill '\(name)'."
            case "delete":
                let id = input["id"] as? String ?? ""
                SkillsService.shared.delete(id: id)
                return "Deleted skill '\(id)'."
            default:
                return "Unknown skill action. Use: list, invoke, save, delete."
            }
        // Sub-agent spawning — isolated concurrent task execution
        case "spawn_agent":
            let name = input["name"] as? String ?? "agent-\(subAgents.count + 1)"
            let prompt = input["prompt"] as? String ?? ""
            guard !prompt.isEmpty else { return "Error: prompt is required for spawn_agent." }
            // Configurable tool groups: "all" or comma-separated group names.
            // The legacy 'coding' / 'automation' aliases are gone with the rest of
            // the mode system; pass explicit group names like "Core,Code,User" if
            // you want to narrow a sub-agent's tool list.
            var toolGroups: Set<String>? = nil
            if let mode = input["tools"] as? String {
                if mode == "all" {
                    toolGroups = Set(Tool.allGroups)
                } else {
                    toolGroups = Set(mode.components(separatedBy: ",").map { $0.trimmingCharacters(in: .whitespaces) })
                }
            }
            let maxIter = input["max_iterations"] as? Int ?? 15
            return spawnSubAgent(name: name, prompt: prompt, toolGroups: toolGroups, maxIterations: maxIter)
        // AskUserQuestion — mid-task dialog, waits for user answer
        case "ask_user":
            let question = input["question"] as? String ?? ""
            guard !question.isEmpty else { return "Error: 'question' is required." }
            appendLog("❓ \(question)")
            flushLog()
            // Post question and wait for answer (up to 5 minutes)
            pendingQuestion = question
            pendingAnswer = nil
            NotificationCenter.default.post(name: .askUserQuestion, object: question)
            let deadline = Date().addingTimeInterval(300)
            while pendingAnswer == nil && Date() < deadline && !Task.isCancelled {
                try? await Task.sleep(for: .milliseconds(500))
            }
            let answer = pendingAnswer ?? "(no answer — timed out after 5 minutes)"
            pendingQuestion = ""
            pendingAnswer = nil
            appendLog("💬 \(answer)")
            flushLog()
            return "User answered: \(answer)"
        // WebFetch — read content from any URL
        // Visual test assertion — click element, verify text appears (opt-in)
        case "visual_test":
            guard visualTestsEnabled else { return "Error: Visual tests disabled. Enable in Coding Preferences." }
            let action = input["action"] as? String ?? "assert"
            switch action {
            case "click_and_verify":
                let clickRole = input["click_role"] as? String
                let clickTitle = input["click_title"] as? String
                let expectRole = input["expect_role"] as? String
                let expectTitle = input["expect_title"] as? String
                let app = input["appBundleId"] as? String
                // Click
                let clickResult = AccessibilityService.shared.clickElement(role: clickRole, title: clickTitle, value: nil, appBundleId: app)
                try? await Task.sleep(for: .seconds(1))
                // Verify
                let findResult = AccessibilityService.shared.findElement(role: expectRole, title: expectTitle, value: nil, appBundleId: app, timeout: 5)
                let passed = findResult.contains("\"success\":true") || findResult.contains("\"success\": true")
                return "VISUAL TEST: \(passed ? "PASS" : "FAIL")\nClick: \(clickResult.prefix(200))\nVerify: \(findResult.prefix(200))"
            case "assert_exists":
                let role = input["role"] as? String
                let title = input["title"] as? String
                let app = input["appBundleId"] as? String
                let result = AccessibilityService.shared.findElement(role: role, title: title, value: nil, appBundleId: app, timeout: 5)
                let passed = result.contains("\"success\":true") || result.contains("\"success\": true")
                return "ASSERTION: \(passed ? "PASS" : "FAIL") — \(role ?? "any") '\(title ?? "any")'\n\(result.prefix(200))"
            default:
                return "Unknown visual_test action. Use: click_and_verify, assert_exists."
            }
        // Git PR workflow — create branch, commit, push, open PR (opt-in)
        case "git_pr":
            guard autoPREnabled else { return "Error: Auto PR disabled. Enable in Coding Preferences." }
            let action = input["action"] as? String ?? "create"
            let branch = input["branch"] as? String ?? "feature/agent-changes"
            let title = input["title"] as? String ?? "Agent! automated changes"
            let body = input["body"] as? String ?? ""
            let dir = projectFolder
            guard !dir.isEmpty else { return "Error: project folder required." }
            switch action {
            case "create":
                let cmds = [
                    "git checkout -b \(branch)",
                    "git add -A",
                    "git commit -m '\(title)'",
                    "git push -u origin \(branch)",
                    "gh pr create --title '\(title)' --body '\(body)' 2>&1 || echo 'Install gh CLI to create PRs automatically'"
                ].joined(separator: " && ")
                let result = await executeViaUserAgent(command: cmds, workingDirectory: dir)
                return result.output.isEmpty ? "PR created on branch \(branch)" : result.output
            default:
                return "Unknown git_pr action. Use: create."
            }
        // Project template — scaffold new Xcode project (opt-in)
        case "create_project":
            guard autoScaffoldEnabled else { return "Error: Project templates disabled. Enable in Coding Preferences." }
            let name = input["name"] as? String ?? "NewApp"
            let template = input["template"] as? String ?? "swiftui"
            let path = input["path"] as? String ?? projectFolder
            guard !path.isEmpty else { return "Error: path required." }

            // Use xcrun to create project via template
            let createCmd: String
            switch template {
            case "swiftui":
                createCmd = """
                mkdir -p "\(path)/\(name)" && cd "\(path)/\(name)" && swift package init --type executable --name \(name) && \
                echo 'import SwiftUI\n@main struct \(name)App: App { var body: some Scene { WindowGroup { Text(\"Hello\") } } }' > Sources/\(name).swift
                """
            case "cli":
                createCmd = "mkdir -p \"\(path)/\(name)\" && cd \"\(path)/\(name)\" && swift package init --type executable --name \(name)"
            case "library":
                createCmd = "mkdir -p \"\(path)/\(name)\" && cd \"\(path)/\(name)\" && swift package init --type library --name \(name)"
            default:
                return "Unknown template. Use: swiftui, cli, library."
            }
            let result = await executeViaUserAgent(command: createCmd, workingDirectory: path)
            return result.status == 0 ? "Project '\(name)' created at \(path)/\(name) (template: \(template))" : result.output
        case "web_fetch":
            let urlStr = input["url"] as? String ?? ""
            guard let url = URL(string: urlStr) else { return "Error: invalid URL '\(urlStr)'" }
            appendLog("🌐 Fetch: \(urlStr)")
            flushLog()
            do {
                // Use a real browser User-Agent so sites don't 403 / serve weird responses
                var request = URLRequest(url: url)
                request.setValue("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15", forHTTPHeaderField: "User-Agent")
                request.setValue("text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8", forHTTPHeaderField: "Accept")
                request.setValue("en-US,en;q=0.9", forHTTPHeaderField: "Accept-Language")
                let (data, response) = try await URLSession.shared.data(for: request)
                let httpResponse = response as? HTTPURLResponse
                let statusCode = httpResponse?.statusCode ?? 0
                guard (200..<400).contains(statusCode) else {
                    return "Error: HTTP \(statusCode) for \(urlStr)"
                }
                let raw = String(data: data, encoding: .utf8) ?? "(binary data, \(data.count) bytes)"
                let cleaned = Self.cleanHTML(raw)
                if cleaned.isEmpty {
                    return "(no readable text content at \(urlStr))"
                }
                let capped = String(cleaned.prefix(8000))
                let truncNote = cleaned.count > 8000 ? "\n\n... [truncated — \(cleaned.count) chars total]" : ""
                return capped + truncNote
            } catch {
                return "Error fetching \(urlStr): \(error.localizedDescription)"
            }
        // Inter-agent messaging — send message to a running sub-agent
        case "tell_agent":
            let to = input["to"] as? String ?? ""
            let message = input["message"] as? String ?? ""
            guard !to.isEmpty && !message.isEmpty else { return "Error: 'to' and 'message' are required." }
            return sendMessageToAgent(name: to, message: message)
        // Task complete — signal via NativeToolContext so the task loop can detect it
        case "task_complete":
            let summary = input["summary"] as? String ?? "Done"

            // Verification gate: if Xcode project + auto-verify + edits were made,
            // build must pass before task_complete is allowed
            let editCommands = commandsRun.filter { $0.hasPrefix("write_file") || $0.hasPrefix("edit_file") || $0.hasPrefix("diff_apply") }
            if autoVerifyEnabled && Self.isXcodeProject(projectFolder) && !editCommands.isEmpty {
                appendLog("🔍 Verify gate: building before allowing completion...")
                flushLog()
                let buildResult = await Self.offMain { XcodeService.shared.buildProject(projectPath: "") }
                if buildResult.contains("BUILD FAILED") || buildResult.contains("error:") {
                    // Extract first 5 errors
                    let errors = buildResult.components(separatedBy: "\n")
                        .filter { $0.contains("error:") }
                        .prefix(5)
                        .joined(separator: "\n")
                    appendLog("❌ Verify gate: build failed — sending errors back to LLM")
                    flushLog()
                    return "CANNOT COMPLETE — build failed. Fix these errors first:\n\n\(errors)\n\nAfter fixing, call task_complete again."
                }
                appendLog("✅ Verify gate: build passed")
                flushLog()
            }

            NativeToolContext.taskCompleteSummary = summary
            return "Task complete: \(summary)"
        // MARK: - Conversation Tools

        // write_text
        case "write_text":
            guard let subject = input["subject"] as? String, !subject.isEmpty else {
                return "Error: subject is required for write_text"
            }

            let style = input["style"] as? String ?? "informative"
            let lengthStr = input["length"] as? String ?? "medium"
            let context = input["context"] as? String ?? ""

            let targetWords: Int
            if let exactWords = Int(lengthStr) {
                targetWords = exactWords
            } else {
                switch lengthStr.lowercased() {
                case "short": targetWords = 100
                case "medium": targetWords = 300
                case "long": targetWords = 600
                default: targetWords = 300
                }
            }

            let guidance = """
            Generate \(style) text about "\(subject)" in approximately \(targetWords) words.
            Style: \(style)
            \(context.isEmpty ? "" : "Context: \(context)")
            Requirements: No emojis, well-structured paragraphs, clear and accurate.
            Begin your response directly with the text content.
            """

            return guidance
        // transform_text
        case "transform_text":
            guard let text = input["text"] as? String, !text.isEmpty else {
                return "Error: text is required for transform_text"
            }
            
            guard let transform = input["transform"] as? String, !transform.isEmpty else {
                return "Error: transform type is required for transform_text"
            }
            
            let options = input["options"] as? String ?? ""
            
            // Validate transform type
            let validTransforms = ["grocery_list", "todo_list", "outline", "summary", "bullet_points", "numbered_list", "table", "qa"]
            guard validTransforms.contains(transform.lowercased()) else {
                return "Error: invalid transform type. Valid types: \(validTransforms.joined(separator: ", "))"
            }
            
            let guidance: String
            
            switch transform.lowercased() {
            case "grocery_list":
                guidance = """
                Transform the following text into a grocery list format.
                
                Original text:
                \(text)
                \(options.isEmpty ? "" : "Options: \(options)")
                
                Requirements:
                - Extract all items that could be grocery/shopping items
                - Format as a clean grocery list organized by category (produce, dairy, meat, pantry, etc.)
                - One item per line
                - No emojis - plain text only
                - Include quantities if mentioned
                
                Output the grocery list now:
                """
                
            case "todo_list":
                guidance = """
                Transform the following text into a todo/checklist format.
                
                Original text:
                \(text)
                \(options.isEmpty ? "" : "Options: \(options)")
                
                Requirements:
                - Extract all actionable tasks
                - Format as a numbered or bulleted todo list
                - Each item should start with a verb (Buy, Call, Fix, etc.)
                - Group related tasks if possible
                - No emojis - plain text only
                
                Output the todo list now:
                """
                
            case "outline":
                guidance = """
                Transform the following text into a structured outline.
                
                Original text:
                \(text)
                \(options.isEmpty ? "" : "Options: \(options)")
                
                Requirements:
                - Create hierarchical outline with main topics and subtopics
                - Use Roman numerals (I, II, III) for main sections
                - Use letters (A, B, C) for subsections
                - Use numbers (1, 2, 3) for details
                - No emojis - plain text only
                
                Output the outline now:
                """
                
            case "summary":
                guidance = """
                Summarize the following text concisely.
                
                Original text:
                \(text)
                \(options.isEmpty ? "" : "Options: \(options)")
                
                Requirements:
                - Capture key points in brief
                - Keep summary to about 20% of original length
                - Maintain essential information
                - No emojis - plain text only
                
                Output the summary now:
                """
                
            case "bullet_points":
                guidance = """
                Transform the following text into bullet points.
                
                Original text:
                \(text)
                \(options.isEmpty ? "" : "Options: \(options)")
                
                Requirements:
                - Extract key points as individual bullets
                - Use hyphens (-) for bullet points
                - Keep each point concise
                - No emojis - plain text only
                
                Output the bullet points now:
                """
                
            case "numbered_list":
                guidance = """
                Transform the following text into a numbered list.
                
                Original text:
                \(text)
                \(options.isEmpty ? "" : "Options: \(options)")
                
                Requirements:
                - Extract items as a numbered sequence
                - Use 1., 2., 3. format
                - Maintain logical order
                - No emojis - plain text only
                
                Output the numbered list now:
                """
                
            case "table":
                guidance = """
                Transform the following text into a table format.
                
                Original text:
                \(text)
                \(options.isEmpty ? "" : "Options: \(options)")
                
                Requirements:
                - Organize information into columns
                - Use pipe (|) separators for table format
                - Include header row
                - No emojis - plain text only
                
                Output the table now:
                """
                
            case "qa":
                guidance = """
                Transform the following text into Q&A format.
                
                Original text:
                \(text)
                \(options.isEmpty ? "" : "Options: \(options)")
                
                Requirements:
                - Generate relevant questions from the content
                - Provide clear answers
                - Format as Q: question, A: answer pairs
                - No emojis - plain text only
                
                Output the Q&A now:
                """
                
            default:
                guidance = "Transform this text: \(text)"
            }
            
            return guidance
        // send_message
        case "send_message":
            guard let content = input["content"] as? String, !content.isEmpty else {
                return "Error: content is required for send_message"
            }
            
            guard let recipient = input["recipient"] as? String, !recipient.isEmpty else {
                return "Error: recipient is required for send_message"
            }
            
            let channel = input["channel"] as? String ?? "imessage"
            let subject = input["subject"] as? String ?? ""
            
            // Ensure no emojis in content (simple emoji removal)
            let cleanContent = content.unicodeScalars.filter { !isEmoji($0) }.map(String.init).joined()
            
            // Handle different channels
            switch channel.lowercased() {
            case "clipboard":
                // Copy to clipboard
                await MainActor.run {
                    let pasteboard = NSPasteboard.general
                    pasteboard.clearContents()
                    pasteboard.setString(cleanContent, forType: .string)
                }
                return "Message copied to clipboard:\n\(cleanContent)"
                
            case "imessage":
                // Use AppleScript to send iMessage (simplified version)
                let escapedRecipient = recipient.replacingOccurrences(of: "\"", with: "\\\"")
                let escapedContent = cleanContent.replacingOccurrences(of: "\"", with: "\\\"")
                
                let script = """
                tell application "Messages"
                    send "\(escapedContent)" to buddy "\(escapedRecipient)"
                end tell
                """
                
                let result = await Self.offMain { () -> String in
                    var err: NSDictionary?
                    guard let applescript = NSAppleScript(source: script) else {
                        return "Error: Failed to create AppleScript"
                    }
                    let _ = applescript.executeAndReturnError(&err)
                    if let e = err {
                        return "AppleScript error: \(e)"
                    }
                    return "iMessage sent to \(recipient)"
                }
                return result
                
            case "email":
                // Open mailto URL
                let escapedSubject = subject.addingPercentEncoding(withAllowedCharacters: CharacterSet.urlQueryAllowed) ?? ""
                let escapedBody = cleanContent.addingPercentEncoding(withAllowedCharacters: CharacterSet.urlQueryAllowed) ?? ""
                let mailtoURL: String
                
                if recipient.lowercased() == "me" {
                    mailtoURL = "mailto:?subject=\(escapedSubject)&body=\(escapedBody)"
                } else {
                    let escapedRecipient = recipient.addingPercentEncoding(withAllowedCharacters: CharacterSet.urlQueryAllowed) ?? recipient
                    mailtoURL = "mailto:\(escapedRecipient)?subject=\(escapedSubject)&body=\(escapedBody)"
                }
                
                await MainActor.run {
                    if let url = URL(string: mailtoURL) {
                        NSWorkspace.shared.open(url)
                    }
                }
                return "Email draft opened for \(recipient)"
                
            case "sms":
                // Open SMS URL scheme
                let escapedBody = cleanContent.addingPercentEncoding(withAllowedCharacters: CharacterSet.urlQueryAllowed) ?? ""
                let smsURL = "sms:\(recipient)?body=\(escapedBody)"
                
                await MainActor.run {
                    if let url = URL(string: smsURL) {
                        NSWorkspace.shared.open(url)
                    }
                }
                return "SMS draft opened for \(recipient)"
                
            default:
                return "Error: Unsupported channel '\(channel)'. Use: imessage, email, sms, or clipboard"
            }
        // fix_text
        case "fix_text":
            guard let text = input["text"] as? String, !text.isEmpty else {
                return "Error: text is required for fix_text"
            }
            
            let fixes = input["fixes"] as? String ?? "all"
            let preserveStyle = input["preserve_style"] as? Bool ?? true
            
            // Validate fixes type
            let validFixes = ["all", "spelling", "grammar", "punctuation", "capitalization"]
            guard validFixes.contains(fixes.lowercased()) else {
                return "Error: invalid fixes type. Valid types: \(validFixes.joined(separator: ", "))"
            }
            
            let guidance: String
            
            switch fixes.lowercased() {
            case "spelling":
                guidance = """
                Fix spelling errors in the following text.
                
                Original text:
                \(text)
                
                Requirements:
                - Correct all spelling mistakes
                - Preserve original meaning and style: \(preserveStyle ? "yes" : "no")
                - Do NOT add any emojis
                - Do NOT change word choices unless misspelled
                - Return only the corrected text
                
                Corrected text:
                """
                
            case "grammar":
                guidance = """
                Fix grammar errors in the following text.
                
                Original text:
                \(text)
                
                Requirements:
                - Correct grammar, verb tense, and sentence structure
                - Preserve original meaning and style: \(preserveStyle ? "yes" : "no")
                - Do NOT add any emojis
                - Do NOT change wording unless grammatically incorrect
                - Return only the corrected text
                
                Corrected text:
                """
                
            case "punctuation":
                guidance = """
                Fix punctuation in the following text.
                
                Original text:
                \(text)
                
                Requirements:
                - Correct all punctuation errors
                - Fix spacing around punctuation
                - Preserve original meaning and style: \(preserveStyle ? "yes" : "no")
                - Do NOT add any emojis
                - Return only the corrected text
                
                Corrected text:
                """
                
            case "capitalization":
                guidance = """
                Fix capitalization in the following text.
                
                Original text:
                \(text)
                
                Requirements:
                - Correct capitalization (sentences start with capitals, proper nouns, etc.)
                - Preserve original meaning and style: \(preserveStyle ? "yes" : "no")
                - Do NOT add any emojis
                - Return only the corrected text
                
                Corrected text:
                """
                
            default: // "all"
                guidance = """
                Fix all spelling and grammar errors in the following text.
                
                Original text:
                \(text)
                
                Requirements:
                - Correct spelling, grammar, punctuation, and capitalization
                - Preserve original meaning and style: \(preserveStyle ? "yes" : "no")
                - Do NOT add any emojis
                - Keep the same tone and voice
                - Return only the corrected text
                
                Corrected text:
                """
            }
            
            return guidance
        // plan_mode
        case "plan_mode":
            let action: String = input["action"] as? String ?? "read"
            return Self.handlePlanMode(action: action, input: input, projectFolder: pf, tabName: "main")
        // project_folder
        case "project_folder":
            return handleProjectFolder(tab: nil, input: input)
        // 'mode' tool removed — there are no modes anymore. Return a no-op so any
        // cached LLM context that still calls it doesn't error out hard.
        case "mode":
            return "Mode switching has been removed. All user-enabled tools are always available."
        // MARK: - Xcode Tools
        case "xcode_build":
            let projectPath = input["project_path"] as? String ?? ""
            let buildResult = await Self.offMain { XcodeService.shared.buildProject(projectPath: projectPath) }
            // Git auto-checkpoint after successful build — saves progress for overnight runs
            if buildResult.contains("BUILD SUCCEEDED") && !projectFolder.isEmpty {
                let dir = projectFolder
                let checkResult = await Self.offMain {
                    let p = Process()
                    p.executableURL = URL(fileURLWithPath: "/usr/bin/git")
                    p.arguments = ["status", "--porcelain"]
                    p.currentDirectoryURL = URL(fileURLWithPath: dir)
                    let pipe = Pipe()
                    p.standardOutput = pipe; p.standardError = pipe
                    try? p.run(); p.waitUntilExit()
                    return String(data: pipe.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8) ?? ""
                }
                if !checkResult.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                    _ = await Self.offMain {
                        let p = Process()
                        p.executableURL = URL(fileURLWithPath: "/usr/bin/git")
                        p.arguments = ["add", "-A"]
                        p.currentDirectoryURL = URL(fileURLWithPath: dir)
                        try? p.run(); p.waitUntilExit()
                        let c = Process()
                        c.executableURL = URL(fileURLWithPath: "/usr/bin/git")
                        c.arguments = ["commit", "-m", "WIP: auto-checkpoint after successful build"]
                        c.currentDirectoryURL = URL(fileURLWithPath: dir)
                        try? c.run(); c.waitUntilExit()
                    }
                }
            }
            // Auto-verify: launch app and capture initial UI state (opt-in)
            if buildResult.contains("BUILD SUCCEEDED") && autoVerifyEnabled {
                appendLog("🔍 Auto-verify: launching app...")
                flushLog()
                let runResult = await Self.offMain { XcodeService.shared.runProject(projectPath: projectPath) }
                // Wait for app to launch
                try? await Task.sleep(for: .seconds(2))
                // Capture accessibility tree of launched app
                let ax = AccessibilityService.shared
                let windows = ax.listWindows(limit: 5)
                let verifyReport = "BUILD SUCCEEDED\n\nAuto-verify:\n- App launched: \(runResult.prefix(200))\n- Windows: \(windows.prefix(500))"
                return verifyReport
            }
            return buildResult
        case "xcode_run":
            let projectPath = input["project_path"] as? String ?? ""
            return await Self.offMain { XcodeService.shared.runProject(projectPath: projectPath) }
        case "xcode_list_projects":
            return await Self.offMain { XcodeService.shared.listProjects() }
        case "xcode_select_project":
            let number = input["number"] as? Int ?? 0
            return await Self.offMain { XcodeService.shared.selectProject(number: number) }
        case "xcode_grant_permission":
            return await Self.offMain { XcodeService.shared.grantPermission() }
        case "xcode_add_file":
            let fp = input["file_path"] as? String ?? ""
            return await Self.offMain { XcodeService.shared.addFileToProject(filePath: fp) }
        case "xcode_remove_file":
            let fp = input["file_path"] as? String ?? ""
            return await Self.offMain { XcodeService.shared.removeFileFromProject(filePath: fp) }
        case "xcode_bump_version":
            let delta = input["delta"] as? Int ?? 1
            return await Self.offMain { XcodeService.shared.bumpVersion(delta: delta) }
        case "xcode_bump_build":
            let delta = input["delta"] as? Int ?? 1
            return await Self.offMain { XcodeService.shared.bumpBuild(delta: delta) }
        case "xcode_get_version":
            return await Self.offMain { XcodeService.shared.getVersionInfo() }
        case "xcode_analyze":
            let fp = input["file_path"] as? String ?? ""
            guard !fp.isEmpty else { return "Error: file_path is required for analyze" }
            guard let data = FileManager.default.contents(atPath: fp),
                  let content = String(data: data, encoding: .utf8) else {
                return "Error: could not read \(fp)"
            }
            // Basic Swift analysis — check for common issues
            let lines = content.components(separatedBy: "\n")
            var issues: [String] = []
            for (i, line) in lines.enumerated() {
                let trimmed = line.trimmingCharacters(in: .whitespaces)
                if trimmed.contains("force_cast") || trimmed.contains("as!") { issues.append("[Warning] Line \(i+1): Force cast (as!)") }
                if trimmed.contains("try!") { issues.append("[Warning] Line \(i+1): Force try (try!)") }
                if trimmed.contains("implicitly unwrapped") || (trimmed.contains("!") && trimmed.contains("var ") && trimmed.contains(": ")) { }
                if trimmed.count > 200 { issues.append("[Style] Line \(i+1): Line too long (\(trimmed.count) chars)") }
            }
            return issues.isEmpty ? "No issues found in \(fp) (\(lines.count) lines)" : issues.joined(separator: "\n")
        case "xcode_snippet":
            let fp = input["file_path"] as? String ?? ""
            guard !fp.isEmpty else { return "Error: file_path is required for snippet" }
            guard let data = FileManager.default.contents(atPath: fp),
                  let content = String(data: data, encoding: .utf8) else {
                return "Error: could not read \(fp)"
            }
            let lines = content.components(separatedBy: "\n")
            let s = (input["start_line"] as? Int ?? 1)
            let e = (input["end_line"] as? Int ?? lines.count)
            let start = max(s - 1, 0)
            let end = min(e, lines.count)
            guard start < end else { return "Error: invalid line range \(s)-\(e)" }
            let ext = (fp as NSString).pathExtension
            let snippet = lines[start..<end].enumerated().map { "\(start + $0 + 1)\t\($1)" }.joined(separator: "\n")
            return "```\(ext)\n\(snippet)\n```"
        // batch_tools — run multiple tool calls in one batch
        case "batch_tools":
            let desc = input["description"] as? String ?? "Batch Tasks"
            guard let tasks = input["tasks"] as? [[String: Any]] else {
                return "Error: tasks must be an array of {\"tool\": \"name\", \"input\": {...}} objects"
            }
            var batchOutput = "● \(desc) (\(tasks.count) tasks)\n"
            var completed = 0
            for (idx, task) in tasks.enumerated() {
                var subName = task["tool"] as? String ?? ""
                var subInput = task["input"] as? [String: Any] ?? [:]
                if subName == "batch_tools" || subName == "batch_commands" || subName == "task_complete" {
                    batchOutput += "[\(idx + 1)] \(subName): skipped (not allowed in batch)\n"
                    continue
                }
                (subName, subInput) = Self.expandConsolidatedTool(name: subName, input: subInput)
                let output = await executeNativeTool(subName, input: subInput)
                completed += 1
                batchOutput += "[\(idx + 1)] \(subName): \(output)\n"
            }
            batchOutput += "● \(completed)/\(tasks.count) tasks completed"
            return batchOutput
        // web_search
        case "web_search":
            let query = input["query"] as? String ?? ""
            guard !query.isEmpty else { return "Error: query is required" }
            return await Self.performWebSearchForTask(query: query, apiKey: tavilyAPIKey, provider: selectedProvider)
        // lookup_sdef
        case "lookup_sdef":
            let bundleID = input["bundle_id"] as? String ?? ""
            let className = input["class_name"] as? String
            if bundleID == "list" {
                let names = SDEFService.shared.availableSDEFs()
                return "Available SDEFs (\(names.count)):\n" + names.joined(separator: "\n")
            } else if let cls = className {
                let props = SDEFService.shared.properties(for: bundleID, className: cls)
                let elems = SDEFService.shared.elements(for: bundleID, className: cls)
                var lines = ["\(cls) properties:"]
                for p in props {
                    let ro = p.readonly == true ? " (readonly)" : ""
                    lines.append("  .\(SDEFService.toCamelCase(p.name)): \(p.type ?? "any")\(ro)\(p.description.map { " — \($0)" } ?? "")")
                }
                if !elems.isEmpty { lines.append("elements: \(elems.joined(separator: ", "))") }
                return lines.isEmpty ? "No class '\(cls)' found for \(bundleID)" : lines.joined(separator: "\n")
            } else {
                return SDEFService.shared.summary(for: bundleID)
            }
        case "symbol_search":
            let query = input["query"] as? String ?? ""
            let path = input["path"] as? String ?? pf
            let exact = input["exact"] as? Bool ?? false
            guard !query.isEmpty else { return "Error: query is required" }
            let results = SymbolSearchService.search(query: query, in: path, exactMatch: exact)
            if results.isEmpty { return "No symbols found matching '\(query)'" }
            return results.prefix(50).map { r in
                "\(r.kind) \(r.name) — \(r.filePath):\(r.line)\n  \(r.signature)"
            }.joined(separator: "\n")
        // AST-based multi-file rename using Swift-Syntax
        case "refactor_rename":
            let oldName = input["old_name"] as? String ?? ""
            let newName = input["new_name"] as? String ?? ""
            let path = input["path"] as? String ?? pf
            guard !oldName.isEmpty && !newName.isEmpty else { return "Error: old_name and new_name required." }
            // Find all occurrences using symbol search
            let occurrences = SymbolSearchService.search(query: oldName, in: path, exactMatch: true)
            if occurrences.isEmpty { return "No symbols found matching '\(oldName)'" }
            // Perform rename across all files
            var renamedFiles: Set<String> = []
            var errors: [String] = []
            for occ in occurrences {
                let filePath = occ.filePath
                guard let data = FileManager.default.contents(atPath: filePath),
                      var content = String(data: data, encoding: .utf8) else { continue }
                let before = content
                // Word-boundary replacement to avoid partial matches
                let pattern = "\\b\(NSRegularExpression.escapedPattern(for: oldName))\\b"
                if let regex = try? NSRegularExpression(pattern: pattern) {
                    content = regex.stringByReplacingMatches(in: content, range: NSRange(content.startIndex..., in: content), withTemplate: newName)
                }
                if content != before {
                    FileBackupService.shared.backup(filePath: filePath, tabID: selectedTabId ?? Self.mainTabID)
                    do {
                        try content.write(toFile: filePath, atomically: true, encoding: .utf8)
                        renamedFiles.insert((filePath as NSString).lastPathComponent)
                    } catch {
                        errors.append("\(filePath): \(error.localizedDescription)")
                    }
                }
            }
            if renamedFiles.isEmpty && errors.isEmpty { return "No changes needed — '\(oldName)' not found in source files." }
            var result = "Renamed '\(oldName)' → '\(newName)' in \(renamedFiles.count) file(s):\n"
            result += renamedFiles.sorted().joined(separator: "\n")
            if !errors.isEmpty { result += "\n\nErrors:\n" + errors.joined(separator: "\n") }
            return result
        // undo_edit
        case "undo_edit":
            let fp = input["file_path"] as? String ?? ""
            let expanded = (fp as NSString).expandingTildeInPath
            guard let original = DiffStore.shared.lastEdit(for: expanded) else {
                return "Error: no edit history for \(fp)"
            }
            let result = CodingService.undoEdit(path: fp, originalContent: original)
            if !result.hasPrefix("Error") { DiffStore.shared.clearEditHistory(for: expanded) }
            return result
        // diff_and_apply
        case "diff_and_apply":
            let fp = input["file_path"] as? String ?? ""
            // Back up before diff_and_apply
            FileBackupService.shared.backup(filePath: fp, tabID: selectedTabId ?? Self.mainTabID)
            let dest = input["destination"] as? String ?? ""
            let source = input["source"] as? String
            let startLine = input["start_line"] as? Int
            let endLine = input["end_line"] as? Int
            let result = CodingService.diffAndApply(path: fp, source: source, destination: dest, startLine: startLine, endLine: endLine)
            return result.output

        // Git tools (expanded from git(action:X) → git_X)
        case "git_status":
            let dir = CodingService.resolveDir(pf.isEmpty ? nil : pf)
            let cmd = CodingService.buildGitStatusCommand(path: pf.isEmpty ? nil : pf)
            let result = await executeViaUserAgent(command: cmd, workingDirectory: dir)
            return result.output.isEmpty ? "(no output)" : result.output
        case "git_diff":
            let staged = input["staged"] as? Bool ?? false
            let target = input["target"] as? String
            let dir = CodingService.resolveDir(pf.isEmpty ? nil : pf)
            let cmd = CodingService.buildGitDiffCommand(path: pf.isEmpty ? nil : pf, staged: staged, target: target)
            let result = await executeViaUserAgent(command: cmd, workingDirectory: dir)
            return result.output.isEmpty ? "(no changes)" : result.output
        case "git_log":
            let count = input["count"] as? Int
            let dir = CodingService.resolveDir(pf.isEmpty ? nil : pf)
            let cmd = CodingService.buildGitLogCommand(path: pf.isEmpty ? nil : pf, count: count)
            let result = await executeViaUserAgent(command: cmd, workingDirectory: dir)
            return result.output.isEmpty ? "(no commits)" : result.output
        case "git_commit":
            let message = input["message"] as? String ?? "Update"
            let files = input["files"] as? [String]
            let dir = CodingService.resolveDir(pf.isEmpty ? nil : pf)
            let cmd = CodingService.buildGitCommitCommand(path: pf.isEmpty ? nil : pf, message: message, files: files)
            let result = await executeViaUserAgent(command: cmd, workingDirectory: dir)
            return result.output.isEmpty ? "(no output)" : result.output
        case "git_branch":
            let branchName = input["name"] as? String ?? ""
            let checkout = input["checkout"] as? Bool ?? false
            let dir = CodingService.resolveDir(pf.isEmpty ? nil : pf)
            let cmd = CodingService.buildGitBranchCommand(path: pf.isEmpty ? nil : pf, name: branchName, checkout: checkout)
            let result = await executeViaUserAgent(command: cmd, workingDirectory: dir)
            return result.output.isEmpty ? "(no output)" : result.output
        case "git_diff_patch":
            let target = input["target"] as? String
            let dir = CodingService.resolveDir(pf.isEmpty ? nil : pf)
            let cmd = CodingService.buildGitDiffCommand(path: pf.isEmpty ? nil : pf, staged: false, target: target)
            let result = await executeViaUserAgent(command: cmd, workingDirectory: dir)
            return result.output.isEmpty ? "(no changes)" : result.output
        // Batch commands — single bash process so env vars / cwd / functions persist
        // across steps. Per-step output is split out via delimiter markers so the LLM
        // sees which command produced what (and the per-command exit code).
        case "batch_commands":
            let commands = (input["commands"] as? String ?? "").components(separatedBy: "\n").filter { !$0.trimmingCharacters(in: .whitespaces).isEmpty }
            guard !commands.isEmpty else { return "(no commands)" }
            for (idx, cmd) in commands.enumerated() {
                appendLog("🔧 [\(idx+1)/\(commands.count)] $ \(cmd)")
            }
            flushLog()
            let delim = "===AGENT_BATCH_STEP_\(UUID().uuidString.prefix(8))==="
            var script = ""
            for cmd in commands {
                script += "\(cmd)\n"
                script += "printf '\\n%s:%d\\n' '\(delim)' $?\n"
            }
            let fullCmd = Self.prependWorkingDirectory(script, projectFolder: pf)
            let result = await executeViaUserAgent(command: fullCmd)

            // Split per-step using the delimiter
            var batchOutput = ""
            var remaining = result.output
            for (idx, cmd) in commands.enumerated() {
                if let range = remaining.range(of: "\(delim):") {
                    let stepOutput = String(remaining[remaining.startIndex..<range.lowerBound])
                    let afterDelim = remaining[range.upperBound...]
                    let nlIdx = afterDelim.firstIndex(of: "\n") ?? afterDelim.endIndex
                    let rc = Int(afterDelim[afterDelim.startIndex..<nlIdx]) ?? 0
                    let trimmed = stepOutput.trimmingCharacters(in: CharacterSet(charactersIn: "\n"))
                    batchOutput += "[\(idx + 1)] $ \(cmd)\n"
                    if rc != 0 { batchOutput += "exit code: \(rc)\n" }
                    batchOutput += (trimmed.isEmpty ? "(no output)" : trimmed) + "\n\n"
                    remaining = nlIdx < afterDelim.endIndex
                        ? String(afterDelim[afterDelim.index(after: nlIdx)...])
                        : ""
                } else {
                    batchOutput += "[\(idx + 1)] $ \(cmd)\n"
                    if remaining.isEmpty {
                        batchOutput += "(no output — batch aborted before this step ran)\n\n"
                    } else {
                        batchOutput += "(batch aborted, trailing output below)\n\(remaining)\n\n"
                        remaining = ""
                    }
                }
            }
            return batchOutput.isEmpty ? "(no output)" : batchOutput
        // Wait/pause for accessibility automation
        case "wait", "sleep", "pause":
            let seconds = input["seconds"] as? Double ?? input["duration"] as? Double ?? 3
            let capped = min(seconds, 30) // max 30 seconds
            try? await Task.sleep(for: .seconds(capped))
            return "Waited \(capped) seconds"

        default:
            // Handle ax_ accessibility tools directly (avoid recursion through executeNativeTool)
            if name.hasPrefix("ax_") {
                let axAction = String(name.dropFirst(3))
                var axInput = input
                axInput["action"] = axAction
                return await handleAccessibilityAction(action: axAction, input: axInput)
            }
            return "⚠️ Tool '\(rawName)' (expanded: '\(name)') not handled — no matching handler found."
        }
    }

    /// Direct accessibility dispatch — no recursion through executeNativeTool
    private func handleAccessibilityAction(action: String, input: [String: Any]) async -> String {
        let ax = AgentAccess.AccessibilityService.shared
        let role = input["role"] as? String
        let title = input["title"] as? String
        let value = input["value"] as? String
        // Resolve app name → bundle ID: "Photo Booth" → "com.apple.PhotoBooth", "photobooth" → "com.apple.PhotoBooth"
        let app = ax.resolveBundleId(input["appBundleId"] as? String ?? input["app"] as? String ?? input["name"] as? String)
        let x = (input["x"] as? Double).map { CGFloat($0) }
        let y = (input["y"] as? Double).map { CGFloat($0) }

        switch action {
        case "open_app":
            // Launch/activate app and return all interactive elements in one call
            return ax.openApp(input["appBundleId"] as? String ?? input["app"] as? String ?? input["name"] as? String)
        case "list_windows":
            // If app specified, filter to just that app's windows
            if let app = app {
                return ax.listWindows(limit: input["limit"] as? Int ?? 50, appBundleId: app)
            }
            return ax.listWindows(limit: input["limit"] as? Int ?? 50)
        case "inspect_element":
            // If role/title provided, find element first then inspect at its position
            if (role != nil || title != nil), x == nil, y == nil {
                return ax.getElementProperties(role: role, title: title, value: value, appBundleId: app, x: nil, y: nil)
            }
            return ax.inspectElementAt(x: x ?? 0, y: y ?? 0, depth: input["depth"] as? Int ?? 3)
        case "get_properties":
            return ax.getElementProperties(role: role, title: title, value: value, appBundleId: app, x: x, y: y)
        case "perform_action":
            return ax.performAction(role: role, title: title, value: value, appBundleId: app, x: x, y: y, action: input["ax_action"] as? String ?? "")
        case "type_text", "type_into_element":
            // AXorcist-only: typing requires an element. There is no "type at the
            // current focus" path — find the text field by role/title first.
            return ax.typeTextIntoElement(role: role, title: title, text: input["text"] as? String ?? "", appBundleId: app, verify: input["verify"] as? Bool ?? true)
        case "click", "click_element":
            // AXorcist-only. Coordinate-based click is not supported — provide
            // role/title/value (and ideally appBundleId) so the click goes through
            // AXorcist's element-finder.
            return ax.clickElement(role: role, title: title, value: value, appBundleId: app, timeout: input["timeout"] as? Double ?? 5, verify: input["verify"] as? Bool ?? false)
        case "scroll", "scroll_to_element":
            // AXorcist-only: scroll to an element by role/title. The old coordinate
            // path through InputDriver was removed.
            return ax.scrollToElement(role: role, title: title, appBundleId: app)
        case "press_key":
            // press_key is no longer supported — AXorcist doesn't drive raw key
            // events and the InputDriver path was removed. Use clickElement for
            // buttons or clickMenuItem for keyboard-shortcut menu commands.
            return "Error: press_key is removed. Find the relevant button via accessibility(action:\"click_element\", role:\"AXButton\", title:..., appBundleId:...) or invoke the menu command via accessibility(action:\"click_menu_item\", appBundleId:..., menuPath:\"File > Save\")."
        case "drag":
            // drag is no longer supported — see the AccessibilityService+Interaction
            // comment for the removal rationale and AXorcist-based alternatives.
            return "Error: drag is removed. For window move/resize use accessibility(action:\"set_window_frame\", appBundleId:..., x:, y:, width:, height:). For sliders use accessibility(action:\"set_properties\", role:\"AXSlider\", ...)."
        case "screenshot":
            // All three paths are async — they dispatch screencapture to a
            // background queue so the main thread stays responsive while the
            // ~100ms screencapture process runs.
            let w = (input["width"] as? Double).map { CGFloat($0) }
            let h = (input["height"] as? Double).map { CGFloat($0) }
            if let wid = input["windowId"] as? Int, wid > 0 {
                return await ax.captureScreenshot(windowID: wid)
            } else if let x, let y, let w, let h {
                return await ax.captureScreenshot(x: x, y: y, width: w, height: h)
            } else {
                return await ax.captureAllWindows()
            }
        case "find_element":
            return ax.findElement(role: role, title: title, value: value, appBundleId: app, timeout: input["timeout"] as? Double ?? 5)
        case "get_focused_element":
            return ax.getFocusedElement(appBundleId: app)
        case "get_children":
            return ax.getChildren(role: role, title: title, value: value, appBundleId: app, x: x, y: y, depth: input["depth"] as? Int ?? 3)
        case "get_audit_log":
            return ax.getAuditLog(limit: input["limit"] as? Int ?? 50)
        case "wait_for_element":
            return ax.waitForElement(role: role, title: title, value: value, appBundleId: app, timeout: input["timeout"] as? Double ?? 10, pollInterval: input["pollInterval"] as? Double ?? 0.5)
        case "wait_adaptive":
            return ax.waitForElementAdaptive(role: role, title: title, value: value, appBundleId: app, timeout: input["timeout"] as? Double ?? 10)
        case "manage_app":
            return ax.manageApp(action: input["action"] as? String ?? "list", bundleId: input["bundleId"] as? String, name: input["name"] as? String)
        case "set_window_frame":
            return ax.setWindowFrame(appBundleId: app, x: x, y: y, width: (input["width"] as? Double).map { CGFloat($0) }, height: (input["height"] as? Double).map { CGFloat($0) })
        case "click_menu_item":
            return ax.clickMenuItem(appBundleId: app, menuPath: (input["menuPath"] as? String)?.components(separatedBy: " > ") ?? [])
        case "get_window_frame":
            return ax.getWindowFrame(windowId: input["windowId"] as? Int ?? 0)
        case "highlight_element":
            return ax.highlightElement(role: role, title: title, value: value, appBundleId: app, x: x, y: y, duration: input["duration"] as? Double ?? 2, color: input["color"] as? String ?? "green")
        case "show_menu":
            return ax.showMenu(role: role, title: title, value: value, appBundleId: app, x: x, y: y)
        case "read_focused":
            return ax.readFocusedElement(appBundleId: app)
        case "set_properties":
            return ax.setProperties(role: role, title: title, value: value, appBundleId: app, x: x, y: y, properties: input["properties"] as? [String: Any] ?? [:])
        case "clipboard":
            let clipAction = input["action"] as? String ?? "read"
            switch clipAction {
            case "read":
                let pb = NSPasteboard.general
                if let text = pb.string(forType: .string) { return text }
                if pb.data(forType: .png) != nil || pb.data(forType: .tiff) != nil {
                    return "Clipboard contains an image (use action:paste to paste it)"
                }
                return "Clipboard is empty"
            case "write":
                let text = input["text"] as? String ?? ""
                guard !text.isEmpty else { return "Error: text is required" }
                let pb = NSPasteboard.general
                pb.clearContents()
                pb.setString(text, forType: .string)
                return "Copied to clipboard: \(text.prefix(100))"
            case "paste":
                // Cmd+V via AppleScript System Events. The old path used
                // AXorcist InputDriver hotkey which is gone. NSAppleScript runs
                // in-process with TCC and produces a real synthesized keystroke
                // without going through CGEvent directly.
                let pasteScript = "tell application \"System Events\" to keystroke \"v\" using command down"
                var asErr: NSDictionary?
                if let script = NSAppleScript(source: pasteScript) {
                    _ = script.executeAndReturnError(&asErr)
                    if let e = asErr { return "Paste failed: \(e)" }
                    return "Pasted clipboard contents"
                }
                return "Paste failed: could not create AppleScript"
            case "copy_image":
                let path = input["file_path"] as? String ?? ""
                guard !path.isEmpty else { return "Error: file_path is required" }
                guard let imageData = try? Data(contentsOf: URL(fileURLWithPath: path)) else {
                    return "Error: cannot read image at \(path)"
                }
                let pb = NSPasteboard.general
                pb.clearContents()
                let ext = (path as NSString).pathExtension.lowercased()
                pb.setData(imageData, forType: ext == "png" ? .png : .tiff)
                return "Image copied to clipboard from \(path)"
            default:
                return "Unknown clipboard action: \(clipAction). Use read, write, paste, or copy_image."
            }
        default:
            return "Unknown accessibility action: \(action)"
        }
    }

    /// Clean an HTML string down to readable text. Strips script/style/noscript blocks
    /// (with their contents), HTML comments, all remaining tags, decodes common entities,
    /// and collapses whitespace. Drops obvious garbage lines (CSS variables, JSON config,
    /// minified JS).
    nonisolated static func cleanHTML(_ html: String) -> String {
        var s = html
        // Remove script/style/noscript/svg blocks WITH their content
        let blockTags = ["script", "style", "noscript", "svg", "iframe", "head"]
        for tag in blockTags {
            s = s.replacingOccurrences(
                of: "<\(tag)\\b[^>]*>[\\s\\S]*?</\(tag)>",
                with: " ",
                options: .regularExpression
            )
        }
        // Strip HTML comments
        s = s.replacingOccurrences(of: "<!--[\\s\\S]*?-->", with: " ", options: .regularExpression)
        // Replace block-level tags with newlines so paragraphs break
        let blockBreak = "(?i)</(p|div|li|tr|h[1-6]|br|article|section)>"
        s = s.replacingOccurrences(of: blockBreak, with: "\n", options: .regularExpression)
        s = s.replacingOccurrences(of: "(?i)<br\\s*/?>", with: "\n", options: .regularExpression)
        // Strip all remaining tags
        s = s.replacingOccurrences(of: "<[^>]+>", with: " ", options: .regularExpression)
        // Decode common HTML entities
        let entities: [(String, String)] = [
            ("&nbsp;", " "), ("&amp;", "&"), ("&lt;", "<"), ("&gt;", ">"),
            ("&quot;", "\""), ("&#39;", "'"), ("&apos;", "'"),
            ("&mdash;", "—"), ("&ndash;", "–"), ("&hellip;", "…"),
            ("&rsquo;", "'"), ("&lsquo;", "'"), ("&ldquo;", "\""), ("&rdquo;", "\""),
        ]
        for (k, v) in entities { s = s.replacingOccurrences(of: k, with: v) }
        // Decode numeric entities like &#1234; and &#xABCD;
        s = s.replacingOccurrences(of: "&#x?[0-9a-fA-F]+;", with: " ", options: .regularExpression)
        // Filter out garbage lines
        let lines = s.components(separatedBy: "\n")
            .map { $0.trimmingCharacters(in: .whitespaces) }
            .filter { line in
                guard !line.isEmpty else { return false }
                // Skip CSS variable declarations and JS/JSON-like junk
                if line.hasPrefix("--") && line.contains(":") { return false }
                if line.hasPrefix("{") || line.hasPrefix("[") { return false }
                if line.hasPrefix(":root") || line.hasPrefix("@media") { return false }
                // Skip lines that are mostly punctuation/symbols
                let alphanumCount = line.unicodeScalars.filter { CharacterSet.alphanumerics.contains($0) }.count
                if alphanumCount * 3 < line.count { return false }
                return true
            }
        // Collapse runs of whitespace within each line
        let collapsed = lines.map { line -> String in
            line.replacingOccurrences(of: "\\s+", with: " ", options: .regularExpression)
        }
        // Collapse runs of 3+ blank lines to 2
        var result = collapsed.joined(separator: "\n")
        result = result.replacingOccurrences(of: "\n{3,}", with: "\n\n", options: .regularExpression)
        return result.trimmingCharacters(in: .whitespacesAndNewlines)
    }
}
