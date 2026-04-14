import AgentAudit
import Foundation
import ScriptingBridge
import XcodeScriptingBridge

/// Xcode automation via ScriptingBridge — build, run, list/select projects, grant permission.
private class SBApplicationDelegateIgnore: NSObject, SBApplicationDelegate {
    func eventDidFail(_ event: UnsafePointer<AppleEvent>, withError error: any Error) -> Any? {
        return nil // Suppress error, return nil to caller
    }
}

final class XcodeService: @unchecked Sendable {
    static let shared = XcodeService()
    private static let xcodeBundleID = "com.apple.dt.Xcode"

    /// Create a fresh Xcode SBApplication connection with error suppression.
    private nonisolated func xcodeApp() -> XcodeApplication? {
        guard let app: XcodeApplication = SBApplication(bundleIdentifier: Self.xcodeBundleID) else { return nil }
        app.delegate = SBApplicationDelegateIgnore()
        return app
    }

    // MARK: - Grant Permission

      /// Grant Automation permission via lightweight osascript no-op (get name).
    nonisolated func grantPermission() -> String {
        let script = """
        tell application "Xcode"
            return name
        end tell
        """

        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/osascript")
        process.arguments = ["-e", script]
        process.currentDirectoryURL = URL(fileURLWithPath: NSHomeDirectory())
        var env = ProcessInfo.processInfo.environment
        env["HOME"] = NSHomeDirectory()
        let extraPaths = "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
        env["PATH"] = extraPaths + ":" + (env["PATH"] ?? "")
        process.environment = env

        let outputPipe = Pipe()
        let errorPipe = Pipe()
        process.standardOutput = outputPipe
        process.standardError = errorPipe

        do {
            try process.run()
            process.waitUntilExit()

            if process.terminationStatus != 0 {
                let errData = errorPipe.fileHandleForReading.readDataToEndOfFile()
                let errStr = String(data: errData, encoding: .utf8) ?? "Unknown error"
                return "Grant failed: \(errStr)"
            }

            let data = outputPipe.fileHandleForReading.readDataToEndOfFile()
            return String(data: data, encoding: .utf8)?.trimmingCharacters(in: .whitespacesAndNewlines)
                ?? "Permission granted"
        } catch {
            return "Grant failed: \(error.localizedDescription)"
        }
    }

    // MARK: - Build

    /// Build a project via ScriptingBridge. Blocks until build completes.
    /// Returns errors/warnings in file:line:col [Error] message format with code snippets.
    nonisolated func buildProject(projectPath: String) -> String {
        AuditLog.log(.xcode, "build: \(projectPath)")
        // Always auto-detect from open Xcode projects — ignore model's guessed path
        let resolvedPath = autoSelectProject() ?? projectPath
        guard isValidProjectPath(resolvedPath) else {
            return
                "Error: Invalid project path '\(resolvedPath)'. "
                + "Use xcode (action: list_projects) to find the correct "
                + "project. Must be .xcodeproj or .xcworkspace."
        }

        guard let xcode = xcodeApp() else {
            return "Error: Failed to connect to Xcode"
        }

        guard let workspace = xcode.open?(resolvedPath as Any) as? XcodeWorkspaceDocument else {
            return "Error: Could not open workspace at \(resolvedPath)"
        }

        guard let buildResult = workspace.build?() else {
            return "Error: Failed to start build"
        }

        // Poll for completion with 10-minute timeout
        let deadline = Date().addingTimeInterval(600)
        while !(buildResult.completed ?? false) {
            if Date() > deadline {
                return "Error: Build timed out after 10 minutes"
            }
            Thread.sleep(forTimeInterval: 0.5)
        }

        // Collect all issue types (matching xcf's pattern)
        var output = ""

        if let errors = buildResult.buildErrors?() {
            collectIssues(errors, type: "Error", into: &output)
        }
        if let warnings = buildResult.buildWarnings?() {
            collectIssues(warnings, type: "Warning", into: &output)
        }
        if let analyzerIssues = buildResult.analyzerIssues?() {
            collectIssues(analyzerIssues, type: "Analyzer", into: &output)
        }
        if let testFailures = buildResult.testFailures?() {
            collectIssues(testFailures, type: "TestFailure", into: &output)
        }

        return output.isEmpty ? "Build succeeded" : output
    }

    // MARK: - Run

    /// Run a project via ScriptingBridge. Builds first — only runs if build is clean.
    nonisolated func runProject(projectPath: String) -> String {
        AuditLog.log(.xcode, "run: \(projectPath)")
        let resolvedPath = autoSelectProject() ?? projectPath
        // Build first to check for errors (matching xcf's pattern)
        let buildOutput = buildProject(projectPath: resolvedPath)
        guard buildOutput == "Build succeeded" else {
            return buildOutput
        }

        guard let xcode = xcodeApp() else {
            return "Error: Failed to connect to Xcode"
        }

        guard let workspace = xcode.open?(resolvedPath as Any) as? XcodeWorkspaceDocument else {
            return "Error: Could not open workspace at \(resolvedPath)"
        }

        workspace.stop?()
        Thread.sleep(forTimeInterval: 1)
        _ = workspace.runWithCommandLineArguments?(nil, withEnvironmentVariables: nil)

        return "Run started for \(resolvedPath)"
    }

    // MARK: - List Projects

    /// List all open Xcode projects and workspaces.
    nonisolated func listProjects() -> String {
        guard let xcode = xcodeApp() else {
            return "Error: Failed to connect to Xcode"
        }

        guard let documents = xcode.documents?() else {
            return "No open projects"
        }

        var projects: Set<String> = []

        for case let document as XcodeDocument in documents {
            guard let name = document.name, let path = document.path else { continue }
            if name.contains(".xcodeproj") || name.contains(".xcworkspace") {
                projects.insert(path)
            }
        }

        if projects.isEmpty {
            return "No open Xcode projects or workspaces"
        }

        let sorted = projects.sorted()
        var result = ""
        for (i, path) in sorted.enumerated() {
            result += "\(i + 1). \(path)\n"
        }
        return result
    }

    /// Select a project by number from the open projects list.
    nonisolated func selectProject(number: Int) -> String {
        guard let xcode = xcodeApp() else {
            return "Error: Failed to connect to Xcode"
        }

        guard let documents = xcode.documents?() else {
            return "Error: No open documents"
        }

        var projects: Set<String> = []

        for case let document as XcodeDocument in documents {
            guard let name = document.name, let path = document.path else { continue }
            if name.contains(".xcodeproj") || name.contains(".xcworkspace") {
                projects.insert(path)
            }
        }

        let sorted = projects.sorted()
        guard (1...sorted.count).contains(number) else {
            return "Error: Project number \(number) out of range (1-\(sorted.count))"
        }

        let selected = sorted[number - 1]

        // Security: must be in user's home directory
        let home = FileManager.default.homeDirectoryForCurrentUser.path
        guard selected.hasPrefix(home) else {
            return "Error: Project must be within your home directory"
        }

        // Persist selection so bump_version/get_version use the right project
        UserDefaults.standard.set(selected, forKey: "xcodeSelectedProjectPath")

        return selected
    }

    /// Auto-select the first open .xcodeproj project (not .xcworkspace wrappers).
    private nonisolated func autoSelectProject() -> String? {
        guard let xcode = xcodeApp() else { return nil }
        guard let documents = xcode.documents?() else { return nil }

        var projects: [String] = []
        for case let document as XcodeDocument in documents {
            guard let name = document.name, let path = document.path else { continue }
            if name.hasSuffix(".xcodeproj") {
                projects.append(path)
            }
        }
        // Prefer .xcodeproj over .xcworkspace
        if let first = projects.first { return first }
        // Fallback to any workspace
        for case let document as XcodeDocument in documents {
            guard let name = document.name, let path = document.path else { continue }
            if name.hasSuffix(".xcworkspace") { return path }
        }
        return nil
    }

    // MARK: - Schemes

    /// List schemes for a project.
    nonisolated func listSchemes(projectPath: String) -> [String] {
        guard isValidProjectPath(projectPath) else { return [] }

        guard let xcode = xcodeApp() else {
            return []
        }

        guard let workspace = xcode.open?(projectPath as Any) as? XcodeWorkspaceDocument,
              let schemes = workspace.schemes?() else
        {
            return []
        }

        var names: [String] = []
        for case let scheme as XcodeScheme in schemes {
            if let name = scheme.name {
                names.append(name)
            }
        }
        return names
    }

    // MARK: - Issue Collection (xcf pattern)

    /// Collect issues from an SBElementArray into a formatted string.
    /// Format: file:line:col [Type] message\n```swift\n<snippet>\n```
    private nonisolated func collectIssues(_ issues: SBElementArray, type: String, into output: inout String) {
        for case let issue as XcodeBuildError in issues {
            guard let message = issue.message else { continue }

            if let filePath = issue.filePath,
               let startLine = issue.startingLineNumber,
               let col = issue.startingColumnNumber
            {
                let endLine = issue.endingLineNumber ?? startLine
                output += "\(filePath):\(startLine):\(col) [\(type)] \(message)\n"

                // Include code snippet for context (matching xcf)
                let snippet = codeSnippet(filePath: filePath, startLine: startLine, endLine: endLine)
                if !snippet.isEmpty {
                    output += "```swift\n\(snippet)\n```\n"
                }
            } else {
                output += "[\(type)] \(message)\n"
            }
        }
    }

    /// Extract a code snippet from a file around the error location.
    private nonisolated func codeSnippet(filePath: String, startLine: Int, endLine: Int) -> String {
        guard let data = FileManager.default.contents(atPath: filePath),
              let content = String(data: data, encoding: .utf8) else
        {
            return ""
        }

        let lines = content.components(separatedBy: "\n")
        let start = max(startLine - 1, 0)
        let end = min(endLine, lines.count)
        guard start < end else { return "" }

        return lines[start..<end].enumerated().map { (i, line) in
            let num = start + i + 1
            return "\(num)\t\(line)"
        }.joined(separator: "\n")
    }

    // MARK: - Security

    /// Validate a project path to prevent command injection.
    private nonisolated func isValidProjectPath(_ path: String) -> Bool {
        let trimmed = path.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return false }
        guard trimmed.hasSuffix(".xcodeproj") || trimmed.hasSuffix(".xcworkspace") else { return false }
        guard !trimmed.contains("..") else { return false }
        guard !trimmed.contains(";") && !trimmed.contains("|") && !trimmed.contains("&") else { return false }
        guard trimmed.count < 1024 else { return false }
        // Verify the project actually has a pbxproj (not just an empty wrapper)
        if trimmed.hasSuffix(".xcodeproj") {
            let pbxproj = (trimmed as NSString).appendingPathComponent("project.pbxproj")
            guard FileManager.default.fileExists(atPath: pbxproj) else { return false }
        }
        return true
    }

    // MARK: - Project File Management (pbxproj editing)

    /// Generate a unique 24-char hex ID not present in the pbxproj content.
    private nonisolated func generateUniqueID(existing: String) -> String {
        var id: String
        repeat {
            id = String(
                format: "%08X%08X%08X",
                UInt32.random(in: 0...UInt32.max),
                UInt32.random(in: 0...UInt32.max),
                UInt32.random(in: 0...UInt32.max)
            )
        } while existing.contains(id)
        return id
    }

    /// Add a source file to the Xcode project's pbxproj.
    nonisolated func addFileToProject(filePath: String) -> String {
        guard let projectPath = autoSelectProject() else {
            return "Error: No open Xcode project found"
        }
        let pbxprojPath = (projectPath as NSString).appendingPathComponent("project.pbxproj")
        guard var content = try? String(contentsOfFile: pbxprojPath, encoding: .utf8) else {
            return "Error: Cannot read project file"
        }

        let fileName = (filePath as NSString).lastPathComponent

        // Check if already in project
        if content.contains("path = \"\(fileName)\"") {
            return "'\(fileName)' is already in the project"
        }

        let fileRefID = generateUniqueID(existing: content)
        let buildFileID = generateUniqueID(existing: content + fileRefID)

        let fileType = fileName.hasSuffix(".swift") ? "sourcecode.swift" : "text"

        // 1. Add PBXFileReference before end marker
        let fileRef =
            "\t\t\(fileRefID) /* \(fileName) */ = {isa = PBXFileReference; "
            + "lastKnownFileType = \(fileType); path = \"\(fileName)\"; "
            + "sourceTree = \"<group>\"; };\n"
        guard let refEnd = content.range(of: "/* End PBXFileReference section */") else {
            return "Error: Cannot find PBXFileReference section"
        }
        content.insert(contentsOf: fileRef, at: refEnd.lowerBound)

        // 2. Add PBXBuildFile for compilable sources
        if fileName.hasSuffix(".swift") || fileName.hasSuffix(".m") || fileName.hasSuffix(".c") {
            let buildFile =
                "\t\t\(buildFileID) /* \(fileName) in Sources */ = {isa = PBXBuildFile; fileRef = \(fileRefID) /* \(fileName) */; };\n"
            if let bfEnd = content.range(of: "/* End PBXBuildFile section */") {
                content.insert(contentsOf: buildFile, at: bfEnd.lowerBound)
            }

            // 3. Add to PBXSourcesBuildPhase files list
            if let sourcesStart = content.range(of: "/* Begin PBXSourcesBuildPhase section */") {
                let after = content[sourcesStart.upperBound...]
                if let filesStart = after.range(of: "files = ("),
                   let filesEnd = after.range(of: ");", range: filesStart.upperBound..<after.endIndex)
                {
                    let entry = "\t\t\t\t\(buildFileID) /* \(fileName) in Sources */,\n"
                    content.insert(contentsOf: entry, at: filesEnd.lowerBound)
                }
            }
        }

        // 4. Add to matching PBXGroup
        let dirName = ((filePath as NSString).deletingLastPathComponent as NSString).lastPathComponent
        let groupPattern = "/* \(dirName) */ = {\n\t\t\tisa = PBXGroup;\n\t\t\tchildren = ("
        let groupEntry = "\t\t\t\t\(fileRefID) /* \(fileName) */,\n"
        if let groupRange = content.range(of: groupPattern) {
            let searchStart = groupRange.upperBound
            if let closeRange = content.range(of: ");", range: searchStart..<content.endIndex) {
                content.insert(contentsOf: groupEntry, at: closeRange.lowerBound)
            }
        }

        do {
            try content.write(toFile: pbxprojPath, atomically: true, encoding: .utf8)
            return "Added '\(fileName)' to project"
        } catch {
            return "Error writing project file: \(error.localizedDescription)"
        }
    }

    /// Remove a source file from the Xcode project's pbxproj.
    nonisolated func removeFileFromProject(filePath: String) -> String {
        guard let projectPath = autoSelectProject() else {
            return "Error: No open Xcode project found"
        }
        let pbxprojPath = (projectPath as NSString).appendingPathComponent("project.pbxproj")
        guard var content = try? String(contentsOfFile: pbxprojPath, encoding: .utf8) else {
            return "Error: Cannot read project file"
        }

        let fileName = (filePath as NSString).lastPathComponent
        let escaped = NSRegularExpression.escapedPattern(for: fileName)

        // Find file reference ID
        let pattern = "([A-F0-9]{24}) /\\* \(escaped) \\*/ = \\{isa = PBXFileReference"
        guard let regex = try? NSRegularExpression(pattern: pattern),
              let match = regex.firstMatch(in: content, range: NSRange(content.startIndex..., in: content)),
              match.numberOfRanges >= 2 else
        {
            return "Error: '\(fileName)' not found in project"
        }
        let fileRefID = (content as NSString).substring(with: match.range(at: 1))

        // Remove all lines containing this file ref ID
        let lines = content.components(separatedBy: "\n")
        var filtered = lines.filter { !$0.contains(fileRefID) }

        // Also remove any build file referencing this file
        let buildPattern = "([A-F0-9]{24}) /\\* \(escaped) in Sources \\*/"
        let remaining = filtered.joined(separator: "\n")
        if let buildRegex = try? NSRegularExpression(pattern: buildPattern),
           let buildMatch = buildRegex.firstMatch(in: remaining, range: NSRange(remaining.startIndex..., in: remaining)),
           buildMatch.numberOfRanges >= 2
        {
            let buildID = (remaining as NSString).substring(with: buildMatch.range(at: 1))
            filtered = filtered.filter { !$0.contains(buildID) }
        }

        content = filtered.joined(separator: "\n")

        do {
            try content.write(toFile: pbxprojPath, atomically: true, encoding: .utf8)
            return "Removed '\(fileName)' from project"
        } catch {
            return "Error writing project file: \(error.localizedDescription)"
        }
    }

    // MARK: - Version & Build Bumping

    /// Read current version and build from pbxproj.
    nonisolated func getVersionInfo() -> String {
        guard let pbxPath = selectedPbxprojPath() else {
            return "Error: no Xcode project selected or pbxproj not found."
        }
        guard let data = FileManager.default.contents(atPath: pbxPath),
              let content = String(data: data, encoding: .utf8) else
        {
            return "Error: could not read \(pbxPath)"
        }
        let nsContent = content as NSString
        let fullRange = NSRange(location: 0, length: nsContent.length)

        let version: String
        if let vMatch = try? NSRegularExpression(pattern: #"MARKETING_VERSION\s*=\s*(\d+[\.\d]*)"#)
            .firstMatch(in: content, range: fullRange)
        {
            version = nsContent.substring(with: vMatch.range(at: 1))
        } else { version = "not found" }

        let build: String
        if let bMatch = try? NSRegularExpression(pattern: #"CURRENT_PROJECT_VERSION\s*=\s*(\d+)"#)
            .firstMatch(in: content, range: fullRange)
        {
            build = nsContent.substring(with: bMatch.range(at: 1))
        } else { build = "not found" }

        return "Version: \(version), Build: \(build)"
    }

    /// Bump MARKETING_VERSION patch by delta (+1 or -1). Also bumps build number by same delta.
    nonisolated func bumpVersion(delta: Int = 1) -> String {
        guard let pbxPath = selectedPbxprojPath() else {
            return "Error: no Xcode project selected or pbxproj not found."
        }
        guard let data = FileManager.default.contents(atPath: pbxPath),
              var content = String(data: data, encoding: .utf8) else
        {
            return "Error: could not read \(pbxPath)"
        }

        let nsContent = content as NSString
        let fullRange = NSRange(location: 0, length: nsContent.length)

        guard let vPattern = try? NSRegularExpression(pattern: #"MARKETING_VERSION\s*=\s*(\d+[\.\d]*)"#),
              let vMatch = vPattern.firstMatch(in: content, range: fullRange) else
        {
            return "Error: MARKETING_VERSION not found in pbxproj."
        }
        let oldVersion = nsContent.substring(with: vMatch.range(at: 1))
        var parts = oldVersion.components(separatedBy: ".").compactMap { Int($0) }
        if parts.isEmpty { return "Error: unexpected version format '\(oldVersion)'" }
        parts[parts.count - 1] = max(0, parts[parts.count - 1] + delta)
        let newVersion = parts.map(String.init).joined(separator: ".")
        content = content.replacingOccurrences(of: "MARKETING_VERSION = \(oldVersion)", with: "MARKETING_VERSION = \(newVersion)")

        // Also bump build by same delta
        guard let bPattern = try? NSRegularExpression(pattern: #"CURRENT_PROJECT_VERSION\s*=\s*(\d+)"#) else {
            return "Error: failed to create build pattern regex"
        }
        var newBuild = ""
        var oldBuild = ""
        let nsContent2 = content as NSString
        if let bMatch = bPattern.firstMatch(in: content, range: NSRange(location: 0, length: nsContent2.length)) {
            oldBuild = nsContent2.substring(with: bMatch.range(at: 1))
            if let n = Int(oldBuild) {
                newBuild = String(max(1, n + delta))
                content = content.replacingOccurrences(
                    of: "CURRENT_PROJECT_VERSION = \(oldBuild)",
                    with: "CURRENT_PROJECT_VERSION = \(newBuild)"
                )
            }
        }

        do {
            try content.write(toFile: pbxPath, atomically: true, encoding: .utf8)
            let buildInfo = newBuild.isEmpty ? "" : ", Build: \(oldBuild) → \(newBuild)"
            return "Version: \(oldVersion) → \(newVersion)\(buildInfo)"
        } catch {
            return "Error writing pbxproj: \(error.localizedDescription)"
        }
    }

    /// Bump CURRENT_PROJECT_VERSION by delta (+1 or -1).
    nonisolated func bumpBuild(delta: Int = 1) -> String {
        guard let pbxPath = selectedPbxprojPath() else {
            return "Error: no Xcode project selected or pbxproj not found."
        }
        guard let data = FileManager.default.contents(atPath: pbxPath),
              var content = String(data: data, encoding: .utf8) else
        {
            return "Error: could not read \(pbxPath)"
        }

        guard let pattern = try? NSRegularExpression(pattern: #"CURRENT_PROJECT_VERSION\s*=\s*(\d+)"#) else {
            return "Error: failed to create build pattern regex"
        }
        let nsContent = content as NSString
        guard let match = pattern.firstMatch(in: content, range: NSRange(location: 0, length: nsContent.length)) else {
            return "Error: CURRENT_PROJECT_VERSION not found in pbxproj."
        }

        let oldBuild = nsContent.substring(with: match.range(at: 1))
        guard let buildNum = Int(oldBuild) else { return "Error: unexpected build format '\(oldBuild)'" }
        let newBuild = String(max(1, buildNum + delta))
        content = content.replacingOccurrences(of: "CURRENT_PROJECT_VERSION = \(oldBuild)", with: "CURRENT_PROJECT_VERSION = \(newBuild)")

        do {
            try content.write(toFile: pbxPath, atomically: true, encoding: .utf8)
            return "Build: \(oldBuild) → \(newBuild)"
        } catch {
            return "Error writing pbxproj: \(error.localizedDescription)"
        }
    }

    /// Resolve the pbxproj path for the currently selected project.
    private nonisolated func selectedPbxprojPath() -> String? {
        // 1. Use the path saved by selectProject()
        if let saved = UserDefaults.standard.string(forKey: "xcodeSelectedProjectPath"),
           !saved.isEmpty {
            // saved path is the .xcodeproj or .xcworkspace directory
            let pbx = (saved as NSString).appendingPathComponent("project.pbxproj")
            if FileManager.default.fileExists(atPath: pbx) { return pbx }
            // Could be an .xcworkspace — look for an .xcodeproj inside
            if saved.hasSuffix(".xcworkspace") {
                let contents = (try? FileManager.default.contentsOfDirectory(atPath: saved)) ?? []
                if let proj = contents.first(where: { $0.hasSuffix(".xcodeproj") }) {
                    let pbx2 = ((saved as NSString).appendingPathComponent(proj) as NSString).appendingPathComponent("project.pbxproj")
                    if FileManager.default.fileExists(atPath: pbx2) { return pbx2 }
                }
            }
        }

        // 2. Fallback: use AGENT_PROJECT_FOLDER env var to find the .xcodeproj
        if let projectFolder = ProcessInfo.processInfo.environment["AGENT_PROJECT_FOLDER"],
           !projectFolder.isEmpty {
            let contents = (try? FileManager.default.contentsOfDirectory(atPath: projectFolder)) ?? []
            // Prefer .xcodeproj
            if let proj = contents.first(where: { $0.hasSuffix(".xcodeproj") }) {
                let pbx = ((projectFolder as NSString).appendingPathComponent(proj) as NSString).appendingPathComponent("project.pbxproj")
                if FileManager.default.fileExists(atPath: pbx) { return pbx }
            }
        }

        // 3. Last resort: ask Xcode for open documents
        guard let app = xcodeApp() else { return nil }
        guard let docs = app.documents?() as? [XcodeDocument], !docs.isEmpty else { return nil }
        let selectedIdx = UserDefaults.standard.integer(forKey: "xcodeSelectedProject")
        let idx = (selectedIdx > 0 && selectedIdx <= docs.count) ? selectedIdx - 1 : 0
        guard let path = docs[idx].path, !path.isEmpty else { return nil }
        return (path as NSString).appendingPathComponent("project.pbxproj")
    }
}
