import Foundation

/// A reusable prompt template loaded from a .md file with YAML frontmatter.
struct Skill: Identifiable {
    let id: String // filename without extension
    var name: String
    var description: String
    var whenToUse: String // helps LLM decide when to invoke
    var content: String // the prompt template body

    /// Parse a skill .md file with frontmatter.
    static func parse(id: String, raw: String) -> Skill? {
        guard raw.hasPrefix("---") else { return nil }
        let parts = raw.components(separatedBy: "---")
        guard parts.count >= 3 else { return nil }
        let frontmatter = parts[1]
        let body = parts.dropFirst(2).joined(separator: "---").trimmingCharacters(in: .whitespacesAndNewlines)

        var name = id
        var description = ""
        var whenToUse = ""

        for line in frontmatter.components(separatedBy: "\n") {
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            if trimmed.hasPrefix("name:") {
                name = String(trimmed.dropFirst(5)).trimmingCharacters(in: .whitespaces)
            } else if trimmed.hasPrefix("description:") {
                description = String(trimmed.dropFirst(12)).trimmingCharacters(in: .whitespaces)
            } else if trimmed.hasPrefix("whenToUse:") {
                whenToUse = String(trimmed.dropFirst(10)).trimmingCharacters(in: .whitespaces)
            }
        }
        return Skill(id: id, name: name, description: description, whenToUse: whenToUse, content: body)
    }

    /// One-line summary for LLM skill listing.
    var summaryLine: String {
        let hint = whenToUse.isEmpty ? description : whenToUse
        return "\(name) — \(hint.prefix(100))"
    }
}

/// Loads and manages reusable prompt-template skills from disk.
/// Skills directory: ~/Documents/AgentScript/skills/
@MainActor
final class SkillsService {
    static let shared = SkillsService()

    private let skillsDir: URL

    private init() {
        let home = FileManager.default.homeDirectoryForCurrentUser
        skillsDir = home.appendingPathComponent("Documents/AgentScript/skills")
        try? FileManager.default.createDirectory(at: skillsDir, withIntermediateDirectories: true)
        installBundledSkillsIfNeeded()
    }

    // MARK: - Skill Loading

    /// List all available skills (frontmatter + content).
    func listAll() -> [Skill] {
        let files = (try? FileManager.default.contentsOfDirectory(at: skillsDir, includingPropertiesForKeys: nil)) ?? []
        return files
            .filter { $0.pathExtension == "md" }
            .sorted { $0.lastPathComponent < $1.lastPathComponent }
            .compactMap { url in
                let id = url.deletingPathExtension().lastPathComponent
                guard let raw = try? String(contentsOf: url, encoding: .utf8) else { return nil }
                return Skill.parse(id: id, raw: raw)
            }
    }

    /// Load a skill by name (case-insensitive match on id or name).
    func load(name: String) -> Skill? {
        let lower = name.lowercased()
        return listAll().first { $0.id.lowercased() == lower || $0.name.lowercased() == lower }
    }

    /// Build a manifest string for the LLM to see available skills.
    func manifest() -> String {
        let skills = listAll()
        guard !skills.isEmpty else { return "No skills installed." }
        return skills.map(\.summaryLine).joined(separator: "\n")
    }

    /// Save a skill to disk.
    func save(_ skill: Skill) {
        let raw = """
        ---
        name: \(skill.name)
        description: \(skill.description)
        whenToUse: \(skill.whenToUse)
        ---

        \(skill.content)
        """
        let url = skillsDir.appendingPathComponent("\(skill.id).md")
        try? raw.write(to: url, atomically: true, encoding: .utf8)
    }

    /// Delete a skill by ID.
    func delete(id: String) {
        let url = skillsDir.appendingPathComponent("\(id).md")
        try? FileManager.default.removeItem(at: url)
    }

    // MARK: - Bundled Skills

    /// Install starter skills if the directory is empty.
    private func installBundledSkillsIfNeeded() {
        let existing = (try? FileManager.default.contentsOfDirectory(at: skillsDir, includingPropertiesForKeys: nil)) ?? []
        guard existing.filter({ $0.pathExtension == "md" }).isEmpty else { return }

        for skill in Self.bundledSkills {
            save(skill)
        }
    }

    private static let bundledSkills: [Skill] = [
        Skill(
            id: "commit",
            name: "commit",
            description: "Create a git commit with a good message",
            whenToUse: "When the user asks to commit changes or says /commit",
            content: """
            Review the current git diff and staged changes. Write a clear, concise commit message that:
            1. Starts with an imperative verb (Add, Fix, Update, Remove, Refactor)
            2. Summarizes the "why" not just the "what"
            3. Keeps the first line under 72 characters
            Then execute the git commit.
            """
        ),
        Skill(
            id: "review-code",
            name: "review-code",
            description: "Review code for bugs, style, and improvements",
            whenToUse: "When the user asks for a code review or says /review",
            content: """
            Review the specified code for:
            1. Bugs and logic errors
            2. Security vulnerabilities
            3. Performance issues
            4. Style and readability
            5. Missing error handling
            Be specific — reference line numbers and suggest concrete fixes. Don't nitpick formatting.
            """
        ),
        Skill(
            id: "explain",
            name: "explain",
            description: "Explain how code works in plain language",
            whenToUse: "When the user asks to explain code or says /explain",
            content: """
            Explain the specified code clearly and concisely:
            1. What it does at a high level (1-2 sentences)
            2. Key data flow and control flow
            3. Important design decisions or patterns used
            4. Any non-obvious behavior or edge cases
            Tailor the explanation depth to the user's expertise level.
            """
        ),
        Skill(
            id: "refactor",
            name: "refactor",
            description: "Refactor code for clarity and maintainability",
            whenToUse: "When the user asks to refactor or clean up code",
            content: """
            Refactor the specified code to improve clarity and maintainability:
            1. Extract repeated logic into functions
            2. Simplify complex conditionals
            3. Improve naming for readability
            4. Remove dead code
            Keep the behavior identical. Show the diff and explain each change.
            """
        ),
        Skill(
            id: "debug",
            name: "debug",
            description: "Diagnose and fix a bug",
            whenToUse: "When the user reports a bug or error to investigate",
            content: """
            Investigate and fix the reported bug:
            1. Reproduce: understand the error message or unexpected behavior
            2. Diagnose: trace the code path, check inputs, find the root cause
            3. Fix: make the minimal change that fixes the issue
            4. Verify: explain why the fix works and check for related issues
            Don't guess — read the relevant code before proposing a fix.
            """
        ),
    ]
}
