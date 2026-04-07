import Foundation
import AgentAudit

/// Backs up files before editing, organized by tab UUID.
/// Structure: ~/Documents/AgentScript/backups/<tabUUID>/<timestamp>_<filename>
/// TTL: 1 week — old backups auto-cleaned on launch.
@MainActor
final class FileBackupService {
    static let shared = FileBackupService()

    private let backupsDir: URL
    private let ttl: TimeInterval = 7 * 24 * 60 * 60  // 1 week

    private init() {
        let home = FileManager.default.homeDirectoryForCurrentUser
        backupsDir = home.appendingPathComponent("Documents/AgentScript/backups")
        try? FileManager.default.createDirectory(at: backupsDir, withIntermediateDirectories: true)
        cleanExpired()
    }

    // MARK: - Backup

    /// Back up a file before editing. Returns the backup path on success.
    @discardableResult
    func backup(filePath: String, tabID: UUID) -> String? {
        let fm = FileManager.default
        guard fm.fileExists(atPath: filePath) else {
            AuditLog.log(.fileBackup, " SKIP — file not found: \(filePath)")
            return nil
        }

        let tabDir = backupsDir.appendingPathComponent(tabID.uuidString)
        do {
            try fm.createDirectory(at: tabDir, withIntermediateDirectories: true)
        } catch {
            AuditLog.log(.fileBackup, " ERROR creating dir: \(error)")
            return nil
        }

        let fileName = (filePath as NSString).lastPathComponent
        let timestamp = ISO8601DateFormatter().string(from: Date())
            .replacingOccurrences(of: ":", with: "-")
        let backupName = "\(timestamp)_\(fileName)"
        let backupURL = tabDir.appendingPathComponent(backupName)

        do {
            try fm.copyItem(atPath: filePath, toPath: backupURL.path)
            AuditLog.log(.fileBackup, " OK — \(backupURL.path)")
            return backupURL.path
        } catch {
            AuditLog.log(.fileBackup, " ERROR copying: \(error)")
            return nil
        }
    }

    // MARK: - Restore

    /// List all backups for a tab, newest first.
    func listBackups(tabID: UUID) -> [(original: String, backup: String, date: Date)] {
        let tabDir = backupsDir.appendingPathComponent(tabID.uuidString)
        let fm = FileManager.default
        guard let files = try? fm.contentsOfDirectory(atPath: tabDir.path) else { return [] }

        let formatter = ISO8601DateFormatter()
        return files.compactMap { name -> (String, String, Date)? in
            // Parse timestamp_filename format: 2026-04-03T04-46-51Z_README.md
            guard let underscoreRange = name.range(of: "_", range: name.index(name.startIndex, offsetBy: 15)..<name.endIndex) else { return nil }
            var timestampStr = String(name[..<underscoreRange.lowerBound])
            // Only restore colons in the time portion (after T), not the date hyphens
            if let tIdx = timestampStr.firstIndex(of: "T") {
                let timePart = String(timestampStr[timestampStr.index(after: tIdx)...])
                    .replacingOccurrences(of: "-", with: ":")
                timestampStr = String(timestampStr[...tIdx]) + timePart
            }
            guard let date = formatter.date(from: timestampStr) else { return nil }
            let fileName = String(name[underscoreRange.upperBound...])
            let backupPath = tabDir.appendingPathComponent(name).path
            return (fileName, backupPath, date)
        }.sorted { $0.2 > $1.2 }
    }

    /// Restore the most recent backup of a specific file for a tab.
    func restore(fileName: String, tabID: UUID) -> Bool {
        let backups = listBackups(tabID: tabID).filter { $0.original == fileName }
        guard let latest = backups.first else { return false }

        // Find the original path by searching common locations
        // The backup only stores the filename, not the full path
        // This is a limitation — caller should provide the full path
        let fm = FileManager.default
        do {
            try fm.copyItem(atPath: latest.backup, toPath: fileName)
            return true
        } catch {
            return false
        }
    }

    /// Restore a specific backup by its full backup path to a target path.
    func restore(backupPath: String, to targetPath: String) -> Bool {
        let fm = FileManager.default
        do {
            if fm.fileExists(atPath: targetPath) {
                try fm.removeItem(atPath: targetPath)
            }
            try fm.copyItem(atPath: backupPath, toPath: targetPath)
            return true
        } catch {
            return false
        }
    }

    // MARK: - Cleanup

    /// Remove backups older than TTL (1 week).
    func cleanExpired() {
        let fm = FileManager.default
        guard let tabDirs = try? fm.contentsOfDirectory(atPath: backupsDir.path) else { return }

        let cutoff = Date().addingTimeInterval(-ttl)

        for tabDir in tabDirs {
            let tabPath = backupsDir.appendingPathComponent(tabDir)
            guard let files = try? fm.contentsOfDirectory(atPath: tabPath.path) else { continue }

            for file in files {
                let filePath = tabPath.appendingPathComponent(file)
                if let attrs = try? fm.attributesOfItem(atPath: filePath.path),
                   let modified = attrs[.modificationDate] as? Date,
                   modified < cutoff {
                    try? fm.removeItem(at: filePath)
                }
            }

            // Remove empty tab dirs
            if let remaining = try? fm.contentsOfDirectory(atPath: tabPath.path), remaining.isEmpty {
                try? fm.removeItem(at: tabPath)
            }
        }
    }

    /// Remove all backups for a specific tab.
    func clearBackups(tabID: UUID) {
        let tabDir = backupsDir.appendingPathComponent(tabID.uuidString)
        try? FileManager.default.removeItem(at: tabDir)
    }

    /// Remove ALL backups across all tabs.
    func clearAllBackups() {
        let fm = FileManager.default
        guard let dirs = try? fm.contentsOfDirectory(atPath: backupsDir.path) else { return }
        for dir in dirs {
            let path = backupsDir.appendingPathComponent(dir)
            try? fm.removeItem(at: path)
        }
    }

    /// Count backups for a tab.
    func backupCount(tabID: UUID) -> Int {
        let tabDir = backupsDir.appendingPathComponent(tabID.uuidString)
        return (try? FileManager.default.contentsOfDirectory(atPath: tabDir.path))?.count ?? 0
    }

    /// List ALL backups across all tabs, newest first.
    func allBackups() -> [(original: String, backup: String, date: Date)] {
        let fm = FileManager.default
        guard let tabDirs = try? fm.contentsOfDirectory(atPath: backupsDir.path) else { return [] }
        var all: [(String, String, Date)] = []
        for dir in tabDirs {
            guard let uuid = UUID(uuidString: dir) else { continue }
            all.append(contentsOf: listBackups(tabID: uuid))
        }
        return all.sorted { $0.2 > $1.2 }
    }

    /// Count ALL backups across all tabs.
    func totalBackupCount() -> Int {
        let fm = FileManager.default
        guard let tabDirs = try? fm.contentsOfDirectory(atPath: backupsDir.path) else { return 0 }
        var count = 0
        for dir in tabDirs {
            let tabPath = backupsDir.appendingPathComponent(dir)
            count += (try? fm.contentsOfDirectory(atPath: tabPath.path))?.count ?? 0
        }
        return count
    }

    // MARK: - Per-Task Edit Snapshots

    /// Tracks file versions within a single task for diff/rollback.
    /// Key = file path, Value = ordered list of backup paths (oldest first).
    private var taskSnapshots: [String: [String]] = [:]

    /// Maximum snapshots per file per task (circular buffer).
    private let maxSnapshots = 100

    /// Record a snapshot for the current task. Call before each edit.
    /// Returns the version number (1-based).
    @discardableResult
    func snapshot(filePath: String, tabID: UUID) -> Int {
        let backupPath = backup(filePath: filePath, tabID: tabID)
        guard let path = backupPath else { return 0 }

        var versions = taskSnapshots[filePath, default: []]
        if versions.count >= maxSnapshots {
            // Remove oldest, circular buffer style
            let removed = versions.removeFirst()
            try? FileManager.default.removeItem(atPath: removed)
        }
        versions.append(path)
        taskSnapshots[filePath] = versions
        return versions.count
    }

    /// Get the version count for a file in the current task.
    func versionCount(for filePath: String) -> Int {
        taskSnapshots[filePath]?.count ?? 0
    }

    /// Rollback a file to a specific version (1-based). Returns true on success.
    func rollback(filePath: String, toVersion: Int) -> Bool {
        guard let versions = taskSnapshots[filePath],
              toVersion >= 1 && toVersion <= versions.count else { return false }
        let backupPath = versions[toVersion - 1]
        return restore(backupPath: backupPath, to: filePath)
    }

    /// Get a summary of all files edited in the current task with version counts.
    func taskEditSummary() -> String {
        guard !taskSnapshots.isEmpty else { return "No files edited in this task." }
        return taskSnapshots.map { path, versions in
            let name = (path as NSString).lastPathComponent
            return "\(name): \(versions.count) version(s)"
        }.sorted().joined(separator: "\n")
    }

    /// Clear task snapshots (call at task start).
    func clearTaskSnapshots() {
        taskSnapshots.removeAll()
    }
}
