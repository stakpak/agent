
@preconcurrency import Foundation
import AgentTools

// MARK: - Native Tool Handler — Project Index

extension AgentViewModel {

    /// Handles index_* tool calls (expanded from `index(action:X)` via
    /// expandConsolidatedTool). Returns `nil` if the name isn't an index tool.
    func handleIndexNativeTool(name: String, input: [String: Any]) async -> String? {
        let pf = projectFolder.trimmingCharacters(in: .whitespaces)
        guard !pf.isEmpty else {
            // Only answer if this IS an index call — otherwise return nil so the
            // dispatcher continues through other handlers.
            if name.hasPrefix("index_") {
                return "⚠️ No project folder selected. Recovery: set a project folder first (directory tool or the folder picker)."
            }
            return nil
        }

        let extensions: Set<String>? = {
            guard let raw = input["extensions"] as? String, !raw.isEmpty else { return nil }
            return Set(raw.split(separator: ",").map {
                $0.trimmingCharacters(in: .whitespaces).lowercased()
            }.filter { !$0.isEmpty })
        }()
        let maxFileSize = input["max_file_size"] as? Int ?? ProjectIndexService.defaultMaxFileSize

        switch name {
        case "index_create":
            do {
                let r = try ProjectIndexService.create(
                    projectFolder: pf,
                    extensions: extensions,
                    maxFileSize: maxFileSize,
                    overwrite: false
                )
                let file = ProjectIndexService.indexFile(in: pf).path
                return "✅ Created index at \(file) — \(r.fileCount) files, \(r.bytes) bytes."
            } catch {
                return "❌ \(error.localizedDescription)"
            }

        case "index_recreate":
            do {
                let r = try ProjectIndexService.create(
                    projectFolder: pf,
                    extensions: extensions,
                    maxFileSize: maxFileSize,
                    overwrite: true
                )
                let file = ProjectIndexService.indexFile(in: pf).path
                return "✅ Recreated index at \(file) — \(r.fileCount) files, \(r.bytes) bytes."
            } catch {
                return "❌ Recreate failed: \(error.localizedDescription)"
            }

        case "index_append", "index_continue":
            do {
                let r = try ProjectIndexService.append(
                    projectFolder: pf,
                    extensions: extensions,
                    maxFileSize: maxFileSize
                )
                let verb = name == "index_continue" ? "Resumed" : "Appended"
                return "✅ \(verb) index — +\(r.added) new, \(r.updated) updated, \(r.total) total."
            } catch {
                return "❌ \(name.replacingOccurrences(of: "index_", with: "")) failed: \(error.localizedDescription)"
            }

        case "index_read":
            do {
                let offset = input["offset"] as? Int ?? 1
                let limit = input["limit"] as? Int ?? 500
                let content = try ProjectIndexService.read(
                    projectFolder: pf,
                    offset: offset,
                    limit: limit
                )
                if content.isEmpty {
                    let file = ProjectIndexService.indexFile(in: pf).path
                    return "(no index found at \(file)). Recovery: call index(action:\"create\") first."
                }
                return content
            } catch {
                return "❌ Read failed: \(error.localizedDescription)"
            }

        case "index_remove":
            do {
                try ProjectIndexService.remove(projectFolder: pf)
                return "🧹 Index removed."
            } catch {
                return "❌ Remove failed: \(error.localizedDescription)"
            }

        default:
            return nil
        }
    }
}
