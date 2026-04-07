import Foundation
import SQLite3
import SwiftData
import AgentAudit

/// Training data captured from Apple Intelligence for LoRA adapter fine-tuning.
/// Stores the full conversation context including Apple AI decisions, user prompts, and LLM responses.
@Model
final class TrainingRecord {
    var id: UUID
    var timestamp: Date
    
    // User input
    var userPrompt: String
    
    // Apple AI mediation (contextual annotations)
    var appleAIDecision: String?        // Apple AI's clarification/context injection
    var appleAIAnnotation: String?       // Apple AI's summary/explanation for user
    
    // LLM response (full text, not just summary)
    var llmResponse: String?
    var llmResponseTruncated: Bool      // True if response exceeded max length
    
    // Task outcome
    var taskSummary: String?
    var commandsExecuted: [String]       // Tools/commands run during task
    var taskSuccessful: Bool?
    
    // Context for next turn (conversation continuity)
    var conversationContext: String?    // Running summary passed to next session
    
    // Metadata
    var modelUsed: String?              // Which LLM provider was used
    var toolsUsed: [String]              // List of tools invoked
    var durationSeconds: Double?        // How long the task took
    
    init(
        userPrompt: String,
        appleAIDecision: String? = nil,
        appleAIAnnotation: String? = nil,
        llmResponse: String? = nil,
        taskSummary: String? = nil,
        commandsExecuted: [String] = [],
        modelUsed: String? = nil
    ) {
        self.id = UUID()
        self.timestamp = Date()
        self.userPrompt = userPrompt
        self.appleAIDecision = appleAIDecision
        self.appleAIAnnotation = appleAIAnnotation
        self.llmResponse = llmResponse
        self.llmResponseTruncated = false
        self.taskSummary = taskSummary
        self.commandsExecuted = commandsExecuted
        self.taskSuccessful = nil
        self.conversationContext = nil
        self.modelUsed = modelUsed
        self.toolsUsed = []
        self.durationSeconds = nil
    }
}

/// Manages training data storage and export for Apple Intelligence LoRA fine-tuning.
/// Automatically captures Apple AI decisions, user prompts, and LLM responses.
@MainActor
final class TrainingDataStore {
    static let shared = TrainingDataStore()
    
    var container: ModelContainer?
    var context: ModelContext?

    private var currentRecord: TrainingRecord?
    private var startTime: Date?
    /// Set to true when save has failed fatally — prevents repeated crash attempts
    private var storeDisabled = false

    /// Maximum characters to store for LLM response (prevent massive records)
    private static let maxResponseLength = 8000

    /// Dedicated store file so training data doesn't share default.store with ChatHistoryStore
    private static var storeURL: URL {
        (FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first ?? FileManager.default.temporaryDirectory)
            .appendingPathComponent("training.store")
    }

    private init() {
        let schema = Schema([TrainingRecord.self])

        // Pre-validate: if store file exists but lacks required tables, delete it
        let url = Self.storeURL
        if FileManager.default.fileExists(atPath: url.path) {
            if !Self.storeHasRequiredTables(at: url) {
                AuditLog.log(.storage, "TrainingDataStore: store missing required tables — deleting for recreation")
                Self.deleteStoreFiles(at: url)
            }
        }

        do {
            let config = ModelConfiguration(schema: schema, url: url)
            container = try ModelContainer(for: schema, configurations: config)
            context = container?.mainContext
        } catch {
            AuditLog.log(.storage, "TrainingDataStore: init failed — recreating: \(error)")
            Self.deleteStoreFiles(at: url)
            do {
                let config = ModelConfiguration(schema: schema, url: url)
                container = try ModelContainer(for: schema, configurations: config)
                context = container?.mainContext
            } catch {
                AuditLog.log(.storage, "TrainingDataStore: Failed to initialize after reset: \(error)")
            }
        }
    }

    /// Open the SQLite file read-only and verify required tables exist.
    private static func storeHasRequiredTables(at url: URL) -> Bool {
        var db: OpaquePointer?
        guard sqlite3_open_v2(url.path, &db, SQLITE_OPEN_READONLY, nil) == SQLITE_OK else {
            sqlite3_close(db)
            return false
        }
        defer { sqlite3_close(db) }

        var stmt: OpaquePointer?
        let query = "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='ZTRAININGRECORD'"
        guard sqlite3_prepare_v2(db, query, -1, &stmt, nil) == SQLITE_OK else { return false }
        defer { sqlite3_finalize(stmt) }
        guard sqlite3_step(stmt) == SQLITE_ROW else { return false }
        return sqlite3_column_int(stmt, 0) > 0
    }

    private static func deleteStoreFiles(at url: URL) {
        let fm = FileManager.default
        for suffix in ["", "-shm", "-wal"] {
            try? fm.removeItem(at: URL(fileURLWithPath: url.path + suffix))
        }
    }
    
    // MARK: - Capture Methods (called during task execution)
    
    /// Start capturing a new task. Call when user submits a prompt.
    func startCapture(userPrompt: String, modelUsed: String? = nil) {
        currentRecord = TrainingRecord(userPrompt: userPrompt, modelUsed: modelUsed)
        startTime = Date()
    }
    
    /// Capture Apple AI's decision/context injection for LLM.
    func captureAppleAIDecision(_ decision: String) {
        currentRecord?.appleAIDecision = String(decision.prefix(1000))
    }
    
    /// Capture Apple AI's annotation shown to user.
    func captureAppleAIAnnotation(_ annotation: String) {
        currentRecord?.appleAIAnnotation = String(annotation.prefix(1000))
    }
    
    /// Capture LLM response text (truncated if too long).
    func captureLLMResponse(_ response: String) {
        let truncated = response.count > Self.maxResponseLength
        currentRecord?.llmResponse = String(response.prefix(Self.maxResponseLength))
        currentRecord?.llmResponseTruncated = truncated
    }
    
    /// Capture a tool/command that was executed.
    func captureToolCall(_ toolName: String) {
        currentRecord?.toolsUsed.append(toolName)
    }
    
    /// Capture a shell command that was run.
    func captureCommand(_ command: String) {
        currentRecord?.commandsExecuted.append(String(command.prefix(200)))
    }
    
    /// Capture conversation context for continuity.
    func captureConversationContext(_ context: String) {
        currentRecord?.conversationContext = String(context.prefix(500))
    }
    
    /// Finish capture and persist the training record.
    func finishCapture(taskSummary: String?, successful: Bool?) {
        guard let record = currentRecord else { return }
        
        record.taskSummary = taskSummary
        record.taskSuccessful = successful
        if let start = startTime {
            record.durationSeconds = Date().timeIntervalSince(start)
        }
        
        context?.insert(record)
        save()
        
        currentRecord = nil
        startTime = nil
    }
    
    /// Cancel the current capture (task failed or was cancelled).
    func cancelCapture() {
        currentRecord = nil
        startTime = nil
    }
    
    // MARK: - Persistence
    
    private func save() {
        guard !storeDisabled, let context else { return }
        do {
            try context.save()
        } catch {
            AuditLog.log(.storage, "TrainingDataStore: Save failed — disabling store: \(error)")
            storeDisabled = true
            context.rollback()
        }
    }
    
    // MARK: - Query
    
    /// Fetch recent training records.
    func fetchRecentRecords(limit: Int = 20) -> [TrainingRecord] {
        guard let context else { return [] }
        let descriptor = FetchDescriptor<TrainingRecord>(
            sortBy: [SortDescriptor(\.timestamp, order: .reverse)]
        )
        do {
            let all = try context.fetch(descriptor)
            return Array(all.prefix(limit))
        } catch {
            return []
        }
    }
    
    /// Count total training records.
    func recordCount() -> Int {
        guard let context else { return 0 }
        do {
            return try context.fetchCount(FetchDescriptor<TrainingRecord>())
        } catch {
            return 0
        }
    }
    
    /// Get all records for export.
    func allRecords() -> [TrainingRecord] {
        guard let context else { return [] }
        let descriptor = FetchDescriptor<TrainingRecord>(
            sortBy: [SortDescriptor(\.timestamp, order: .forward)]
        )
        do {
            return try context.fetch(descriptor)
        } catch {
            return []
        }
    }
    
    // MARK: - JSONL Export for LoRA Training
    
    /// Export training records as JSONL for Apple Intelligence fine-tuning.
    /// Format matches Apple's training data requirements.
    func exportAsJSONL() -> URL? {
        let records = allRecords()
        guard !records.isEmpty else { return nil }
        
        var lines: [String] = []
        
        for record in records {
            // Build the training example
            // Format: {"messages": [{"role": "user", "content": "..."}, {"role": "assistant", "content": "..."}]}
            
            var userContent = record.userPrompt
            
            // Include Apple AI decision as context if available
            if let decision = record.appleAIDecision, !decision.isEmpty {
                userContent = "[Context: \(decision)]\n\n\(userContent)"
            }
            
            // Build assistant response
            var assistantContent = ""
            
            // If we have Apple AI annotation, include it as reasoning
            if let annotation = record.appleAIAnnotation, !annotation.isEmpty {
                assistantContent += "[AI → User] \(annotation)\n\n"
            }
            
            // Include the LLM response
            if let response = record.llmResponse, !response.isEmpty {
                assistantContent += response
            } else if let summary = record.taskSummary, !summary.isEmpty {
                assistantContent += summary
            } else {
                assistantContent += "Task completed."
            }
            
            // Include tools used as metadata
            if !record.toolsUsed.isEmpty {
                assistantContent += "\n\n[Tools: \(record.toolsUsed.joined(separator: ", "))]"
            }
            
            let entry: [String: Any] = [
                "messages": [
                    ["role": "user", "content": userContent],
                    ["role": "assistant", "content": assistantContent]
                ]
            ]
            
            if let data = try? JSONSerialization.data(withJSONObject: entry),
               let line = String(data: data, encoding: .utf8) {
                lines.append(line)
            }
        }
        
        guard !lines.isEmpty else { return nil }
        
        let content = lines.joined(separator: "\n")
        let formatter = DateFormatter()
        formatter.dateFormat = "yyyy-MM-dd_HHmmss"
        let filename = "apple_ai_training_\(formatter.string(from: Date())).jsonl"
        let fileURL = LoRAAdapterManager.jsonlDir.appendingPathComponent(filename)
        
        do {
            try content.write(to: fileURL, atomically: true, encoding: .utf8)
            return fileURL
        } catch {
            AuditLog.log(.storage, "TrainingDataStore: Export failed: \(error)")
            return nil
        }
    }
    
    /// Export in Apple's preferred training format (prompt-completion pairs).
    func exportAsPromptCompletion() -> URL? {
        let records = allRecords()
        guard !records.isEmpty else { return nil }
        
        var lines: [String] = []
        
        for record in records {
            var prompt = record.userPrompt
            if let decision = record.appleAIDecision, !decision.isEmpty {
                prompt = "[Context: \(decision)]\n\n\(prompt)"
            }
            
            var completion = ""
            if let annotation = record.appleAIAnnotation, !annotation.isEmpty {
                completion += "[AI → User] \(annotation)\n\n"
            }
            if let response = record.llmResponse, !response.isEmpty {
                completion += response
            } else if let summary = record.taskSummary, !summary.isEmpty {
                completion += summary
            }
            
            let entry: [String: Any] = [
                "prompt": prompt,
                "completion": completion
            ]
            
            if let data = try? JSONSerialization.data(withJSONObject: entry),
               let line = String(data: data, encoding: .utf8) {
                lines.append(line)
            }
        }
        
        guard !lines.isEmpty else { return nil }
        
        let content = lines.joined(separator: "\n")
        let formatter = DateFormatter()
        formatter.dateFormat = "yyyy-MM-dd_HHmmss"
        let filename = "apple_ai_prompts_\(formatter.string(from: Date())).jsonl"
        let fileURL = LoRAAdapterManager.jsonlDir.appendingPathComponent(filename)
        
        do {
            try content.write(to: fileURL, atomically: true, encoding: .utf8)
            return fileURL
        } catch {
            return nil
        }
    }
    
    /// Clear all training records.
    func clearAll() {
        guard !storeDisabled, let context else { return }
        try? context.delete(model: TrainingRecord.self)
        try? context.save()
    }
}