import Foundation

/// / Hard local guardrail for shell commands — runs BEFORE every execution surface / and rejects catastrophic commands
/// without dispatching them. Primary defense; / LLM system prompts are backstops, not the enforcement layer.
enum ShellSafetyService {

    struct Verdict {
        /// True when the command is permitted to run.
        let allowed: Bool
        /// Human-readable explanation when blocked, suitable to return as a
        /// tool result so the LLM understands why and doesn't retry.
        let reason: String?
        /// Short identifier of the matched rule, for AuditLog.
        let rule: String?

        static let ok = Verdict(allowed: true, reason: nil, rule: nil)

        static func block(reason: String, rule: String) -> Verdict {
            Verdict(allowed: false, reason: reason, rule: rule)
        }
    }

    /// / Inspect a shell command and return whether it's safe to dispatch. / Splits compound commands on shell
    /// separators (`;`, `&&`, `||`, `|`, / newline) and checks each segment independently — so `ls; rm -rf /` / is blocked even though the first half is harmless.
    static func check(_ command: String) -> Verdict {
        // Whole-command checks (fork bomb relies on `;` and `|` which are exactly what splitOnShellSeparators tears
        // apart, so it has to run BEFORE splitting).
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

    /// / Tokenized rm check. We collect every flag (combined like `-rf`, / separated like `-r -f`, or long-form like
    /// `--recursive --force`) and / every non-flag positional arg, then refuse the command if it has both / recursive AND force flags AND any positional that resolves to a / dangerous target.
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

    /// / Returns a reason string when the path is too broad to ever be a / reasonable target for `rm -rf`. Returns nil
    /// for safe targets so the / caller can keep checking the rest of the positionals.
    private static func dangerousRmTargetReason(_ target: String) -> String? {
        // Strip surrounding quotes the tokenizer left intact.
        var t = target
        if (t.hasPrefix("\"") && t.hasSuffix("\"")) || (t.hasPrefix("'") && t.hasSuffix("'")) {
            t = String(t.dropFirst().dropLast())
        }

        // Bare wildcard or current/parent dir — context-dependent but the
        // worst-case is catastrophic, so we refuse.
        if t == "*" || t == "*.*" || t == "." || t == ".." || t == "./*" || t == "./" || t == ".*" {
            return "this glob/relative path is too broad — the working directory could be `/` or `~`"
        }

        // Root and root-glob.
        if t == "/" || t == "/*" || t == "/.*" || t == "/." || t == "/.." {
            return "this would erase the entire filesystem"
        }

        // Top-level system directories on macOS + Linux.
        let systemRoots: Set<String> = [
            "/etc", "/usr", "/bin", "/sbin", "/var", "/lib", "/lib64",
            "/boot", "/proc", "/sys", "/dev", "/run", "/opt",
            "/System", "/Library", "/Applications", "/private",
            "/Volumes", "/Network", "/cores", "/Users", "/home",
        ]
        // Match exact and trailing-slash and `/dir/*` glob forms.
        for root in systemRoots {
            if t == root || t == root + "/" || t == root + "/*" || t == root + "/.*" {
                return "this is a critical system directory"
            }
        }

        // Home directory in any of its written forms.
        let homeForms: Set<String> = [
            "~", "~/", "~/*", "~/.*",
            "$HOME", "${HOME}", "$HOME/", "${HOME}/",
            "$HOME/*", "${HOME}/*", "$HOME/.*", "${HOME}/.*",
        ]
        if homeForms.contains(t) {
            return "this is your home directory"
        }

        // The literal expanded home — best-effort match.
        let realHome = NSHomeDirectory()
        if t == realHome || t == realHome + "/" || t == realHome + "/*" {
            return "this is your home directory"
        }

        return nil
    }

    // MARK: - Rule: find -delete

    private static func checkFindDelete(tokens: [String]) -> Verdict? {
        guard tokens.contains("-delete") else { return nil }
        guard let findIdx = tokens.firstIndex(of: "find") else { return nil }
        // The first positional after `find` is the search root.
        if findIdx + 1 < tokens.count {
            let root = tokens[findIdx + 1]
            if dangerousRmTargetReason(root) != nil {
                return .block(
                    reason: "Refused: `find \(root) ... -delete` — `find -delete` recursively removes everything matching the predicates and the search root is too broad. Narrow the search root.",
                    rule: "find.delete-broad-root"
                )
            }
        }
        return nil
    }

    // MARK: - Rule: chmod/chown -R against system roots

    private static func checkRecursivePermsOnRoot(tokens: [String]) -> Verdict? {
        guard let cmdIdx = tokens.firstIndex(where: { $0 == "chmod" || $0 == "chown" }) else { return nil }
        let cmd = tokens[cmdIdx]
        var recursive = false
        var positionals: [String] = []
        for j in (cmdIdx + 1)..<tokens.count {
            let t = tokens[j]
            if t == "-R" || t == "--recursive" {
                recursive = true
            } else if t.hasPrefix("-") {
                if t.dropFirst().contains("R") { recursive = true }
            } else {
                positionals.append(t)
            }
        }
        guard recursive else { return nil }
        for target in positionals {
            if dangerousRmTargetReason(target) != nil {
                return .block(
                    reason: "Refused: `\(cmd) -R ... \(target)` — recursively changing permissions/ownership on a system root will brick the OS or your account. Narrow the target.",
                    rule: "perms.recursive-on-root"
                )
            }
        }
        return nil
    }

    // MARK: - Rule: disk wipes

    private static func checkDiskWipe(stripped: String, tokens: [String]) -> Verdict? {
        // dd ... of=/dev/disk*  or  of=/dev/sd*  or  of=/dev/rdisk*
        if tokens.contains("dd") {
            for t in tokens where t.hasPrefix("of=/dev/") {
                let dest = String(t.dropFirst(3))
                if isRawDiskDevice(dest) {
                    return .block(
                        reason: "Refused: `dd of=\(dest)` writes raw bytes to a physical disk device, which destroys the partition table and any filesystem on it.",
                        rule: "dd.raw-disk"
                    )
                }
            }
        }

        // mkfs.* — formatting a filesystem.
        if let first = tokens.first, first.hasPrefix("mkfs") {
            return .block(
                reason: "Refused: `\(first)` formats a filesystem and erases everything on the target device.",
                rule: "mkfs"
            )
        }

        // diskutil eraseDisk / zeroDisk / secureErase / eraseVolume
        if tokens.first == "diskutil" && tokens.count >= 2 {
            let verb = tokens[1].lowercased()
            if verb == "erasedisk" || verb == "zerodisk" || verb == "secureerase" || verb == "erasevolume" {
                return .block(
                    reason: "Refused: `diskutil \(tokens[1])` erases a physical disk or volume. Mount/list operations are fine, but erase verbs are blocked.",
                    rule: "diskutil.erase"
                )
            }
        }

        return nil
    }

    private static func isRawDiskDevice(_ path: String) -> Bool {
        // /dev/disk2, /dev/rdisk2, /dev/sda, /dev/nvme0n1, /dev/hd0
        let lower = path.lowercased()
        return lower.hasPrefix("/dev/disk")
            || lower.hasPrefix("/dev/rdisk")
            || lower.hasPrefix("/dev/sd")
            || lower.hasPrefix("/dev/hd")
            || lower.hasPrefix("/dev/nvme")
    }

    // MARK: - Rule: redirect to disk device

    /// / Catches `> /dev/disk2` or `cat foo > /dev/sda` even when the rest of / the command is a different binary. Done
    /// as a regex on the raw segment / because the redirect operator can sit anywhere.
    private static func checkRedirectToDisk(_ command: String) -> Verdict {
        let pattern = #">+\s*/dev/(?:r?disk[0-9]|sd[a-z]|hd[a-z]|nvme[0-9])"#
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
        for t in positionals.dropLast() {  // dropLast = the destination /dev/null itself
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

    /// Strip leading `sudo` and `exec` (and chains thereof) so an attacker
    /// can't disguise `rm -rf /` as `sudo exec sudo rm -rf /`.
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
            // Also strip env-var assignments at the start, like `FOO=bar rm -rf /`.
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

    /// Whitespace tokenizer that preserves quoted substrings as a single
    /// token. Good enough for safety classification — not a full bash parser.
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

    /// Split a command on shell separators so each side of `;`/`&&`/`||`/`|`
    /// gets independently classified. Quoted strings are preserved.
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
