import Foundation

/// Persistent task queue for overnight runs. Survives crashes.
/// Stored at ~/Documents/AgentScript/taskqueue.json
@MainActor
final class TaskQueueStore {
    static let shared = TaskQueueStore()

    struct QueuedTask: Codable, Identifiable {
        let id: UUID
        var title: String
        var status: Status
        var error: String?
        var startedAt: Date?
        var completedAt: Date?

        enum Status: String, Codable {
            case pending, running, done, failed, skipped
        }

        init(title: String) {
            self.id = UUID()
            self.title = title
            self.status = .pending
        }
    }

    private(set) var tasks: [QueuedTask] = []
    private let fileURL: URL

    private init() {
        let home = FileManager.default.homeDirectoryForCurrentUser
        let dir = home.appendingPathComponent("Documents/AgentScript")
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        fileURL = dir.appendingPathComponent("taskqueue.json")
        load()
    }

    // MARK: - Queue Management

    /// Set a new task list (from plan mode or user input). Replaces existing queue.
    func setTasks(_ titles: [String]) {
        tasks = titles.map { QueuedTask(title: $0) }
        save()
    }

    /// Mark a task as running.
    func start(_ id: UUID) {
        if let idx = tasks.firstIndex(where: { $0.id == id }) {
            tasks[idx].status = .running
            tasks[idx].startedAt = Date()
            save()
        }
    }

    /// Mark a task as done.
    func complete(_ id: UUID) {
        if let idx = tasks.firstIndex(where: { $0.id == id }) {
            tasks[idx].status = .done
            tasks[idx].completedAt = Date()
            save()
        }
    }

    /// Mark a task as failed.
    func fail(_ id: UUID, error: String = "") {
        if let idx = tasks.firstIndex(where: { $0.id == id }) {
            tasks[idx].status = .failed
            tasks[idx].error = error
            tasks[idx].completedAt = Date()
            save()
        }
    }

    /// Mark a task as skipped.
    func skip(_ id: UUID) {
        if let idx = tasks.firstIndex(where: { $0.id == id }) {
            tasks[idx].status = .skipped
            tasks[idx].completedAt = Date()
            save()
        }
    }

    /// Get the next pending task.
    var nextPending: QueuedTask? {
        tasks.first { $0.status == .pending }
    }

    /// Progress summary: "3/7 done, 1 failed, 3 pending"
    var progressSummary: String {
        let done = tasks.filter { $0.status == .done }.count
        let failed = tasks.filter { $0.status == .failed }.count
        let skipped = tasks.filter { $0.status == .skipped }.count
        let pending = tasks.filter { $0.status == .pending }.count
        let running = tasks.filter { $0.status == .running }.count
        var parts: [String] = []
        if done > 0 { parts.append("\(done) done") }
        if running > 0 { parts.append("\(running) running") }
        if failed > 0 { parts.append("\(failed) failed") }
        if skipped > 0 { parts.append("\(skipped) skipped") }
        if pending > 0 { parts.append("\(pending) pending") }
        return parts.isEmpty ? "No tasks" : parts.joined(separator: ", ")
    }

    /// Clear all tasks.
    func clear() {
        tasks.removeAll()
        save()
    }

    // MARK: - Persistence

    private func load() {
        guard let data = try? Data(contentsOf: fileURL),
              let decoded = try? JSONDecoder().decode([QueuedTask].self, from: data) else { return }
        tasks = decoded
    }

    private func save() {
        guard let data = try? JSONEncoder().encode(tasks) else { return }
        try? data.write(to: fileURL, options: .atomic)
    }
}
