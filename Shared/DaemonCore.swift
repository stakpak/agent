import Foundation

/// Shared output context for streaming command output via XPC.
final class OutputContext: @unchecked Sendable {
    var output = ""
    let outputLock = NSLock()
    let progressHandler: ((String) -> Void)?

    init(progressHandler: ((String) -> Void)?) {
        self.progressHandler = progressHandler
    }
}

/// Shared command execution logic for both AgentHelper (root) and AgentUser (user) daemons.
/// The only differences are the XPC protocol types and Mach service name.
enum DaemonCore {
    nonisolated(unsafe) static var runningProcesses: [String: Process] = [:]
    static let lock = NSLock()

    static func execute(
        script: String,
        instanceID: String,
        workingDirectory: String,
        progressHandler: ((String) -> Void)?,
        reply: @escaping (Int32, String) -> Void
    ) {
        lock.lock()
        if let old = runningProcesses[instanceID], old.isRunning {
            old.terminate()
            old.waitUntilExit()
        }
        runningProcesses[instanceID] = nil
        lock.unlock()

        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/bin/zsh")
        process.arguments = ["-c", script]

        if !workingDirectory.isEmpty {
            process.currentDirectoryURL = URL(fileURLWithPath: workingDirectory)
        }

        var env = ProcessInfo.processInfo.environment
        env["CLICOLOR_FORCE"] = "1"
        env["TERM"] = env["TERM"] ?? "xterm-256color"
        let extraPaths = "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
        env["PATH"] = extraPaths + ":" + (env["PATH"] ?? "")
        if !workingDirectory.isEmpty {
            env["PWD"] = workingDirectory
        }
        process.environment = env

        let pipe = Pipe()
        process.standardOutput = pipe
        process.standardError = pipe

        lock.lock()
        runningProcesses[instanceID] = process
        lock.unlock()

        let ctx = OutputContext(progressHandler: progressHandler)

        pipe.fileHandleForReading.readabilityHandler = { [ctx] handle in
            let data = handle.availableData
            guard !data.isEmpty, let chunk = String(data: data, encoding: .utf8) else { return }
            ctx.outputLock.lock()
            ctx.output += chunk
            ctx.outputLock.unlock()
            ctx.progressHandler?(chunk)
        }

        do {
            try process.run()
            process.waitUntilExit()
        } catch {
            reply(-1, error.localizedDescription)
            return
        }

        pipe.fileHandleForReading.readabilityHandler = nil

        let remainingData = pipe.fileHandleForReading.readDataToEndOfFile()
        if !remainingData.isEmpty, let chunk = String(data: remainingData, encoding: .utf8) {
            ctx.outputLock.lock()
            ctx.output += chunk
            ctx.outputLock.unlock()
            ctx.progressHandler?(chunk)
        }

        ctx.outputLock.lock()
        let output = ctx.output
        ctx.outputLock.unlock()

        reply(process.terminationStatus, output)

        lock.lock()
        runningProcesses.removeValue(forKey: instanceID)
        lock.unlock()
    }

    static func cancel(instanceID: String) {
        lock.lock()
        if let process = runningProcesses[instanceID], process.isRunning {
            process.terminate()
        }
        runningProcesses.removeValue(forKey: instanceID)
        lock.unlock()
    }
}
