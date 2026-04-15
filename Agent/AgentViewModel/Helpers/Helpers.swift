@preconcurrency import Foundation
import AppKit
import AgentMCP

// MARK: - Helper Functions

extension AgentViewModel {

    // MARK: - Project Folder Tool

    /// Handle project_folder tool: get, set, home, documents, library, none.
    /// When tab is nil, operates on the main projectFolder.
    func handleProjectFolder(tab: ScriptTab?, input: [String: Any]) -> String {
        let action = (input["action"] as? String ?? "get").lowercased()
        let home = NSHomeDirectory()

        switch action {
        case "get":
            if let tab {
                let folder = tab.projectFolder.isEmpty ? projectFolder : tab.projectFolder
                return folder.isEmpty ? "No project folder set." : "Project folder: \(CodingService.trimHome(folder))"
            }
            return projectFolder.isEmpty ? "No project folder set." : "Project folder: \(CodingService.trimHome(projectFolder))"

        case "set":
            guard let path = input["path"] as? String, !path.isEmpty else {
                return "Error: path is required for project_folder set"
            }
            // Resolve relative paths against current project folder (so `set xox4` works like `cd ./xox4`)
            let tabFolder = tab?.projectFolder ?? ""
            let current = tabFolder.isEmpty ? projectFolder : tabFolder
            let expanded: String
            if path.hasPrefix("/") || path.hasPrefix("~") {
                expanded = (path as NSString).expandingTildeInPath
            } else {
                let stripped = path.hasPrefix("./") ? String(path.dropFirst(2)) : path
                expanded = (current as NSString).appendingPathComponent(stripped)
            }
            let fm = FileManager.default
            var isDir: ObjCBool = false
            guard fm.fileExists(atPath: expanded, isDirectory: &isDir), isDir.boolValue else {
                return "Error: '\(expanded)' is not a valid directory."
            }
            if let tab {
                tab.projectFolder = expanded
                persistScriptTabs()
            } else {
                projectFolder = expanded
            }
            return "Project folder set to: \(expanded)"

        case "home":
            if let tab {
                tab.projectFolder = home
                persistScriptTabs()
            } else {
                projectFolder = home
            }
            return "Project folder set to: \(home)"

        case "documents":
            let docs = home + "/Documents"
            if let tab {
                tab.projectFolder = docs
                persistScriptTabs()
            } else {
                projectFolder = docs
            }
            return "Project folder set to: \(docs)"

        case "library":
            let lib = home + "/Library"
            if let tab {
                tab.projectFolder = lib
                persistScriptTabs()
            } else {
                projectFolder = lib
            }
            return "Project folder set to: \(lib)"

        case "none":
            if let tab {
                tab.projectFolder = ""
                persistScriptTabs()
            } else {
                projectFolder = ""
            }
            return "Project folder cleared."

        case "cd":
            let path = input["path"] as? String ?? "~"
            let current = (tab != nil ? (tab!.projectFolder.isEmpty ? projectFolder : tab!.projectFolder) : projectFolder)
            let resolved: String
            if path == "~" || path == "~/" {
                resolved = home
            } else if path == ".." || path == "../" {
                resolved = (current as NSString).deletingLastPathComponent
            } else if path == "." || path == "./" {
                return "Project folder: \(CodingService.trimHome(current))"
            } else if path.hasPrefix("/") {
                resolved = path
            } else if path.hasPrefix("~/") {
                resolved = (path as NSString).expandingTildeInPath
            } else if path.hasPrefix("../") {
                let parent = (current as NSString).deletingLastPathComponent
                let rest = String(path.dropFirst(3))
                resolved = (parent as NSString).appendingPathComponent(rest)
            } else if path.hasPrefix("./") {
                let rest = String(path.dropFirst(2))
                resolved = (current as NSString).appendingPathComponent(rest)
            } else {
                resolved = (current as NSString).appendingPathComponent(path)
            }
            let fm = FileManager.default
            var isDir: ObjCBool = false
            guard fm.fileExists(atPath: resolved, isDirectory: &isDir), isDir.boolValue else {
                return "Error: '\(resolved)' is not a valid directory."
            }
            if let tab {
                tab.projectFolder = resolved
                persistScriptTabs()
            } else {
                projectFolder = resolved
            }
            // Quick directory listing so LLM sees what's here
            let contents = (try? fm.contentsOfDirectory(atPath: resolved)) ?? []
            let visible = contents.filter { !$0.hasPrefix(".") }.sorted()
            let listing = visible.map { name -> String in
                var d: ObjCBool = false
                fm.fileExists(atPath: (resolved as NSString).appendingPathComponent(name), isDirectory: &d)
                return d.boolValue ? "📁 \(name)/" : "  \(name)"
            }.joined(separator: "\n")
            return "cd \(CodingService.trimHome(resolved))\n\(listing)"

        default:
            return "Error: invalid action '\(action)'. Use get, set, cd, home, documents, library, or none."
        }
    }

    // MARK: - Consolidated Tool Expansion

    /// / Expands consolidated CRUDL tool names (git, agent, applescript_tool, javascript_tool) / into legacy tool names
    /// so existing handlers work unchanged. / Maps short tool names to their handler names. Supports both old and new names.
    private static let toolAliases = Tool.aliases

    static func expandConsolidatedTool(name: String, input: [String: Any]) -> (String, [String: Any]) {
        let action = input["action"] as? String ?? ""
        // Normalize empty/relative path to nil so handlers fall back to project folder
        var newInput = input
        if let p = newInput["path"] as? String, (p.isEmpty || p == "." || p == "./") { newInput["path"] = nil }
        if let p = newInput["file_path"] as? String, p.isEmpty { newInput["file_path"] = nil }

        // Resolve short aliases to handler names — no handler changes needed
        if let resolved = toolAliases[name] {
            return (resolved, newInput)
        }

        switch name {
        case "git":
            switch action {
            case "status": return ("git_status", newInput)
            case "diff": return ("git_diff", newInput)
            case "log": return ("git_log", newInput)
            case "commit": return ("git_commit", newInput)
            case "diff_patch": return ("git_diff_patch", newInput)
            case "branch": return ("git_branch", newInput)
            case "worktree": return ("git_worktree", newInput)
            default: return ("git_status", newInput)
            }

        case "index":
            switch action {
            case "create": return ("index_create", newInput)
            case "read": return ("index_read", newInput)
            case "remove", "delete": return ("index_remove", newInput)
            case "recreate", "rebuild": return ("index_recreate", newInput)
            case "append", "update": return ("index_append", newInput)
            case "continue", "resume": return ("index_continue", newInput)
            default: return ("index_read", newInput)
            }

        case "agent_script", "agent":
            switch action {
            case "list": return ("list_agents", newInput)
            case "read": return ("read_agent", newInput)
            case "create": return ("create_agent", newInput)
            case "update": return ("update_agent", newInput)
            case "run": return ("run_agent", newInput)
            case "delete": return ("delete_agent", newInput)
            case "combine": return ("combine_agents", newInput)
            case "restore": return ("restore_agent", newInput)
            case "list_backups", "backups": return ("list_agent_backups", newInput)
            case "pull", "pull_remote", "fetch": return ("pull_agent", newInput)
            case "edit":
                // Resolve agent script name → file_path so the model never has to know
                // ~/Documents/AgentScript/agents/Sources/Scripts/<name>.swift exists. Dispatch to the existing edit_file handler with the resolved path.
                var mapped = newInput
                if let scriptName = (newInput["name"] as? String), !scriptName.isEmpty {
                    let clean = scriptName.replacingOccurrences(of: ".swift", with: "")
                    let resolvedPath = ScriptService.scriptsDirURL
                        .appendingPathComponent("\(clean).swift").path
                    mapped["file_path"] = resolvedPath
                    mapped.removeValue(forKey: "name")
                }
                return ("edit_file", mapped)
            default: return ("list_agents", newInput)
            }

        case "applescript", "as":
            switch action {
            case "execute": return ("run_applescript", newInput)
            case "lookup_sdef": return ("lookup_sdef", newInput)
            case "list": return ("list_apple_scripts", newInput)
            case "run": return ("run_apple_script", newInput)
            case "save": return ("save_apple_script", newInput)
            case "delete": return ("delete_apple_script", newInput)
            case "quit_app", "quit",
                 "launch_app", "launch",
                 "open_app", "open",
                 "activate_app", "activate":
                // App lifecycle: synthesize `tell application "X" to <verb>` and route to execute.
                // open/launch both map to AppleScript `launch` (which opens the app without bringing it forward). Use `activa...
                let verb: String = {
                    switch action {
                    case "launch_app", "launch", "open_app", "open": return "launch"
                    case "activate_app", "activate": return "activate"
                    default: return "quit"
                    }
                }()
                let appName = (newInput["name"] as? String)
                    ?? (newInput["app"] as? String)
                    ?? (newInput["bundleId"] as? String) ?? ""
                guard !appName.isEmpty else { return ("run_applescript", newInput) }
                var mapped = newInput
                mapped["source"] = "tell application \"\(appName)\" to \(verb)"
                return ("run_applescript", mapped)
            default: return ("list_apple_scripts", newInput)
            }

        case "javascript", "jxa", "js":
            switch action {
            case "execute": return ("execute_javascript", newInput)
            case "list": return ("list_javascript", newInput)
            case "run": return ("run_javascript", newInput)
            case "save": return ("save_javascript", newInput)
            case "delete": return ("delete_javascript", newInput)
            case "quit_app", "quit",
                 "launch_app", "launch",
                 "open_app", "open",
                 "activate_app", "activate":
                // App lifecycle: synthesize JXA `Application("X").<verb>()` and route to execute.
                let verb: String = {
                    switch action {
                    case "launch_app", "launch", "open_app", "open": return "launch"
                    case "activate_app", "activate": return "activate"
                    default: return "quit"
                    }
                }()
                let appName = (newInput["name"] as? String)
                    ?? (newInput["app"] as? String)
                    ?? (newInput["bundleId"] as? String) ?? ""
                guard !appName.isEmpty else { return ("execute_javascript", newInput) }
                var mapped = newInput
                mapped["source"] = "Application(\"\(appName)\").\(verb)()"
                return ("execute_javascript", mapped)
            default: return ("list_javascript", newInput)
            }

        case "file", "file_manager":
            // Resolve `target` + `name` to a file_path for known directories so the model never has to know paths
            // for system prompts / hooks. Mirrors the agent_script(action:"edit", name:X) pattern.
            if let target = newInput["target"] as? String,
               let lookupName = newInput["name"] as? String, !lookupName.isEmpty,
               newInput["file_path"] == nil
            {
                let home = NSHomeDirectory()
                let resolvedPath: String?
                switch target {
                case "agent", "agent_script", "script":
                    resolvedPath = ScriptService.scriptsDirURL
                        .appendingPathComponent("\(lookupName.replacingOccurrences(of: ".swift", with: "")).swift").path
                case "system_prompt", "system":
                    resolvedPath = "\(home)/Documents/AgentScript/system/\(lookupName).md"
                case "hook":
                    resolvedPath = "\(home)/Documents/AgentScript/hooks/\(lookupName)"
                default:
                    resolvedPath = nil
                }
                if let resolved = resolvedPath {
                    newInput["file_path"] = resolved
                    newInput.removeValue(forKey: "target")
                    newInput.removeValue(forKey: "name")
                }
            }
            switch action {
            case "read": return ("read_file", newInput)
            case "write": return ("write_file", newInput)
            case "edit": return ("edit_file", newInput)
            case "create": return ("create_diff", newInput)
            case "apply": return ("apply_diff", newInput)
            case "list": return ("list_files", newInput)
            case "search": return ("search_files", newInput)
            case "read_dir": return ("list_files", newInput)
            case "if_to_switch": return ("if_to_switch", newInput)
            case "extract_function": return ("extract_function", newInput)
            case "undo": return ("undo_edit", newInput)
            case "diff_apply": return ("diff_and_apply", newInput)
            case "mkdir": return ("mkdir", newInput)
            case "restore", "restore_file": return ("restore_file", newInput)
            case "list_backups", "backups": return ("list_file_backups", newInput)
            case "cd": return ("project_folder", ["action": "cd", "path": newInput["path"] as? String ?? "~"])
            default: return ("read_file", newInput)
            }

        case "xcode", "xc":
            switch action {
            case "build": return ("xcode_build", newInput)
            case "run": return ("xcode_run", newInput)
            case "list_projects": return ("xcode_list_projects", newInput)
            case "select_project": return ("xcode_select_project", newInput)
            case "grant_permission": return ("xcode_grant_permission", newInput)
            case "add_file": return ("xcode_add_file", newInput)
            case "remove_file": return ("xcode_remove_file", newInput)
            case "analyze": return ("xcode_analyze", newInput)
            case "snippet": return ("xcode_snippet", newInput)
            case "code_review": return ("xcode_code_review", newInput)
            case "bump_version": return ("xcode_bump_version", newInput)
            case "bump_build": return ("xcode_bump_build", newInput)
            case "get_version": return ("xcode_get_version", newInput)
            default: return ("xcode_build", newInput)
            }

        case "safari", "web", "w":
            switch action {
            case "open": return ("web_open", newInput)
            case "find": return ("web_find", newInput)
            case "click": return ("web_click", newInput)
            case "type": return ("web_type", newInput)
            case "execute_js": return ("web_execute_js", newInput)
            case "get_url": return ("web_get_url", newInput)
            case "get_title": return ("web_get_title", newInput)
            case "read_content": return ("web_read_content", newInput)
            case "google_search": return ("web_google_search", newInput)
            case "scroll_to": return ("web_scroll_to", newInput)
            case "select": return ("web_select", newInput)
            case "submit": return ("web_submit", newInput)
            case "navigate": return ("web_navigate", newInput)
            case "list_tabs": return ("web_list_tabs", newInput)
            case "switch_tab": return ("web_switch_tab", newInput)
            case "list_windows": return ("web_list_windows", newInput)
            case "scan": return ("web_scan", newInput)
            case "search": return ("web_search", newInput)
            default: return ("web_open", newInput)
            }

        case "selenium", "sel":
            switch action {
            case "start": return ("selenium_start", newInput)
            case "stop": return ("selenium_stop", newInput)
            case "navigate": return ("selenium_navigate", newInput)
            case "find": return ("selenium_find", newInput)
            case "click": return ("selenium_click", newInput)
            case "type": return ("selenium_type", newInput)
            case "execute": return ("selenium_execute", newInput)
            case "screenshot": return ("selenium_screenshot", newInput)
            case "wait": return ("selenium_wait", newInput)
            default: return ("selenium_start", newInput)
            }

        case "accessibility", "ax":
            // Remap "action" for perform_action and manage_app to avoid colliding with the dispatch "action".
            // Callers pass `ax_action` (perform_action) or `sub_action` (manage_app) and the handler reads `action` — we ...
            var mapped = newInput
            if let axAction = mapped["ax_action"] as? String {
                mapped["action"] = axAction
            } else if let subAction = mapped["sub_action"] as? String {
                mapped["action"] = subAction
            }
            // Convenience verbs: accessibility(action:"quit_app",name:"X") routes to manage_app.
            // open/launch are aliases — both call NSWorkspace.openApplication via manageApp.launch.
            switch action {
            case "quit_app", "quit":
                mapped["action"] = "quit"
                return ("ax_manage_app", mapped)
            case "open_app", "open", "launch_app", "launch":
                mapped["action"] = "launch"
                return ("ax_manage_app", mapped)
            case "activate_app", "activate":
                mapped["action"] = "activate"
                return ("ax_manage_app", mapped)
            case "hide_app", "hide":
                mapped["action"] = "hide"
                return ("ax_manage_app", mapped)
            case "unhide_app", "unhide":
                mapped["action"] = "unhide"
                return ("ax_manage_app", mapped)
            case "list_apps":
                mapped["action"] = "list"
                return ("ax_manage_app", mapped)
            default: break
            }
            switch action {
            case "list_windows": return ("ax_list_windows", mapped)
            case "get_properties": return ("ax_get_properties", mapped)
            case "perform_action": return ("ax_perform_action", mapped)
            case "type_text": return ("ax_type_text", mapped)
            case "click": return ("ax_click", mapped)
            case "press_key": return ("ax_press_key", mapped)
            case "screenshot": return ("ax_screenshot", mapped)
            case "set_properties": return ("ax_set_properties", mapped)
            case "find_element": return ("ax_find_element", mapped)
            case "click_element": return ("ax_click_element", mapped)
            case "type_into_element": return ("ax_type_into_element", mapped)
            case "get_children": return ("ax_get_children", mapped)
            case "inspect_element": return ("ax_inspect_element", mapped)
            case "get_focused_element": return ("ax_get_focused_element", mapped)
            case "drag": return ("ax_drag", mapped)
            case "wait": return ("ax_wait_for_element", mapped)
            case "scroll": return ("ax_scroll", mapped)
            case "highlight_element": return ("ax_highlight_element", mapped)
            case "scroll_to_element": return ("ax_scroll_to_element", mapped)
            case "manage_app":
                // If sub_action wasn't provided, action is still "manage_app" which is invalid
                let resolvedAction = mapped["action"] as? String ?? "manage_app"
                if resolvedAction == "manage_app" {
                    mapped["action"] = "list" // default to list when no sub_action given
                }
                return ("ax_manage_app", mapped)
            case "show_menu": return ("ax_show_menu", mapped)
            case "click_menu_item": return ("ax_click_menu_item", mapped)
            case "check_permission": return ("ax_check_permission", mapped)
            case "request_permission": return ("ax_request_permission", mapped)
            default: return ("ax_\(action)", mapped)
            }

        case "conversation", "chat":
            switch action {
            case "write": return ("write_text", newInput)
            case "transform": return ("transform_text", newInput)
            case "fix": return ("fix_text", newInput)
            case "about": return ("about_self", newInput)
            default: return ("about_self", newInput)
            }

        default:
            return (name, input)
        }
    }

    /// Convert Any to JSONValue, handling arrays and nested objects recursively.
    static func toJSONValue(_ value: Any) -> JSONValue {
        if let s = value as? String { return .string(s) }
        if let i = value as? Int { return .int(i) }
        if let d = value as? Double { return .double(d) }
        if let b = value as? Bool { return .bool(b) }
        if let arr = value as? [Any] { return .array(arr.map { toJSONValue($0) }) }
        if let dict = value as? [String: Any] { return .object(dict.mapValues { toJSONValue($0) }) }
        return .string(String(describing: value))
    }
}
