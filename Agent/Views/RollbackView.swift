import SwiftUI

/// Time Machine-style file backups — grouped by filename with expandable versions.
struct RollbackView: View {
    @Bindable var viewModel: AgentViewModel
    @State private var backups: [(original: String, backup: String, date: Date)] = []
    @State private var restoreResult: String?
    @State private var showClearConfirmation = false
    @State private var expandedFile: String?

    private var tabID: UUID {
        viewModel.selectedTabId ?? AgentViewModel.mainTabID
    }

    /// Group backups by filename, newest first within each group.
    private var groupedBackups: [(name: String, versions: [(backup: String, date: Date)])] {
        var groups: [String: [(backup: String, date: Date)]] = [:]
        for b in backups {
            groups[b.original, default: []].append((b.backup, b.date))
        }
        // Sort groups by most recent backup date
        return groups.map { (name: $0.key, versions: $0.value.sorted { $0.date > $1.date }) }
            .sorted { ($0.versions.first?.date ?? .distantPast) > ($1.versions.first?.date ?? .distantPast) }
    }

    private var totalVersions: Int { backups.count }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text("File Backups")
                    .font(.headline)
                Spacer()
                Text(
                    "\(groupedBackups.count) file\(groupedBackups.count == 1 ? "" : "s"), \(totalVersions) version\(totalVersions == 1 ? "" : "s")"
                )
                .font(.caption)
                .foregroundStyle(.secondary)
            }

            if backups.isEmpty {
                Text("No backups yet. Files are backed up automatically before edits.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .padding(.vertical, 20)
            } else {
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 4) {
                        ForEach(groupedBackups, id: \.name) { group in
                            // File header row
                            Button {
                                withAnimation(.easeInOut(duration: 0.15)) {
                                    expandedFile = expandedFile == group.name ? nil : group.name
                                }
                            } label: {
                                HStack {
                                    Image(systemName: expandedFile == group.name ? "chevron.down" : "chevron.right")
                                        .font(.caption2)
                                        .foregroundStyle(.secondary)
                                        .frame(width: 12)
                                    Text(group.name)
                                        .font(.caption.monospaced().bold())
                                        .lineLimit(1)
                                    Text("(\(group.versions.count))")
                                        .font(.caption2)
                                        .foregroundStyle(.secondary)
                                    Spacer()
                                    Text(formatDate(group.versions.first?.date ?? Date()))
                                        .font(.caption2)
                                        .foregroundStyle(.secondary)
                                    Button("Restore") {
                                        if let latest = group.versions.first {
                                            restore(original: group.name, backupPath: latest.backup)
                                        }
                                    }
                                    .buttonStyle(.bordered)
                                    .controlSize(.mini)
                                }
                            }
                            .buttonStyle(.plain)
                            .padding(.vertical, 3)
                            .padding(.horizontal, 4)

                            // Expanded versions
                            if expandedFile == group.name {
                                ForEach(group.versions, id: \.backup) { version in
                                    HStack {
                                        Text(formatTimestamp(version.date))
                                            .font(.caption2.monospaced())
                                            .foregroundStyle(.secondary)
                                            .padding(.leading, 20)
                                        Spacer()
                                        Button("Restore") {
                                            restore(original: group.name, backupPath: version.backup)
                                        }
                                        .buttonStyle(.bordered)
                                        .controlSize(.mini)
                                    }
                                    .padding(.vertical, 1)
                                    .padding(.horizontal, 4)
                                }
                            }

                            Divider()
                        }
                    }
                }
                .frame(maxHeight: 500)
            }

            if let result = restoreResult {
                Text(result)
                    .font(.caption)
                    .foregroundStyle(result.hasPrefix("Error") ? .red : .green)
            }

            HStack {
                Button("Clear All") {
                    showClearConfirmation = true
                }
                .buttonStyle(.bordered)
                .controlSize(.small)

                Button("Open Folder") {
                    let url = FileManager.default.homeDirectoryForCurrentUser
                        .appendingPathComponent("Documents/AgentScript/backups")
                    NSWorkspace.shared.open(url)
                }
                .buttonStyle(.bordered)
                .controlSize(.small)

                Spacer()

                Button("Refresh") {
                    loadBackups()
                }
                .buttonStyle(.bordered)
                .controlSize(.small)
            }
        }
        .padding(16)
        .frame(width: 520, height: 600)
        .onAppear { loadBackups() }
        .alert("Clear All Backups?", isPresented: $showClearConfirmation) {
            Button("Cancel", role: .cancel) {}
            Button("Clear All", role: .destructive) {
                FileBackupService.shared.clearAllBackups()
                loadBackups()
                restoreResult = "All backups cleared"
            }
        } message: {
            Text("This will permanently delete all \(totalVersions) backup version(s). This cannot be undone.")
        }
    }

    private func loadBackups() {
        let tabBackups = FileBackupService.shared.listBackups(tabID: tabID)
        if !tabBackups.isEmpty {
            backups = tabBackups
        } else {
            backups = FileBackupService.shared.allBackups()
        }
        restoreResult = nil
    }

    private func restore(original: String, backupPath: String) {
        let projectFolder = viewModel.projectFolder
        let originalPath = projectFolder.isEmpty
            ? original
            : (projectFolder as NSString).appendingPathComponent(original)

        let candidates = [originalPath, original]
        for path in candidates {
            if FileBackupService.shared.restore(backupPath: backupPath, to: path) {
                restoreResult = "Restored \(original)"
                viewModel.appendLog("🔄 Restored \(original) from backup")
                viewModel.flushLog()
                return
            }
        }
        restoreResult = "Error: could not find original path for \(original)"
    }

    private func formatDate(_ date: Date) -> String {
        let formatter = RelativeDateTimeFormatter()
        formatter.unitsStyle = .abbreviated
        return formatter.localizedString(for: date, relativeTo: Date())
    }

    private func formatTimestamp(_ date: Date) -> String {
        let formatter = DateFormatter()
        formatter.dateFormat = "MMM d, h:mm:ss a"
        return formatter.string(from: date)
    }
}
