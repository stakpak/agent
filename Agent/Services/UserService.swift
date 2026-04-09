import AgentAudit
import AppKit
import ServiceManagement

// MARK: - SMAppService Safe Wrapper / Safely wraps SMAppService operations to prevent crashes from malformed/missing
// plists. / The crash happens inside Objective-C code that Swift can't catch, so we verify / the plist exists BEFORE calling SMAppService methods.
enum SafeSMAppService {
    /// The plist filename for user agent
    static let userAgentPlistName = AppConstants.userPlist

    /// Path to the plist inside the app bundle (where SMAppService reads from)
    static var bundlePlistURL: URL? {
        Bundle.main.bundleURL
            .appendingPathComponent("Contents/Library/LaunchAgents")
            .appendingPathComponent(userAgentPlistName)
    }

    /// Check if the user agent plist exists and is readable inside the app bundle
    static func userAgentPlistExists() -> Bool {
        guard let plistURL = bundlePlistURL else { return false }
        let path = plistURL.path

        // Check file exists
        guard FileManager.default.fileExists(atPath: path) else { return false }

        // Check file is readable
        guard FileManager.default.isReadableFile(atPath: path) else { return false }

        // Verify file has valid content (not empty, not corrupted)
        guard let data = FileManager.default.contents(atPath: path),
              !data.isEmpty,
              let plist = try? PropertyListSerialization.propertyList(from: data, options: [], format: nil),
              plist is [String: Any] else
        {
            return false
        }

        return true
    }

    /// Create user agent service ONLY if plist is valid
    static func createUserAgent() -> SMAppService? {
        // CRITICAL: Only create SMAppService if plist exists and is valid
        // SMAppService crashes in Objective-C code if plist is malformed
        guard userAgentPlistExists() else { return nil }
        return SMAppService.agent(plistName: userAgentPlistName)
    }

    /// Safely check if user agent is ready - returns false if any issue
    static func isUserAgentReady() -> Bool {
        // First verify plist exists
        guard userAgentPlistExists() else { return false }

        // Create service and check status (may still crash in ObjC)
        guard let service = createUserAgent() else { return false }

        // Accessing .status could crash if plist is malformed, but we validated above
        return service.status == .enabled
    }

    /// Safely register user agent with comprehensive error handling
    static func registerUserAgent() -> (success: Bool, message: String) {
        // First verify plist exists
        guard userAgentPlistExists() else {
            return (false, "User agent plist not found in app bundle. Rebuild and reinstall Agent.")
        }

        guard let service = createUserAgent() else {
            return (false, "User agent unavailable. Reinstall Agent.")
        }

        let status = service.status
        let statusName = statusNameFor(status)

        do {
            try service.register()
            return (true, "User agent registered. (was: \(statusName))")
        } catch {
            // Check if already enabled after attempted registration
            if service.status == .enabled {
                return (true, "User agent is active.")
            }
            if service.status == .requiresApproval {
                SMAppService.openSystemSettingsLoginItems()
                NSWorkspace.shared.open(URL(string: "x-apple.systempreferences:com.apple.LoginItems-Settings.extension")!)
                return (false, "Please approve Agent in System Settings > Login Items.")
            }
            // Try re-registering if was enabled
            if status == .enabled {
                try? service.unregister()
                do {
                    try service.register()
                    return (true, "User agent updated.")
                } catch {
                    return (false, "Update failed: \(error.localizedDescription)")
                }
            }
            return (false, "Registration failed: \(error.localizedDescription)")
        }
    }

    /// Safely unregister user agent
    static func unregisterUserAgent() {
        guard userAgentPlistExists(),
              let service = createUserAgent() else { return }
        try? service.unregister()
    }

    /// Get status name safely
    private static func statusNameFor(_ status: SMAppService.Status) -> String {
        switch status {
        case .notRegistered: return "notRegistered"
        case .enabled: return "enabled"
        case .requiresApproval: return "requiresApproval"
        case .notFound: return "notFound"
        @unknown default: return "unknown"
        }
    }
}

final class UserOutputHandler: NSObject, UserProgressProtocol, @unchecked Sendable {
    private let handler: @Sendable (String) -> Void

    init(handler: @escaping @Sendable (String) -> Void) {
        self.handler = handler
    }

    func progressUpdate(_ line: String) {
        handler(line)
    }
}

@MainActor @Observable
final class UserService {
    nonisolated static let userID = AppConstants.userID
    nonisolated let instanceID = UUID().uuidString

    var onOutput: (@MainActor @Sendable (String) -> Void)?

    nonisolated init() {}

    var userReady: Bool {
        SafeSMAppService.isUserAgentReady()
    }

    @discardableResult
    func registerUser() -> String {
        let result = SafeSMAppService.registerUserAgent()
        return result.message
    }

    /// Completely shut down and unregister the user agent.
    func shutdownAgent() {
        let kill = Process()
        kill.executableURL = URL(fileURLWithPath: "/usr/bin/pkill")
        kill.arguments = ["-f", "AgentUser"]
        kill.currentDirectoryURL = URL(fileURLWithPath: NSHomeDirectory())
        try? kill.run()
        kill.waitUntilExit()

        SafeSMAppService.unregisterUserAgent()
    }

    /// Kill any stale agent processes, unregister, and re-register.
    @discardableResult
    func restartAgent() -> String {
        // Kill any lingering processes
        let kill = Process()
        kill.executableURL = URL(fileURLWithPath: "/usr/bin/pkill")
        kill.arguments = ["-f", "AgentUser"]
        kill.currentDirectoryURL = URL(fileURLWithPath: NSHomeDirectory())
        try? kill.run()
        kill.waitUntilExit()

        SafeSMAppService.unregisterUserAgent()
        // Brief pause for launchd to clean up
        Thread.sleep(forTimeInterval: 0.5)
        return registerUser()
    }

    func execute(command: String, workingDirectory: String = "") async -> (status: Int32, output: String) {
        // Defense-in-depth: if a caller passed a file path as workingDirectory,
        // strip the filename so the daemon doesn't crash with "Not a directory".
        var workingDirectory = workingDirectory
        if !workingDirectory.isEmpty {
            var isDir: ObjCBool = false
            if FileManager.default.fileExists(atPath: workingDirectory, isDirectory: &isDir),
               !isDir.boolValue {
                workingDirectory = (workingDirectory as NSString).deletingLastPathComponent
            }
        }
        AuditLog.log(.launchAgent, "execute: \(command.prefix(100))")
        // Hard local guardrail — refuse catastrophic commands before they
        // cross the XPC boundary into the user-context daemon.
        let verdict = ShellSafetyService.check(command)
        if !verdict.allowed {
            AuditLog.log(.launchAgent, "BLOCKED [\(verdict.rule ?? "?")]: \(command.prefix(200))")
            return (-1, verdict.reason ?? "Refused: command blocked by Agent! shell safety guardrail.")
        }
        if !userReady {
            let msg = restartAgent()
            if !userReady {
                return (-1, "Error: User agent is not running — \(msg). Check System Settings > Login Items.")
            }
        }

        let handler = UserOutputHandler { [weak self] chunk in
            Task { @MainActor in
                self?.onOutput?(chunk)
            }
        }

        // Always send an absolute path — relative paths resolve wrong in the daemon
        let dir: String
        if workingDirectory.isEmpty || workingDirectory == "." || workingDirectory == "./" {
            dir = NSHomeDirectory()
        } else if !workingDirectory.hasPrefix("/") && !workingDirectory.hasPrefix("~") {
            dir = (NSHomeDirectory() as NSString).appendingPathComponent(workingDirectory)
        } else {
            dir = (workingDirectory as NSString).expandingTildeInPath
        }
        // Prepend `export AGENT_PROJECT_FOLDER='<dir>'; cd '<dir>' && ` so the command runs in the right directory AND
        // has the project folder env var available the same way agent scripts do. The export is sent through the XPC boundary as part of the command string — no XPC protocol change required.
        let fullCommand: String
        if !dir.isEmpty && !command.hasPrefix("cd ") {
            let escaped = "'" + dir.replacingOccurrences(of: "'", with: "'\\''") + "'"
            fullCommand = "export AGENT_PROJECT_FOLDER=\(escaped); cd \(escaped) && \(command)"
        } else {
            fullCommand = command
        }
        // Honor the user's zsh/bash toggle. The Launch Agent always invokes /bin/zsh (XPC protocol stays unchanged) but
        // if the user picked a different shell, we wrap the command with `exec <shell> -c '...'` so the outer zsh process is replaced by the chosen shell. Defaults to no wrapping when /bin/zsh is selected.
        let wrapped = Self.wrapForShell(fullCommand, shellPath: AppConstants.shellPath)
        return await executeViaXPC(script: wrapped, workingDirectory: dir, outputHandler: handler)
    }

    /// / Wrap a command with `exec <shell> -c '...'` when the user has chosen a / shell other than /bin/zsh.
    /// Single-quote escape pattern survives `!` / and every shell metacharacter.
    private static func wrapForShell(_ command: String, shellPath: String) -> String {
        guard shellPath != "/bin/zsh", !shellPath.isEmpty else { return command }
        let escaped = command.replacingOccurrences(of: "'", with: "'\\''")
        return "exec \(shellPath) -c '\(escaped)'"
    }

    /// Quick connectivity test with 5-second timeout. Returns true if XPC responds.
    func ping() async -> Bool {
        let handler = UserOutputHandler { _ in }
        let conn = makeConnection(outputHandler: handler)
        return await Self.performPing(connection: conn)
    }

    /// Runs XPC ping off the main actor so continuation can be resumed from any thread.
    private nonisolated static func performPing(connection: NSXPCConnection) async -> Bool {
        await withCheckedContinuation { continuation in
            var didResume = false
            let resumeLock = NSLock()
            func safeResume(_ value: Bool) {
                resumeLock.lock()
                defer { resumeLock.unlock() }
                guard !didResume else { return }
                didResume = true
                continuation.resume(returning: value)
            }

            guard let proxy = connection.remoteObjectProxyWithErrorHandler({ _ in
                safeResume(false)
            }) as? UserToolProtocol else {
                connection.invalidate()
                safeResume(false)
                return
            }

            let timeout = DispatchWorkItem {
                connection.invalidate()
                safeResume(false)
            }
            DispatchQueue.global().asyncAfter(deadline: .now() + 5, execute: timeout)

            proxy.execute(script: "echo ping", instanceID: UUID().uuidString) { status, _ in
                timeout.cancel()
                connection.invalidate()
                safeResume(status == 0)
            }
        }
    }

    func cancel() {
        onOutput = nil // Clear handler to prevent memory leaks
        Self.cancelProcess(instanceID: instanceID)
    }

    nonisolated static func cancelProcess(instanceID: String) {
        Task.detached {
            await cancelViaXPC(instanceID: instanceID)
        }
    }

    // MARK: - XPC

    nonisolated private func makeConnection(outputHandler: UserOutputHandler) -> NSXPCConnection {
        // No .privileged option — runs as current user
        let connection = NSXPCConnection(machServiceName: UserService.userID, options: [])
        connection.remoteObjectInterface = NSXPCInterface(with: UserToolProtocol.self)
        connection.exportedInterface = NSXPCInterface(with: UserProgressProtocol.self)
        connection.exportedObject = outputHandler
        connection.resume()
        return connection
    }

    nonisolated private func executeViaXPC(
        script: String,
        workingDirectory: String = "",
        outputHandler: UserOutputHandler
    ) async -> (status: Int32, output: String)
    {
        await withCheckedContinuation { continuation in
            var didResume = false
            let resumeLock = NSLock()

            func safeResume(_ value: (Int32, String)) {
                resumeLock.lock()
                defer { resumeLock.unlock() }
                guard !didResume else { return }
                didResume = true
                continuation.resume(returning: value)
            }

            let connection = makeConnection(outputHandler: outputHandler)
            guard let proxy = connection.remoteObjectProxyWithErrorHandler({ error in
                safeResume((-1, "XPC error: \(error.localizedDescription)"))
            }) as? UserToolProtocol else {
                connection.invalidate()
                safeResume((-1, "XPC proxy cast failed"))
                return
            }

            // Start timeout — tool must begin executing within toolStartTimeout seconds.
            var started = false
            let startedLock = NSLock()
            let startTimer = DispatchWorkItem {
                startedLock.lock()
                let didStart = started
                startedLock.unlock()
                if !didStart {
                    connection.invalidate()
                    safeResume((-1, "Tool failed to start within \(Int(toolStartTimeout))s"))
                }
            }
            DispatchQueue.global().asyncAfter(deadline: .now() + toolStartTimeout, execute: startTimer)

            // Finish timeout — tool must complete within toolFinishTimeout seconds.
            let finishTimer = DispatchWorkItem {
                connection.invalidate()
                safeResume((-1, "Tool timed out after \(Int(toolFinishTimeout))s"))
            }
            DispatchQueue.global().asyncAfter(deadline: .now() + toolFinishTimeout, execute: finishTimer)

            proxy.execute(script: script, instanceID: self.instanceID, workingDirectory: workingDirectory) { status, output in
                startedLock.lock()
                started = true
                startedLock.unlock()
                startTimer.cancel()
                finishTimer.cancel()
                connection.invalidate()
                safeResume((status, output))
            }
        }
    }

    nonisolated private static func cancelViaXPC(instanceID: String) async {
        await withCheckedContinuation { (continuation: CheckedContinuation<Void, Never>) in
            let connection = NSXPCConnection(machServiceName: userID, options: [])
            connection.remoteObjectInterface = NSXPCInterface(with: UserToolProtocol.self)
            connection.resume()
            guard let proxy = connection.remoteObjectProxyWithErrorHandler({ _ in
                continuation.resume()
            }) as? UserToolProtocol else {
                connection.invalidate()
                continuation.resume()
                return
            }
            proxy.cancelOperation(instanceID: instanceID) {
                connection.invalidate()
                continuation.resume()
            }
        }
    }
}
