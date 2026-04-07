import Foundation

extension ScriptService {
    // MARK: - Saved AppleScripts (~/Documents/AgentScript/applescript/)

    /// List all saved .applescript files
    func listAppleScripts() -> [ScriptInfo] {
        let fm = FileManager.default
        let dir = Self.applescriptDir
        try? fm.createDirectory(at: dir, withIntermediateDirectories: true)
        guard let files = try? fm.contentsOfDirectory(atPath: dir.path) else { return [] }

        return files.filter { $0.hasSuffix(".applescript") }.sorted().compactMap { file in
            let path = dir.appendingPathComponent(file).path
            guard let attrs = try? fm.attributesOfItem(atPath: path) else { return nil }
            let name = file.replacingOccurrences(of: ".applescript", with: "")
            return ScriptInfo(
                name: name,
                path: path,
                modifiedDate: attrs[.modificationDate] as? Date ?? Date(),
                size: attrs[.size] as? Int ?? 0
            )
        }
    }

    /// Read a saved AppleScript's source
    func readAppleScript(name: String) -> String? {
        let scriptName = name.replacingOccurrences(of: ".applescript", with: "")
        let file = Self.applescriptDir.appendingPathComponent("\(scriptName).applescript")
        return try? String(contentsOf: file, encoding: .utf8)
    }

    /// Save an AppleScript to disk (create or overwrite)
    func saveAppleScript(name: String, source: String) -> String {
        let fm = FileManager.default
        let dir = Self.applescriptDir
        try? fm.createDirectory(at: dir, withIntermediateDirectories: true)

        let scriptName = name.replacingOccurrences(of: ".applescript", with: "")
        let file = dir.appendingPathComponent("\(scriptName).applescript")
        do {
            try source.write(to: file, atomically: true, encoding: .utf8)
            return "Saved \(scriptName).applescript (\(source.count) bytes)"
        } catch {
            return "Error saving: \(error.localizedDescription)"
        }
    }

    /// Delete a saved AppleScript
    func deleteAppleScript(name: String) -> String {
        let scriptName = name.replacingOccurrences(of: ".applescript", with: "")
        let file = Self.applescriptDir.appendingPathComponent("\(scriptName).applescript")
        guard FileManager.default.fileExists(atPath: file.path) else {
            return "Error: '\(scriptName)' not found"
        }
        do {
            try FileManager.default.removeItem(at: file)
            return "Deleted \(scriptName).applescript"
        } catch {
            return "Error: \(error.localizedDescription)"
        }
    }

    // MARK: - Saved JavaScript/JXA (~/Documents/AgentScript/javascript/)

    func listJavaScripts() -> [ScriptInfo] {
        let fm = FileManager.default
        let dir = Self.javascriptDir
        try? fm.createDirectory(at: dir, withIntermediateDirectories: true)
        guard let files = try? fm.contentsOfDirectory(atPath: dir.path) else { return [] }

        return files.filter { $0.hasSuffix(".js") }.sorted().compactMap { file in
            let path = dir.appendingPathComponent(file).path
            guard let attrs = try? fm.attributesOfItem(atPath: path) else { return nil }
            let name = file.replacingOccurrences(of: ".js", with: "")
            return ScriptInfo(
                name: name,
                path: path,
                modifiedDate: attrs[.modificationDate] as? Date ?? Date(),
                size: attrs[.size] as? Int ?? 0
            )
        }
    }

    func readJavaScript(name: String) -> String? {
        let scriptName = name.replacingOccurrences(of: ".js", with: "")
        let file = Self.javascriptDir.appendingPathComponent("\(scriptName).js")
        return try? String(contentsOf: file, encoding: .utf8)
    }

    func saveJavaScript(name: String, source: String) -> String {
        let fm = FileManager.default
        let dir = Self.javascriptDir
        try? fm.createDirectory(at: dir, withIntermediateDirectories: true)
        let scriptName = name.replacingOccurrences(of: ".js", with: "")
        let file = dir.appendingPathComponent("\(scriptName).js")
        do {
            try source.write(to: file, atomically: true, encoding: .utf8)
            return "Saved \(scriptName).js (\(source.count) bytes)"
        } catch {
            return "Error saving: \(error.localizedDescription)"
        }
    }

    func deleteJavaScript(name: String) -> String {
        let scriptName = name.replacingOccurrences(of: ".js", with: "")
        let file = Self.javascriptDir.appendingPathComponent("\(scriptName).js")
        guard FileManager.default.fileExists(atPath: file.path) else {
            return "Error: '\(scriptName)' not found"
        }
        do {
            try FileManager.default.removeItem(at: file)
            return "Deleted \(scriptName).js"
        } catch {
            return "Error: \(error.localizedDescription)"
        }
    }
}
