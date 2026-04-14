import Foundation

@_cdecl("script_main")
public func scriptMain() -> Int32 {
    // Get parameters from environment
    let version = ProcessInfo.processInfo.environment["AGENT_SCRIPT_ARGS"] ?? ""
    
    // Args: version|notes|binaryPath|workingDir (e.g. "v1.0.20|Release notes|./build/App.dmg|/path/to/repo")
    let args = version.components(separatedBy: "|")
    
    guard args.count >= 1, !args[0].isEmpty else {
        print("Usage: Provide arguments as version|notes|binaryPath|workingDir")
        print("Example: v1.0.20|Release notes here|./build/export/App.dmg|/path/to/repo")
        print("Notes support markdown. Use \\n for line breaks in single-line format.")
        return 1
    }
    
    let releaseVersion = args[0]
    let releaseNotes = args.count > 1
        ? formatReleaseNotes(
            args[1].replacingOccurrences(of: "\\n", with: "\n")
        )
        : "Release \(releaseVersion)"
    let binaryPath = args.count > 2 ? args[2] : ""
    let workingDir = args.count > 3 ? args[3] : FileManager.default.currentDirectoryPath
    
    // Check common paths for gh CLI
    let ghPaths = [
        "/opt/homebrew/bin/gh",
        "/usr/local/bin/gh",
        "/opt/local/bin/gh",
        "/usr/bin/gh"
    ]
    
    var ghPath: String?
    let fm = FileManager.default
    
    for path in ghPaths {
        if fm.fileExists(atPath: path) {
            ghPath = path
            print("Found gh CLI at: \(path)")
            break
        }
    }
    
    // Fallback to PATH lookup
    if ghPath == nil {
        let whichTask = Process()
        whichTask.executableURL = URL(fileURLWithPath: "/bin/sh")
        whichTask.arguments = ["-c", "which gh 2>/dev/null"]
        whichTask.currentDirectoryURL = URL(fileURLWithPath: workingDir)
        var whichEnv = ProcessInfo.processInfo.environment
        whichEnv["HOME"] = NSHomeDirectory()
        let whichPaths = "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
        whichEnv["PATH"] = whichPaths + ":" + (whichEnv["PATH"] ?? "")
        whichTask.environment = whichEnv

        let whichPipe = Pipe()
        whichTask.standardOutput = whichPipe
        whichTask.standardError = whichPipe
        
        do {
            try whichTask.run()
            whichTask.waitUntilExit()
            let data = whichPipe.fileHandleForReading.readDataToEndOfFile()
            if let output = String(data: data, encoding: .utf8), !output.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                ghPath = output.trimmingCharacters(in: .whitespacesAndNewlines)
                print("Found gh in PATH: \(ghPath!)")
            }
        } catch {
            print("Error finding gh: \(error)")
        }
    }
    
    guard let gh = ghPath else {
        print("Error: gh CLI not found. Install with: brew install gh")
        return 1
    }
    
    // Build release command
    var arguments = [
        "release", "create", releaseVersion,
        "--title", releaseVersion,
        "--notes", releaseNotes
    ]
    
    if !binaryPath.isEmpty {
        arguments.append(binaryPath)
    }
    
    // Create the release
    let task = Process()
    task.executableURL = URL(fileURLWithPath: gh)
    task.arguments = arguments
    task.currentDirectoryURL = URL(fileURLWithPath: workingDir)
    
    let pipe = Pipe()
    task.standardOutput = pipe
    task.standardError = pipe
    
    print("\nCreating GitHub release \(releaseVersion)...")
    
    do {
        try task.run()
        task.waitUntilExit()
        
        let data = pipe.fileHandleForReading.readDataToEndOfFile()
        if let output = String(data: data, encoding: .utf8) {
            print(output)
        }
        
        if task.terminationStatus == 0 {
            print("\n✓ Release \(releaseVersion) created successfully!")
        } else {
            print("\n✗ Failed to create release")
        }
        
        return task.terminationStatus
    } catch {
        print("Error creating release: \(error)")
        return 1
    }
    // Format release notes with collapsible sections
    func formatReleaseNotes(_ rawNotes: String) -> String {
        var formatted = ""
        
        // Split by sections (separated by --- or ###)
        let sections = rawNotes.components(separatedBy: "---").map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
        
        for section in sections {
            if section.isEmpty { continue }
            
            // Extract section title (first line)
            let lines = section.components(separatedBy: "\n")
            let title = lines.first?.trimmingCharacters(in: .whitespacesAndNewlines) ?? "Section"
            let content = lines.dropFirst().joined(separator: "\n").trimmingCharacters(in: .whitespacesAndNewlines)
            
            // Create collapsible section
            formatted += "<details>\n<summary>\(title)</summary>\n\n\(content)\n\n</details>\n\n"
        }
        
        return formatted.isEmpty ? rawNotes : formatted
    }
}