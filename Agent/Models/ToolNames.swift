import Foundation
import AgentTools

/// / Single source of truth for all tool/group names. Use constants everywhere, never hardcode. / Names reference
/// AgentTools.Name.* where possible. Local tools (memory/skill/spawn/tell/ask/fetch/file/git) / are defined here until hoisted into the AgentTools package.
enum Tool {
    // MARK: - Tool Names (what the LLM calls)

    // Core
    static let done = AgentTools.Name.taskComplete
    static let tools = AgentTools.Name.listNativeTools
    static let search = AgentTools.Name.webSearch
    static let folder = AgentTools.Name.projectFolderTool

    // Core (also)
    static let chat = AgentTools.Name.conversation
    static let msg = AgentTools.Name.sendMessage

    // Work
    static let agent = AgentTools.Name.agentScript
    static let plan = AgentTools.Name.planMode
    static let git = AgentTools.Name.git
    static let batch = AgentTools.Name.batchCommands
    static let multi = AgentTools.Name.batchTools

    // Code
    static let file = AgentTools.Name.fileManager
    static let xc = AgentTools.Name.xcode
    static let sh = AgentTools.Name.runShellScript

    // Auto
    static let `as` = AgentTools.Name.appleScriptTool
    static let ax = AgentTools.Name.accessibility
    static let js = AgentTools.Name.javascriptTool

    // User / Root
    static let user = AgentTools.Name.executeAgentCommand
    static let root = AgentTools.Name.executeDaemonCommand

    // Web
    static let web = AgentTools.Name.safari

    // Exp
    static let sel = AgentTools.Name.seleniumTool

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
        static let subAgents = "Sub-agents"
        static let exp = "Experimental"
    }

    // MARK: - Group Order

    static let allGroups: [String] = [Group.core, Group.work, Group.code, Group.auto, Group.user, Group.root, Group.subAgents, Group.exp]

    // MARK: - Legacy Aliases (old name → handler name)
    // LLM sends short name, alias resolves to the handler the app uses

    static let aliases: [String: String] = [
        // Canonical short names (the strings the LLM actually emits, defined in AgentTools.Name.*) → internal handler
        // names. ONLY include entries here when the canonical maps to a single leaf handler. For consolidated tools that need action sub-dispatch (chat, javascript, applescript, etc.), let expandConsolidatedTool's switch handle them — adding them here would short-circuit the action routing and break the tool.
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
        "spawn": "spawn_agent",
        "multi": "batch_tools",
        "msg": "send_message",
        // No legacy aliases below this line. Older short names (sh/user/root/dir/
        // tools/mem/batch_shell/ask_user_question/message_agent/ send_message_to_agent) were retired — every supported provider now sees and emits the canonical names from AgentTools.Name.*. Re-add an entry ONLY if a model in active use is observed emitting the old name.
    ]
}
