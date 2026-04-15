import Foundation

/// Single source of truth for per-project hidden directories the agent writes
/// to. Everything lives under `{projectFolder}/.agent/…` so one `.gitignore`
/// entry (`.agent/`) covers them all.
///
/// Subdirs:
/// - `.agent/index/`     — project index JSONL (`index.jsonl`)
/// - `.agent/memory/`    — project-scoped memory files (Claude-compatible)
/// - `.agent/worktrees/` — git worktrees created via the git tool
enum AgentProjectPaths {

    /// Root hidden dir — never write directly here, always a subdir.
    static let rootDirName = ".agent"

    /// Named subdirs. Add new ones alongside as features grow.
    enum Subdir: String {
        case index = "index"
        case memory = "memory"
        case worktrees = "worktrees"
        case plans = "plans"
    }

    /// Return `{projectFolder}/.agent/{subdir}/` as a URL.
    static func url(in projectFolder: String, _ subdir: Subdir) -> URL {
        URL(fileURLWithPath: projectFolder)
            .appendingPathComponent(rootDirName, isDirectory: true)
            .appendingPathComponent(subdir.rawValue, isDirectory: true)
    }

    /// Return `{projectFolder}/.agent/{subdir}/` as a plain string path
    /// (for shell command interpolation — `git worktree add`, etc.).
    static func path(in projectFolder: String, _ subdir: Subdir) -> String {
        url(in: projectFolder, subdir).path
    }
}
