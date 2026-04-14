import Foundation
import AgentTools

// MARK: - Task Mode (auto tool subsetting)

/// / Determines which tool groups are sent to the LLM based on task type. / Reduces token usage by only sending
/// relevant tools. TaskMode removed — all tool groups always available, user controls via UI toggles.

/// / Manages which internal tools are enabled per LLM provider. / Claude/Ollama: all tools on by default. / Apple AI:
/// only core tools on by default (context window is too small for 40+).
@MainActor @Observable
final class ToolPreferencesService {
    static let shared = ToolPreferencesService()

    private var disabledTools: Set<String> = [] {
        didSet { persist() }
    }

    /// Globally disabled tool groups - applies to ALL providers
    private var disabledGroups: Set<String> = [] {
        didSet { persistGroups() }
    }

    private static let udKey = "agent.disabledTools"
    private static let udGroupsKey = "agent.disabledToolGroups"
    private static let appleAISeededKey = "agent.appleAISeeded.v2"

    /// / Tool group definitions - maps group name to tool name prefixes. / Each entry includes both the canonical short
    /// name (Tool.xxx) and the / post-expansion handler name so the user-pref filter catches both.
    static let toolGroups: [String: Set<String>] = [
        Tool.Group.core: Set([
            Tool.done, "task_complete", Tool.tools, Tool.search, "web_search",
            Tool.chat, "conversation", Tool.msg, "send_message", Tool.sh,
            "run_shell_script", Tool.plan, "plan_mode", Tool.mem, "memory",
            Tool.skill, "invoke_skill", Tool.file, "file_manager", Tool.folder,
            "project_folder", Tool.webFetch, "web_fetch", Tool.ask, "ask_user"
        ]),
        Tool.Group.work: Set([Tool.batch, "batch_commands", Tool.multi, "batch_tools"]),
        Tool.Group.code: Set([Tool.xc, Tool.git, Tool.agent]),
        Tool.Group.auto: Set([Tool.as, Tool.ax, Tool.js, "jxa", "lookup_sdef", Tool.web]),
        Tool.Group.user: Set([Tool.user, "execute_agent_command"]),
        Tool.Group.root: Set([Tool.root, "execute_daemon_command"]),
        // Sub-agents group: spawn_agent and tell_agent were previously split across the Work and Core groups
        // respectively, which made no sense — they're a coherent feature set (parent agent orchestrates isolated child tasks via mailbox messaging). One toggle now hides/shows both.
        Tool.Group.subAgents: Set([Tool.spawn, "spawn_agent", Tool.messageAgent, "tell_agent"]),
        Tool.Group.exp: Set([Tool.sel, "selenium", "ax_screenshot"]),
    ]

    /// Tools enabled by default for Apple Intelligence (small context window).
    static let appleAIDefaults: Set<String> = [
        AgentTools.Name.executeAgentCommand, AgentTools.Name.fileManager,
        AgentTools.Name.agentScript, AgentTools.Name.taskComplete
    ]

    private static let groupSeededKey = "agent.groupsSeeded.v2"

    private init() {
        let arr = UserDefaults.standard.stringArray(forKey: Self.udKey) ?? []
        disabledTools = Set(arr)
        let groupArr = UserDefaults.standard.stringArray(forKey: Self.udGroupsKey) ?? []
        disabledGroups = Set(groupArr)
        seedDefaultDisabledGroups()
        seedAppleAIDefaults()
    }

    /// On first launch, disable Experimental group by default. Migrate old "Exp" name.
    private func seedDefaultDisabledGroups() {
        // Migrate old "Exp" → "Experimental"
        if disabledGroups.contains("Exp") {
            disabledGroups.remove("Exp")
            disabledGroups.insert(Tool.Group.exp)
            persistGroups()
        }
        guard !UserDefaults.standard.bool(forKey: Self.groupSeededKey) else { return }
        UserDefaults.standard.set(true, forKey: Self.groupSeededKey)
        disabledGroups.insert(Tool.Group.exp)
    }

    /// On first launch, disable all Apple AI tools not in the core default set.
    private func seedAppleAIDefaults() {
        guard !UserDefaults.standard.bool(forKey: Self.appleAISeededKey) else { return }
        UserDefaults.standard.set(true, forKey: Self.appleAISeededKey)
        let all = AgentTools.tools(for: .foundationModel).map { $0.name }
        var updated = disabledTools
        for name in all where !Self.appleAIDefaults.contains(name) {
            updated.insert(toolKey(.foundationModel, name))
        }
        disabledTools = updated // single persist
    }

    private func persist() {
        UserDefaults.standard.set(Array(disabledTools), forKey: Self.udKey)
    }

    private func persistGroups() {
        UserDefaults.standard.set(Array(disabledGroups), forKey: Self.udGroupsKey)
    }

    private func toolKey(_ provider: APIProvider, _ name: String) -> String {
        "\(provider.rawValue).\(name)"
    }

    func isEnabled(_ provider: APIProvider, _ toolName: String) -> Bool {
        // First check if the tool's group is globally disabled
        for (group, tools) in Self.toolGroups {
            if tools.contains(toolName) && disabledGroups.contains(group) {
                return false
            }
        }
        // Then check per-provider setting
        return !disabledTools.contains(toolKey(provider, toolName))
    }

    /// Check if a group is enabled (not in disabledGroups)
    func isGroupEnabled(_ groupName: String) -> Bool {
        !disabledGroups.contains(groupName)
    }

    /// Toggle a group globally
    func toggleGroup(_ groupName: String) {
        if disabledGroups.contains(groupName) {
            disabledGroups.remove(groupName)
        } else {
            disabledGroups.insert(groupName)
        }
    }

    /// Enable all groups (except Experimental, which must be toggled explicitly)
    func enableAllGroups() {
        let keepDisabled = disabledGroups.contains(Tool.Group.exp)
        disabledGroups.removeAll()
        if keepDisabled { disabledGroups.insert(Tool.Group.exp) }
    }

    /// Disable all groups
    func disableAllGroups() {
        disabledGroups = Set(Self.toolGroups.keys)
    }

    /// Get all group names sorted alphabetically
    static var allGroupNames: [String] {
        toolGroups.keys.sorted()
    }

    /// Check if a tool is enabled considering active task groups, global group toggles, and per-provider settings.
    func isEnabled(_ provider: APIProvider, _ toolName: String, activeGroups: Set<String>?) -> Bool {
        // If activeGroups is set, check if tool belongs to any active group
        if let activeGroups {
            let toolInActiveGroup = Self.toolGroups.contains { group, tools in
                activeGroups.contains(group) && tools.contains(toolName)
            }
            if !toolInActiveGroup { return false }
        }
        return isEnabled(provider, toolName)
    }

    func toggle(_ provider: APIProvider, _ toolName: String) {
        let k = toolKey(provider, toolName)
        if disabledTools.contains(k) { disabledTools.remove(k) }
        else { disabledTools.insert(k) }
    }

    func enableAll(for provider: APIProvider) {
        disabledTools = disabledTools.filter { !$0.hasPrefix("\(provider.rawValue).") }
    }

}
