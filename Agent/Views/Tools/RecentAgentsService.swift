import Foundation

/// Tracks recently run agent script prompts for the Agents menu.
/// Stores agent name, arguments, and the full prompt. Auto-bumps version numbers.
@MainActor
final class RecentAgentsService: ObservableObject {
    static let shared = RecentAgentsService()

    private let key = "recentAgentPrompts"
    private let maxCount = 20

    enum RunStatus: String, Codable {
        case pending // recorded upfront, not yet finished
        case success // exit 0
        case cancelled // user cancelled
        case failed // non-zero exit / usage error
    }

    struct AgentEntry: Codable, Identifiable {
        let id: UUID
        let agentName: String
        let arguments: String
        let prompt: String
        let date: Date
        var status: RunStatus

        init(agentName: String, arguments: String, prompt: String, status: RunStatus = .pending) {
            self.id = UUID()
            self.agentName = agentName
            self.arguments = arguments
            self.prompt = prompt
            self.date = Date()
            self.status = status
        }

        /// Reconstruct the prompt with arguments for the task input.
        /// Auto-bumps version numbers (e.g. 1.0.45 → 1.0.46).
        var populatedPrompt: String {
            if arguments.isEmpty {
                return "run \(agentName)"
            }
            let bumped = Self.bumpVersions(in: arguments)
            return "run \(agentName) \(bumped)"
        }

        /// Display label for menu — shows agent name + argument hint.
        var menuLabel: String {
            if arguments.isEmpty {
                return agentName
            }
            let short = arguments.count > 40 ? String(arguments.prefix(40)) + "..." : arguments
            return "\(agentName) — \(short)"
        }

        /// Auto-bump semver-like version numbers: 1.0.45 → 1.0.46
        private static func bumpVersions(in text: String) -> String {
            let pattern = #"(\d+)\.(\d+)\.(\d+)"#
            guard let regex = try? NSRegularExpression(pattern: pattern) else { return text }
            let ns = text as NSString
            var result = text
            // Process matches in reverse so replacements don't shift ranges
            let matches = regex.matches(in: text, range: NSRange(location: 0, length: ns.length))
            for match in matches.reversed() {
                guard match.numberOfRanges == 4 else { continue }
                let major = ns.substring(with: match.range(at: 1))
                let minor = ns.substring(with: match.range(at: 2))
                let patchStr = ns.substring(with: match.range(at: 3))
                guard let patch = Int(patchStr) else { continue }
                let bumped = "\(major).\(minor).\(patch + 1)"
                let range = Range(match.range, in: result)!
                result.replaceSubrange(range, with: bumped)
            }
            return result
        }
    }

    @Published private(set) var entries: [AgentEntry] = []

    private init() {
        load()
    }

    /// Record that an agent was run with given arguments and prompt.
    func recordRun(agentName: String, arguments: String = "", prompt: String) {
        guard !agentName.trimmingCharacters(in: .whitespaces).isEmpty else { return }
        // Remove duplicates of same agent + arguments combo
        entries.removeAll { $0.agentName == agentName && $0.arguments == arguments }
        entries.insert(AgentEntry(agentName: agentName, arguments: arguments, prompt: prompt), at: 0)
        if entries.count > maxCount {
            entries = Array(entries.prefix(maxCount))
        }
        save()
    }

    /// Update the status of the most recent run for a given agent + arguments.
    func updateStatus(agentName: String, arguments: String, status: RunStatus) {
        if let idx = entries.firstIndex(where: { $0.agentName == agentName && $0.arguments == arguments }) {
            entries[idx].status = status
            save()
        }
    }

    /// Remove a specific failed agent run from the menu.
    /// Only removes the exact agentName + arguments match. Good entries stay.
    func removeRun(agentName: String, arguments: String) {
        entries.removeAll { $0.agentName == agentName && $0.arguments == arguments }
        save()
    }

    /// Remove a specific entry by UUID.
    func removeById(_ id: UUID) {
        entries.removeAll { $0.id == id }
        save()
    }

    /// Remove ALL entries for an agent by name.
    func removeAgent(name: String) {
        entries.removeAll { $0.agentName == name }
        save()
    }

    /// Clear all entries.
    func clearAll() {
        entries.removeAll()
        save()
    }

    private func load() {
        guard let data = UserDefaults.standard.data(forKey: key),
              let decoded = try? JSONDecoder().decode([AgentEntry].self, from: data) else { return }
        entries = decoded
    }

    private func save() {
        if let data = try? JSONEncoder().encode(entries) {
            UserDefaults.standard.set(data, forKey: key)
        }
    }
}
