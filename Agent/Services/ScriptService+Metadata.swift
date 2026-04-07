import AgentAudit
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

    /// Delete a script and remove from Package.swift (idempotent — succeeds even if file already gone)
    func deleteScript(name: String) -> String {
        AuditLog.log(.agentScript, "deleteScript: \(name)")
        let scriptName = name.replacingOccurrences(of: ".swift", with: "")
        let scriptFile = scriptsDir.appendingPathComponent("\(scriptName).swift")
        let fm = FileManager.default

        // Remove file if it exists
        if fm.fileExists(atPath: scriptFile.path) {
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
        return "Deleted \(scriptName). Removed from Package.swift."
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
        return "cd '\(agentsPath)' && swift build --product '\(escapedName)' 2>&1 && codesign --force --sign - --identifier \(AppConstants.bundleID) '\(dylibFile)' 2>&1"
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
    func loadAndRunScript(
        name: String, arguments: String = "", captureStderr: Bool = false,
        isCancelled: (@Sendable () -> Bool)? = nil,
        onOutput: (@Sendable (String) -> Void)? = nil
    ) async -> (output: String, status: Int32) {
        let scriptName = name.replacingOccurrences(of: ".swift", with: "")
        let path = dylibPath(name: scriptName)

        return await withCheckedContinuation { continuation in
            Self.compilationQueue.async {
                // Set arguments via environment variable for scripts that need them
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
