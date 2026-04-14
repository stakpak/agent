import AgentAudit
import Foundation
import Darwin

final class ScriptService: @unchecked Sendable {
    static let agentsDir: URL = {
        let home = FileManager.default.homeDirectoryForCurrentUser
        return home.appendingPathComponent("Documents/AgentScript/agents")
    }()

    private var sourcesDir: URL { Self.agentsDir.appendingPathComponent("Sources") }
    var scriptsDir: URL { sourcesDir.appendingPathComponent("Scripts") }

      /// Static accessor for the scripts directory — used by the consolidated tool
      /// dispatcher to resolve `agent_script(action:"edit")` into a full file_path.
    static var scriptsDirURL: URL { agentsDir.appendingPathComponent("Sources/Scripts") }

    /// Directory for saved AppleScript files
    static let applescriptDir: URL = {
        let home = FileManager.default.homeDirectoryForCurrentUser
        return home.appendingPathComponent("Documents/AgentScript/applescript")
    }()

    /// Directory for saved JavaScript (JXA) files
    static let javascriptDir: URL = {
        let home = FileManager.default.homeDirectoryForCurrentUser
        return home.appendingPathComponent("Documents/AgentScript/javascript")
    }()

    struct ScriptInfo {
        let name: String
        let path: String
        let modifiedDate: Date
        let size: Int
    }

    // MARK: - Thread Safety

    /// Lock to prevent concurrent Package.swift modifications
    let packageLock = NSLock()

    /// Serial queue for script compilation (prevents concurrent swift build calls)
    nonisolated static let compilationQueue = DispatchQueue(label: "com.agent.scriptcompilation", qos: .userInitiated)

    // MARK: - Remote repos

    private static let scriptsRepoURL = "https://github.com/macOS26/AgentScripts.git"
    private static let bridgesRepoURL = "https://github.com/macOS26/AgentEventBridges.git"

      /// Pinned AgentScripts release tag. Bump when a new release ships.
      /// Pull/sync URLs use this tag — users get an immutable snapshot, not main HEAD.
    static let scriptsRelease = "1.0.6"

    /// Pinned AgentEventBridges release tag. Same rationale as scriptsRelease — every
    /// user clones the same immutable bridges snapshot rather than main HEAD.
    static let bridgesRelease = "1.1.0"

      /// Raw GitHub URL prefix for pulling individual script files (single-file recovery
      /// without a full clone). Uses pinned release tag for immutability.
    static let scriptsRawURLPrefix = "https://raw.githubusercontent.com/macOS26/AgentScripts/refs/tags/\(scriptsRelease)/Agent/agents/Sources/Scripts"

    /// Installed location: ~/Documents/AgentScript/bridges/
    static let installedBridgesPath: URL = {
        FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Documents/AgentScript/bridges")
    }()
    // MARK: - Package.swift generation

    /// Generate a clean Package.swift from the actual files on disk.
    /// Uses AgentEventBridges package dependency for bridge modules.
    func generatePackageSwift() {
        let fm = FileManager.default

        // Discover scripts on disk
        let scriptFiles = (try? fm.contentsOfDirectory(atPath: scriptsDir.path)) ?? []
        let scriptNames = scriptFiles
            .filter { $0.hasSuffix(".swift") }
            .map { $0.replacingOccurrences(of: ".swift", with: "") }
            .filter { !$0.isEmpty }
            .sorted()

        let scriptList = scriptNames.map { "    \"\($0)\"," }.joined(separator: "\n")

        // Read bridge names from the installed copy at ~/Documents/AgentScript/bridges/
        let bridgesPackagePath = Self.installedBridgesPath.appendingPathComponent("Sources/AgentEventBridges")
        let bridgeNames: [String] = {
            let fm = FileManager.default
            guard let files = try? fm.contentsOfDirectory(atPath: bridgesPackagePath.path) else { return [] }
            return files
                .filter { $0.hasSuffix("Bridge.swift") && $0 != "ScriptingBridgeCommon.swift" }
                .map { $0.replacingOccurrences(of: ".swift", with: "") }
                .sorted()
        }()

        let content = """
        // swift-tools-version: 6.2
        import PackageDescription
        import Foundation

        // Scripts compile as dynamic libraries (.dylib) loaded into Agent! via dlopen.
        // ScriptService adds/removes entries when scripts are created/deleted.
        let scriptNames = [
        \(scriptList)
        ]

        // Bridge names match those in AgentEventBridges package
        let bridgeNames = [
        \(bridgeNames.map { "    \"\($0)\"," }.joined(separator: "\n"))
        ]

        let scripts = "Sources/Scripts"
        let bridgeNameSet = Set(bridgeNames)

        // Local package dependency for shared bridges (installed at ~/Documents/AgentScript/bridges/)
        let packageDependencies: [PackageDescription.Package.Dependency] = [
            .package(name: "AgentEventBridges", path: "\(Self.installedBridgesPath.path)")
        ]

        // Build Target.Dependency for each bridge (explicit package reference)
        func bridgeDep(_ name: String) -> Target.Dependency {
            .product(name: name, package: "AgentEventBridges")
        }

        // Auto-detect bridge imports in each script
        func parseDeps(for name: String) -> [Target.Dependency] {
            let url = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
                .appendingPathComponent(scripts).appendingPathComponent("\\(name).swift")
            guard let contents = try? String(contentsOf: url, encoding: .utf8) else { return [] }
            var deps: [Target.Dependency] = []
            for line in contents.components(separatedBy: .newlines) {
                let trimmed = line.trimmingCharacters(in: .whitespaces)
                if trimmed.hasPrefix("import ") {
                    let module = String(trimmed.dropFirst(7)).trimmingCharacters(in: .whitespaces)
                    if bridgeNameSet.contains(module) {
                        deps.append(bridgeDep(module))
                    } else if module == "ScriptingBridgeCommon" {
                        deps.append(bridgeDep("ScriptingBridgeCommon"))
                    } else if module == "AgentAccessibility" {
                        deps.append(.init(stringLiteral: "AgentAccessibility"))
                    }
                }
                if !trimmed.isEmpty && !trimmed.hasPrefix("import ") &&
                   !trimmed.hasPrefix("//") && !trimmed.hasPrefix("@") {
                    break
                }
            }
            return deps
        }

        let allScriptFiles = scriptNames.map { "\\($0).swift" }

        let scriptProducts: [Product] = scriptNames.map {
            .library(name: $0, type: .dynamic, targets: [$0])
        }

        let coreTargets: [Target] = [
            .target(name: "AgentAccessibility", path: "Sources/AgentAccessibility"),
        ]

        let scriptTargets: [Target] = scriptNames.map { name in
            .target(name: name, dependencies: parseDeps(for: name), path: scripts,
                    exclude: allScriptFiles.filter { $0 != "\\(name).swift" },
                    sources: ["\\(name).swift"])
        }

        let package = Package(
            name: "agents",
            platforms: [.macOS(.v26)],
            products: scriptProducts,
            dependencies: packageDependencies,
            targets: coreTargets + scriptTargets
        )
        """

        // Remove leading whitespace from each line (heredoc indentation)
        let trimmed = content.components(separatedBy: "\n")
            .map { line in
                var s = line
                while s.hasPrefix("        ") { s = String(s.dropFirst(8)) }
                return s
            }
            .joined(separator: "\n")

        try? trimmed.write(to: packageSwiftURL, atomically: true, encoding: .utf8)
    }

    // MARK: - Ensure package

    /// Ensure ~/Documents/AgentScript/agents/ exists with scripts and Package.swift
    func ensurePackage() {
        packageLock.lock()
        defer { packageLock.unlock() }

        let fm = FileManager.default
        let agentsPath = Self.agentsDir.path

        // Migrate: rename AgentEventBridges → bridges and regenerate Package.swift
        let oldBridgesPath = fm.homeDirectoryForCurrentUser.appendingPathComponent("Documents/AgentScript/AgentEventBridges")
        var didMigrateBridges = false
        if fm.fileExists(atPath: oldBridgesPath.path) && !fm.fileExists(atPath: Self.installedBridgesPath.path) {
            try? fm.moveItem(at: oldBridgesPath, to: Self.installedBridgesPath)
            didMigrateBridges = true
        }

        if !fm.fileExists(atPath: agentsPath) {
            // Fresh install — clone scripts from GitHub
            cloneScriptsRepo()
            generatePackageSwift()
        } else {
            // Existing install — update bridges
            cloneBridgesRepo()
            if didMigrateBridges {
                generatePackageSwift()
            }
        }
        createOutputFolders()
    }


    // MARK: - JSON files

    /// The parent directory ~/Documents/AgentScript/ where JSON input/output files live
    static let agentDir: URL = {
        let home = FileManager.default.homeDirectoryForCurrentUser
        return home.appendingPathComponent("Documents/AgentScript")
    }()

    /// Create organized output subfolders in ~/Documents/AgentScript/
    private func createOutputFolders() {
        let fm = FileManager.default
        try? fm.createDirectory(at: Self.agentDir, withIntermediateDirectories: true)
        for sub in ["json", "photos", "images", "screenshots", "html", "applescript", "javascript", "logs", "recordings"] {
            try? fm.createDirectory(at: Self.agentDir.appendingPathComponent(sub), withIntermediateDirectories: true)
        }
    }

      // MARK: - Git clone AgentScripts (→ ~/Documents/AgentScript/agents/Sources/) and AgentEventBridges (→
      // ~/Documents/AgentScript/bridges/Sources/) are cloned from their GitHub repos on first use, not bundled at build.

    /// Clone AgentScripts repo to ~/Documents/AgentScript/agents/
    private func cloneScriptsRepo() {
        let dest = Self.agentsDir
        let fm = FileManager.default
        if fm.fileExists(atPath: dest.appendingPathComponent("Sources").path) { return }
        let tmpDir = fm.temporaryDirectory.appendingPathComponent("AgentScripts_clone")
        try? fm.removeItem(at: tmpDir)
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/git")
        process.arguments = ["clone", "--depth", "1", "--branch", Self.scriptsRelease, Self.scriptsRepoURL, tmpDir.path]
        process.currentDirectoryURL = URL(fileURLWithPath: NSHomeDirectory())
        var env = ProcessInfo.processInfo.environment
        env["HOME"] = NSHomeDirectory()
        let extraPaths = "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
        env["PATH"] = extraPaths + ":" + (env["PATH"] ?? "")
        process.environment = env
        try? process.run()
        process.waitUntilExit()
        // Move cloned Agent/agents/Sources into place
        let clonedSources = tmpDir.appendingPathComponent("Agent/agents/Sources")
        try? fm.createDirectory(at: dest, withIntermediateDirectories: true)
        if fm.fileExists(atPath: clonedSources.path) {
            try? fm.copyItem(at: clonedSources, to: dest.appendingPathComponent("Sources"))
        }
        try? fm.removeItem(at: tmpDir)
        // Also clone bridges
        cloneBridgesRepo()
    }

    /// Clone AgentEventBridges repo to ~/Documents/AgentScript/bridges/.
    /// Pinned to `bridgesRelease` so every user clones the same immutable snapshot.
    private func cloneBridgesRepo() {
        let dest = Self.installedBridgesPath
        let fm = FileManager.default
        if fm.fileExists(atPath: dest.appendingPathComponent("Sources").path) { return }
        try? fm.removeItem(at: dest)
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/git")
        process.arguments = ["clone", "--depth", "1", "--branch", Self.bridgesRelease, Self.bridgesRepoURL, dest.path]
        process.currentDirectoryURL = URL(fileURLWithPath: NSHomeDirectory())
        var cloneEnv = ProcessInfo.processInfo.environment
        cloneEnv["HOME"] = NSHomeDirectory()
        let cloneExtraPaths = "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
        cloneEnv["PATH"] = cloneExtraPaths + ":" + (cloneEnv["PATH"] ?? "")
        process.environment = cloneEnv
        try? process.run()
        process.waitUntilExit()
    }

    // MARK: - Deleted scripts blocklist

    private static let deletedScriptsKey = "agentDeletedScripts"

    private var deletedScripts: Set<String> {
        Set(UserDefaults.standard.stringArray(forKey: Self.deletedScriptsKey) ?? [])
    }

    func markScriptDeleted(_ name: String) {
        var deleted = deletedScripts
        deleted.insert(name)
        UserDefaults.standard.set(Array(deleted), forKey: Self.deletedScriptsKey)
    }

    func unmarkScriptDeleted(_ name: String) {
        var deleted = deletedScripts
        deleted.remove(name)
        UserDefaults.standard.set(Array(deleted), forKey: Self.deletedScriptsKey)
    }


    // MARK: - Helpers

    /// Returns true if source file has a newer modification date than destination
    private func isNewer(_ src: URL, than dst: URL) -> Bool {
        let fm = FileManager.default
        guard let srcAttrs = try? fm.attributesOfItem(atPath: src.path),
              let dstAttrs = try? fm.attributesOfItem(atPath: dst.path),
              let srcDate = srcAttrs[.modificationDate] as? Date,
              let dstDate = dstAttrs[.modificationDate] as? Date else { return true }
        return srcDate > dstDate
    }

    // MARK: - Script CRUD

    /// List all scripts in Sources/Scripts/
    func listScripts() -> [ScriptInfo] {
        ensurePackage()
        let fm = FileManager.default
        guard let files = try? fm.contentsOfDirectory(atPath: scriptsDir.path) else { return [] }

        return files.filter { $0.hasSuffix(".swift") }.sorted().compactMap { file in
            let path = scriptsDir.appendingPathComponent(file).path
            guard let attrs = try? fm.attributesOfItem(atPath: path) else { return nil }
            let name = file.replacingOccurrences(of: ".swift", with: "")
            return ScriptInfo(
                name: name,
                path: path,
                modifiedDate: attrs[.modificationDate] as? Date ?? Date(),
                size: attrs[.size] as? Int ?? 0
            )
        }
    }

    /// Format scripts as a numbered list
    func numberedList() -> String {
        let scripts = listScripts()
        guard !scripts.isEmpty else { return "No agents found in ~/Documents/AgentScript/agents/" }
        return scripts.enumerated().map { "#\($0.offset + 1) \($0.element.name) (\($0.element.size) bytes)" }.joined(separator: "\n")
    }

    /// Compact comma-separated list of agent names (for LLM context injection)
    func compactNameList() -> String {
        let names = listScripts().map { $0.name }
        guard !names.isEmpty else { return "" }
        return names.joined(separator: ", ")
    }

    /// Resolve a script name — accepts a name or a number like "#4" or "4"
    func resolveScriptName(_ input: String) -> String {
        let trimmed = input.trimmingCharacters(in: .whitespaces)
        let numStr = trimmed.hasPrefix("#") ? String(trimmed.dropFirst()) : trimmed
        if let num = Int(numStr) {
            let scripts = listScripts()
            if num >= 1 && num <= scripts.count {
                return scripts[num - 1].name
            }
        }
        return trimmed
    }

}
