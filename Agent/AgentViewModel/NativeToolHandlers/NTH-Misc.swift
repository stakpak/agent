
@preconcurrency import Foundation
import AgentTools
import AgentMCP
import AgentD1F
import AgentSwift
import AgentAccess
import Cocoa

// MARK: - Native Tool Handler — Misc (discovery, memory, skills, agents, web_fe

extension AgentViewModel {

    /// / Handles list_tools, memory, skills, spawn_agent, ask_user, visual_test
    func handleMiscNativeTool(name: String, input: [String: Any]) async -> String? {
        switch name {
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
            // Configurable tool groups: "all" or comma-separated group names. T
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
        // WebFetch — read content from any URL Visual test assertion — click el
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
                let findResult = AccessibilityService.shared.findElement(
                    role: expectRole, title: expectTitle,
                    value: nil, appBundleId: app, timeout: 5)
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
                let srcCode =
                    "import SwiftUI\\n"
                    + "@main struct \(name)App: App { "
                    + "var body: some Scene { "
                    + "WindowGroup { Text(\"Hello\") } } }"
                createCmd = """
                mkdir -p "\(path)/\(name)" \
                && cd "\(path)/\(name)" \
                && swift package init --type executable --name \(name) \
                && echo '\(srcCode)' > Sources/\(name).swift
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
            guard !urlStr.isEmpty else { return "Error: url is required for web_
            guard let url = URL(string: urlStr) else { return "Error: invalid UR
            appendLog("🌐 Fetch: \(urlStr)")
            flushLog()
            do {
                // Use a real browser User-Agent so sites don't 403 / serve weir
                var request = URLRequest(url: url)
                request.setValue(
                    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) "
                    + "AppleWebKit/605.1.15 (KHTML, like Gecko) "
                    + "Version/17.0 Safari/605.1.15",
                    forHTTPHeaderField: "User-Agent")
                request.setValue("text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8", forHTTPHeaderField: "Accept")
                request.setValue("en-US,en;q=0.9", forHTTPHeaderField: "Accept-Language")
                let (data, response) = try await URLSession.shared.data(for: request)
                let httpResponse = response as? HTTPURLResponse
                let statusCode = httpResponse?.statusCode ?? 0
                guard (200..<400).contains(statusCode) else {
                    return "Error: HTTP \(statusCode) for \(urlStr). Recovery: URL may be down or require auth. Try a different URL or check manually."
                }
                let raw = String(data: data, encoding: .utf8) ?? "(binary data, \(data.count) bytes)"
                let cleaned = Self.cleanHTML(raw)
                if cleaned.isEmpty {
                    return "(no readable text content at \(urlStr))"
                }
                return LogLimits.trim(cleaned, cap: LogLimits.webFetchChars)
            } catch {
                return "Error fetching \(urlStr): \(error.localizedDescription)"
            }
        // Inter-agent messaging — send message to a running sub-agent
        case "tell_agent":
            let to = input["to"] as? String ?? ""
            let message = input["message"] as? String ?? ""
            guard !to.isEmpty && !message.isEmpty else { return "Error: 'to' and 'message' are required." }
            return sendMessageToAgent(name: to, message: message)
        // Task complete — signal via NativeToolContext so the task loop can det
        case "task_complete":
            let summary = input["summary"] as? String ?? "Done"

            // Verification gate: if Xcode project + auto-verify + edits were ma
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
                    return """
                        CANNOT COMPLETE — build failed. \
                        Fix these errors first:

                        \(errors)

                        After fixing, call task_complete again.
                        """
                }
                appendLog("✅ Verify gate: build passed")
                flushLog()
            }

            NativeToolContext.taskCompleteSummary = summary
            return "Task complete: \(summary)"
        default:
            return nil
        }
    }
}
