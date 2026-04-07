import Foundation

extension CodingService {
    // MARK: - Shell Command Builders (testable, executed via UserService XPC)

    /// Default directory for tools when no path is provided.
    static let defaultDir = FileManager.default.homeDirectoryForCurrentUser.path

    /// Shell-escape a string using single quotes (POSIX safe).
    static func shellEscape(_ s: String) -> String {
        "'" + s.replacingOccurrences(of: "'", with: "'\\''") + "'"
    }

    /// Trim the home directory prefix from a path for cleaner output.
    static func trimHome(_ path: String) -> String {
        let home = defaultDir
        if path.hasPrefix(home) {
            let trimmed = String(path.dropFirst(home.count))
            return trimmed.hasPrefix("/") ? "~" + trimmed : "~/" + trimmed
        }
        return path
    }

    /// Format a flat list of file paths into a pretty directory tree.
    static func formatFileTree(_ output: String) -> String {
        let paths = output.components(separatedBy: "\n").filter { !$0.isEmpty }
        guard !paths.isEmpty else { return output }

        // Group by directory
        var dirs: [String: [String]] = [:] // dir -> [filename]
        var rootFiles: [String] = []
        for path in paths {
            let comps = path.components(separatedBy: "/")
            if comps.count == 1 {
                rootFiles.append(comps[0])
            } else {
                let dir = comps.dropLast().joined(separator: "/")
                guard let file = comps.last else { continue }
                dirs[dir, default: []].append(file)
            }
        }

        var result = ""
        let sortedDirs = dirs.keys.sorted()

        // Root files first
        if !rootFiles.isEmpty {
            for file in rootFiles {
                result += "  \(file)\n"
            }
        }

        // Then each directory group
        for dir in sortedDirs {
            let files = dirs[dir]!
            result += "📁 \(dir)/\n"
            for (i, file) in files.enumerated() {
                let prefix = (i == files.count - 1) ? "   └─ " : "   ├─ "
                result += "\(prefix)\(file)\n"
            }
        }

        return result.trimmingCharacters(in: .newlines)
    }

    static func buildListFilesCommand(pattern: String, path: String?) -> String {
        let pat = shellEscape(pattern)
        // Working directory set on Process — find . outputs relative paths (saves tokens)
        // -type f: files only, prune dotdirs and build artifacts
        return "find . -maxdepth 8 -type f -name \(pat)"
            + " -not -path '*/.*'"
            + " -not -path '*/.build/*'"
            + " -not -path '*/.swiftpm/*'"
            + " -not -path '*/.git/*'"
            + " -not -path '*/Library/*'"
            + " -not -path '*/Movies/*'"
            + " -not -path '*/Music/*'"
            + " -not -path '*/Pictures/*'"
            + " -not -path '*/DerivedData/*'"
            + " -not -name '.DS_Store'"
            + " -not -name '*.xcuserstate'"
            + " 2>/dev/null | sed 's|^\\./||' | sort | head -200"
    }

    /// Resolve the working directory for a command from an optional path.
    static func resolveDir(_ path: String?) -> String {
        return path ?? defaultDir
    }

    static func buildSearchFilesCommand(pattern: String, path: String?, include: String?) -> String {
        let dir = shellEscape(path ?? defaultDir)
        let pat = shellEscape(pattern)
        var cmd = "grep -rn --color=never"
        if let include {
            cmd += " --include=\(shellEscape(include))"
        }
        cmd +=
             " --exclude-dir=.git --exclude-dir=.build "
                + "--exclude-dir=build --exclude-dir=.swiftpm "
                + "--exclude-dir=node_modules --exclude-dir=DerivedData "
                + "--exclude-dir=Library --exclude-dir=Movies "
                + "--exclude-dir=Music --exclude-dir=Pictures"
        cmd += " --binary-files=without-match"
        cmd += " \(pat) \(dir) 2>/dev/null | head -100"
        return cmd
    }

    static func buildGitStatusCommand(path: String?) -> String {
        // Working directory set on Process — no cd needed
        return "echo \"Branch: $(git branch --show-current)\" && git status --short"
    }

    static func buildGitDiffCommand(path: String?, staged: Bool, target: String?) -> String {
        var cmd = "git diff --stat -p"
        if staged { cmd += " --cached" }
        if let target { cmd += " \(shellEscape(target))" }
        return cmd
    }

    static func buildGitLogCommand(path: String?, count: Int?) -> String {
        let n = min(count ?? 20, 100)
        return "git log --oneline --no-decorate -\(n)"
    }

    static func buildGitCommitCommand(path: String?, message: String, files: [String]?) -> String {
        var cmd: String
        if let files, !files.isEmpty {
            let escaped = files.map { shellEscape($0) }.joined(separator: " ")
            cmd = "git add \(escaped)"
        } else {
            cmd = "git add -A"
        }
        cmd += " && git diff --cached --quiet && echo 'Nothing to commit (no staged changes)' || git commit -m \(shellEscape(message))"
        return cmd
    }

    static func buildGitBranchCommand(path: String?, name: String, checkout: Bool) -> String {
        if checkout {
            return "git checkout -b \(shellEscape(name))"
        } else {
            return "git branch \(shellEscape(name))"
        }
    }

}
