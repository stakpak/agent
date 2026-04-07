import Foundation

/// Single source of truth for all tool names and group names.
/// Use these constants everywhere — never hardcode tool or group strings.
enum Tool {
    // MARK: - Tool Names (what the LLM calls)

    // Core
    static let done = "done"
    static let tools = "list_tools"
    static let search = "search"
    static let folder = "directory"
    static let code = "mode"

    // Core (also)
    static let chat = "chat"
    static let msg = "msg"

    // Work
    static let agent = "agent_script"
    static let plan = "plan"
    static let git = "git"
    static let batch = "batch"
    static let multi = "multi"

    // Code
    static let file = "file"
    static let xc = "xcode"
    static let sh = "shell"

    // Auto
    static let `as` = "applescript"
    static let ax = "accessibility"
    static let js = "javascript"

    // User / Root
    static let user = "user_shell"
    static let root = "root_shell"

    // Web
    static let web = "safari"

    // Exp
    static let sel = "selenium"

    // Memory
    static let mem = "memory"

    // Skills
    static let skill = "skill"

    // Sub-agents
    static let spawn = "spawn_agent"
    static let messageAgent = "tell_agent"
    static let ask = "ask_user"
    static let webFetch = "fetch"

    // MARK: - Group Names

    enum Group {
        static let core = "Core"
        static let work = "Work"
        static let code = "Code"
        static let auto = "Auto"
        static let user = "User"
        static let root = "Root"
        static let exp = "Experimental"
    }

    // MARK: - Mode Groups

    static let codingGroups: Set<String> = [Group.core, Group.work, Group.code, Group.user]
    static let automationGroups: Set<String> = [Group.core, Group.work, Group.auto, Group.user]
    static let allGroups: [String] = [Group.core, Group.work, Group.code, Group.auto, Group.user, Group.root, Group.exp]

    // MARK: - Legacy Aliases (old name → handler name)
    // LLM sends short name, alias resolves to the handler the app uses

    static let aliases: [String: String] = [
        // Bare canonical names → handler names. ONLY include entries here when the
        // canonical name maps to a single leaf handler (no action sub-dispatch). For
        // consolidated tools that need action dispatch (chat, javascript, applescript,
        // etc.), let expandConsolidatedTool's switch handle them — adding them here
        // would short-circuit the action routing and break the tool.
        "user_shell": "execute_agent_command",
        "root_shell": "execute_daemon_command",
        "batch": "batch_commands",
        "shell": "run_shell_script",
        "done": "task_complete",
        "search": "web_search",
        "plan": "plan_mode",
        "directory": "project_folder",
        "skill": "invoke_skill",
        "fetch": "web_fetch",
        "ask": "ask_user",
        "ask_user_question": "ask_user",
        "spawn": "spawn_agent",
        "message_agent": "tell_agent",
        "send_message_to_agent": "tell_agent",
        // Backwards-compat for AgentTools 2.41.0 holistic rename — old canonicals batch_shell/
        // invoke_skill/web_fetch may still be in cached LLM contexts. invoke_skill/web_fetch
        // need no entry because their handler cases are still 'invoke_skill' / 'web_fetch'.
        // batch_shell needs an entry because the new short 'batch' aliases to 'batch_commands'.
        "batch_shell": "batch_commands",
        // Legacy short names (still accepted)
        "user": "execute_agent_command",
        "sh": "run_shell_script",
        "root": "execute_daemon_command",
        "multi": "batch_tools",
        "msg": "send_message",
        "dir": "project_folder",
        "tools": "list_tools",
        "mem": "memory",
    ]
}
