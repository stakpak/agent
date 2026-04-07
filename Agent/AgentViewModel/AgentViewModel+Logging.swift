import AppKit
import Foundation

// MARK: - Logging, Streaming & Media

extension AgentViewModel {

    // MARK: - Error History

    /// Log an error to both the log and error history
    func appendError(_ error: Error, context: String = "") {
        let timestamp = Self.timestampFormatter.string(from: Date())
        let errorMessage: String
        if let agentError = error as? AgentError {
            errorMessage = agentError.errorDescription ?? "Unknown error"
        } else {
            errorMessage = error.localizedDescription
        }

        let fullMessage = context.isEmpty ? errorMessage : "\(context): \(errorMessage)"
        let formattedMessage = "[\(timestamp)] ERROR: \(fullMessage)"

        // Store in SwiftData
        ChatHistoryStore.shared.appendMessage(formattedMessage)

        // Also store in error history
        let errorRecord = ErrorRecord(
            timestamp: Date(),
            message: fullMessage,
            errorType: String(describing: type(of: error)),
            context: context,
            stackTrace: Thread.callStackSymbols.joined(separator: "\n")
        )
        ErrorHistory.shared.add(errorRecord)

        // Also store per-tab error if a tab is active
        if let selectedId = selectedTabId,
           let tab = tab(for: selectedId)
        {
            tab.tabErrors.append("[\(timestamp)] \(String(describing: type(of: error))): \(fullMessage.truncate(to: 100))")
        }

        // Log to buffer
        if !logBuffer.isEmpty && !logBuffer.hasSuffix("\n") {
            logBuffer += "\n"
        }
        logBuffer += formattedMessage + "\n"
        scheduleLogFlush()
    }

    /// Log a tool error with specific tool context
    func appendToolError(tool: String, error: Error, input: [String: Any]? = nil) {
        var context = "Tool '\(tool)' failed"
        if let input = input, !input.isEmpty {
            let inputStr = input.map { "\($0.key)=\($0.value)" }.joined(separator: ", ")
            context += " with input: \(inputStr)"
        }
        appendError(error, context: context)
    }

    /// Log a task error (when a whole task fails)
    func appendTaskError(task: String, error: Error, commandsRun: [String] = []) {
        var context = "Task '\(task.truncate(to: 50))' failed"
        if !commandsRun.isEmpty {
            context += ", commands run: \(commandsRun.prefix(3).joined(separator: "; "))"
        }
        appendError(error, context: context)
    }

    // MARK: - Screenshot

    func captureScreenshot() {
        let tempPath = NSTemporaryDirectory() + "agent_screenshot_\(UUID().uuidString).png"

        Task {
            let status = await Self.offMain {
                let process = Process()
                process.executableURL = URL(fileURLWithPath: "/usr/sbin/screencapture")
                process.arguments = ["-i", tempPath]
                process.currentDirectoryURL = URL(fileURLWithPath: NSTemporaryDirectory())
                var env = ProcessInfo.processInfo.environment
                env["HOME"] = NSHomeDirectory()
                let extraPaths = "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
                env["PATH"] = extraPaths + ":" + (env["PATH"] ?? "")
                process.environment = env
                do {
                    try process.run()
                    process.waitUntilExit()
                    return process.terminationStatus
                } catch {
                    return Int32(-1)
                }
            }

            guard status == 0,
                  FileManager.default.fileExists(atPath: tempPath),
                  let image = NSImage(contentsOfFile: tempPath),
                  let tiffData = image.tiffRepresentation,
                  let bitmap = NSBitmapImageRep(data: tiffData),
                  let pngData = bitmap.representation(using: .png, properties: [:]) else
            {
                if status != 0 && status != -1 {
                    appendLog("❌ Screenshot failed (exit \(status))")
                }
                return
            }

            if let tab = selectedTabId.flatMap({ tab(for: $0) }) {
                tab.attachedImages.append(image)
                tab.attachedImagesBase64.append(pngData.base64EncodedString())
            } else {
                attachedImages.append(image)
                attachedImagesBase64.append(pngData.base64EncodedString())
            }
            try? FileManager.default.removeItem(atPath: tempPath)
        }
    }

    func removeAttachment(at index: Int) {
        guard attachedImages.indices.contains(index) else { return }
        attachedImages.remove(at: index)
        attachedImagesBase64.remove(at: index)
    }

    func removeAllAttachments() {
        attachedImages.removeAll()
        attachedImagesBase64.removeAll()
    }

    /// Try all pasteboard formats to grab an image.
    /// Returns true if image data was found (encoding happens async in background).
    @discardableResult
    func pasteImageFromClipboard() -> Bool {
        let pb = NSPasteboard.general

        var rawData: Data?

        // Try raw data types first (avoids full NSImage deserialization overhead)
        for type in [
            NSPasteboard.PasteboardType.png,
            NSPasteboard.PasteboardType.tiff,
            NSPasteboard.PasteboardType(rawValue: "public.jpeg")
        ]
        {
            if let data = pb.data(forType: type) {
                rawData = data
                break
            }
        }

        // Try NSImage as fallback
        if rawData == nil,
           let images = pb.readObjects(forClasses: [NSImage.self]) as? [NSImage],
           let img = images.first,
           let tiff = img.tiffRepresentation
        {
            rawData = tiff
        }

        // Try file URLs (e.g. screenshot file copied from Finder)
        if rawData == nil,
           let urls = pb.readObjects(forClasses: [NSURL.self]) as? [URL]
        {
            for url in urls {
                let ext = url.pathExtension.lowercased()
                if ["png", "jpg", "jpeg", "tiff", "bmp", "gif"].contains(ext),
                   let data = try? Data(contentsOf: url)
                {
                    rawData = data
                    break
                }
            }
        }

        guard let imageData = rawData else { return false }

        // Encode on a background thread to avoid blocking the main thread
        let currentTabId = selectedTabId
        Task {
            let base64 = await Self.encodeImageToBase64(imageData)
            guard let base64 else { return }
            if let image = NSImage(data: imageData) {
                if let tabId = currentTabId, let tab = self.tab(for: tabId) {
                    tab.attachedImages.append(image)
                    tab.attachedImagesBase64.append(base64)
                } else {
                    attachedImages.append(image)
                    attachedImagesBase64.append(base64)
                }
            }
        }

        return true
    }

    /// Encode image data to a base64 PNG string off the main thread.
    /// Downscales images larger than 2048px to prevent memory issues.
    private static nonisolated func encodeImageToBase64(_ data: Data) async -> String? {
        guard let bitmap = NSBitmapImageRep(data: data) else { return nil }

        let maxDim = 2048
        let w = bitmap.pixelsWide
        let h = bitmap.pixelsHigh

        if w > maxDim || h > maxDim {
            let scale = min(Double(maxDim) / Double(w), Double(maxDim) / Double(h))
            let newW = Int(Double(w) * scale)
            let newH = Int(Double(h) * scale)

            guard let cgImage = bitmap.cgImage,
                  let ctx = CGContext(
                      data: nil, width: newW, height: newH,
                      bitsPerComponent: 8, bytesPerRow: 0,
                      space: CGColorSpaceCreateDeviceRGB(),
                      bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
                  ) else { return nil }

            ctx.interpolationQuality = .high
            ctx.draw(cgImage, in: CGRect(x: 0, y: 0, width: newW, height: newH))

            guard let resizedCG = ctx.makeImage() else { return nil }
            let resizedBitmap = NSBitmapImageRep(cgImage: resizedCG)
            guard let pngData = resizedBitmap.representation(using: .png, properties: [:]) else { return nil }
            return pngData.base64EncodedString()
        }

        guard let pngData = bitmap.representation(using: .png, properties: [:]) else { return nil }
        return pngData.base64EncodedString()
    }

    // MARK: - Log Buffering

    /// Snapshot any image files found in text to persistent cache, rewriting paths to UUID copies.
    private func snapshotImages(in text: String) -> String {
        let nsText = text as NSString
        let matches = Self.imagePathRegex?.matches(in: text, range: NSRange(location: 0, length: nsText.length)) ?? []
        guard !matches.isEmpty else { return text }

        var result = text
        // Process in reverse so earlier offsets stay valid
        for match in matches.reversed() {
            let range = match.range(at: 1)
            let path = nsText.substring(with: range)

            // Skip if already a cached path (old or new cache dirs)
            if path.contains("/log_images/") || path.contains("/Caches/Agent/") { continue }

            // Skip if file doesn't exist
            guard FileManager.default.fileExists(atPath: path) else { continue }

            let ext = (path as NSString).pathExtension
            let uuid = UUID().uuidString
            let cachedURL = Self.logImageCacheDir.appendingPathComponent("\(uuid).\(ext)")

            do {
                try FileManager.default.copyItem(atPath: path, toPath: cachedURL.path)
                guard let swiftRange = Range(range, in: result) else { continue }
                result.replaceSubrange(swiftRange, with: cachedURL.path)
            } catch {
                // Copy failed — leave original path
            }
        }

        return result
    }

    static let newTaskMarker = "--- New Task ---"

    /// Shared log formatting — prepends spacing for first task, strips blank lines before Cancelled.
    static func prepareLogBuffer(message: String, buffer: inout String, existingLog: String) {
        let combined = existingLog + buffer
        if message.contains(newTaskMarker) && !combined.contains(newTaskMarker) {
            buffer += String(repeating: "\n", count: 1)
        }
        if message.contains("Cancelled") {
            while buffer.hasSuffix("\n\n") { buffer.removeLast() }
        }
        if !buffer.isEmpty && !buffer.hasSuffix("\n") { buffer += "\n" }
    }

    func appendLog(_ message: String) {
        let timestamp = Self.timestampFormatter.string(from: Date())
        let cached = snapshotImages(in: message)
        let formattedMessage = "[\(timestamp)] \(cached)"

        // Store in SwiftData
        ChatHistoryStore.shared.appendMessage(formattedMessage)

        Self.prepareLogBuffer(message: message, buffer: &logBuffer, existingLog: activityLog)
        logBuffer += formattedMessage + "\n"
        scheduleLogFlush()
    }

    func appendRawOutput(_ text: String) {
        guard !text.isEmpty else { return }
        let cached = snapshotImages(in: text)
        ChatHistoryStore.shared.appendMessage(cached)
        logBuffer += cached
        if !cached.hasSuffix("\n") {
            logBuffer += "\n"
        }
        scheduleLogFlush()
    }

    /// Collapse heredoc bodies in commands to keep the log clean.
    /// "cat > file.html <<'EOF'\n<html>...\nEOF" -> "cat > file.html <<'EOF'\n...(heredoc)...\nEOF"
    static func collapseHeredocs(_ command: String) -> String {
        let lines = command.components(separatedBy: "\n")
        guard lines.count > 3 else { return command }

        // Find the line containing a heredoc marker: <<'DELIM', <<DELIM, <<"DELIM", <<-'DELIM'
        let pattern = #"<<-?\s*'?"?(\w+)'?"?"#
        guard let regex = try? NSRegularExpression(pattern: pattern) else { return command }

        for (i, line) in lines.enumerated() {
            let nsLine = line as NSString
            guard let match = regex.firstMatch(in: line, range: NSRange(location: 0, length: nsLine.length)),
                  match.range(at: 1).location != NSNotFound else { continue }
            let delimiter = nsLine.substring(with: match.range(at: 1))

            // Find the closing delimiter line after the heredoc start
            guard let endIdx = lines[(i + 1)...].firstIndex(where: {
                $0.trimmingCharacters(in: .whitespaces) == delimiter
            }), endIdx > i + 1 else { continue }

            // Collapse: keep lines before + heredoc line, placeholder, delimiter + remainder
            var result = Array(lines[...i])
            result.append("...(\(delimiter) heredoc)...")
            result.append(contentsOf: lines[endIdx...])
            return result.joined(separator: "\n")
        }
        return command
    }

    func resetStreamCounters() {
        streamLineCount = 0
        streamTruncated = false
    }

    func clearLog() {
        logBuffer = ""
        logFlushTask?.cancel()
        logFlushTask = nil
        logPersistTask?.cancel()
        logPersistTask = nil
        activityLog = ""
        ChatHistoryStore.shared.clearAll()
        // Clear Apple AI conversation context when log is cleared
        AppleIntelligenceMediator.shared.clearContext()
        // Clean up cached image snapshots
        try? FileManager.default.removeItem(at: Self.logImageCacheDir)
        try? FileManager.default.createDirectory(at: Self.logImageCacheDir, withIntermediateDirectories: true)
    }

    /// Clear the selected tab's log, or main log if no tab selected.
    func clearSelectedLog() {
        if let selectedId = selectedTabId,
           let tab = tab(for: selectedId)
        {
            tab.activityLog = ""
            tab.logBuffer = ""
            tab.logFlushTask?.cancel()
            tab.logFlushTask = nil
            tab.streamLineCount = 0
            tab.rawLLMOutput = ""
            tab.lastElapsed = 0
            tab.tabInputTokens = 0
            tab.tabOutputTokens = 0
            tab.thinkingDismissed = true
            tab.thinkingExpanded = false
            tab.thinkingOutputExpanded = false
            persistScriptTabs()
        } else {
            rawLLMOutput = ""
            thinkingDismissed = true
            thinkingExpanded = false
            thinkingOutputExpanded = false
            taskInputTokens = 0
            taskOutputTokens = 0
            clearLog()
        }
    }

    /// Clear everything: log, LLM output, prompt history, task history, token counts.
    func clearAll() {
        // Clear log
        clearSelectedLog()
        // Clear LLM output
        rawLLMOutput = ""
        thinkingDismissed = true
        thinkingExpanded = false
        thinkingOutputExpanded = false
        // Clear prompt history
        promptHistory.removeAll()
        UserDefaults.standard.removeObject(forKey: "agentPromptHistory")
        historyIndex = -1
        savedInput = ""
        // Clear task history
        history.clearAll()
        // Clear token counts
        taskInputTokens = 0
        taskOutputTokens = 0
        sessionInputTokens = 0
        sessionOutputTokens = 0
        // Clear selected tab if applicable
        if let selectedId = selectedTabId,
           let tab = tab(for: selectedId)
        {
            tab.rawLLMOutput = ""
            tab.llmMessages = []
            tab.promptHistory.removeAll()
            tab.tabTaskSummaries.removeAll()
            tab.tabErrors.removeAll()
            tab.tabInputTokens = 0
            tab.tabOutputTokens = 0
            tab.historyIndex = -1
            tab.savedInput = ""
        }
        appendLog("🧹 All cleared.")
        flushLog()
    }

    // MARK: - LLM Streaming

    func appendStreamDelta(_ delta: String) {
        if !streamingTextStarted {
            rawLLMOutput = ""
            displayedLLMOutput = ""
            dripDisplayIndex = 0
        }
        streamingTextStarted = true
        rawLLMOutput += delta
        startDripIfNeeded()
    }

    /// Drip characters from rawLLMOutput into displayedLLMOutput one at a time (terminal effect)
    func startDripIfNeeded() {
        guard dripTask == nil else { return }
        dripTask = Task { [weak self] in
            guard let self else { return }
            while !Task.isCancelled {
                if self.dripDisplayIndex < self.rawLLMOutput.count {
                    let idx = self.rawLLMOutput.index(self.rawLLMOutput.startIndex, offsetBy: self.dripDisplayIndex)
                    self.displayedLLMOutput.append(self.rawLLMOutput[idx])
                    self.dripDisplayIndex += 1
                    try? await Task.sleep(for: .milliseconds(self.terminalSpeed.rawValue))
                } else if !self.streamingTextStarted {
                    break // Stream ended and all chars dripped
                } else {
                    try? await Task.sleep(for: .milliseconds(max(5, self.terminalSpeed.rawValue / 2))) // Wait for more
                }
            }
            self.dripTask = nil
        }
    }

    /// Collapse runs of 3+ newlines to 2 (one blank line max) to prevent huge gaps from chatty models.
    static func collapseNewlines(_ text: String) -> String {
        var result = ""
        var newlineCount = 0
        for ch in text {
            if ch == "\n" {
                newlineCount += 1
                if newlineCount <= 2 { result.append(ch) }
            } else {
                newlineCount = 0
                result.append(ch)
            }
        }
        return result
    }

    private func scheduleStreamFlush() {
        guard streamFlushTask == nil else { return }
        streamFlushTask = Task {
            self.streamFlushTask = nil
            if !self.streamBuffer.isEmpty {
                let collapsed = Self.collapseNewlines(self.streamBuffer)
                ChatHistoryStore.shared.appendStreamingContent(collapsed)
                self.activityLog += collapsed
                self.streamBuffer = ""
            }
        }
    }

    func flushStreamBuffer() {
        streamFlushTask?.cancel()
        streamFlushTask = nil
        var didFlush = false
        if !streamBuffer.isEmpty {
            let collapsed = Self.collapseNewlines(streamBuffer)
            ChatHistoryStore.shared.appendStreamingContent(collapsed)
            activityLog += collapsed
            streamBuffer = ""
            didFlush = true
        }
        if streamingTextStarted {
            if didFlush {
                ChatHistoryStore.shared.appendStreamingContent("\n")
                activityLog += "\n"
            }
            streamingTextStarted = false
            // Let drip task finish naturally — no instant dump
        }
    }

    private func scheduleLogFlush() {
        guard logFlushTask == nil else { return }
        logFlushTask = Task {
            try? await Task.sleep(for: .milliseconds(50))
            flushLog()
        }
    }

    func flushLog() {
        logFlushTask?.cancel()
        logFlushTask = nil
        // Drain stream buffer first so streamed text precedes the log entries
        var combined = ""
        if !streamBuffer.isEmpty {
            ChatHistoryStore.shared.appendStreamingContent(streamBuffer)
            combined += streamBuffer
            streamBuffer = ""
        }
        // Ensure streamed text ends with a newline before timestamped entries
        if streamingTextStarted {
            ChatHistoryStore.shared.appendStreamingContent("\n")
            combined += "\n"
            streamingTextStarted = false
        }
        if !logBuffer.isEmpty {
            // Ensure timestamps always start on a new line
            if !activityLog.isEmpty && !activityLog.hasSuffix("\n") && combined.isEmpty {
                combined += "\n"
            }
            ChatHistoryStore.shared.save()
            combined += logBuffer
            logBuffer = ""
            schedulePersist()
        }
        // Single mutation of activityLog instead of multiple
        if !combined.isEmpty {
            activityLog += combined
            NotificationCenter.default.post(name: .activityLogDidChange, object: nil)
        }
        // Truncation handled by ActivityLogView at render time (50K cap)
    }

    private func schedulePersist() {
        guard logPersistTask == nil else { return }
        logPersistTask = Task {
            try? await Task.sleep(for: .seconds(5))
            guard !Task.isCancelled else { return }
            logPersistTask = nil
            ChatHistoryStore.shared.save()
        }
    }

    /// Keep only the last N tasks visible in the chat (controlled by visibleTaskCount preference)
    func trimToRecentTasks() {
        let marker = Self.newTaskMarker
        let parts = activityLog.components(separatedBy: marker)
        let limit = visibleTaskCount
        guard parts.count > limit + 1 else { return }
        let kept = parts.suffix(limit).joined(separator: marker)
        activityLog = marker + kept
    }

    func persistLogNow() {
        logPersistTask?.cancel()
        logPersistTask = nil
        ChatHistoryStore.shared.save()
    }
}
