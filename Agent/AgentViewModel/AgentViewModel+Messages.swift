import Foundation
import AppKit
import SQLite3

extension AgentViewModel {
    // MARK: - Messages Monitor

    /// Check if we have Full Disk Access by testing a read on chat.db.
    nonisolated static func checkFullDiskAccess() -> Bool {
        var db: OpaquePointer?
        let rc = sqlite3_open_v2(messagesDBPath, &db, SQLITE_OPEN_READONLY | SQLITE_OPEN_NOMUTEX, nil)
        defer { sqlite3_close(db) }
        guard rc == SQLITE_OK else { return false }
        // Try a simple query to confirm we can actually read
        var stmt: OpaquePointer?
        let sql = "SELECT ROWID FROM message ORDER BY ROWID DESC LIMIT 1"
        guard sqlite3_prepare_v2(db, sql, -1, &stmt, nil) == SQLITE_OK else { return false }
        defer { sqlite3_finalize(stmt) }
        return sqlite3_step(stmt) == SQLITE_ROW
    }

    func startMessagesMonitor() {
        stopMessagesMonitor()

        // Gate on Full Disk Access — don't poll without it
        guard Self.checkFullDiskAccess() else {
            appendLog("⚠️ Messages: Full Disk Access required. Enable in System Settings > Privacy & Security > Full Disk Access.")
            flushLog()
            messagesMonitorEnabled = false
            return
        }

        refreshMessageRecipients()
        appendLog("💬 Messages: ON")
        flushLog()

        messagesMonitorTask = Task { [weak self] in
            guard let self else { return }
            // Seed the last-seen ROWID so we only act on NEW messages
            await self.seedLastSeenROWID()

            // Pulse on startup
            self.flashMessagesDot()

            while !Task.isCancelled {
                try? await Task.sleep(nanoseconds: 5_000_000_000) // poll every 5s
                guard !Task.isCancelled else { break }
                await self.pollMessages()
            }
        }
    }

    func stopMessagesMonitor() {
        messagesMonitorTask?.cancel()
        messagesMonitorTask = nil
        messagesPolling = false
    }

    /// Send an immediate acknowledgment via iMessage when a task starts.
    private func sendAgentAck() {
        guard let handle = agentReplyHandle else { return }
        let ack = "Working on it..."
        let escaped = ack.replacingOccurrences(of: "\\", with: "\\\\")
            .replacingOccurrences(of: "\"", with: "\\\"")
        let script = """
        tell application "Messages"
            set targetService to 1st account whose service type = iMessage
            set targetBuddy to participant "\(handle)" of targetService
            send "\(escaped)" to targetBuddy
        end tell
        """
        Task {
            let result = await Self.executeTCC(command: "osascript -e '\(script.replacingOccurrences(of: "'", with: "'\\''"))'")

            if result.status == 0 {
                appendLog("Agent! ack sent to \(handle)")
            } else {
                appendLog("Agent! ack failed: \(result.output.prefix(100))")
            }
            flushLog()
        }
    }

    /// Send a reply via iMessage to the handle that triggered the Agent! task.
    func sendAgentReply(_ summary: String) {
        guard let handle = agentReplyHandle else { return }
        agentReplyHandle = nil

        // Strip leading "Agent!" / "Agent " from outgoing replies so the
        // receiving Mac doesn't loop on its own command. Use the same
        // case-insensitive prefix logic as inbound parsing.
        // Cap via LogLimits.messageReplyChars (4K) — iMessage tolerates more
        // but carriers split unpredictably above that.
        let reply = LogLimits.trim(Self.stripAgentPrefix(from: summary), cap: LogLimits.messageReplyChars)
        // Escape for AppleScript
        let escaped = reply
            .replacingOccurrences(of: "\\", with: "\\\\")
            .replacingOccurrences(of: "\"", with: "\\\"")

        let script = """
        tell application "Messages"
            set targetService to 1st account whose service type = iMessage
            set targetBuddy to participant "\(handle)" of targetService
            send "\(escaped)" to targetBuddy
        end tell
        """
        Task {
            let result = await Self.executeTCC(command: "osascript -e '\(script.replacingOccurrences(of: "'", with: "'\\''"))'")

            if result.status == 0 {
                appendLog("Agent! reply sent to \(handle)")
            } else {
                appendLog("Agent! reply failed: \(result.output.prefix(100))")
            }
            flushLog()
        }
    }

    /// Briefly flash the Messages StatusDot green.
    private func flashMessagesDot() {
        messagesPolling = true
        Task {
            try? await Task.sleep(nanoseconds: 800_000_000)
            messagesPolling = false
        }
    }

    // MARK: - Progress Updates for Long-Running Tasks

    /// Start periodic progress updates via iMessage for long-running tasks.
    /// Sends an update every 10 minutes with elapsed time and current status.
    func startProgressUpdates(for taskDescription: String) {
        stopProgressUpdates() // Cancel any existing updates

        currentTaskDescription = taskDescription
        taskStartTime = Date()
        progressUpdateCount = 0

        progressUpdateTask = Task { [weak self] in
            while !Task.isCancelled {
                // Wait 10 minutes between updates
                do {
                    try await Task.sleep(for: .seconds(600))
                } catch {
                    break // Task cancelled
                }

                guard let self = self, self.isRunning else { break }

                self.progressUpdateCount += 1
                let elapsed: String
                if let startTime = self.taskStartTime {
                    let interval = Date().timeIntervalSince(startTime)
                    let minutes = Int(interval) / 60
                    let seconds = Int(interval) % 60
                    if minutes > 0 {
                        elapsed = "\(minutes)m \(seconds)s"
                    } else {
                        elapsed = "\(seconds)s"
                    }
                } else {
                    elapsed = "unknown"
                }

                let statusMessage: String
                if self.isThinking {
                    statusMessage = "thinking..."
                } else if self.userServiceActive || self.rootServiceActive {
                    statusMessage = "executing command..."
                } else {
                    statusMessage = "processing..."
                }

                // Send progress update
                let update = "⏳ Progress: \(elapsed) elapsed, \(statusMessage) (update #\(self.progressUpdateCount))"
                self.sendProgressUpdate(update)
            }
        }
    }

    /// Stop progress updates when task completes or is cancelled.
    func stopProgressUpdates() {
        progressUpdateTask?.cancel()
        progressUpdateTask = nil
        taskStartTime = nil
        progressUpdateCount = 0
        currentTaskDescription = ""
    }

    /// Send a progress update message via iMessage.
    func sendProgressUpdate(_ message: String) {
        guard let handle = agentReplyHandle else { return }
        let escaped = message.replacingOccurrences(of: "\\", with: "\\\\")
            .replacingOccurrences(of: "\"", with: "\\\"")
        let script = """
        tell application "Messages"
            set targetService to 1st account whose service type = iMessage
            set targetBuddy to participant "\(handle)" of targetService
            send "\(escaped)" to targetBuddy
        end tell
        """
        Task {
            let result = await Self.executeTCC(command: "osascript -e '\(script.replacingOccurrences(of: "'", with: "'\\''"))'")

            if result.status == 0 {
                appendLog("📤 Progress: \(message)")
            } else {
                appendLog("❌ Progress failed: \(result.output.prefix(50))")
            }
            flushLog()
        }
    }

    // Stored outside @MainActor so nonisolated static methods can access it
    private nonisolated static let messagesDBPath = NSHomeDirectory() + "/Library/Messages/chat.db"

    /// Decode attributedBody blob (typedstream/NSArchiver format).
    /// NSUnarchiver is the only way to decode the typedstream format used by the Messages database.
    private nonisolated static func decodeAttributedBody(_ data: Data) -> NSAttributedString? {
        guard let cls = NSClassFromString("NSUnarchiver") else { return nil }
        let sel = NSSelectorFromString("unarchiveObjectWithData:")
        guard let method = class_getClassMethod(cls, sel) else { return nil }
        typealias Fn = @convention(c) (AnyClass, Selector, NSData) -> AnyObject?
        let imp = method_getImplementation(method)
        let f = unsafeBitCast(imp, to: Fn.self)
        return f(cls, sel, data as NSData) as? NSAttributedString
    }

    // MARK: - "Agent!" Prefix Detection
    // Accept "agent"/"Agent!" case-insensitively, with or without trailing "!".
    // Must be a complete word (followed by end-of-string, "!", or whitespace)
    // so "agency"/"agentic" don't trigger.

    /// True iff `text` starts with "agent" / "agent!" as a complete leading
    /// word (case-insensitive). Followed by end-of-string, "!", or whitespace.
    nonisolated static func hasAgentPrefix(_ text: String) -> Bool {
        let lower = text.lowercased()
        guard lower.hasPrefix("agent") else { return false }
        let after = lower.index(lower.startIndex, offsetBy: 5) // 5 = len("agent")
        guard after < lower.endIndex else { return true } // bare "agent"
        let nextChar = lower[after]
        return nextChar == "!" || nextChar == " " || nextChar == "\t" || nextChar == "\n"
    }

    /// Strip the leading "agent" / "agent!" prefix and any following
    /// whitespace. Returns `text` unchanged if no prefix is present.
    nonisolated static func stripAgentPrefix(from text: String) -> String {
        guard hasAgentPrefix(text) else { return text }
        // Skip "agent"
        var idx = text.index(text.startIndex, offsetBy: 5)
        // Skip an optional "!"
        if idx < text.endIndex, text[idx] == "!" {
            idx = text.index(after: idx)
        }
        // Skip any whitespace
        while idx < text.endIndex, text[idx].isWhitespace {
            idx = text.index(after: idx)
        }
        return String(text[idx...])
    }

    /// Read new messages directly from chat.db using SQLite3 C API.
    private nonisolated static func queryMessages(afterROWID: Int, filter: MessageFilter) -> [RawMessage] {
        var db: OpaquePointer?
        guard sqlite3_open_v2(messagesDBPath, &db, SQLITE_OPEN_READONLY | SQLITE_OPEN_NOMUTEX, nil) == SQLITE_OK else { return [] }
        defer { sqlite3_close(db) }

        let whereClause: String
        switch filter {
        case .fromOthers: whereClause = "m.ROWID > ?1 AND m.is_from_me = 0"
        case .fromMe: whereClause = "m.ROWID > ?1 AND m.is_from_me = 1"
        case .noFilter: whereClause = "m.ROWID > ?1"
        }

        let sql = """
        SELECT m.ROWID, m.text, m.attributedBody, \
        COALESCE(h.id, ''), m.handle_id, COALESCE(cmj.chat_id, 0), \
        COALESCE(m.service, ''), COALESCE(m.account, '') \
        FROM message m \
        LEFT JOIN handle h ON h.ROWID = m.handle_id \
        LEFT JOIN chat_message_join cmj ON cmj.message_id = m.ROWID \
        WHERE \(whereClause) ORDER BY m.ROWID ASC LIMIT 10
        """
        var stmt: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &stmt, nil) == SQLITE_OK else { return [] }
        defer { sqlite3_finalize(stmt) }

        sqlite3_bind_int64(stmt, 1, Int64(afterROWID))

        var results: [RawMessage] = []
        while sqlite3_step(stmt) == SQLITE_ROW {
            let rowid = Int(sqlite3_column_int64(stmt, 0))
            let handleId = sqlite3_column_text(stmt, 3).map { String(cString: $0) } ?? ""
            let handleRowId = Int(sqlite3_column_int64(stmt, 4))
            let chatId = Int(sqlite3_column_int64(stmt, 5))
            let service = sqlite3_column_text(stmt, 6).map { String(cString: $0) } ?? ""
            let account = sqlite3_column_text(stmt, 7).map { String(cString: $0) } ?? ""

            // Try the `text` column first
            var text: String?
            if let cStr = sqlite3_column_text(stmt, 1) {
                let s = String(cString: cStr)
                if !s.isEmpty { text = s }
            }

            // Fall back to decoding attributedBody blob (NSArchiver typedstream format)
            if text == nil, let blobPtr = sqlite3_column_blob(stmt, 2) {
                let blobLen = Int(sqlite3_column_bytes(stmt, 2))
                let data = Data(bytes: blobPtr, count: blobLen)
                if let attrStr = Self.decodeAttributedBody(data) {
                    let s = attrStr.string
                    if !s.isEmpty { text = s }
                }
            }

            results.append(RawMessage(
                rowid: rowid,
                text: text ?? "",
                handleId: handleId,
                handleRowId: handleRowId,
                chatId: chatId,
                service: service,
                account: account
            ))
        }
        return results
    }

    /// Auto-add a recipient from an incoming message if not already known.
    private func autoAddRecipient(from row: RawMessage) {
        guard !row.handleId.isEmpty else { return }
        if messageRecipients.contains(where: { $0.id == row.handleId }) { return }
        let fromMe = messageFilter == .fromMe
        let recipient = MessageRecipient(id: row.handleId, displayName: row.handleId, service: row.service, fromMe: fromMe)
        messageRecipients.append(recipient)
        persistRecipients()
    }

    private func persistRecipients() {
        let ids = messageRecipients.map(\.id)
        let services = messageRecipients.map(\.service)
        let fromMes = messageRecipients.map(\.fromMe)
        UserDefaults.standard.set(ids, forKey: "agentDiscoveredHandles")
        UserDefaults.standard.set(services, forKey: "agentDiscoveredServices")
        UserDefaults.standard.set(fromMes, forKey: "agentDiscoveredFromMe")
    }

    /// Reload previously discovered recipients from UserDefaults.
    func refreshMessageRecipients() {
        let ids = UserDefaults.standard.stringArray(forKey: "agentDiscoveredHandles") ?? []
        let services = UserDefaults.standard.stringArray(forKey: "agentDiscoveredServices") ?? []
        let fromMes = UserDefaults.standard.array(forKey: "agentDiscoveredFromMe") as? [Bool] ?? []
        var recipients: [MessageRecipient] = []
        for (i, id) in ids.enumerated() {
            let service = i < services.count ? services[i] : ""
            let fromMe = i < fromMes.count ? fromMes[i] : false
            recipients.append(MessageRecipient(id: id, displayName: id, service: service, fromMe: fromMe))
        }
        messageRecipients = recipients
    }

    /// Recipients filtered by the current message filter setting.
    var filteredRecipients: [MessageRecipient] {
        switch messageFilter {
        case .fromOthers: return messageRecipients.filter { !$0.fromMe }
        case .fromMe: return messageRecipients.filter { $0.fromMe }
        case .noFilter: return messageRecipients
        }
    }

    /// Query for the max ROWID in the Messages database.
    private nonisolated static func maxMessageROWID() -> Int? {
        var db: OpaquePointer?
        guard sqlite3_open_v2(messagesDBPath, &db, SQLITE_OPEN_READONLY | SQLITE_OPEN_NOMUTEX, nil) == SQLITE_OK else { return nil }
        defer { sqlite3_close(db) }

        var stmt: OpaquePointer?
        guard sqlite3_prepare_v2(db, "SELECT MAX(ROWID) FROM message", -1, &stmt, nil) == SQLITE_OK else { return nil }
        defer { sqlite3_finalize(stmt) }

        guard sqlite3_step(stmt) == SQLITE_ROW else { return nil }
        return Int(sqlite3_column_int64(stmt, 0))
    }

    /// Seed the ROWID cursor so we only process messages arriving after monitor starts.
    private func seedLastSeenROWID() async {
        // Retry up to 3 times with a delay (macOS may need a moment to grant DB access)
        for attempt in 1...3 {
            if let rowid = await Self.offMain({ Self.maxMessageROWID() }) {
                lastSeenMessageROWID = rowid
                appendLog("💬 Messages: seeded at ROWID \(rowid)")
                flushLog()
                return
            }
            if attempt < 3 {
                try? await Task.sleep(nanoseconds: 2_000_000_000)
            }
        }
        // Cannot read chat.db — open Full Disk Access settings for the user
        lastSeenMessageROWID = Int.max
        appendLog("💬 Messages: Full Disk Access required to read iMessages. Opening System Settings…")
        flushLog()
        NSWorkspace.shared.open(URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles")!)
    }

    // MARK: - Messages Tab

    /// Find or create the dedicated Messages tab. Always uses main tab's LLM settings.
    func ensureMessagesTab() -> ScriptTab {
        if let existing = scriptTabs.first(where: { $0.isMessagesTab }) {
            return existing
        }
        let tab = ScriptTab(scriptName: "Messages")
        tab.isMessagesTab = true
        tab.isRunning = false
        scriptTabs.append(tab)
        persistScriptTabs()
        return tab
    }

    /// Send an iMessage reply from the Messages tab after its task completes.
    func sendMessagesTabReply(_ summary: String, handle: String) {
        // Strip leading "Agent!" / "Agent " (case-insensitive, ! optional) so
        // the receiving Mac doesn't loop on its own command.
        let reply = Self.stripAgentPrefix(from: summary)
        let escaped = reply.replacingOccurrences(of: "\\", with: "\\\\")
            .replacingOccurrences(of: "\"", with: "\\\"")
        let script = """
        tell application "Messages"
            set targetService to 1st account whose service type = iMessage
            set targetBuddy to participant "\(handle)" of targetService
            send "\(escaped)" to targetBuddy
        end tell
        """
        Task {
            let result = await Self.executeTCC(command: "osascript -e '\(script.replacingOccurrences(of: "'", with: "'\\''"))'")

            let msgTab = scriptTabs.first(where: { $0.isMessagesTab })
            if result.status == 0 {
                msgTab?.appendLog("💬 Reply sent to \(handle)")
            } else {
                msgTab?.appendLog("❌ Reply failed: \(result.output.prefix(100))")
            }
            msgTab?.flush()
        }
    }

    /// Send an iMessage acknowledgment from the Messages tab.
    private func sendMessagesTabAck(handle: String) {
        let ack = "Working on it..."
        let escaped = ack.replacingOccurrences(of: "\\", with: "\\\\")
            .replacingOccurrences(of: "\"", with: "\\\"")
        let script = """
        tell application "Messages"
            set targetService to 1st account whose service type = iMessage
            set targetBuddy to participant "\(handle)" of targetService
            send "\(escaped)" to targetBuddy
        end tell
        """
        Task {
            let result = await Self.executeTCC(command: "osascript -e '\(script.replacingOccurrences(of: "'", with: "'\\''"))'")

            let msgTab = scriptTabs.first(where: { $0.isMessagesTab })
            if result.status == 0 {
                msgTab?.appendLog("💬 Ack sent to \(handle)")
            } else {
                msgTab?.appendLog("❌ Ack failed: \(result.output.prefix(100))")
            }
            msgTab?.flush()
        }
    }

    /// Poll for new incoming messages; log from enabled handles, act on "Agent!" prefix.
    /// Routes messages to the dedicated Messages tab instead of the main/active tab.
    private func pollMessages() async {
        // If seed failed, try to reseed now
        if lastSeenMessageROWID == Int.max {
            if let rowid = await Self.offMain({ Self.maxMessageROWID() }) {
                lastSeenMessageROWID = rowid
                appendLog("💬 Messages: reseeded at ROWID \(rowid)")
                flushLog()
            }
            return
        }

        let after = lastSeenMessageROWID
        let enabled = enabledHandleIds
        let filter = messageFilter
        let rows = await Self.offMain({ Self.queryMessages(afterROWID: after, filter: filter) })
        guard !rows.isEmpty else { return }

        for row in rows {
            lastSeenMessageROWID = row.rowid

            guard !row.text.isEmpty else { continue }

            // Only process messages that start with the wake prefix.
            // Case-insensitive, exclamation mark optional (iPhone autocorrect
            // strips "!" routinely; older Macs/contacts use lowercase "agent ").
            // hasAgentPrefix returns true for "Agent!", "agent!", "AGENT!",
            // "Agent ", "agent ", "AGENT " — anything where the first word is
            // "agent" (with or without trailing punctuation).
            guard Self.hasAgentPrefix(row.text) else { continue }

            // Auto-discover this sender
            autoAddRecipient(from: row)

            let approved = enabled.contains(row.handleId)

            // Always show the message in the main log
            flashMessagesDot()
            if approved {
                appendLog("iMessage (\(row.handleId)): \(row.text)")
            } else {
                appendLog("iMessage not approved (\(row.handleId)): \(row.text) — select this recipient in the Messages toolbar button")
            }
            flushLog()

            guard approved else { continue }

            let prompt = Self.stripAgentPrefix(from: row.text).trimmingCharacters(in: .whitespaces)
            guard !prompt.isEmpty else { continue }

            // Route to dedicated Messages tab
            let msgTab = ensureMessagesTab()

            // Skip if Messages tab is already running a task
            guard !msgTab.isLLMRunning else {
                msgTab.appendLog("Busy — skipped: \(prompt)")
                msgTab.flush()
                continue
            }

            msgTab.replyHandle = row.handleId
            msgTab.appendLog("iMessage from \(row.handleId): \(prompt)")
            msgTab.flush()

            // Select the Messages tab so the user sees it
            selectedTabId = msgTab.id

            // Run the task on the Messages tab (uses main tab's LLM config via resolvedLLMConfig fallback)
            msgTab.taskInput = prompt
            runTabTask(tab: msgTab)
        }
    }

}
