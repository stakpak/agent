import AgentAudit
import AppKit
import ServiceManagement

// MARK: - SMAppService Safe Wrapper for Daemon
/// Safely wraps SMAppService daemon operations to prevent crashes from malformed/missing plists.
/// The crash happens inside Objective-C code that Swift can't catch, so we verify
/// the plist exists BEFORE calling SMAppService methods.
enum SafeSMAppServiceDaemon {
    /// The plist filename for daemon
    static let daemonPlistName = AppConstants.helperPlist

    /// Path to the plist inside the app bundle (where SMAppService reads from)
    static var bundlePlistURL: URL? {
        Bundle.main.bundleURL
            .appendingPathComponent("Contents/Library/LaunchDaemons")
            .appendingPathComponent(daemonPlistName)
    }

    /// Check if the daemon plist exists and is readable inside the app bundle
    static func daemonPlistExists() -> Bool {
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

    /// Create daemon service ONLY if plist is valid
    static func createDaemon() -> SMAppService? {
        // Note: For daemons, SMAppService may manage registration state internally
        // We check existence but still allow creation for already-registered daemons
        // CRITICAL: Only create if plist exists and is valid to prevent ObjC crash
        guard daemonPlistExists() else { return nil }
        return SMAppService.daemon(plistName: daemonPlistName)
    }

    /// Safely check if daemon is ready - returns false if any issue
    static func isDaemonReady() -> Bool {
        // First verify plist exists
        guard daemonPlistExists() else { return false }

        // Create service and check status (may still crash in ObjC)
        guard let service = createDaemon() else { return false }

        // Accessing .status could crash if plist is malformed, but we validated above
        return service.status == .enabled
    }

    /// Safely register daemon with comprehensive error handling
    static func registerDaemon() -> (success: Bool, message: String) {
        // First verify plist exists
        guard daemonPlistExists() else {
            return (false, "Daemon plist not found in app bundle. Rebuild and reinstall Agent.")
        }

        guard let service = createDaemon() else {
            return (false, "Daemon service unavailable. Reinstall Agent.")
        }

        let status = service.status
        let statusName = statusNameFor(status)

        do {
            try service.register()
            return (true, "Helper daemon registered. (was: \(statusName))")
        } catch {
            // Check if already enabled after attempted registration
            if service.status == .enabled {
                return (true, "Helper daemon is active.")
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
                    return (true, "Helper daemon updated.")
                } catch {
                    return (false, "Update failed: \(error.localizedDescription)")
                }
            }
            return (false, "Registration failed: \(error.localizedDescription)")
        }
    }

    /// Safely unregister daemon
    static func unregisterDaemon() {
        guard daemonPlistExists(),
              let service = createDaemon() else { return }
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

final class OutputHandler: NSObject, HelperProgressProtocol, @unchecked Sendable {
    private let handler: @Sendable (String) -> Void

    init(handler: @escaping @Sendable (String) -> Void) {
        self.handler = handler
    }

    func progressUpdate(_ line: String) {
        handler(line)
    }
}

@MainActor @Observable
final class HelperService {
    nonisolated static let helperID = AppConstants.helperID
    nonisolated let instanceID = UUID().uuidString

    var onOutput: (@MainActor @Sendable (String) -> Void)?

    nonisolated init() {}

    var helperReady: Bool {
        SafeSMAppServiceDaemon.isDaemonReady()
    }

    @discardableResult
    func registerHelper() -> String {
        let result = SafeSMAppServiceDaemon.registerDaemon()
        return result.message
    }

    /// Completely shut down and unregister the daemon for security.
    func shutdownDaemon() {
        let kill = Process()
        kill.executableURL = URL(fileURLWithPath: "/usr/bin/pkill")
        kill.arguments = ["-f", "AgentHelper"]
        kill.currentDirectoryURL = URL(fileURLWithPath: NSHomeDirectory())
        try? kill.run()
        kill.waitUntilExit()

        SafeSMAppServiceDaemon.unregisterDaemon()
    }

    /// Kill any stale daemon processes, unregister, and re-register.
    @discardableResult
    func restartDaemon() -> String {
        // Kill any lingering processes
        let kill = Process()
        kill.executableURL = URL(fileURLWithPath: "/usr/bin/pkill")
        kill.arguments = ["-f", "AgentHelper"]
        kill.currentDirectoryURL = URL(fileURLWithPath: NSHomeDirectory())
        try? kill.run()
        kill.waitUntilExit()

        SafeSMAppServiceDaemon.unregisterDaemon()
        // Brief pause for launchd to clean up
        Thread.sleep(forTimeInterval: 0.5)
        return registerHelper()
    }

    func execute(command: String, workingDirectory: String = "") async -> (status: Int32, output: String) {
        AuditLog.log(.launchDaemon, "execute: \(command.prefix(100))")
        // Hard local guardrail — refuse catastrophic commands before they
        // cross the XPC boundary into the privileged daemon. The daemon runs
        // as root, so this is the LAST place we can stop a destructive
        // command from doing maximum damage.
        let verdict = ShellSafetyService.check(command)
        if !verdict.allowed {
            AuditLog.log(.launchDaemon, "BLOCKED [\(verdict.rule ?? "?")]: \(command.prefix(200))")
            return (-1, verdict.reason ?? "Refused: command blocked by Agent! shell safety guardrail.")
        }
        if !helperReady {
            let msg = restartDaemon()
            if !helperReady {
                return (-1, "Error: Launch Daemon is not running — \(msg). Check System Settings > Login Items.")
            }
        }

        let handler = OutputHandler { [weak self] chunk in
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
        // Prepend cd so the shell runs in the right directory regardless of daemon cwd
        let fullCommand: String
        if !dir.isEmpty && !command.hasPrefix("cd ") {
            let escaped = "'" + dir.replacingOccurrences(of: "'", with: "'\\''") + "'"
            fullCommand = "cd \(escaped) && \(command)"
        } else {
            fullCommand = command
        }
        // Honor the user's zsh/bash toggle. The daemon always invokes /bin/zsh
        // (XPC protocol stays unchanged) but if the user picked a different
        // shell, we wrap the command with `exec <shell> -c '...'` so the outer
        // zsh process is replaced by the chosen shell. Defaults to no wrapping
        // when /bin/zsh is selected (most common case, zero overhead).
        let wrapped = Self.wrapForShell(fullCommand, shellPath: AppConstants.shellPath)
        return await executeViaXPC(script: wrapped, workingDirectory: dir, outputHandler: handler)
    }

    /// Wrap a command with `exec <shell> -c '...'` when the user has chosen a
    /// shell other than /bin/zsh (the daemon's default). Single quotes inside
    /// the command are escaped using the standard `'\''` pattern so the
    /// wrapping survives any shell metacharacter — including the `!` in
    /// `Agent!.app` paths and arbitrary user content.
    private static func wrapForShell(_ command: String, shellPath: String) -> String {
        guard shellPath != "/bin/zsh", !shellPath.isEmpty else { return command }
        let escaped = command.replacingOccurrences(of: "'", with: "'\\''")
        return "exec \(shellPath) -c '\(escaped)'"
    }

    /// Quick connectivity test with 5-second timeout. Returns true if XPC responds.
    func ping() async -> Bool {
        let handler = OutputHandler { _ in }
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
            }) as? HelperToolProtocol else {
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

    nonisolated private func makeConnection(outputHandler: OutputHandler) -> NSXPCConnection {
        let connection = NSXPCConnection(machServiceName: HelperService.helperID, options: .privileged)
        connection.remoteObjectInterface = NSXPCInterface(with: HelperToolProtocol.self)
        connection.exportedInterface = NSXPCInterface(with: HelperProgressProtocol.self)
        connection.exportedObject = outputHandler
        connection.resume()
        return connection
    }

    nonisolated private func executeViaXPC(
        script: String,
        workingDirectory: String = "",
        outputHandler: OutputHandler
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
            }) as? HelperToolProtocol else {
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
            let connection = NSXPCConnection(machServiceName: helperID, options: .privileged)
            connection.remoteObjectInterface = NSXPCInterface(with: HelperToolProtocol.self)
            connection.resume()
            guard let proxy = connection.remoteObjectProxyWithErrorHandler({ _ in
                continuation.resume()
            }) as? HelperToolProtocol else {
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
