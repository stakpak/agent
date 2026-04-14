import Foundation

/// Events that can trigger hooks.
enum HookEvent: String, CaseIterable, Codable {
    case preToolUse // Before a tool executes — can block or modify
    case postToolUse // After a tool executes — can transform output
    case taskStart // When a new task begins
    case taskComplete // When a task finishes
    case buildFailure // When xc build fails
}

/// Result of a pre-tool hook — determines whether the tool call proceeds.
enum HookDecision: String {
    case allow // Proceed normally
    case block // Block the tool call, return the message instead
}

/// A user-defined hook that runs on specific events.
struct Hook: Codable, Identifiable {
    let id: UUID
    var name: String
    var event: HookEvent
    /// Tool name pattern to match (empty = all tools). Supports prefix matching with *.
    var toolPattern: String
    /// Shell command to execute. Receives tool name and input as env vars.
    var command: String
    var enabled: Bool

    init(name: String, event: HookEvent, toolPattern: String = "", command: String, enabled: Bool = true) {
        self.id = UUID()
        self.name = name
        self.event = event
        self.toolPattern = toolPattern
        self.command = command
        self.enabled = enabled
    }

    /// Check if this hook matches a tool name.
    func matches(toolName: String) -> Bool {
        guard !toolPattern.isEmpty else { return true }
        if toolPattern.hasSuffix("*") {
            return toolName.hasPrefix(String(toolPattern.dropLast()))
        }
        return toolName == toolPattern
    }
}

/// Manages user-defined hooks for tool execution events.
/// Hooks are stored at ~/Documents/AgentScript/hooks.json
@MainActor
final class HooksService {
    static let shared = HooksService()

    private(set) var hooks: [Hook] = []
    private let fileURL: URL

    private init() {
        let home = FileManager.default.homeDirectoryForCurrentUser
        let dir = home.appendingPathComponent("Documents/AgentScript")
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        fileURL = dir.appendingPathComponent("hooks.json")
        load()
    }

    // MARK: - CRUD

    func add(_ hook: Hook) {
        hooks.append(hook)
        save()
    }

    func remove(id: UUID) {
        hooks.removeAll { $0.id == id }
        save()
    }

    func update(_ hook: Hook) {
        if let idx = hooks.firstIndex(where: { $0.id == hook.id }) {
            hooks[idx] = hook
            save()
        }
    }

    func toggle(id: UUID) {
        if let idx = hooks.firstIndex(where: { $0.id == id }) {
            hooks[idx].enabled.toggle()
            save()
        }
    }

    // MARK: - Execution

    /// Run all matching pre-tool hooks. Returns .block(message) if any hook blocks.
    func runPreToolHooks(toolName: String, input: [String: Any]) async -> (decision: HookDecision, message: String?) {
        let matching = hooks.filter { $0.enabled && $0.event == .preToolUse && $0.matches(toolName: toolName) }
        for hook in matching {
            let result = await executeHook(hook, toolName: toolName, input: input)
            if result.lowercased().hasPrefix("block:") {
                let message = String(result.dropFirst(6)).trimmingCharacters(in: .whitespaces)
                return (.block, message.isEmpty ? "Blocked by hook '\(hook.name)'" : message)
            }
        }
        return (.allow, nil)
    }

    /// Run all matching post-tool hooks. Returns transformed output or nil.
    func runPostToolHooks(toolName: String, input: [String: Any], output: String) async -> String? {
        let matching = hooks.filter { $0.enabled && $0.event == .postToolUse && $0.matches(toolName: toolName) }
        var result = output
        for hook in matching {
            let hookResult = await executeHook(hook, toolName: toolName, input: input, output: result)
            if !hookResult.isEmpty {
                result = hookResult
            }
        }
        return matching.isEmpty ? nil : result
    }

    /// Run event hooks (taskStart, taskComplete, buildFailure).
    func runEventHooks(_ event: HookEvent, context: [String: String] = [:]) async {
        let matching = hooks.filter { $0.enabled && $0.event == event }
        for hook in matching {
            _ = await executeHook(hook, toolName: event.rawValue, input: context.mapValues { $0 as Any })
        }
    }

    /// Execute a single hook command with environment variables.
    private func executeHook(_ hook: Hook, toolName: String, input: [String: Any], output: String = "") async -> String {
        let inputJSON = (try? JSONSerialization.data(withJSONObject: input))
            .flatMap { String(data: $0, encoding: .utf8) } ?? "{}"

        return await withCheckedContinuation { continuation in
            let process = Process()
            process.executableURL = URL(fileURLWithPath: "/bin/zsh")
            process.arguments = ["-c", hook.command]
            process.currentDirectoryURL = URL(fileURLWithPath: NSHomeDirectory())
            var env = ProcessInfo.processInfo.environment
            env["HOOK_TOOL_NAME"] = toolName
            env["HOOK_INPUT"] = inputJSON
            env["HOOK_OUTPUT"] = output
            env["HOOK_EVENT"] = hook.event.rawValue
            // Hooks run from $HOME with no project context — export AGENT_PROJECT_FOLDER pointing at home so hook
            // scripts can rely on the same env contract as every other shell-execution path in Agent!.
            env["AGENT_PROJECT_FOLDER"] = NSHomeDirectory()
            process.environment = env

            let pipe = Pipe()
            process.standardOutput = pipe
            process.standardError = pipe

            do {
                try process.run()
                process.waitUntilExit()
                let data = pipe.fileHandleForReading.readDataToEndOfFile()
                let result = String(data: data, encoding: .utf8) ?? ""
                continuation.resume(returning: result.trimmingCharacters(in: .whitespacesAndNewlines))
            } catch {
                continuation.resume(returning: "")
            }
        }
    }

    // MARK: - Persistence

    private func load() {
        guard let data = try? Data(contentsOf: fileURL),
              let decoded = try? JSONDecoder().decode([Hook].self, from: data) else { return }
        hooks = decoded
    }

    private func save() {
        guard let data = try? JSONEncoder().encode(hooks) else { return }
        try? data.write(to: fileURL, options: .atomic)
    }

    /// List hooks as a summary string for the LLM.
    func summary() -> String {
        guard !hooks.isEmpty else { return "No hooks configured." }
        return hooks.map { h in
            let status = h.enabled ? "on" : "off"
            let pattern = h.toolPattern.isEmpty ? "*" : h.toolPattern
            return "[\(status)] \(h.name) — \(h.event.rawValue) on \(pattern)"
        }.joined(separator: "\n")
    }
}
