@preconcurrency import Foundation
import AppKit
import AgentMCP

// MARK: - Plan Mode

extension AgentViewModel {

    /// Git repo root for plan files.
    private static func planDir(_ projectFolder: String) -> String? {
        let base = projectFolder.isEmpty ? NSHomeDirectory() : resolvedWorkingDirectory(projectFolder)
        var dir = base
        let fm = FileManager.default
        while dir != "/" && !dir.isEmpty {
            let gitDir = (dir as NSString).appendingPathComponent(".git")
            if fm.fileExists(atPath: gitDir) {
                return dir
            }
            dir = (dir as NSString).deletingLastPathComponent
        }
        return nil
    }

    /// Resolve the plan file path for a given plan_id.
    private static func planFilePath(_ planId: String, projectFolder: String) -> String? {
        guard let dir = planDir(projectFolder) else { return nil }
        return (dir as NSString).appendingPathComponent("plan_\(planId).md")
    }

    /// Sanitize a tab name into a safe filename slug.
    static func sanitizeTabName(_ name: String) -> String {
        let slug = name.lowercased()
            .components(separatedBy: CharacterSet.alphanumerics.inverted)
            .filter { !$0.isEmpty }
            .joined(separator: "_")
        return slug.isEmpty ? "main" : slug
    }

    /// Find the most recent plan file in the plans directory.
    private static func mostRecentPlan(_ projectFolder: String) -> (id: String, path: String)? {
        guard let dir = planDir(projectFolder) else { return nil }
        let fm = FileManager.default
        guard let files = try? fm.contentsOfDirectory(atPath: dir) else { return nil }
        let plans = files.filter { $0.hasPrefix("plan_") && $0.hasSuffix(".md") }
        guard !plans.isEmpty else { return nil }
        // Sort by modification date, most recent first
        let sorted = plans.sorted { a, b in
            let pathA = (dir as NSString).appendingPathComponent(a)
            let pathB = (dir as NSString).appendingPathComponent(b)
            let dateA = (try? fm.attributesOfItem(atPath: pathA)[.modificationDate] as? Date) ?? .distantPast
            let dateB = (try? fm.attributesOfItem(atPath: pathB)[.modificationDate] as? Date) ?? .distantPast
            return dateA > dateB
        }
        let filename = sorted[0]
        let id = String(filename.dropFirst(5).dropLast(3)) // strip "plan_" and
        return (id, (dir as NSString).appendingPathComponent(filename))
    }

    /// Handle plan_mode tool calls: create, update, read, list, or delete.
    static func handlePlanMode(
        action: String,
        input: [String: Any],
        projectFolder: String,
        tabName: String = "main",
        userPrompt: String = ""
    ) -> String {

        let fm = FileManager.default
        guard let dir = planDir(projectFolder) else {
            return "Error: plan_mode requires a git repository. Set the project folder to a directory inside a git repo."
        }

        // Accept both "name" and "plan_id" for the plan identifier
        let planIdFromInput = (input["name"] as? String) ?? (input["plan_id"] as? String)

        switch action.lowercased() {
        case "create":
            guard let title = (input["title"] as? String ?? planIdFromInput), !title.isEmpty else {
                return "Error: title or name is required for plan_mode create"
            }
            guard let stepsRaw = input["steps"] as? String, !stepsRaw.isEmpty else {
                return "Error: steps is required for plan_mode create"
            }
            let planId = sanitizeTabName(tabName)

            // Only 1 plan per tab — delete existing plan for this tab first
            let existingFiles = (try? fm.contentsOfDirectory(atPath: dir)) ?? []
            let tabSlug = sanitizeTabName(tabName)
            for file in existingFiles where file.hasPrefix("plan_\(tabSlug)") && file.hasSuffix(".md") {
                try? fm.removeItem(atPath: (dir as NSString).appendingPathComponent(file))
            }
            let steps = stepsRaw.components(separatedBy: "\n").filter { !$0.trimmingCharacters(in: .whitespaces).isEmpty }
            var md = "# \(title)\n\n"
            for (i, step) in steps.enumerated() {
                md += "- [ ] \(i + 1). \(step)\n"
            }
            md += "\n---\n*Status: \(steps.count) steps pending*\n"
            do {
                guard let path = planFilePath(planId, projectFolder: projectFolder) else {
                    return "Error: could not resolve plan file path."
                }
                try md.write(toFile: path, atomically: true, encoding: .utf8)
                // Sync to persistent task queue for crash recovery
                TaskQueueStore.shared.setTasks(steps)
                return "Plan created: \(title) (\(steps.count) steps)\nplan_id: \(planId)\nFile: \(path)"
            } catch {
                return "Error writing plan: \(error.localizedDescription)"
            }

        case "update":
            let rawStep: Int
            if let n = input["step"] as? Int {
                rawStep = n
            } else if let s = input["step"] as? String, let n = Int(s) {
                rawStep = n
            } else {
                return "Error: step number is required for plan_mode update"
            }
            // Be permissive about step indexing. Steps are 1-based, but LLMs fr
            guard rawStep >= 0 else {
                return "Error: step number must be ≥ 0 (steps are 1-based; 0 is accepted as a synonym for 1)"
            }
            let stepNum = max(1, rawStep)
            guard let status = input["status"] as? String else {
                return "Error: status is required for plan_mode update (in_progress, completed, failed)"
            }
            let planId: String
            let path: String
            if let id = planIdFromInput, !id.isEmpty,
               let p = planFilePath(id, projectFolder: projectFolder),
               fm.fileExists(atPath: p)
            {
                // Explicit plan_id that exists on disk
                planId = id
                path = p
            } else {
                // Fall back to this tab's own plan, then most recent
                let tabSlug = sanitizeTabName(tabName)
                if let p = planFilePath(tabSlug, projectFolder: projectFolder), fm.fileExists(atPath: p) {
                    planId = tabSlug
                    path = p
                } else if let recent = mostRecentPlan(projectFolder) {
                    planId = recent.id
                    path = recent.path
                } else {
                    return "Error: no plan found. Use plan_mode create first."
                }
            }
            guard fm.fileExists(atPath: path),
                  let data = fm.contents(atPath: path),
                  let content = String(data: data, encoding: .utf8) else
            {
                let available = (try? fm.contentsOfDirectory(atPath: dir))?
                    .filter { $0.hasPrefix("plan_") && $0.hasSuffix(".md") }
                    .map { "plan_id: " + String($0.dropFirst(5).dropLast(3)) }
                    .joined(separator: ", ") ?? "none"
                return "Error: plan '\(planId)' not found. Available plans: \(available)"
            }

            let marker: String
            switch status.lowercased() {
            case "in_progress": marker = "- [⏳]"
            case "completed": marker = "- [✅]"
            case "failed": marker = "- [❌]"
            default: return "Error: invalid status. Use in_progress, completed, or failed."
            }

            var lines = content.components(separatedBy: "\n")
            let target = "\(stepNum)."
            var found = false
            for i in 0..<lines.count {
                let trimmed = lines[i].trimmingCharacters(in: .whitespaces)
                if trimmed
                    .contains(target) &&
                    (trimmed.hasPrefix("- [") || trimmed.hasPrefix("- [x]") || trimmed.hasPrefix("- [⏳]") || trimmed.hasPrefix("- [❌]"))
                {
                    if let bracketEnd = lines[i].range(of: "] ") {
                        let rest = String(lines[i][bracketEnd.upperBound...])
                        let indent = String(lines[i].prefix(while: { $0 == " " || $0 == "\t" }))
                        lines[i] = "\(indent)\(marker) \(rest)"
                        found = true
                        break
                    }
                }
            }

            guard found else {
                return "Error: step \(stepNum) not found in plan '\(planId)'."
            }

            var completed = 0, inProgress = 0, failed = 0, total = 0
            for line in lines {
                guard line.trimmingCharacters(in: .whitespaces).hasPrefix("- [") else { continue }
                total += 1
                if line.contains("- [✅]") { completed += 1 }
                else if line.contains("- [⏳]") { inProgress += 1 }
                else if line.contains("- [❌]") { failed += 1 }
            }
            let pending = total - completed - inProgress - failed

            if let statusIdx = lines.firstIndex(where: { $0.hasPrefix("*Status:") }) {
                lines[statusIdx] = "*Status: \(completed) done, \(inProgress) in progress, \(failed) failed, \(pending) pending*"
            }

            do {
                try lines.joined(separator: "\n").write(toFile: path, atomically: true, encoding: .utf8)
                // Sync task queue status for crash recovery
                let queue = TaskQueueStore.shared
                if stepNum - 1 < queue.tasks.count {
                    let taskId = queue.tasks[stepNum - 1].id
                    switch status.lowercased() {
                    case "in_progress": queue.start(taskId)
                    case "completed": queue.complete(taskId)
                    case "failed": queue.fail(taskId)
                    default: break
                    }
                }
                return "[\(planId)] Step \(stepNum) → \(status)"
            } catch {
                return "Error writing plan: \(error.localizedDescription)"
            }

        case "read":
            let path: String
            let planId: String
            if let id = planIdFromInput, !id.isEmpty,
               let p = planFilePath(id, projectFolder: projectFolder),
               fm.fileExists(atPath: p)
            {
                // Explicit plan_id that exists on disk
                planId = id
                path = p
            } else {
                // Fall back to this tab's own plan, then most recent
                let tabSlug = sanitizeTabName(tabName)
                if let p = planFilePath(tabSlug, projectFolder: projectFolder), fm.fileExists(atPath: p) {
                    planId = tabSlug
                    path = p
                } else if let recent = mostRecentPlan(projectFolder) {
                    planId = recent.id
                    path = recent.path
                } else {
                    return "No plans found. Use plan_mode create to start a plan."
                }
            }
            guard fm.fileExists(atPath: path),
                  let data = fm.contents(atPath: path),
                  let content = String(data: data, encoding: .utf8) else
            {
                // Include available plans in error so LLM can self-correct
                let available = (try? fm.contentsOfDirectory(atPath: dir))?
                    .filter { $0.hasPrefix("plan_") && $0.hasSuffix(".md") }
                    .map { "plan_id: " + String($0.dropFirst(5).dropLast(3)) }
                    .joined(separator: ", ") ?? "none"
                return "Error: plan '\(planId)' not found. Available plans: \(available)"
            }
            return "plan_id: \(planId) (use this ID for read/update/delete)\n\(content)"

        case "list":
            guard let files = try? fm.contentsOfDirectory(atPath: dir) else {
                return "No plans directory found."
            }
            let plans = files.filter { $0.hasPrefix("plan_") && $0.hasSuffix(".md") }.sorted()
            if plans.isEmpty { return "No plans found." }
            return plans.map { filename in
                let id = String(filename.dropFirst(5).dropLast(3))
                let path = (dir as NSString).appendingPathComponent(filename)
                // Read first line for title
                let title: String
                if let data = fm.contents(atPath: path),
                   let content = String(data: data, encoding: .utf8),
                   let firstLine = content.components(separatedBy: "\n").first,
                   firstLine.hasPrefix("# ")
                {
                    title = String(firstLine.dropFirst(2))
                } else {
                    title = id
                }
                return "plan_id: \(id) — \(title)"
            }.joined(separator: "\n")

        case "delete":
            guard let id = planIdFromInput, !id.isEmpty else {
                return "Error: name is required for plan_mode delete"
            }
            guard let path = planFilePath(id, projectFolder: projectFolder) else {
                return "Error: could not resolve plan file path."
            }
            guard fm.fileExists(atPath: path) else {
                return "Error: plan '\(id)' not found."
            }
            do {
                try fm.removeItem(atPath: path)
                return "Deleted plan: \(id)"
            } catch {
                return "Error deleting plan: \(error.localizedDescription)"
            }

        default:
            return "Error: invalid action '\(action)'. Use create, update, read, list, or delete."
        }
    }
}
