import Foundation
import SQLite3
import SwiftData
import AgentAudit

/// A single log entry in the chat history
@Model
final class ChatMessage {
    var timestamp: Date
    var content: String
    var isStreaming: Bool
    /// Monotonically increasing sequence number — guarantees insertion order when timestamps collide
    var ordinal: Int

    // Relationship to task - many messages belong to one task
    var task: ChatTask?

    init(timestamp: Date = Date(), content: String, task: ChatTask? = nil, isStreaming: Bool = false, ordinal: Int = 0) {
        self.timestamp = timestamp
        self.content = content
        self.task = task
        self.isStreaming = isStreaming
        self.ordinal = ordinal
    }
}

/// A task grouping - represents one "New Task" section
@Model
final class ChatTask {
    var id: UUID
    var startTime: Date
    var endTime: Date?
    var prompt: String
    var summary: String?
    var isCancelled: Bool

    @Relationship(deleteRule: .cascade)
    var messages: [ChatMessage] = []

    init(id: UUID = UUID(), startTime: Date = Date(), prompt: String = "") {
        self.id = id
        self.startTime = startTime
        self.prompt = prompt
        self.isCancelled = false
    }
}

/// Persisted script tab log data
@Model
final class ScriptTabRecord {
    var tabId: UUID
    var scriptName: String
    var activityLog: String
    var exitCode: Int // -999 = nil (SwiftData doesn't support optional Int32)
    var llmConfigJSON: String? // JSON-encoded LLMConfig for main tabs
    var parentTabIdString: String? // UUID string of parent main tab
    var isMessagesTab: Bool // Dedicated Messages tab flag
    var projectFolder: String // Per-tab project folder
    var promptHistoryJSON: String? // JSON-encoded [String] for prompt history
    var taskSummariesJSON: String? // JSON-encoded [String] for per-tab task summaries
    var errorsJSON: String? // JSON-encoded [String] for per-tab errors
    var rawLLMOutput: String = "" // Last LLM Output text
    var lastElapsed: Double = 0 // Last thinking elapsed seconds
    var thinkingExpanded: Bool = false
    var thinkingOutputExpanded: Bool = false
    var thinkingDismissed: Bool = true
    var tabInputTokens: Int = 0
    var tabOutputTokens: Int = 0

    init(
        tabId: UUID,
        scriptName: String,
        activityLog: String,
        exitCode: Int = -999,
        llmConfigJSON: String? = nil,
        parentTabIdString: String? = nil,
        isMessagesTab: Bool = false,
        projectFolder: String = "",
        promptHistoryJSON: String? = nil,
        taskSummariesJSON: String? = nil,
        errorsJSON: String? = nil,
        rawLLMOutput: String = "",
        lastElapsed: Double = 0,
        thinkingExpanded: Bool = false,
        thinkingOutputExpanded: Bool = false,
        thinkingDismissed: Bool = true,
        tabInputTokens: Int = 0,
        tabOutputTokens: Int = 0
    )
    {
        self.tabId = tabId
        self.scriptName = scriptName
        self.activityLog = activityLog
        self.exitCode = exitCode
        self.llmConfigJSON = llmConfigJSON
        self.parentTabIdString = parentTabIdString
        self.isMessagesTab = isMessagesTab
        self.projectFolder = projectFolder
        self.promptHistoryJSON = promptHistoryJSON
        self.taskSummariesJSON = taskSummariesJSON
        self.errorsJSON = errorsJSON
        self.rawLLMOutput = rawLLMOutput
        self.lastElapsed = lastElapsed
        self.thinkingExpanded = thinkingExpanded
        self.thinkingOutputExpanded = thinkingOutputExpanded
        self.thinkingDismissed = thinkingDismissed
        self.tabInputTokens = tabInputTokens
        self.tabOutputTokens = tabOutputTokens
    }
}

/// Manages chat history storage with SwiftData
@MainActor
final class ChatHistoryStore {
    static let shared = ChatHistoryStore()

    var container: ModelContainer?
    var context: ModelContext?

    private var currentTask: ChatTask?
    /// Monotonically increasing counter for message ordering within a task
    private var nextOrdinal: Int = 0
    /// Set to true when save has failed fatally — prevents repeated crash attempts
    private var storeDisabled = false

    /// Dedicated store file — avoids sharing default.store with TrainingDataStore
    private static var storeURL: URL {
        let urls = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)
        guard let url = urls.first else {
            return FileManager.default.temporaryDirectory.appendingPathComponent("chat2.store")
        }
        return url.appendingPathComponent("chat2.store")
    }

    private init() {
        let schema = Schema([ChatMessage.self, ChatTask.self, ScriptTabRecord.self])
        let url = Self.storeURL

        // Migrate: if old default.store has our tables, rename it to chat2.store
        Self.migrateDefaultStoreToChatStore()

        // Pre-validate: if store file exists but lacks required tables, delete it
        // before creating the ModelContainer. This prevents ObjC NSExceptions
        // (e.g. _PFFaultHandlerLookupRow) that Swift do/catch cannot intercept.
        if FileManager.default.fileExists(atPath: url.path) {
            if !Self.storeHasRequiredTables(at: url) {
                AuditLog.log(.storage, "SwiftData store missing required tables — deleting for recreation")
                deleteStoreFiles()
            }
        }

        do {
            let config = ModelConfiguration(schema: schema, url: url)
            container = try ModelContainer(for: schema, configurations: config)
            context = container?.mainContext
        } catch {
            AuditLog.log(.storage, "SwiftData init failed — recreating: \(error)")
            deleteStoreFiles()
            do {
                let config = ModelConfiguration(schema: schema, url: url)
                container = try ModelContainer(for: schema, configurations: config)
                context = container?.mainContext
            } catch {
                AuditLog.log(.storage, "Failed to initialize SwiftData after reset: \(error)")
            }
        }
    }

    /// One-time migration: rename default.store → chat2.store if it has our tables
    private static func migrateDefaultStoreToChatStore() {
        guard let appSupport = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first else { return }
        let oldURL = appSupport.appendingPathComponent("default.store")
        let newURL = storeURL
        let fm = FileManager.default
        // Only migrate if old exists and new doesn't
        guard fm.fileExists(atPath: oldURL.path), !fm.fileExists(atPath: newURL.path) else { return }
        guard storeHasRequiredTables(at: oldURL) else { return }
        for suffix in ["", "-shm", "-wal"] {
            let old = URL(fileURLWithPath: oldURL.path + suffix)
            let new = URL(fileURLWithPath: newURL.path + suffix)
            try? fm.moveItem(at: old, to: new)
        }
        AuditLog.log(.storage, "Migrated default.store → chat2.store")
    }

    /// Open the SQLite file read-only and verify all required tables exist.
    private static func storeHasRequiredTables(at url: URL) -> Bool {
        var db: OpaquePointer?
        guard sqlite3_open_v2(url.path, &db, SQLITE_OPEN_READONLY, nil) == SQLITE_OK else {
            sqlite3_close(db)
            return false
        }
        defer { sqlite3_close(db) }

        for table in ["ZCHATTASK", "ZCHATMESSAGE", "ZSCRIPTTABRECORD"] {
            var stmt: OpaquePointer?
            let query = "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='\(table)'"
            guard sqlite3_prepare_v2(db, query, -1, &stmt, nil) == SQLITE_OK else { return false }
            defer { sqlite3_finalize(stmt) }
            guard sqlite3_step(stmt) == SQLITE_ROW else { return false }
            if sqlite3_column_int(stmt, 0) == 0 { return false }
        }
        return true
    }

    private func deleteStoreFiles() {
        container = nil
        context = nil
        let fm = FileManager.default
        let url = Self.storeURL
        for suffix in ["", "-shm", "-wal"] {
            try? fm.removeItem(at: URL(fileURLWithPath: url.path + suffix))
        }
    }

    // MARK: - Task Management

    /// Start a new task grouping
    @discardableResult
    func startNewTask(prompt: String) -> UUID {
        let task = ChatTask(prompt: prompt)
        context?.insert(task)
        currentTask = task
        nextOrdinal = 0
        safeSave()
        return task.id
    }

    /// End current task with optional summary — crash-safe against stale SwiftData objects.
    /// Does NOT call context.save() — SwiftData auto-saves, and calling save() here
    /// triggers _PFFaultHandlerLookupRow crashes via inverse relationship maintenance
    /// on stale/deleted objects (an ObjC NSException that Swift can't catch).
    func endCurrentTask(summary: String? = nil, cancelled: Bool = false) {
        defer { currentTask = nil }
        guard let task = currentTask else { return }
        guard task.modelContext != nil, !task.isDeleted else { return }
        task.endTime = Date()
        task.summary = summary
        task.isCancelled = cancelled
        // Don't save — SwiftData auto-saves. Explicit save crashes on stale objects.
    }

    /// Get the current active task
    var activeTask: ChatTask? { currentTask }

    // MARK: - Message Operations

    /// Append a message to the current task
    func appendMessage(_ content: String, timestamp: Date = Date()) {
        guard let task = currentTask, task.modelContext != nil else { return }
        let message = ChatMessage(timestamp: timestamp, content: content, task: task, ordinal: nextOrdinal)
        nextOrdinal += 1
        context?.insert(message)
    }

    /// Append streaming content (LLM output)
    func appendStreamingContent(_ content: String) {
        guard let task = currentTask, task.modelContext != nil else { return }
        let message = ChatMessage(timestamp: Date(), content: content, task: task, isStreaming: true, ordinal: nextOrdinal)
        nextOrdinal += 1
        context?.insert(message)
    }

    /// Save pending changes — guarded against stale/deleted managed objects.
    /// Rolls back inserted objects whose relationships point to deleted/faulted
    /// targets, preventing _PFFaultHandlerLookupRow crashes during inverse
    /// relationship maintenance (an ObjC NSException that Swift can't catch and
    /// that CoreData's internal performBlockAndWait rethrows past ObjCTry).
    func save() {
        guard !storeDisabled, let context else { return }
        // Check if current task is still valid before saving
        if let task = currentTask, task.modelContext == nil || task.isDeleted {
            currentTask = nil
            context.rollback()
            return
        }
        guard context.hasChanges else { return }
        // Purge any inserted messages whose task relationship is stale/deleted
        // to prevent _PFFaultHandlerLookupRow during inverse maintenance on save
        for obj in context.insertedModelsArray {
            if let msg = obj as? ChatMessage,
               let task = msg.task,
               task.modelContext == nil || task.isDeleted
            {
                context.delete(msg)
            }
        }
        do {
            try context.save()
        } catch {
            AuditLog.log(.storage, "SwiftData save failed — disabling store: \(error)")
            storeDisabled = true
            context.rollback()
        }
    }

    /// Alias for save()
    private func safeSave() { save() }

    /// Fetch recent tasks with their messages
    func fetchRecentTasks(limit: Int = 3) -> [(task: ChatTask, messages: [ChatMessage])] {
        guard let context else { return [] }

        let descriptor = FetchDescriptor<ChatTask>(
            sortBy: [SortDescriptor(\.startTime, order: .reverse)]
        )

        do {
            let tasks = try context.fetch(descriptor)
            let recent = Array(tasks.prefix(limit))

            return recent.compactMap { task in
                let sorted = task.messages.sorted {
                    // Primary: ordinal (monotonic insertion order)
                    // Fallback: timestamp (for legacy data where ordinal is 0)
                    if $0.ordinal != $1.ordinal { return $0.ordinal < $1.ordinal }
                    return $0.timestamp < $1.timestamp
                }
                return (task: task, messages: sorted)
            }.reversed().map { $0 } // Reverse to get chronological order
        } catch {
            return []
        }
    }

    // MARK: - UI Display (full messages, never summarized)

    /// Build the activity log text for the UI. Always uses full messages — never summaries.
    func buildActivityLogText(maxTasks: Int = 3) -> String {
        let tasks = fetchRecentTasks(limit: maxTasks)
        var result = ""

        for (_, messages) in tasks {
            for msg in messages {
                if msg.isStreaming {
                    // Streaming fragments are partial tokens — concatenate without extra newlines
                    // (the final newline is stored as its own streaming message by flushStreamBuffer)
                    result += msg.content
                } else {
                    // Non-streaming messages (appendLog, appendRawOutput) are complete lines
                    result += msg.content
                    if !msg.content.hasSuffix("\n") {
                        result += "\n"
                    }
                }
            }
        }

        return result
    }

    // MARK: - LLM Context (uses summaries for older tasks)

    /// Build a concise context string for the LLM system prompt.
    /// Recent tasks get full messages; older tasks use their summary if available.
    func buildLLMContext(recentFullTasks: Int = 1, maxOlderSummaries: Int = 5) -> String {
        guard let context else { return "" }

        let descriptor = FetchDescriptor<ChatTask>(
            sortBy: [SortDescriptor(\.startTime, order: .reverse)]
        )

        guard let allTasks = try? context.fetch(descriptor), !allTasks.isEmpty else { return "" }

        var result = "\n\nChat history (most recent last):\n"

        let formatter = DateFormatter()
        formatter.dateFormat = "HH:mm:ss"

        // Older tasks: use summary only (skip those without a summary)
        let olderTasks = allTasks.dropFirst(recentFullTasks).prefix(maxOlderSummaries).reversed()
        for task in olderTasks {
            let time = formatter.string(from: task.startTime)
            if let summary = task.summary, !summary.isEmpty {
                result += "[\(time)] Task: \(task.prompt) → \(summary)\n"
            } else {
                result += "[\(time)] Task: \(task.prompt)\n"
            }
        }

        // Most recent task(s): include full messages so the LLM has detailed context
        let recentTasks = allTasks.prefix(recentFullTasks).reversed()
        for task in recentTasks {
            result += "--- Recent Task ---\n"
            result += "[\(formatter.string(from: task.startTime))] Task: \(task.prompt)\n"
            let sorted = task.messages.sorted {
                if $0.ordinal != $1.ordinal { return $0.ordinal < $1.ordinal }
                return $0.timestamp < $1.timestamp
            }
            for msg in sorted {
                if msg.isStreaming {
                    result += msg.content
                } else {
                    result += msg.content
                    if !msg.content.hasSuffix("\n") {
                        result += "\n"
                    }
                }
            }
            if let summary = task.summary {
                result += "Result: \(summary)\n"
            }
        }

        return result
    }

    // MARK: - Script Tab Persistence

    /// Save script tab data to SwiftData. Replaces any existing records.
    func saveScriptTabs(_ tabs: [
        (
            id: UUID,
            scriptName: String,
            activityLog: String,
            exitCode: Int32?,
            llmConfigJSON: String?,
            parentTabIdString: String?,
            isMessagesTab: Bool,
            projectFolder: String,
            promptHistoryJSON: String?,
            taskSummariesJSON: String?,
            errorsJSON: String?,
            rawLLMOutput: String,
            lastElapsed: Double,
            thinkingExpanded: Bool,
            thinkingOutputExpanded: Bool,
            thinkingDismissed: Bool,
            tabInputTokens: Int,
            tabOutputTokens: Int
        )
    ]) {
        guard !storeDisabled, let context else { return }
        // Delete old records
        try? context.delete(model: ScriptTabRecord.self)
        // Insert new
        for tab in tabs {
            let record = ScriptTabRecord(
                tabId: tab.id,
                scriptName: tab.scriptName,
                activityLog: tab.activityLog,
                exitCode: tab.exitCode.map { Int($0) } ?? -999,
                llmConfigJSON: tab.llmConfigJSON,
                parentTabIdString: tab.parentTabIdString,
                isMessagesTab: tab.isMessagesTab,
                projectFolder: tab.projectFolder,
                promptHistoryJSON: tab.promptHistoryJSON,
                taskSummariesJSON: tab.taskSummariesJSON,
                errorsJSON: tab.errorsJSON,
                rawLLMOutput: tab.rawLLMOutput,
                lastElapsed: tab.lastElapsed,
                thinkingExpanded: tab.thinkingExpanded,
                thinkingOutputExpanded: tab.thinkingOutputExpanded,
                thinkingDismissed: tab.thinkingDismissed,
                tabInputTokens: tab.tabInputTokens,
                tabOutputTokens: tab.tabOutputTokens
            )
            context.insert(record)
        }
        try? context.save()
    }

    /// Restore script tab data from SwiftData keyed by tab UUID.
    func fetchScriptTabs() -> [ScriptTabRecord] {
        guard let context else { return [] }
        do {
            return try context.fetch(FetchDescriptor<ScriptTabRecord>())
        } catch {
            return []
        }
    }

    /// Clear persisted script tab records.
    func clearScriptTabs() {
        guard !storeDisabled, let context else { return }
        try? context.delete(model: ScriptTabRecord.self)
        try? context.save()
    }

    /// Clear all history
    func clearAll() {
        currentTask = nil
        storeDisabled = false
        // Nuke the store files and recreate — batch deletes trigger CoreData
        // ObjC exceptions on inverse relationships that Swift can't catch
        deleteStoreFiles()
        let schema = Schema([ChatMessage.self, ChatTask.self, ScriptTabRecord.self])
        do {
            let config = ModelConfiguration(schema: schema, url: Self.storeURL)
            container = try ModelContainer(for: schema, configurations: config)
            context = container?.mainContext
        } catch {
            AuditLog.log(.storage, "Failed to recreate store after clear: \(error)")
        }
    }

    /// Count total tasks
    func taskCount() -> Int {
        guard let context else { return 0 }
        do {
            return try context.fetchCount(FetchDescriptor<ChatTask>())
        } catch {
            return 0
        }
    }

    /// Count total messages
    func messageCount() -> Int {
        guard let context else { return 0 }
        do {
            return try context.fetchCount(FetchDescriptor<ChatMessage>())
        } catch {
            return 0
        }
    }

    /// Migrate old UserDefaults data to SwiftData (one-time)
    func migrateFromUserDefaults() {
        let key = "agentActivityLog"
        guard let saved = UserDefaults.standard.string(forKey: key),
              !saved.isEmpty else { return }

        // Check if we've already migrated
        if UserDefaults.standard.bool(forKey: "agentActivityLogMigrated") {
            return
        }

        // Don't migrate if we already have tasks in SwiftData
        if taskCount() > 0 {
            UserDefaults.standard.set(true, forKey: "agentActivityLogMigrated")
            return
        }

        let marker = AgentViewModel.newTaskMarker
        let sections = saved.components(separatedBy: marker)

        let timestampPattern = #"^\[(\d{2}:\d{2}:\d{2})\]\s*(.*)$"#
        guard let regex = try? NSRegularExpression(pattern: timestampPattern) else { return }

        for section in sections where !section.isEmpty {
            let lines = section.components(separatedBy: "\n")
            var taskPrompt = "Migrated task"

            // First non-timestamp line might be the task description
            for line in lines.prefix(3) {
                let trimmed = line.trimmingCharacters(in: .whitespaces)
                if !trimmed.isEmpty, !trimmed.hasPrefix("[") {
                    taskPrompt = trimmed
                    break
                }
            }

            let task = ChatTask(prompt: taskPrompt)
            context?.insert(task)

            for line in lines {
                let nsLine = line as NSString
                let range = NSRange(location: 0, length: nsLine.length)

                if let match = regex.firstMatch(in: line, range: range) {
                    let timeStr = nsLine.substring(with: match.range(at: 1))
                    let content = nsLine.substring(with: match.range(at: 2))

                    // Parse time and create date
                    let parts = timeStr.components(separatedBy: ":")
                    var date = Date()
                    if parts.count == 3,
                       let hour = Int(parts[0]),
                       let minute = Int(parts[1]),
                       let second = Int(parts[2])
                    {
                        let cal = Calendar.current
                        date = cal.date(bySettingHour: hour, minute: minute, second: second, of: Date()) ?? Date()
                    }

                    let message = ChatMessage(timestamp: date, content: content, task: task)
                    context?.insert(message)
                }
            }
        }

        if !storeDisabled { try? context?.save() }
        UserDefaults.standard.set(true, forKey: "agentActivityLogMigrated")
        AuditLog.log(.storage, "Migrated chat history to SwiftData")
    }
}
