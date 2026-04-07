import AgentAudit
import Foundation

extension ScriptService {
    // MARK: - Out-of-Process Script Execution (concurrent)

    /// Path to the cached ScriptRunner helper executable.
    private static let runnerPath: String = {
        agentsDir.appendingPathComponent(".build/ScriptRunner").path
    }()

    private static let runnerSource = """
    import Foundation
    import Darwin

    guard CommandLine.arguments.count > 1 else {
        fputs("Usage: ScriptRunner <dylib-path>\\n", stderr)
        exit(1)
    }

    let dylibPath = CommandLine.arguments[1]

    guard let handle = dlopen(dylibPath, RTLD_NOW) else {
        let err = String(cString: dlerror())
        fputs("dlopen error: \\(err)\\n", stderr)
        exit(1)
    }

    guard let sym = dlsym(handle, "script_main") else {
        fputs("script_main not found\\n", stderr)
        dlclose(handle)
        exit(1)
    }

    // Line-buffered so output streams in real time
    setvbuf(stdout, nil, _IOLBF, 0)

    typealias ScriptMainFunc = @convention(c) () -> Int32
    let scriptMain = unsafeBitCast(sym, to: ScriptMainFunc.self)
    let status = scriptMain()
    dlclose(handle)
    exit(status)
    """

    /// Compile the ScriptRunner helper if it doesn't exist or is outdated.
    func ensureRunner() async -> Bool {
        let fm = FileManager.default
        let runnerPath = Self.runnerPath
        if fm.fileExists(atPath: runnerPath) { return true }

        let srcPath = Self.agentsDir.appendingPathComponent(".build/ScriptRunner.swift").path
        let buildDir = Self.agentsDir.appendingPathComponent(".build").path
        try? fm.createDirectory(atPath: buildDir, withIntermediateDirectories: true)
        try? Self.runnerSource.write(toFile: srcPath, atomically: true, encoding: .utf8)

        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/swiftc")
        process.arguments = ["-O", "-o", runnerPath, srcPath]
        process.currentDirectoryURL = Self.agentsDir
        var swiftcEnv = ProcessInfo.processInfo.environment
        swiftcEnv["HOME"] = NSHomeDirectory()
        let swiftcPaths = "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
        swiftcEnv["PATH"] = swiftcPaths + ":" + (swiftcEnv["PATH"] ?? "")
        process.environment = swiftcEnv
        let pipe = Pipe()
        process.standardOutput = pipe
        process.standardError = pipe

        do {
            try process.run()
            process.waitUntilExit()
            return process.terminationStatus == 0
        } catch {
            return false
        }
    }

    /// Run a compiled script dylib out-of-process via a small runner executable.
    /// Each invocation gets its own process with its own stdout — fully concurrent.
    func loadAndRunScriptViaProcess(
        name: String, arguments: String = "", captureStderr: Bool = false,
        isCancelled: (@Sendable () -> Bool)? = nil,
        onOutput: (@Sendable (String) -> Void)? = nil
    ) async -> (output: String, status: Int32) {
        AuditLog.log(.agentScript, "run: \(name) args: \(arguments.prefix(80))")
        let scriptName = name.replacingOccurrences(of: ".swift", with: "")
        let dylib = dylibPath(name: scriptName)

        guard await ensureRunner() else {
            return ("Failed to compile ScriptRunner helper", 1)
        }

        let process = Process()
        process.executableURL = URL(fileURLWithPath: Self.runnerPath)
        process.arguments = [dylib]
        process.currentDirectoryURL = URL(fileURLWithPath: NSHomeDirectory())

        // Inherit current environment and add script args
        var env = ProcessInfo.processInfo.environment
        env["HOME"] = NSHomeDirectory()
        let runnerPaths = "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
        env["PATH"] = runnerPaths + ":" + (env["PATH"] ?? "")
        if !arguments.isEmpty {
            env["AGENT_SCRIPT_ARGS"] = arguments
        }
        process.environment = env

        let stdoutPipe = Pipe()
        process.standardOutput = stdoutPipe
        if captureStderr {
            process.standardError = stdoutPipe
        }

        // Collected output buffer
        final class OutputBuffer: @unchecked Sendable {
            private let lock = NSLock()
            private var buffer = ""
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

        // Stream output as it arrives
        stdoutPipe.fileHandleForReading.readabilityHandler = { handle in
            let data = handle.availableData
            guard !data.isEmpty, let chunk = String(data: data, encoding: .utf8) else { return }
            collected.append(chunk)
            onOutput?(chunk)
        }

        do {
            try process.run()
        } catch {
            return ("Failed to launch script: \(error.localizedDescription)", 1)
        }

        // Wait for completion, checking cancellation
        return await withCheckedContinuation { continuation in
            DispatchQueue.global(qos: .userInitiated).async {
                while process.isRunning {
                    if isCancelled?() == true {
                        process.terminate()
                        break
                    }
                    Thread.sleep(forTimeInterval: 0.05)
                }
                process.waitUntilExit()
                // Drain any remaining output
                stdoutPipe.fileHandleForReading.readabilityHandler = nil
                let remaining = stdoutPipe.fileHandleForReading.availableData
                if !remaining.isEmpty,
                   let chunk = String(data: remaining, encoding: .utf8)
                {
                    collected.append(chunk)
                    onOutput?(chunk)
                }
                let status = process.terminationStatus
                continuation.resume(returning: (collected.output, status))
            }
        }
    }

}
