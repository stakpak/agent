@preconcurrency import Foundation
import AgentAudit
import CoreGraphics
import ImageIO


// MARK: - Vision Verification
extension AgentViewModel {
    /// Capture a screenshot of the frontmost window and return base64-encoded PNG data.
    /// Used by the vision loop to auto-verify UI actions.
    nonisolated static func captureVerificationScreenshot() async -> String? {
        await withCheckedContinuation { continuation in
            DispatchQueue.global().async {
                let tempPath = NSTemporaryDirectory() + "agent_vision_\(UUID().uuidString).png"
                let process = Process()
                process.executableURL = URL(fileURLWithPath: "/usr/sbin/screencapture")
                process.arguments = ["-x", "-t", "png", tempPath]
                process.currentDirectoryURL = URL(fileURLWithPath: NSTemporaryDirectory())
                var env = ProcessInfo.processInfo.environment
                env["HOME"] = NSHomeDirectory()
                let extraPaths = "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
                env["PATH"] = extraPaths + ":" + (env["PATH"] ?? "")
                process.environment = env
                do {
                    try process.run()
                    process.waitUntilExit()
                    guard process.terminationStatus == 0,
                          let data = try? Data(contentsOf: URL(fileURLWithPath: tempPath)) else
                    {
                        continuation.resume(returning: nil)
                        return
                    }
                    // Resize to 50% to save tokens
                    let resized = Self.resizeImageData(data, scale: 0.5)
                    let base64 = resized.base64EncodedString()
                    try? FileManager.default.removeItem(atPath: tempPath)
                    continuation.resume(returning: base64)
                } catch {
                    continuation.resume(returning: nil)
                }
            }
        }
    }

    /// Resize image data by a scale factor (e.g. 0.5 = 50%).
    nonisolated private static func resizeImageData(_ data: Data, scale: CGFloat) -> Data {
        guard scale < 1.0,
              let source = CGImageSourceCreateWithData(data as CFData, nil),
              let image = CGImageSourceCreateImageAtIndex(source, 0, nil) else { return data }
        let newW = Int(CGFloat(image.width) * scale)
        let newH = Int(CGFloat(image.height) * scale)
        guard newW > 0, newH > 0 else { return data }
        guard let ctx = CGContext(
            data: nil,
            width: newW,
            height: newH,
            bitsPerComponent: 8,
            bytesPerRow: 0,
            space: CGColorSpaceCreateDeviceRGB(),
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
        ) else { return data }
        ctx.interpolationQuality = .high
        ctx.draw(image, in: CGRect(x: 0, y: 0, width: newW, height: newH))
        guard let resized = ctx.makeImage() else { return data }
        let mutableData = NSMutableData()
        guard let dest = CGImageDestinationCreateWithData(mutableData, "public.png" as CFString, 1, nil) else { return data }
        CGImageDestinationAddImage(dest, resized, nil)
        CGImageDestinationFinalize(dest)
        return mutableData as Data
    }
}

// MARK: - Shell Execution Tools
extension AgentViewModel {

    /// Execute a command via UserService XPC with streaming output.
    /// Falls back to in-process execution when working directory is TCC-protected.
    func executeViaUserAgent(
        command: String,
        workingDirectory: String = "",
        silent: Bool = false
    ) async -> (status: Int32, output: String)
    {
        resetStreamCounters()
        userServiceActive = true
        userWasActive = true
        if !silent {
            userService.onOutput = { [weak self] chunk in
                self?.appendRawOutput(chunk)
            }
        }
        // Prepend cd to ensure shell runs in the right directory
        let dir = workingDirectory.isEmpty ? projectFolder : workingDirectory
        let fullCommand = Self.prependWorkingDirectory(command, projectFolder: dir)

        // TCC-protected folders must run in-process (app has permissions, launch agent doesn't)
        let result: (status: Int32, output: String)
        if Self.isTCCProtectedPath(dir) || Self.needsTCCPermissions(command) {
            result = await Self.executeTCCStreaming(command: fullCommand, workingDirectory: dir) { [weak self] chunk in
                Task { @MainActor in self?.appendRawOutput(chunk) }
            }
        } else {
            result = await userService.execute(command: fullCommand, workingDirectory: dir)
        }
        userService.onOutput = nil
        userServiceActive = false

        // Only show exit code on real errors (not cancellation, not success)
        if result.status > 0 {
            appendLog("exit code: \(result.status)")
        }
        flushLog()
        return result
    }

    /// Returns true if the path is under a TCC-protected folder that the launch agent can't access.
    nonisolated static func isTCCProtectedPath(_ path: String) -> Bool {
        let expanded = (path as NSString).expandingTildeInPath
        let home = NSHomeDirectory()
        let protected = ["/Documents", "/Desktop", "/Downloads"]
        return protected.contains { expanded.hasPrefix(home + $0) }
    }

    /// Runs a command in the Agent app process to inherit TCC permissions
    /// (Automation, Accessibility, ScreenRecording).
    nonisolated static func executeTCC(command: String, workingDirectory: String = "") async -> (status: Int32, output: String) {
        // Hard local guardrail — refuses catastrophic commands like
        // `rm -rf /` BEFORE the Process is even constructed. The verdict
        // string is shaped to be informative to the LLM, so it understands
        // why the command was rejected and can pick a narrower target on
        // the retry instead of looping the same broken request.
        let verdict = ShellSafetyService.check(command)
        if !verdict.allowed {
            AuditLog.log(.shell, "BLOCKED [\(verdict.rule ?? "?")]: \(command.prefix(200))")
            return (-1, verdict.reason ?? "Refused: command blocked by Agent! shell safety guardrail.")
        }
        return await withCheckedContinuation { continuation in
            DispatchQueue.global().async {
                let process = Process()
                process.executableURL = URL(fileURLWithPath: AppConstants.shellPath)
                process.arguments = ["-c", command]

                if !workingDirectory.isEmpty {
                    process.currentDirectoryURL = URL(fileURLWithPath: workingDirectory)
                }

                var env = ProcessInfo.processInfo.environment
                env["HOME"] = FileManager.default.homeDirectoryForCurrentUser.path
                let extraPaths = "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
                env["PATH"] = extraPaths + ":" + (env["PATH"] ?? "")
                process.environment = env

                let stdoutPipe = Pipe()
                let stderrPipe = Pipe()
                process.standardOutput = stdoutPipe
                process.standardError = stderrPipe

                do {
                    try process.run()
                } catch {
                    continuation.resume(returning: (-1, "Failed to launch: \(error.localizedDescription)"))
                    return
                }

                // Read pipes then wait — osascript output is small, no deadlock risk
                let stdoutData = stdoutPipe.fileHandleForReading.readDataToEndOfFile()
                let stderrData = stderrPipe.fileHandleForReading.readDataToEndOfFile()
                process.waitUntilExit()

                var output = String(data: stdoutData, encoding: .utf8) ?? ""
                let errStr = String(data: stderrData, encoding: .utf8) ?? ""
                if !errStr.isEmpty {
                    if !output.isEmpty { output += "\n" }
                    output += errStr
                }

                continuation.resume(returning: (process.terminationStatus, output))
            }
        }
    }

    /// Run a command in the Agent app process with streaming output.
    /// Inherits TCC permissions (Automation, Accessibility, ScreenRecording).
    nonisolated static func executeTCCStreaming(
        command: String,
        workingDirectory: String = "",
        onOutput: @escaping @Sendable (String) -> Void
    ) async -> (status: Int32, output: String)
    {
        // Same guardrail as executeTCC — refuse catastrophic commands before
        // the Process is constructed.
        let verdict = ShellSafetyService.check(command)
        if !verdict.allowed {
            AuditLog.log(.shell, "BLOCKED [\(verdict.rule ?? "?")]: \(command.prefix(200))")
            let msg = verdict.reason ?? "Refused: command blocked by Agent! shell safety guardrail."
            onOutput(msg)
            return (-1, msg)
        }
        return await withCheckedContinuation { continuation in
            DispatchQueue.global().async {
                let process = Process()
                process.executableURL = URL(fileURLWithPath: AppConstants.shellPath)
                process.arguments = ["-c", command]

                if !workingDirectory.isEmpty {
                    process.currentDirectoryURL = URL(fileURLWithPath: workingDirectory)
                }

                var env = ProcessInfo.processInfo.environment
                env["HOME"] = FileManager.default.homeDirectoryForCurrentUser.path
                // Ensure common tool paths are in PATH
                let extraPaths = "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
                env["PATH"] = extraPaths + ":" + (env["PATH"] ?? "")
                process.environment = env

                let pipe = Pipe()
                process.standardOutput = pipe
                process.standardError = pipe

                do {
                    try process.run()
                } catch {
                    let msg = "Failed to launch: \(error.localizedDescription)"
                    onOutput(msg)
                    continuation.resume(returning: (-1, msg))
                    return
                }

                // Stream output chunks as they arrive
                var collected = ""
                let handle = pipe.fileHandleForReading
                while true {
                    let data = handle.availableData
                    if data.isEmpty { break }
                    if let chunk = String(data: data, encoding: .utf8) {
                        collected += chunk
                        onOutput(chunk)
                    }
                }
                process.waitUntilExit()

                continuation.resume(returning: (process.terminationStatus, collected))
            }
        }
    }

    /// Returns true if the command contains osascript and needs TCC.
    nonisolated static func isOsascriptCommand(_ command: String) -> Bool {
        command.contains("osascript") || command.contains("/usr/bin/osascript")
    }

    /// Returns true if the command needs TCC permissions (run in Agent process).
    ///
    /// THE HARD RULE — anything that touches macOS TCC MUST run in-process.
    /// The Launch Agent and Launch Daemon are SEPARATE processes with
    /// SEPARATE bundle IDs and SEPARATE TCC grants (typically NONE). The
    /// user's "allow Agent! to send keystrokes" / "allow Agent! to control
    /// Music" / etc. lives on the main app's bundle ID and DOES NOT
    /// propagate to subprocesses with different bundle IDs.
    ///
    /// Add new keywords here whenever the LLM finds a way to invoke a
    /// TCC-touching binary the existing list doesn't catch. Better to
    /// over-route (a few extra in-process executions) than to send
    /// AppleScript through the root daemon and confuse the LLM with a
    /// "not authorized" failure.
    nonisolated static func needsTCCPermissions(_ command: String) -> Bool {
        let lower = command.lowercased()
        return lower.contains("osascript")           // AppleScript / JXA CLI
            || lower.contains("applescript")         // any literal mention
            || lower.contains("nsapplescript")       // Foundation API name
            || lower.contains("jxa")                 // JavaScript for Automation alias
            || lower.contains("scriptingbridge")     // SBApplication-using binaries
            || lower.contains("tell application")    // literal AppleScript embedded in heredoc
            || lower.contains("do shell script")     // AppleScript that wraps shell
            || lower.contains("screencapture")       // Screen Recording TCC
            || lower.contains("accessibility")       // AX TCC
            || lower.contains("axorcist")            // AX library binary
            || lower.contains("automation")          // Automation TCC
            || lower.contains("agentscript")         // agent script dylibs (use ScriptingBridge)
            || lower.contains("appleevent")          // raw Apple Events
            || lower.contains("automator")           // Automator workflows (Apple Events)
            || lower.contains("shortcuts run")       // Shortcuts CLI (often needs Automation)
    }

}
