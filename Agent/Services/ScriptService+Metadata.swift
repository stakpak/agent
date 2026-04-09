import AgentAudit
import CryptoKit
import Foundation

extension ScriptService {
    // MARK: - Script Metadata

    /// Cached metadata about script requirements (persisted in UserDefaults)
    private static let metadataKey = "agentScriptMetadata"

    struct ScriptMetadata: Codable {
        var requiresArguments: Bool
        var requiresInput: Bool // stdin / readLine / JSON input
        var lastModified: Date
    }

    /// Scan script source and return metadata about its requirements
    static func analyzeScript(_ source: String) -> ScriptMetadata {
        let requiresArgs = source.contains("CommandLine.arguments")
            || source.contains("ProcessInfo.processInfo.arguments")
            || source.contains("CommandLine.argc")
        let requiresInput = source.contains("readLine(")
            || source.contains("FileHandle.standardInput")
            || source.contains("stdin")
            || source.contains("JSONDecoder()")
            || source.contains("Codable")
        return ScriptMetadata(requiresArguments: requiresArgs, requiresInput: requiresInput, lastModified: Date())
    }

    /// Store metadata for a script in UserDefaults
    func updateMetadata(name: String, source: String) {
        let meta = Self.analyzeScript(source)
        var all = loadAllMetadata()
        all[name] = meta
        if let data = try? JSONEncoder().encode(all) {
            UserDefaults.standard.set(data, forKey: Self.metadataKey)
        }
    }

    /// Load all script metadata from UserDefaults
    func loadAllMetadata() -> [String: ScriptMetadata] {
        guard let data = UserDefaults.standard.data(forKey: Self.metadataKey),
              let dict = try? JSONDecoder().decode([String: ScriptMetadata].self, from: data) else
        {
            return [:]
        }
        return dict
    }

    /// Get metadata for a single script. Returns nil if not yet analyzed.
    func metadata(for name: String) -> ScriptMetadata? {
        return loadAllMetadata()[name]
    }

    /// Check if a script can run without arguments (safe for direct execution)
    func canRunDirectly(name: String) -> Bool {
        guard let meta = metadata(for: name) else {
            // Not yet analyzed — scan it now
            if let source = readScript(name: name) {
                updateMetadata(name: name, source: source)
                let fresh = Self.analyzeScript(source)
                return !fresh.requiresArguments && !fresh.requiresInput
            }
            return false
        }
        return !meta.requiresArguments && !meta.requiresInput
    }

    /// Rebuild metadata for all scripts (call on app launch or after bulk changes)
    func rebuildAllMetadata() {
        var all: [String: ScriptMetadata] = [:]
        for script in listScripts() {
            if let source = readScript(name: script.name) {
                all[script.name] = Self.analyzeScript(source)
            }
        }
        if let data = try? JSONEncoder().encode(all) {
            UserDefaults.standard.set(data, forKey: Self.metadataKey)
        }
    }

    /// Read a script's source code
    func readScript(name: String) -> String? {
        ensurePackage()
        let scriptName = name.replacingOccurrences(of: ".swift", with: "")
        let scriptFile = scriptsDir.appendingPathComponent("\(scriptName).swift")
        return try? String(contentsOf: scriptFile, encoding: .utf8)
    }

    /// Strip any shebang line — scripts are compiled via swift build, not run directly
    func stripShebang(_ content: String) -> String {
        if content.hasPrefix("#!/") {
            if let newline = content.firstIndex(of: "\n") {
                return String(content[content.index(after: newline)...])
            }
        }
        return content
    }

    /// Convert a name to UpperCamelCase: "compress_dmg" → "CompressDmg", "hello" → "Hello"
    static func toUpperCamelCase(_ name: String) -> String {
        let parts = name.split(whereSeparator: { $0 == "_" || $0 == "-" || $0 == " " })
        if parts.isEmpty { return name }
        return parts.map { $0.prefix(1).uppercased() + $0.dropFirst() }.joined()
    }

    /// Create a new script as Sources/Scripts/{name}.swift and register in Package.swift
    func createScript(name: String, content: String) -> String {
        AuditLog.log(.agentScript, "createScript: \(name)")
        // Ensure package exists first (without lock - just creates directories)
        ensurePackage()

        let raw = name.replacingOccurrences(of: ".swift", with: "")
            .replacingOccurrences(of: ".md", with: "")
        guard !raw.isEmpty else {
            return "Error: script name cannot be empty."
        }
        // Auto-convert to UpperCamelCase
        let scriptName = Self.toUpperCamelCase(raw)
        // Reject invalid names: pure numbers, names with dots, tool name conflicts
        let invalidNames: Set<String> = [
            "ListAgents",
            "RunAgent",
            "ReadAgent",
            "CreateAgent",
            "UpdateAgent",
            "DeleteAgent",
            "CombineAgents",
            "Agent"
        ]
        if Int(scriptName) != nil {
            return "Error: script name cannot be a number. Use a descriptive name like 'MyScript'."
        }
        if scriptName.contains(".") {
            return "Error: script name cannot contain dots."
        }
        if invalidNames.contains(scriptName) {
            return "Error: '\(scriptName)' is a reserved tool name."
        }
        let scriptFile = scriptsDir.appendingPathComponent("\(scriptName).swift")
        let fm = FileManager.default

        if fm.fileExists(atPath: scriptFile.path) {
            return "Error: script '\(scriptName)' already exists. Use update_agent to modify it."
        }

        let final = stripShebang(content)
        do {
            try fm.createDirectory(at: scriptsDir, withIntermediateDirectories: true)
            try final.write(to: scriptFile, atomically: true, encoding: .utf8)
            unmarkScriptDeleted(scriptName)
            updateMetadata(name: scriptName, source: final)

            // Regenerate Package.swift to include the new script
            packageLock.lock()
            defer { packageLock.unlock() }
            generatePackageSwift()
            return "Created \(scriptName) (\(final.count) bytes). Registered in Package.swift."
        } catch {
            return "Error creating script: \(error.localizedDescription)"
        }
    }

    /// Update an existing script
    func updateScript(name: String, content: String) -> String {
        let scriptName = name.replacingOccurrences(of: ".swift", with: "")
        let scriptFile = scriptsDir.appendingPathComponent("\(scriptName).swift")
        let fm = FileManager.default

        if !fm.fileExists(atPath: scriptFile.path) {
            return "Error: script '\(scriptName)' not found. Use create_agent to create it."
        }

        let final = stripShebang(content)
        do {
            try final.write(to: scriptFile, atomically: true, encoding: .utf8)
            updateMetadata(name: scriptName, source: final)
            return "Updated \(scriptName) (\(final.count) bytes)"
        } catch {
            return "Error updating script: \(error.localizedDescription)"
        }
    }

    /// Trash directory for deleted agent scripts. Lives next to Package.swift so users
    /// can recover via `restoreScript(name:)`.
    var scriptsTrashDir: URL { Self.agentsDir.appendingPathComponent(".Trash") }

    /// Return all backups for a given script name (or all backups if name is empty),
    /// sorted newest-first by filename timestamp.
    func listScriptBackups(name: String = "") -> [URL] {
        let fm = FileManager.default
        guard let entries = try? fm.contentsOfDirectory(at: scriptsTrashDir, includingPropertiesForKeys: nil) else {
            return []
        }
        let scriptName = name.replacingOccurrences(of: ".swift", with: "")
        let filtered = entries.filter { url in
            guard url.pathExtension == "swift" else { return false }
            if scriptName.isEmpty { return true }
            // Match "<scriptName>-<timestamp>.swift"
            return url.lastPathComponent.hasPrefix("\(scriptName)-")
        }
        return filtered.sorted { $0.lastPathComponent > $1.lastPathComponent }
    }

      /// Restore the most recent backup of `name` into Sources/Scripts/.
      /// Refuses to overwrite an existing live script — caller must delete first.
    func restoreScript(name: String, backupFilename: String? = nil) -> String {
        AuditLog.log(.agentScript, "restoreScript: \(name) backup=\(backupFilename ?? "latest")")
        let scriptName = name.replacingOccurrences(of: ".swift", with: "")
        let scriptFile = scriptsDir.appendingPathComponent("\(scriptName).swift")
        let fm = FileManager.default

        // Don't clobber a live script
        if fm.fileExists(atPath: scriptFile.path) {
            return "Error: '\(scriptName)' already exists. Recovery: call agent_script(action:\"delete\", name:\"\(scriptName)\") first (delete creates a backup automatically), then retry restore."
        }

        // Locate the backup
        let backupURL: URL
        if let explicit = backupFilename {
            let candidate = scriptsTrashDir.appendingPathComponent(explicit)
            guard fm.fileExists(atPath: candidate.path) else {
                return "Error: backup '\(explicit)' not found in .Trash. Recovery: call agent_script(action:\"list_backups\", name:\"\(scriptName)\") to see available backup filenames, then retry."
            }
            backupURL = candidate
        } else {
            let backups = listScriptBackups(name: scriptName)
            guard let latest = backups.first else {
                return "Error: no backups found for '\(scriptName)'. Recovery: call agent_script(action:\"pull\", name:\"\(scriptName)\") to fetch the upstream version from AgentScripts instead."
            }
            backupURL = latest
        }

        // Restore
        do {
            try fm.createDirectory(at: scriptsDir, withIntermediateDirectories: true)
            try fm.copyItem(at: backupURL, to: scriptFile)
            let content = (try? String(contentsOf: scriptFile, encoding: .utf8)) ?? ""
            unmarkScriptDeleted(scriptName)
            updateMetadata(name: scriptName, source: content)
            packageLock.lock()
            defer { packageLock.unlock() }
            generatePackageSwift()
            return "Restored \(scriptName) from \(backupURL.lastPathComponent). Re-registered in Package.swift."
        } catch {
            return "Error restoring script: \(error.localizedDescription)"
        }
    }

      /// Copy a script to the trash with a timestamp suffix before deletion.
      /// Best-effort — failures are logged but don't block the delete.
    @discardableResult
    private func backupScriptToTrash(_ scriptFile: URL, scriptName: String) -> URL? {
        let fm = FileManager.default
        do {
            try fm.createDirectory(at: scriptsTrashDir, withIntermediateDirectories: true)
            let formatter = DateFormatter()
            formatter.dateFormat = "yyyyMMdd-HHmmss"
            let stamp = formatter.string(from: Date())
            let backupURL = scriptsTrashDir.appendingPathComponent("\(scriptName)-\(stamp).swift")
            try fm.copyItem(at: scriptFile, to: backupURL)
            return backupURL
        } catch {
            AuditLog.log(.agentScript, "deleteScript backup failed for \(scriptName): \(error.localizedDescription)")
            return nil
        }
    }

    // MARK: - Bundled Script Sync

    private static let lastSyncedAgentVersionKey = "agentLastSyncedBundledVersion"
    private static let bundledCatalogAPIURL =
        "https://api.github.com/repos/macOS26/AgentScripts/contents/Agent/agents/Sources/Scripts"

    /// Refresh upstream-bundled scripts on app launch when version changes.
    /// Version-gated, catalog-driven, SHA-compared, backed up before replacing,
    /// silent on failure, and never auto-resurrects locally-deleted scripts.
    func syncBundledScriptsFromRemote() async {
        AuditLog.log(.agentScript, "syncBundledScriptsFromRemote: start")
        // Include both short version and build number so a TestFlight build bump
        // (same X.Y.Z, new build) re-syncs. Resilient to UserDefaults sync across
        // machines too — different machines with different builds re-sync.
        let info = Bundle.main.infoDictionary
        let shortVersion = info?["CFBundleShortVersionString"] as? String ?? "unknown"
        let buildNumber = info?["CFBundleVersion"] as? String ?? "0"
        let currentVersion = "\(shortVersion)+\(buildNumber)"
        let lastSynced = UserDefaults.standard.string(forKey: Self.lastSyncedAgentVersionKey) ?? ""
        if lastSynced == currentVersion {
            AuditLog.log(.agentScript, "syncBundledScriptsFromRemote: skip (\(currentVersion) already synced)")
            return
        }

        // Fetch upstream catalog
        guard let catalogURL = URL(string: Self.bundledCatalogAPIURL) else {
            AuditLog.log(.agentScript, "syncBundledScriptsFromRemote: bad catalog URL")
            return
        }
        var req = URLRequest(url: catalogURL)
        req.setValue("application/vnd.github+json", forHTTPHeaderField: "Accept")
        let catalog: [[String: Any]]
        do {
            let (data, response) = try await URLSession.shared.data(for: req)
            guard let http = response as? HTTPURLResponse else {
                AuditLog.log(.agentScript, "syncBundledScriptsFromRemote: invalid catalog response")
                return
            }
            guard http.statusCode == 200 else {
                AuditLog.log(.agentScript, "syncBundledScriptsFromRemote: catalog HTTP \(http.statusCode)")
                return
            }
            guard let parsed = try JSONSerialization.jsonObject(with: data) as? [[String: Any]] else {
                AuditLog.log(.agentScript, "syncBundledScriptsFromRemote: catalog parse failed")
                return
            }
            catalog = parsed
        } catch {
            AuditLog.log(.agentScript, "syncBundledScriptsFromRemote: catalog error \(error.localizedDescription)")
            return
        }

        let fm = FileManager.default
        var replaced = 0
        var checked = 0

        for entry in catalog {
            guard let name = entry["name"] as? String, name.hasSuffix(".swift") else { continue }
            guard let upstreamSHA = entry["sha"] as? String else { continue }
            guard let downloadURLString = entry["download_url"] as? String,
                  let downloadURL = URL(string: downloadURLString) else { continue }

            let scriptName = name.replacingOccurrences(of: ".swift", with: "")
            let localFile = scriptsDir.appendingPathComponent(name)

            // Skip files the user has deleted — don't auto-resurrect
            guard fm.fileExists(atPath: localFile.path) else { continue }
            guard let localContent = try? String(contentsOf: localFile, encoding: .utf8) else { continue }

            checked += 1

            let localSHA = Self.gitBlobSHA1(of: localContent)
            if localSHA == upstreamSHA { continue }

            // Local differs from upstream — fetch upstream content
            do {
                let (data, response) = try await URLSession.shared.data(from: downloadURL)
                guard let http = response as? HTTPURLResponse, http.statusCode == 200 else {
                    AuditLog.log(.agentScript, "syncBundledScriptsFromRemote: skip \(name) (HTTP \((response as? HTTPURLResponse)?.statusCode ?? 0))")
                    continue
                }
                guard let upstreamContent = String(data: data, encoding: .utf8) else { continue }

                // Backup user's local copy to .Trash before replacing
                _ = backupScriptToTrash(localFile, scriptName: scriptName)
                try upstreamContent.write(to: localFile, atomically: true, encoding: .utf8)
                updateMetadata(name: scriptName, source: upstreamContent)
                replaced += 1
                AuditLog.log(.agentScript, "syncBundledScriptsFromRemote: replaced \(name)")
            } catch {
                AuditLog.log(.agentScript, "syncBundledScriptsFromRemote: error fetching \(name): \(error.localizedDescription)")
            }
        }

        // Sync finalization (lock + Package.swift regen) must happen in a sync
        // function so we don't try to grab `packageLock.lock()` from an async context.
        finalizeBundledSync(currentVersion: currentVersion, didReplaceScripts: replaced > 0)
        AuditLog.log(.agentScript, "syncBundledScriptsFromRemote: done (\(replaced)/\(checked) replaced)")
    }

    /// Synchronous tail of the sync — runs after all awaits complete so we can
    /// safely grab `packageLock` and bump UserDefaults.
    private func finalizeBundledSync(currentVersion: String, didReplaceScripts: Bool) {
        if didReplaceScripts {
            packageLock.lock()
            defer { packageLock.unlock() }
            generatePackageSwift()
        }
        UserDefaults.standard.set(currentVersion, forKey: Self.lastSyncedAgentVersionKey)
    }

    /// Compute the git blob SHA1 of a file's content. Matches the SHA reported
    /// by GitHub's contents API in the `sha` field, so we can skip downloading
    /// files that haven't changed upstream.
    static func gitBlobSHA1(of content: String) -> String {
        let bodyData = content.data(using: .utf8) ?? Data()
        guard let header = "blob \(bodyData.count)\0".data(using: .utf8) else { return "" }
        var combined = header
        combined.append(bodyData)
        let digest = Insecure.SHA1.hash(data: combined)
        return digest.map { String(format: "%02x", $0) }.joined()
    }

    /// Pull a missing script from AgentScripts GitHub (raw file URL, no clone).
    /// Recovery path for restoring upstream version. Refuses to overwrite a diverged
    /// live script — delete first. Returns no-op success if already identical.
    func pullScriptFromRemote(name: String) async -> String {
        AuditLog.log(.agentScript, "pullScriptFromRemote: \(name)")
        let scriptName = name.replacingOccurrences(of: ".swift", with: "")
        let scriptFile = scriptsDir.appendingPathComponent("\(scriptName).swift")
        let fm = FileManager.default

        // Try `main` first, fall back to `master` for older repos. Both URLs come
        // from the raw.githubusercontent.com host so the failure mode is just an
        // HTTP 404 — no DNS or auth differences to worry about.
        let fetchedContent: String
        switch await fetchUpstreamScript(scriptName: scriptName, branch: "main") {
        case .success(let content):
            fetchedContent = content
        case .notFound:
            switch await fetchUpstreamScript(scriptName: scriptName, branch: "master") {
            case .success(let content):
                fetchedContent = content
            case .notFound:
                return "Error: '\(scriptName)' not found in AgentScripts remote on either main or master branch. Recovery: call agent_script(action:\"list\") to see local scripts, or agent_script(action:\"create\", name:\"\(scriptName)\", content:\"...\") to create it from scratch. Catalog: \(Self.scriptsRawURLPrefix.replacingOccurrences(of: "raw.githubusercontent.com", with: "github.com").replacingOccurrences(of: "/main/", with: "/tree/main/"))"
            case .failure(let err):
                return "Error pulling '\(scriptName)' from remote (master fallback): \(err). Recovery: try again in a moment, or use agent_script(action:\"restore\", name:\"\(scriptName)\") to recover from .Trash backup."
            }
        case .failure(let err):
            return "Error pulling '\(scriptName)' from remote: \(err). Recovery: try again in a moment, or use agent_script(action:\"restore\", name:\"\(scriptName)\") to recover from .Trash backup."
        }

        // If the live file is byte-identical to upstream, treat as a no-op success
        // rather than an error. Lets the model call `pull` defensively without
        // having to delete first when nothing actually needs to change.
        if fm.fileExists(atPath: scriptFile.path) {
            if let local = try? String(contentsOf: scriptFile, encoding: .utf8), local == fetchedContent {
                return "'\(scriptName)' already matches AgentScripts upstream (no-op, \(fetchedContent.count) bytes)."
            }
            return "Error: '\(scriptName)' has local edits that differ from upstream. Recovery: call agent_script(action:\"delete\", name:\"\(scriptName)\") first (delete creates a backup automatically), then retry pull. Or use agent_script(action:\"restore\", name:\"\(scriptName)\") if you want to recover the previous local version instead."
        }

        // Synchronous: write file, lock package, regen — no await between lock and unlock
        return installPulledScript(scriptName: scriptName, scriptFile: scriptFile, content: fetchedContent)
    }

    /// Result of a single-branch upstream fetch.
    private enum UpstreamFetchResult {
        case success(String)
        case notFound
        case failure(String)
    }

    /// Fetch a script's raw content from the AgentScripts repo on a specific branch.
    /// Returns `.notFound` for HTTP 404 (so the caller can try a fallback branch),
    /// `.failure` for any other error, `.success` with the content otherwise.
    private func fetchUpstreamScript(scriptName: String, branch: String) async -> UpstreamFetchResult {
        let prefix = Self.scriptsRawURLPrefix.replacingOccurrences(of: "/main/", with: "/\(branch)/")
        guard let url = URL(string: "\(prefix)/\(scriptName).swift") else {
            return .failure("could not build URL")
        }
        do {
            let (data, response) = try await URLSession.shared.data(from: url)
            guard let http = response as? HTTPURLResponse else {
                return .failure("invalid response")
            }
            if http.statusCode == 404 {
                return .notFound
            }
            guard http.statusCode == 200 else {
                return .failure("HTTP \(http.statusCode)")
            }
            guard let decoded = String(data: data, encoding: .utf8) else {
                return .failure("could not decode content")
            }
            return .success(decoded)
        } catch {
            return .failure(error.localizedDescription)
        }
    }

    /// Synchronous installer for `pullScriptFromRemote` — write the file, regenerate
    /// Package.swift under the lock, no async hops in between.
    private func installPulledScript(scriptName: String, scriptFile: URL, content: String) -> String {
        let fm = FileManager.default
        do {
            try fm.createDirectory(at: scriptsDir, withIntermediateDirectories: true)
            try content.write(to: scriptFile, atomically: true, encoding: .utf8)
            unmarkScriptDeleted(scriptName)
            updateMetadata(name: scriptName, source: content)

            packageLock.lock()
            defer { packageLock.unlock() }
            generatePackageSwift()
            return "Pulled \(scriptName) from AgentScripts remote (\(content.count) bytes). Re-registered in Package.swift."
        } catch {
            return "Error writing pulled script '\(scriptName)': \(error.localizedDescription)"
        }
    }

    /// Delete a script and remove from Package.swift (idempotent — succeeds even if file already gone).
    /// Always backs the file up to `~/Documents/AgentScript/agents/.Trash/` first.
    func deleteScript(name: String) -> String {
        AuditLog.log(.agentScript, "deleteScript: \(name)")
        let scriptName = name.replacingOccurrences(of: ".swift", with: "")
        let scriptFile = scriptsDir.appendingPathComponent("\(scriptName).swift")
        let fm = FileManager.default

        // Backup + remove file if it exists
        var backupNote = ""
        if fm.fileExists(atPath: scriptFile.path) {
            if let backupURL = backupScriptToTrash(scriptFile, scriptName: scriptName) {
                backupNote = " Backup: \(backupURL.lastPathComponent)"
            }
            do {
                try fm.removeItem(at: scriptFile)
            } catch {
                return "Error deleting script: \(error.localizedDescription)"
            }
        }

        // Always mark deleted and regenerate Package.swift (self-heal stale entries)
        markScriptDeleted(scriptName)
        packageLock.lock()
        defer { packageLock.unlock() }
        generatePackageSwift()
        return "Deleted \(scriptName). Removed from Package.swift.\(backupNote)"
    }

    // MARK: - Package.swift path

    var packageSwiftURL: URL {
        Self.agentsDir.appendingPathComponent("Package.swift")
    }

    /// Return the swift build command to compile a script as a dynamic library
    func compileCommand(name: String) -> String? {
        ensurePackage()
        let scriptName = name.replacingOccurrences(of: ".swift", with: "")
        let scriptFile = scriptsDir.appendingPathComponent("\(scriptName).swift")
        let fm = FileManager.default
        guard fm.fileExists(atPath: scriptFile.path) else { return nil }

        let agentsPath = Self.agentsDir.path.replacingOccurrences(of: "'", with: "'\\''")
        let dylibFile = dylibPath(name: scriptName).replacingOccurrences(of: "'", with: "'\\''")
        let escapedName = scriptName.replacingOccurrences(of: "'", with: "'\\''")
        // No `touch Package.swift` here — it had no documented reason and was
        // failing under TCC when the User Launch Agent tried to update mtime on
        // a file in ~/Documents/. Swift Package Manager detects source changes
        // via the .swift file mtimes, so touching Package.swift is a no-op for
        // build invalidation.
        // Re-sign dylib with the app's identity so macOS attributes AppleScript
        // permission prompts to "Agent!" instead of "Xcode".
        return
            "cd '\(agentsPath)' && swift build --product '\(escapedName)' 2>&1 "
            + "&& codesign --force --sign - --identifier \(AppConstants.bundleID) "
            + "'\(dylibFile)' 2>&1"
    }

    /// Path to the compiled dylib for a script
    func dylibPath(name: String) -> String {
        let scriptName = name.replacingOccurrences(of: ".swift", with: "")
        return Self.agentsDir.appendingPathComponent(".build/debug/lib\(scriptName).dylib").path
    }

    /// Check if the compiled dylib is up to date (newer than source file).
    func isDylibCurrent(name: String) -> Bool {
        let scriptName = name.replacingOccurrences(of: ".swift", with: "")
        let sourceFile = scriptsDir.appendingPathComponent("\(scriptName).swift")
        let dylib = dylibPath(name: scriptName)
        let fm = FileManager.default
        guard fm.fileExists(atPath: dylib),
              let sourceAttrs = try? fm.attributesOfItem(atPath: sourceFile.path),
              let dylibAttrs = try? fm.attributesOfItem(atPath: dylib),
              let sourceDate = sourceAttrs[.modificationDate] as? Date,
              let dylibDate = dylibAttrs[.modificationDate] as? Date else
        {
            return false
        }
        return dylibDate > sourceDate
    }

    /// Load and run a compiled script dylib via dlopen/dlsym. Captures stdout
    /// (and optionally stderr), returns output + exit status. Runs on background thread.
    /// `projectFolder` → AGENT_PROJECT_FOLDER env var (always set, separate from args).
    /// In-process variant can't safely chdir, so scripts must read AGENT_PROJECT_FOLDER.
    func loadAndRunScript(
        name: String, arguments: String = "", projectFolder: String = "",
        captureStderr: Bool = false,
        isCancelled: (@Sendable () -> Bool)? = nil,
        onOutput: (@Sendable (String) -> Void)? = nil
    ) async -> (output: String, status: Int32) {
        let scriptName = name.replacingOccurrences(of: ".swift", with: "")
        let path = dylibPath(name: scriptName)

        // Resolve project folder: expand tilde + verify it exists; fall back to home.
        let fm = FileManager.default
        let resolvedProjectFolder: String = {
            let expanded = (projectFolder as NSString).expandingTildeInPath
            var isDir: ObjCBool = false
            if !expanded.isEmpty, fm.fileExists(atPath: expanded, isDirectory: &isDir), isDir.boolValue {
                return expanded
            }
            return NSHomeDirectory()
        }()

        return await withCheckedContinuation { continuation in
            Self.compilationQueue.async {
                // AGENT_PROJECT_FOLDER is always set; AGENT_SCRIPT_ARGS only when present.
                setenv("AGENT_PROJECT_FOLDER", resolvedProjectFolder, 1)
                if !arguments.isEmpty {
                    setenv("AGENT_SCRIPT_ARGS", arguments, 1)
                }

                // Create pipe to capture stdout (and optionally stderr)
                var pipefd: [Int32] = [0, 0]
                pipe(&pipefd)
                let savedStdout = dup(STDOUT_FILENO)
                let savedStderr = captureStderr ? dup(STDERR_FILENO) : -1
                dup2(pipefd[1], STDOUT_FILENO)
                if captureStderr {
                    dup2(pipefd[1], STDERR_FILENO)
                }

                // Force line-buffered stdout so print() in dylibs flushes on each newline
                // (pipes default to full buffering which delays output until script exits)
                setvbuf(stdout, nil, _IOLBF, 0)
                if captureStderr {
                    setvbuf(stderr, nil, _IONBF, 0)
                }

                /// Restore stdout (and stderr if captured) to their original file descriptors.
                func restoreFDs() {
                    dup2(savedStdout, STDOUT_FILENO)
                    if captureStderr { dup2(savedStderr, STDERR_FILENO) }
                    setvbuf(stdout, nil, _IOFBF, 0)
                    if captureStderr { setvbuf(stderr, nil, _IOFBF, 0) }
                    close(savedStdout)
                    if captureStderr { close(savedStderr) }
                }

                // Load dylib
                guard let handle = dlopen(path, RTLD_NOW) else {
                    restoreFDs()
                    close(pipefd[0])
                    close(pipefd[1])
                    unsetenv("AGENT_SCRIPT_ARGS")
                    unsetenv("AGENT_PROJECT_FOLDER")
                    let err = String(cString: dlerror())
                    continuation.resume(returning: ("dlopen error: \(err)", 1))
                    return
                }

                // Find entry point
                guard let sym = dlsym(handle, "script_main") else {
                    dlclose(handle)
                    restoreFDs()
                    close(pipefd[0])
                    close(pipefd[1])
                    unsetenv("AGENT_SCRIPT_ARGS")
                    unsetenv("AGENT_PROJECT_FOLDER")
                    continuation.resume(returning: ("dlsym error: script_main not found in \(scriptName)", 1))
                    return
                }

                // Start a reader thread to stream pipe output to LogView
                final class OutputBuffer: @unchecked Sendable {
                    private let lock = NSLock()
                    var buffer = ""
                    func append(_ chunk: String) {
                        lock.lock()
                        buffer += chunk
                        lock.unlock()
                    }
                    var output: String {
                        lock.lock()
                        defer { lock.unlock() }
                        return buffer
                    }
                }
                let collected = OutputBuffer()
                let readerQueue = DispatchQueue(label: "com.agent.script-output-reader")
                let readHandle = FileHandle(fileDescriptor: pipefd[0], closeOnDealloc: false)
                let readerDone = DispatchSemaphore(value: 0)

                readerQueue.async {
                    while true {
                        let data = readHandle.availableData
                        if data.isEmpty { break }
                        if let chunk = String(data: data, encoding: .utf8) {
                            collected.append(chunk)
                            onOutput?(chunk)
                        }
                    }
                    readerDone.signal()
                }

                // Check cancellation before running
                if isCancelled?() == true {
                    dlclose(handle)
                    fflush(stdout)
                    if captureStderr { fflush(stderr) }
                    close(pipefd[1])
                    restoreFDs()
                    readerDone.wait()
                    close(pipefd[0])
                    unsetenv("AGENT_SCRIPT_ARGS")
                    unsetenv("AGENT_PROJECT_FOLDER")
                    continuation.resume(returning: ("Cancelled before execution", -1))
                    return
                }

                // Call script_main
                typealias ScriptMainFunc = @convention(c) () -> Int32
                let scriptMain = unsafeBitCast(sym, to: ScriptMainFunc.self)
                let status = scriptMain()

                // Flush and restore
                fflush(stdout)
                if captureStderr { fflush(stderr) }
                close(pipefd[1])
                restoreFDs()

                // Wait for reader to finish draining the pipe
                readerDone.wait()
                close(pipefd[0])

                dlclose(handle)
                unsetenv("AGENT_SCRIPT_ARGS")
                unsetenv("AGENT_PROJECT_FOLDER")

                continuation.resume(returning: (collected.output, status))
            }
        }
    }

    /// Block until any in-flight compilation/script work finishes.
    /// Call before exit to avoid stdout deadlock in C++ static destructors.
    nonisolated static func drainCompilationQueue() {
        compilationQueue.sync {}
    }

}
