import Foundation
import AppKit

/// A chunk of long text pasted into the task input. Rendered as a removable
/// chip above the TextField; prepended to the prompt at task-run time.
struct PastedText: Identifiable, Equatable {
    let id: UUID
    let text: String
    init(_ text: String) {
        self.id = UUID()
        self.text = text
    }
}

/// Thread-safe boolean flag for cross-thread cancellation checks.
final class AtomicFlag: @unchecked Sendable {
    private let lock = NSLock()
    private var _value = false
    var value: Bool { lock.lock(); defer { lock.unlock() }; return _value }
    func set() { lock.lock(); _value = true; lock.unlock() }
}

@MainActor @Observable
final class ScriptTab: Identifiable {
    let id: UUID
    let scriptName: String
    var activityLog: String = ""
    var isRunning: Bool = true {
        didSet {
            // Clear stale LLM output when a script-only tab starts a new run
            if isRunning && !isLLMRunning && !isLLMThinking && !isMainTab {
                rawLLMOutput = ""
                llmMessages = []
                tabInputTokens = 0
                tabOutputTokens = 0
                thinkingDismissed = true
            }
        }
    }
    var isCancelled: Bool = false {
        didSet { if isCancelled { _cancelFlag.set() } }
    }
    var exitCode: Int32?
    var cancelHandler: (() -> Void)?

    /// Thread-safe flag readable from any thread (for Sendable closures)
    nonisolated let _cancelFlag = AtomicFlag()

    // MARK: - Multi-Main-Tab LLM Config

    /// Non-nil when this is a "Main" tab with its own LLM provider/model
    var llmConfig: LLMConfig?
    /// Which main tab spawned this script tab (for LLM inheritance)
    var parentTabId: UUID?
    /// Whether this tab acts as an independent main tab
    var isMainTab: Bool { llmConfig != nil }
    /// Whether this is the dedicated Messages tab (for iMessage Agent! commands)
    var isMessagesTab: Bool = false
    /// The iMessage handle to reply to when a Messages tab task completes
    var replyHandle: String?
    /// Display name: scriptName (numbered for duplicate LLM tabs; "Messages" for iMessage tab)
    var displayTitle: String { isMessagesTab ? "Messages" : scriptName }

    // Log buffering (mirrors AgentViewModel pattern)
    var logBuffer = ""
    var logFlushTask: Task<Void, Never>?
    var streamLineCount = 0

    // MARK: - LLM Conversation State

    var taskInput: String = ""
    var isLLMRunning: Bool = false
    var isLLMThinking: Bool = false
    var thinkingDismissed: Bool = true
    var thinkingExpanded: Bool = false
    var thinkingOutputExpanded: Bool = false
    /// Expanded state for tool steps disclosure (persists across toggles)
    var toolStepsExpanded: Bool = false
    /// User's drag-resized height for the LLM Output HUD on this tab. Persisted across tab switches.
    var llmOutputHeight: Double = 80

    /// Unified busy check — true when the tab is doing anything (running, LLM, thinking)
    var isBusy: Bool { isRunning || isLLMRunning || isLLMThinking }
    var runningLLMTask: Task<Void, Never>?
    var llmMessages: [[String: Any]] = []
    var taskQueue: [String] = []
    var currentTaskPrompt: String = ""
    var currentAppleAIPrompt: String = ""

    // MARK: - Per-Tab Project Folder

    /// Each tab can have its own project folder
    var projectFolder: String = ""

    // MARK: - Per-Tab Prompt History

    var promptHistory: [String] = []
    var historyIndex: Int = -1
    var savedInput: String = ""

    // MARK: - Per-Tab Task & Error History

    var tabTaskSummaries: [String] = []
    var tabErrors: [String] = []

    // MARK: - Tool Steps (structured tool call tracking)

    var toolSteps: [AgentViewModel.ToolStep] = []

    @discardableResult
    func recordToolStep(name: String, detail: String) -> UUID {
        let step = AgentViewModel.ToolStep(name: name, detail: detail, startTime: Date())
        toolSteps.append(step)
        return step.id
    }

    func completeToolStep(id: UUID, status: AgentViewModel.ToolStep.Status = .success) {
        if let idx = toolSteps.firstIndex(where: { $0.id == id }) {
            toolSteps[idx].duration = Date().timeIntervalSince(toolSteps[idx].startTime)
            toolSteps[idx].status = status
        }
    }

    // MARK: - Per-Tab Attached Images

    var attachedImages: [NSImage] = []
    var attachedImagesBase64: [String] = []

    // MARK: - Per-Tab Pasted Text Attachments

    /// Long text pastes are captured as attachments (chips) instead of being
    /// inserted inline into the TextField — SwiftUI's TextField beach-balls
    /// on very long content. Prepended to the prompt when the task runs.
    var pastedTexts: [PastedText] = []

    // LLM streaming state
    var llmStreamBuffer: String = ""
    var rawLLMOutput: String = ""
    /// Character-by-character dripped version of rawLLMOutput for terminal effect
    var displayedLLMOutput: String = ""
    var dripDisplayIndex: Int = 0
    var dripTask: Task<Void, Never>?
    var lastElapsed: Double = 0
    var taskStartDate: Date? // Set when task starts, nil when idle
    var taskElapsed: Double { // Computes live elapsed — works even when tab is in background
        get {
            if let start = taskStartDate, isRunning || isLLMRunning {
                return Date().timeIntervalSince(start)
            }
            return _taskElapsedFrozen
        }
        set { _taskElapsedFrozen = newValue }
    }
    var _taskElapsedFrozen: Double = 0 // Stored value when task stops
    var tabInputTokens: Int = 0
    var tabOutputTokens: Int = 0
    var llmStreamFlushTask: Task<Void, Never>?
    var llmStreamingStarted: Bool = false

    init(scriptName: String, id: UUID = UUID()) {
        self.id = id
        self.scriptName = scriptName
    }

    /// Create a new main tab with its own LLM configuration
    init(llmConfig: LLMConfig, id: UUID = UUID()) {
        self.id = id
        self.scriptName = llmConfig.displayName
        self.llmConfig = llmConfig
        self.isRunning = false
    }

    /// Restore a tab from persisted SwiftData record
    init(record: ScriptTabRecord) {
        self.id = record.tabId
        self.scriptName = record.scriptName
        // Truncation handled by ActivityLogView at render time
        self.activityLog = record.activityLog
        self.exitCode = record.exitCode == -999 ? nil : Int32(record.exitCode)
        self.isRunning = false
        self.isMessagesTab = record.isMessagesTab
        self.projectFolder = record.projectFolder
        // Restore LLM config if present
        if let json = record.llmConfigJSON, let data = json.data(using: .utf8) {
            self.llmConfig = try? JSONDecoder().decode(LLMConfig.self, from: data)
        }
        if let parentStr = record.parentTabIdString {
            self.parentTabId = UUID(uuidString: parentStr)
        }
        if let json = record.promptHistoryJSON, let data = json.data(using: .utf8),
           let history = try? JSONDecoder().decode([String].self, from: data)
        {
            self.promptHistory = history
        }
        if let json = record.taskSummariesJSON, let data = json.data(using: .utf8),
           let summaries = try? JSONDecoder().decode([String].self, from: data)
        {
            self.tabTaskSummaries = summaries
        }
        if let json = record.errorsJSON, let data = json.data(using: .utf8),
           let errors = try? JSONDecoder().decode([String].self, from: data)
        {
            self.tabErrors = errors
        }
        self.rawLLMOutput = record.rawLLMOutput
        self.displayedLLMOutput = record.rawLLMOutput // Show full text on restore (no drip)
        self.dripDisplayIndex = record.rawLLMOutput.count
        self.lastElapsed = record.lastElapsed
        self.thinkingExpanded = record.thinkingExpanded
        self.thinkingOutputExpanded = record.thinkingOutputExpanded
        // If there's LLM output, show the indicator (don't dismiss)
        self.thinkingDismissed = record.rawLLMOutput.isEmpty ? true : record.thinkingDismissed
        self.tabInputTokens = record.tabInputTokens
        self.tabOutputTokens = record.tabOutputTokens
        // Trim main/script tab logs on relaunch (skip Messages/automation tabs)
        if !isMessagesTab { activityLog = Self.trimLog(activityLog) }
    }

    // MARK: - Logging

    func appendOutput(_ text: String) {
        guard !text.isEmpty else { return }
        let newlines = text.reduce(0) { $0 + ($1 == "\n" ? 1 : 0) }
        streamLineCount += max(newlines, 1)
        logBuffer += text
        if !text.hasSuffix("\n") { logBuffer += "\n" }
        scheduleFlush()
    }

    func appendLog(_ message: String) {
        let timestamp = AgentViewModel.timestampFormatter.string(from: Date())
        AgentViewModel.prepareLogBuffer(message: message, buffer: &logBuffer, existingLog: activityLog)
        // Multi-line messages (diffs, edit payloads) drop onto their own line
        // so the first content line isn't jammed next to the timestamp.
        if message.contains("\n") {
            logBuffer += "[\(timestamp)]\n\(message)\n"
        } else {
            logBuffer += "[\(timestamp)] \(message)\n"
        }
        scheduleFlush()
    }

    private func scheduleFlush() {
        guard logFlushTask == nil else { return }
        logFlushTask = Task {
            try? await Task.sleep(for: .milliseconds(50))
            flush()
        }
    }

    /// Hard cap for activityLog — applied at every mutation site.
    /// 50K is small enough for ActivityLogView to render without beach-balling.
    nonisolated static let logCap = 50_000

    /// Banner inserted when the log is trimmed; ActivityLogView styles it with a yellow background.
    nonisolated static let trimBanner = "··· earlier output trimmed ···\n\n"

    /// THE ONE log trim function: hard 50K cap, snaps to next newline, prepends banner. Idempotent.
    nonisolated static func trimLog(_ log: String) -> String {
        guard log.count > logCap else { return log }
        let target = max(0, logCap - trimBanner.count)
        var trimmed = String(log.dropFirst(log.count - target))
        if let nl = trimmed.firstIndex(of: "\n") {
            trimmed = String(trimmed[trimmed.index(after: nl)...])
        }
        return trimBanner + trimmed
    }

    func flush() {
        logFlushTask?.cancel()
        logFlushTask = nil
        if !logBuffer.isEmpty {
            activityLog = Self.trimLog(activityLog + logBuffer)
            logBuffer = ""
            NotificationCenter.default.post(name: .activityLogDidChange, object: id)
        }
    }

    // MARK: - LLM Streaming

    func appendStreamDelta(_ delta: String) {
        if !llmStreamingStarted {
            llmStreamingStarted = true
            llmStreamBuffer = ""
            rawLLMOutput = ""
            displayedLLMOutput = ""
            dripDisplayIndex = 0
        }
        rawLLMOutput += delta
        startDripIfNeeded()
    }

    /// Drip characters from rawLLMOutput into displayedLLMOutput for a terminal typing effect
    func startDripIfNeeded() {
        guard dripTask == nil else { return }
        dripTask = Task { [weak self] in
            guard let self else { return }
            while !Task.isCancelled {
                if self.dripDisplayIndex < self.rawLLMOutput.count {
                    let idx = self.rawLLMOutput.index(self.rawLLMOutput.startIndex, offsetBy: self.dripDisplayIndex)
                    self.displayedLLMOutput.append(self.rawLLMOutput[idx])
                    self.dripDisplayIndex += 1
                    let speed = UserDefaults.standard.integer(forKey: "terminalSpeed")
                    try? await Task.sleep(for: .milliseconds(speed > 0 ? speed : 22))
                } else if self.llmStreamingStarted {
                    let speed = UserDefaults.standard.integer(forKey: "terminalSpeed")
                    try? await Task.sleep(for: .milliseconds(max(5, (speed > 0 ? speed : 22) / 2)))
                } else {
                    break
                }
            }
            self.dripTask = nil
        }
    }

    func flushStreamBuffer() {
        llmStreamFlushTask?.cancel()
        llmStreamFlushTask = nil
        // Stream text goes to LLM output only — not the activity log
        llmStreamBuffer = ""
        llmStreamingStarted = false
        // Let drip task finish naturally — no instant dump
    }

    private func scheduleLLMStreamFlush() {
        flushStreamBuffer()
    }

    func resetLLMStreamCounters() {
        streamLineCount = 0
    }

    // MARK: - Prompt History Navigation

    func addToHistory(_ prompt: String) {
        promptHistory.append(prompt)
        historyIndex = -1
        savedInput = ""
    }

    func navigateHistory(direction: Int) {
        guard !promptHistory.isEmpty else { return }

        if historyIndex == -1 {
            savedInput = taskInput
            if direction == -1 {
                historyIndex = promptHistory.count - 1
            } else {
                return
            }
        } else {
            historyIndex += direction
        }

        if historyIndex < 0 {
            historyIndex = -1
            taskInput = savedInput
            return
        }

        if historyIndex >= promptHistory.count {
            historyIndex = -1
            taskInput = savedInput
            return
        }

        taskInput = promptHistory[historyIndex]
    }
}
