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

    /// Restore the most recent backup of `name` (or a specific backup filename) into
    /// Sources/Scripts/. Refuses to overwrite an existing live script — caller must
    /// delete it first (which itself creates a fresh backup).
    func restoreScript(name: String, backupFilename: String? = nil) -> String {
        AuditLog.log(.agentScript, "restoreScript: \(name) backup=\(backupFilename ?? "latest")")
        let scriptName = name.replacingOccurrences(of: ".swift", with: "")
        let scriptFile = scriptsDir.appendingPathComponent("\(scriptName).swift")
        let fm = FileManager.default

        // Don't clobber a live script
        if fm.fileExists(atPath: scriptFile.path) {
            return "Error: '\(scriptName)' already exists. Delete it first (the delete will create a fresh backup)."
        }

        // Locate the backup
        let backupURL: URL
        if let explicit = backupFilename {
            let candidate = scriptsTrashDir.appendingPathComponent(explicit)
            guard fm.fileExists(atPath: candidate.path) else {
                return "Error: backup '\(explicit)' not found in .Trash."
            }
            backupURL = candidate
        } else {
            let backups = listScriptBackups(name: scriptName)
            guard let latest = backups.first else {
                return "Error: no backups found for '\(scriptName)'."
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

    /// Copy a script to the trash with a timestamp suffix before deletion. Best-effort:
    /// failures are logged but don't block the delete (the audit log captures the loss).
    /// Returns the backup path on success, nil on failure.
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

    /// On app launch, refresh upstream-bundled scripts if Agent! has been upgraded
    /// since the last sync. Strategy:
    ///
    /// 1. **Version-gated** — runs only when `CFBundleShortVersionString` differs
    ///    from `lastSyncedAgentVersion`. Cold launches without an upgrade are no-ops.
    /// 2. **Catalog-driven** — fetches the GitHub contents API for the upstream
    ///    Scripts directory. Only files that appear in that catalog are considered
    ///    "bundled". Anything else is user-authored and never touched.
    /// 3. **SHA-comparison** — uses the git blob SHA from the contents API and
    ///    computes the equivalent for each local file. Files with matching SHAs
    ///    are skipped without re-downloading.
    /// 4. **Backed up before replacing** — every replacement first copies the local
    ///    file to `.Trash/<name>-<timestamp>.swift` so the user's edits remain
    ///    recoverable via `agent_script(action:"restore", name:"X")`.
    /// 5. **Silent on failure** — every error path logs to AuditLog and returns
    ///    without throwing. App launch never blocks on this.
    /// 6. **Locally absent → leave alone** — if the user deleted a bundled script,
    ///    we don't auto-resurrect it. They can `pull` it explicitly.
    func syncBundledScriptsFromRemote() async {
        AuditLog.log(.agentScript, "syncBundledScriptsFromRemote: start")
        let currentVersion = Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "unknown"
        let lastSynced = UserDefaults.standard.string(forKey: Self.lastSyncedAgentVersionKey) ?? ""
        if lastSynced == currentVersion {
            AuditLog.log(.agentScript, "syncBundledScriptsFromRemote: skip (version \(currentVersion) already synced)")
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

    /// Pull a missing script straight from the AgentScripts GitHub repo (raw file URL,
    /// no full clone). Recovery path for when the model deleted a script the user wants
    /// the upstream version of (rather than a `.Trash` backup that might be a local edit).
    ///
    /// Refuses to overwrite an existing live script — caller must delete it first
    /// (which itself creates a fresh backup), so the operation is always reversible.
    func pullScriptFromRemote(name: String) async -> String {
        AuditLog.log(.agentScript, "pullScriptFromRemote: \(name)")
        let scriptName = name.replacingOccurrences(of: ".swift", with: "")
        let scriptFile = scriptsDir.appendingPathComponent("\(scriptName).swift")
        let fm = FileManager.default

        // Don't clobber a live script
        if fm.fileExists(atPath: scriptFile.path) {
            return "Error: '\(scriptName)' already exists locally. Delete it first (delete creates a backup) before pulling from remote."
        }

        let urlString = "\(Self.scriptsRawURLPrefix)/\(scriptName).swift"
        guard let url = URL(string: urlString) else {
            return "Error: could not build URL for '\(scriptName)'"
        }

        let fetchedContent: String
        do {
            let (data, response) = try await URLSession.shared.data(from: url)
            guard let http = response as? HTTPURLResponse else {
                return "Error: invalid response pulling '\(scriptName)'"
            }
            guard http.statusCode == 200 else {
                return "Error: '\(scriptName)' not found in AgentScripts remote (HTTP \(http.statusCode)). Browse \(Self.scriptsRawURLPrefix.replacingOccurrences(of: "raw.githubusercontent.com", with: "github.com").replacingOccurrences(of: "/main/", with: "/tree/main/")) for available script names."
            }
            guard let decoded = String(data: data, encoding: .utf8) else {
                return "Error: could not decode '\(scriptName)' content from remote"
            }
            fetchedContent = decoded
        } catch {
            return "Error pulling '\(scriptName)' from remote: \(error.localizedDescription)"
        }

        // Synchronous: write file, lock package, regen — no await between lock and unlock
        return installPulledScript(scriptName: scriptName, scriptFile: scriptFile, content: fetchedContent)
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
        //
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

    /// Load and run a compiled script dylib in-process via dlopen/dlsym.
    /// Captures stdout (and optionally stderr) and returns the output + exit status.
    /// Runs on a background thread to avoid blocking the main thread.
    ///
    /// `projectFolder` is the active tab/main project directory. It's exported as
    /// `AGENT_PROJECT_FOLDER` (always set, separate from `AGENT_SCRIPT_ARGS`).
    /// Scripts read it directly when they need a default working directory.
    /// Note: this in-process variant cannot safely chdir without affecting the
    /// rest of the host app, so scripts that need cwd MUST read
    /// `AGENT_PROJECT_FOLDER` rather than relying on `getcwd()`.
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
