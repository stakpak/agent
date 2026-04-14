import Foundation

/// / Hard local guardrail for shell commands
enum ShellSafetyService {

    struct Verdict {
        /// True when the command is permitted to run.
        let allowed: Bool
        /// Human-readable explanation when blocked
        let reason: String?
        /// Short identifier of the matched rule, for AuditLog.
        let rule: String?

        static let ok = Verdict(allowed: true, reason: nil, rule: nil)

        static func block(reason: String, rule: String) -> Verdict {
            Verdict(allowed: false, reason: reason, rule: rule)
        }
    }

    /// / Inspect a shell command and return whether it's safe to dispatch.
    static func check(_ command: String) -> Verdict {
        // Whole-command checks (fork bomb relies on `;` and `|` which are exact
        let forkVerdict = checkForkBomb(command)
        if !forkVerdict.allowed { return forkVerdict }

        for segment in splitOnShellSeparators(command) {
            let trimmed = segment.trimmingCharacters(in: .whitespacesAndNewlines)
            if trimmed.isEmpty { continue }
            let verdict = checkSingleSegment(trimmed)
            if !verdict.allowed { return verdict }
        }
        return .ok
    }

    // MARK: - Single segment

    private static func checkSingleSegment(_ command: String) -> Verdict {
        // Strip leading sudo/exec wrappers so they can't disguise the payload.
        let stripped = stripPrefixWrappers(command)
        let tokens = tokenize(stripped)
        if tokens.isEmpty { return .ok }

        // 1. rm -rf <dangerous-target>
        if let v = checkDangerousRm(tokens: tokens), !v.allowed { return v }

        // 2. find <dangerous-root> ... -delete
        if let v = checkFindDelete(tokens: tokens), !v.allowed { return v }

        // 3. chmod / chown -R against system roots
        if let v = checkRecursivePermsOnRoot(tokens: tokens), !v.allowed { return v }

        // 4. dd / mkfs / diskutil eraseDisk
        if let v = checkDiskWipe(stripped: stripped, tokens: tokens), !v.allowed { return v }

        // 5. Output redirection to a raw disk device
        let redirectVerdict = checkRedirectToDisk(stripped)
        if !redirectVerdict.allowed { return redirectVerdict }

        // 6. (fork bomb checked at the top of check() before splitting)

        // 7. Move home/system to /dev/null
        if let v = checkMoveToDevNull(tokens: tokens), !v.allowed { return v }

        return .ok
    }

    // MARK: - Rule: dangerous rm

    /// / Tokenized rm check. We collect every flag
    private static func checkDangerousRm(tokens: [String]) -> Verdict? {
        guard let rmIdx = tokens.firstIndex(of: "rm") else { return nil }
        var hasR = false
        var hasF = false
        var positionals: [String] = []

        var i = rmIdx + 1
        while i < tokens.count {
            let t = tokens[i]
            if t == "--recursive" || t == "--Recursive" { hasR = true }
            else if t == "--force" { hasF = true }
            else if t == "--no-preserve-root" {
                // Only ever passed when someone explicitly wants to wipe /.
                return .block(
                    reason: "Refused: `rm --no-preserve-root` is only used to bypass macOS/GNU's safeguard against deleting `/`. This command is permanently disabled in Agent!.",
                    rule: "rm.no-preserve-root"
                )
            }
            else if t.hasPrefix("--") {
                // Other long options — ignore.
            } else if t.hasPrefix("-") && t.count >= 2 {
                let chars = t.dropFirst()
                if chars.contains("r") || chars.contains("R") { hasR = true }
                if chars.contains("f") || chars.contains("F") { hasF = true }
            } else {
                positionals.append(t)
            }
            i += 1
        }

        guard hasR && hasF else { return nil }

        for target in positionals {
            if let reason = dangerousRmTargetReason(target) {
                return .block(
                    reason: "Refused: `rm -rf \(target)` — \(reason). Agent! blocks this pattern locally before it reaches any shell. Narrow the path to a specific subdirectory you actually want to delete.",
                    rule: "rm.dangerous-target"
                )
            }
        }
        return nil
    }

    /// / Returns a reason string when path is too broad to ever be a / reasonab
    private static func dangerousRmTargetReason(_ target: String) -> String? {
        // Strip surrounding quotes the tokenizer left intact.
        var t = target
        if (t.hasPrefix("\"") && t.hasSuffix("\"")) || (t.hasPrefix("'") && t.hasSuffix("'")) {
            t = String(t.dropFirst().dropLast())
        }

        // Bare wildcard or current/parent dir — context-dependent but the worst
        /// " || t == "./" || t == ".*" { return "this glob/relative path is too
        if command.range(of: pattern, options: .regularExpression) != nil {
            return .block(
                reason: "Refused: redirecting output to a raw disk device (`> /dev/disk*`, `> /dev/sd*`) destroys the disk's filesystem.",
                rule: "redirect.raw-disk"
            )
        }
        return .ok
    }

    // MARK: - Rule: fork bomb

    private static func checkForkBomb(_ command: String) -> Verdict {
        // The classic `:(){ :|:& };:` and minor variations.
        let collapsed = command.replacingOccurrences(of: " ", with: "")
        if collapsed.contains(":(){:|:&};:") || collapsed.contains(":(){:|:&};:&") {
            return .block(
                reason: "Refused: classic fork bomb. This recursively spawns processes until the kernel runs out of process slots and the machine becomes unresponsive.",
                rule: "fork-bomb"
            )
        }
        return .ok
    }

    // MARK: - Rule: mv ~ /dev/null and friends

    private static func checkMoveToDevNull(tokens: [String]) -> Verdict? {
        guard tokens.first == "mv" else { return nil }
        guard tokens.contains("/dev/null") else { return nil }
        // Look at every non-flag positional except the destination.
        let positionals = tokens.dropFirst().filter { !$0.hasPrefix("-") }
        for t in positionals.dropLast() {  // dropLast = the destination /dev/nu
            if dangerousRmTargetReason(t) != nil {
                return .block(
                    reason: "Refused: moving `\(t)` to `/dev/null` is equivalent to deleting it permanently.",
                    rule: "mv.to-devnull"
                )
            }
        }
        return nil
    }

    // MARK: - Helpers

    /// Strip leading `sudo` and `exec`
    private static func stripPrefixWrappers(_ command: String) -> String {
        var result = command.trimmingCharacters(in: .whitespacesAndNewlines)
        let prefixes = ["sudo ", "exec ", "command ", "builtin ", "eval ", "doas "]
        var changed = true
        while changed {
            changed = false
            for prefix in prefixes where result.lowercased().hasPrefix(prefix) {
                result = String(result.dropFirst(prefix.count)).trimmingCharacters(in: .whitespacesAndNewlines)
                changed = true
            }
            // Also strip env-var assignments at the start, like `FOO=bar rm -rf
            if let space = result.firstIndex(of: " "),
               result[..<space].contains("="),
               !result[..<space].contains("/")
            {
                result = String(result[result.index(after: space)...])
                    .trimmingCharacters(in: .whitespacesAndNewlines)
                changed = true
            }
        }
        return result
    }

    /// Whitespace tokenizer preserves quoted substrings as a single token.
    private static func tokenize(_ command: String) -> [String] {
        var tokens: [String] = []
        var current = ""
        var inSingle = false
        var inDouble = false
        var escape = false
        for ch in command {
            if escape {
                current.append(ch)
                escape = false
                continue
            }
            if ch == "\\" {
                current.append(ch)
                escape = true
                continue
            }
            if ch == "'" && !inDouble {
                inSingle.toggle()
                current.append(ch)
                continue
            }
            if ch == "\"" && !inSingle {
                inDouble.toggle()
                current.append(ch)
                continue
            }
            if (ch == " " || ch == "\t") && !inSingle && !inDouble {
                if !current.isEmpty {
                    tokens.append(current)
                    current = ""
                }
                continue
            }
            current.append(ch)
        }
        if !current.isEmpty { tokens.append(current) }
        return tokens
    }

    /// Split a command on shell separators so each side of `;`/`&&`/`||`/`|` ge
    private static func splitOnShellSeparators(_ command: String) -> [String] {
        var segments: [String] = []
        var current = ""
        var inSingle = false
        var inDouble = false
        var i = command.startIndex
        while i < command.endIndex {
            let ch = command[i]
            if ch == "'" && !inDouble { inSingle.toggle(); current.append(ch); i = command.index(after: i); continue }
            if ch == "\"" && !inSingle { inDouble.toggle(); current.append(ch); i = command.index(after: i); continue }
            if !inSingle && !inDouble {
                let next = command.index(after: i)
                // && or ||
                if next < command.endIndex {
                    let two = String(command[i...next])
                    if two == "&&" || two == "||" {
                        if !current.isEmpty { segments.append(current); current = "" }
                        i = command.index(after: next)
                        continue
                    }
                }
                if ch == ";" || ch == "|" || ch == "\n" {
                    if !current.isEmpty { segments.append(current); current = "" }
                    i = command.index(after: i)
                    continue
                }
            }
            current.append(ch)
            i = command.index(after: i)
        }
        if !current.isEmpty { segments.append(current) }
        return segments
    }
}
