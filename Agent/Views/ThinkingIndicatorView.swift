import SwiftUI
import AgentTools
import AgentTerminalNeo

/// Collapsible thinking indicator shown in the activity log area while the LLM is processing.
/// Shows real-time model info, token counts, and message stats when expanded.
struct ThinkingIndicatorView: View {
    @Environment(\.colorScheme) private var colorScheme
    @Bindable var viewModel: AgentViewModel
    var tab: ScriptTab?

    private var isExpanded: Bool {
        get { tab?.thinkingExpanded ?? viewModel.thinkingExpanded }
        nonmutating set {
            if let tab { tab.thinkingExpanded = newValue }
            else { viewModel.thinkingExpanded = newValue }
        }
    }
    private var showStreamText: Bool {
        get { tab?.thinkingOutputExpanded ?? viewModel.thinkingOutputExpanded }
        nonmutating set {
            if let tab { tab.thinkingOutputExpanded = newValue }
            else { viewModel.thinkingOutputExpanded = newValue }
        }
    }
    /// Live LLM Output HUD height — synced from/to the tab (or main viewModel) on appear and on change.
    /// Using @State here keeps the Binding identity stable across renders, which avoids drip
    /// stuttering caused by closure-based bindings creating new identities each frame.
    @State private var outputHeight: CGFloat = 80
    @State private var dots = ""
    @State private var tick = 0
    /// Elapsed time — stored on the tab to survive tab switches
    private var elapsed: TimeInterval {
        get { tab?.taskElapsed ?? viewModel.mainTaskElapsed }
        nonmutating set {
            if let tab { tab.taskElapsed = newValue }
            else { viewModel.mainTaskElapsed = newValue }
        }
    }
    private let refreshTimer = Timer.publish(every: 0.25, on: .main, in: .common).autoconnect()

    private var isActive: Bool {
        if let tab {
            return tab.isLLMRunning || tab.isLLMThinking || tab.isRunning
        }
        return viewModel.isRunning || viewModel.isThinking
    }

    /// True when this tab is running a script and LLM has never been used
    private var isScriptOnly: Bool {
        guard let tab else { return false }
        return tab.isRunning && !tab.isMainTab
            && !tab.isLLMRunning && !tab.isLLMThinking
            && tab.llmMessages.isEmpty && tab.rawLLMOutput.isEmpty
    }

    /// True when the tab's script is executing (LLM was used before but isn't active now)
    private var isExecuting: Bool {
        guard let tab else { return false }
        return tab.isRunning && !tab.isLLMRunning && !tab.isLLMThinking
    }

    private var streamText: String {
        if let tab {
            return tab.displayedLLMOutput
        }
        return viewModel.displayedLLMOutput
    }

    private var rawStreamText: String {
        if let tab {
            return tab.rawLLMOutput
        }
        return viewModel.rawLLMOutput
    }

    private var modelName: String {
        if let tab {
            let (provider, model) = viewModel.resolvedLLMConfig(for: tab)
            return "\(provider.displayName) / \(model)"
        }
        let p = viewModel.selectedProvider
        return "\(p.displayName) / \(viewModel.globalModelForProvider(p))"
    }

    private var messageCount: Int {
        if let tab {
            return tab.llmMessages.count
        }
        return 0
    }

    static func fmtTokens(_ count: Int) -> String {
        if count >= 1_000_000 { return String(format: "%.1fM", Double(count) / 1_000_000) }
        if count >= 1_000 { return String(format: "%.1fK", Double(count) / 1_000) }
        return "\(count)"
    }

    static func formatElapsed(_ t: TimeInterval) -> String {
        let s = Int(t)
        if s < 60 { return "\(s)s" }
        let m = s / 60
        let sec = s % 60
        if m < 60 { return "\(m)m \(sec)s" }
        let h = m / 60
        let min = m % 60
        return "\(h)h \(min)m \(sec)s"
    }

    private var inputTokens: Int { tab?.tabInputTokens ?? viewModel.taskInputTokens }
    private var outputTokens: Int {
        let real = tab?.tabOutputTokens ?? viewModel.taskOutputTokens
        // Live estimate from raw stream during streaming (~4 chars per token)
        let streamEstimate = rawStreamText.count / 4
        return max(real, streamEstimate)
    }
    private var toolSteps: [AgentViewModel.ToolStep] { tab?.toolSteps ?? viewModel.toolSteps }

    /// Approximate context window for the current provider/model
    private var contextWindow: Int {
        let provider: APIProvider
        if let tab {
            provider = tab.llmConfig?.provider ?? viewModel.selectedProvider
        } else {
            provider = viewModel.selectedProvider
        }
        switch provider {
        case .claude: return 1_000_000
        case .openAI: return 272_000
        case .deepSeek: return 128_000
        case .gemini: return 2_000_000
        case .grok: return 2_000_000
        case .zAI: return 128_000
        case .bigModel: return 128_000
        case .qwen: return 131_072
        case .mistral: return 256_000
        case .codestral: return 256_000
        case .vibe: return 128_000
        case .huggingFace: return 32_000
        case .ollama, .localOllama: return viewModel.localOllamaContextSize > 0 ? viewModel.localOllamaContextSize : 32_000
        case .vLLM, .lmStudio: return 32_000
        case .foundationModel: return 4_096
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Button {
                withAnimation(.easeInOut(duration: 0.2)) {
                    isExpanded.toggle()
                }
            } label: {
                HStack(spacing: 6) {
                    Image(systemName: "chevron.right")
                        .font(.system(size: 9, weight: .bold))
                        .foregroundStyle(.secondary)
                        .rotationEffect(.degrees(isExpanded ? 90 : 0))

                    if isActive {
                        ProgressView()
                            .controlSize(.mini)
                    } else {
                        Image(systemName: "checkmark.circle.fill")
                            .font(.caption)
                            .foregroundStyle(.green)
                    }

                    Text("(\(Self.formatElapsed(elapsed)))")
                        .font(.caption.monospaced())
                        .foregroundStyle(.secondary)

                    if isActive {
                        if isScriptOnly {
                            HStack(spacing: 3) {
                                Image(systemName: "play.fill").font(.caption).foregroundStyle(.green)
                                ShimmerText("Running\(dots)", color: .green)
                            }
                        } else if isExecuting {
                            if viewModel.rootServiceActive {
                                HStack(spacing: 3) {
                                    Image(systemName: "lock.shield").font(.caption).foregroundStyle(.red)
                                    ShimmerText("Root\(dots)", color: .red)
                                }
                            } else if viewModel.userServiceActive {
                                HStack(spacing: 3) {
                                    Image(systemName: "terminal").font(.caption).foregroundStyle(.orange)
                                    ShimmerText("Executing\(dots)", color: .orange)
                                }
                            } else {
                                HStack(spacing: 3) {
                                    Image(systemName: "terminal").font(.caption).foregroundStyle(.orange)
                                    ShimmerText("Executing\(dots)", color: .orange)
                                }
                            }
                        } else if let t = tab, t.isLLMThinking {
                            HStack(spacing: 3) {
                                Image(systemName: "brain").font(.caption).foregroundStyle(.blue)
                                ShimmerText("Thinking\(dots)", color: .blue)
                            }
                        } else if tab == nil && viewModel.isThinking {
                            HStack(spacing: 3) {
                                Image(systemName: "brain").font(.caption).foregroundStyle(.blue)
                                ShimmerText("Thinking\(dots)", color: .blue)
                            }
                        } else {
                            HStack(spacing: 3) {
                                Image(systemName: "play.fill").font(.caption).foregroundStyle(.green)
                                ShimmerText("Running\(dots)", color: .green)
                            }
                        }
                    } else {
                        Text("Done")
                            .font(.caption.bold())
                            .foregroundStyle(.green)
                    }

                    // Queue count
                    if let t = tab, !t.taskQueue.isEmpty {
                        Text("+\(t.taskQueue.count) queued")
                            .font(.caption)
                            .foregroundStyle(.teal)
                    } else if tab == nil && !viewModel.mainTaskQueue.isEmpty {
                        Text("+\(viewModel.mainTaskQueue.count) queued")
                            .font(.caption)
                            .foregroundStyle(.teal)
                    }

                    Spacer()
                }
                .padding(.horizontal, 12)
                .padding(.vertical, 6)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)

            if isExpanded {
                VStack(alignment: .leading, spacing: 4) {
                    HStack(spacing: 8) {
                        // LLM Output toggle
                        HStack(spacing: 3) {
                            Image(systemName: showStreamText ? "chevron.down" : "chevron.right")
                                .font(.system(size: 8, weight: .bold))
                            Text("LLM Output").font(.caption)
                        }
                        .foregroundStyle(.secondary)

                        // Model name
                        HStack(spacing: 3) {
                            Image(systemName: "brain").font(.caption)
                            Text(modelName).font(.caption).lineLimit(1)
                        }
                        .foregroundStyle(.secondary)

                        HStack(spacing: 2) {
                            Text("↑").font(.caption).foregroundStyle(.blue)
                            Text(Self.fmtTokens(inputTokens)).font(.caption).foregroundStyle(.secondary)
                            Text("↓").font(.caption).foregroundStyle(.green)
                            Text(Self.fmtTokens(outputTokens)).font(.caption).foregroundStyle(.secondary)
                        }

                        // Context budget bar
                        if inputTokens > 0 {
                            let used = inputTokens + outputTokens
                            let rawFraction = min(Double(used) / Double(contextWindow), 1.0)
                            let isRunning = viewModel.isRunning || (tab?.isLLMRunning ?? false)
                            // Show 100% when task is done, actual usage while running
                            let fraction: Double = !isRunning ? 1.0 : (rawFraction > 0.95 ? 1.0 : rawFraction)
                            let barColor: Color = isRunning ? .green : .blue
                            HStack(spacing: 4) {
                                GeometryReader { geo in
                                    ZStack(alignment: .leading) {
                                        RoundedRectangle(cornerRadius: 2)
                                            .fill(Color.gray.opacity(0.3))
                                        RoundedRectangle(cornerRadius: 2)
                                            .fill(barColor)
                                            .frame(width: geo.size.width * fraction)
                                    }
                                }
                                .frame(width: 40, height: 6)
                                Text("\(Int(fraction * 100))%")
                                    .font(.system(size: 9, design: .monospaced))
                                    .foregroundStyle(barColor)
                            }
                            .accessibilityLabel("Context usage")
                            .accessibilityValue(
                                "\(Int(fraction * 100)) percent, \(Self.fmtTokens(used)) of \(Self.fmtTokens(contextWindow))"
                            )
                        }


                        Spacer()
                    }
                    .padding(.horizontal, 12)
                    .contentShape(Rectangle())
                    .onTapGesture {
                        withAnimation(.easeInOut(duration: 0.2)) {
                            showStreamText.toggle()
                        }
                    }

                    if showStreamText {
                        LLMOutputBox(
                            text: streamText,
                            rawText: rawStreamText,
                            height: $outputHeight,
                            isStreaming: isActive,
                            showDismiss: true,
                            dismissEnabled: !isActive,
                            showScanlines: viewModel.scanLinesEnabled,
                            onDismiss: {
                                withAnimation(.easeInOut(duration: 0.2)) {
                                    showStreamText = false
                                }
                            }
                        )
                    }

                    // Tool steps list
                    if !toolSteps.isEmpty {
                        ToolStepsView(
                            steps: toolSteps,
                            isExpanded: Binding(
                                get: { tab?.toolStepsExpanded ?? viewModel.toolStepsExpanded },
                                set: { newValue in
                                    if let tab { tab.toolStepsExpanded = newValue }
                                    else { viewModel.toolStepsExpanded = newValue }
                                }
                            )
                        )
                    }
                }
                .padding(.horizontal, 20)
                .padding(.bottom, 5)
                .transition(.opacity)
            }
        }
        .background(colorScheme == .dark ? Color.clear : Color.white.opacity(0.53))
        .background(.ultraThinMaterial.opacity(colorScheme == .dark ? 0.95 : 0.97))
        .onAppear {
            // Restore persisted height for the active context (tab or main viewModel)
            outputHeight = CGFloat(tab?.llmOutputHeight ?? viewModel.llmOutputHeight)
        }
        .onChange(of: tab?.id) { _, _ in
            // Tab switched — reload the new tab's persisted height
            outputHeight = CGFloat(tab?.llmOutputHeight ?? viewModel.llmOutputHeight)
        }
        .onChange(of: outputHeight) { _, newHeight in
            // Persist the live height to the active context
            if let tab { tab.llmOutputHeight = Double(newHeight) }
            else { viewModel.llmOutputHeight = Double(newHeight) }
        }
        .onChange(of: tab?.isLLMRunning) { oldValue, newValue in
            // Only fire on actual transitions, not tab swaps. The auto-expand of the
            // chevron + dismiss is handled at the run-start sites in executeTabTask
            // so it doesn't get applied to the wrong tab during a swap.
            guard let tab, oldValue != newValue else { return }
            if newValue == true {
                tab.taskStartDate = Date()
                tab._taskElapsedFrozen = 0
            } else {
                // Freeze elapsed when LLM stops — keep time visible
                if let start = tab.taskStartDate {
                    tab._taskElapsedFrozen = Date().timeIntervalSince(start)
                }
            }
        }
        .onChange(of: viewModel.isRunning) { oldValue, newValue in
            // Only fire on actual transitions on the main tab — auto-expand happens in executeTask.
            guard tab == nil, oldValue != newValue else { return }
            if newValue {
                viewModel.mainTaskStartDate = Date()
                viewModel.mainTaskElapsed = 0
            } else {
                // Freeze elapsed when task stops/cancels
                if let start = viewModel.mainTaskStartDate {
                    viewModel.mainTaskElapsed = Date().timeIntervalSince(start)
                }
            }
        }
        .onReceive(refreshTimer) { _ in
            guard isActive else { return }
            tick += 1 // Just refresh UI — elapsed is computed from taskStartDate
            switch dots.count {
            case 0: dots = "."
            case 1: dots = ".."
            case 2: dots = "..."
            default: dots = ""
            }
        }
    }
}

/// Resizable LLM output box — neo-retro terminal look, adapts to dark/light mode.
private struct LLMOutputBox: View {
    @Environment(\.colorScheme) private var colorScheme
    let text: String
    var rawText: String = ""
    @Binding var height: CGFloat
    var isStreaming: Bool = false
    var showDismiss: Bool = false
    var dismissEnabled: Bool = true
    var showScanlines: Bool = true
    var onDismiss: (() -> Void)?
    @State private var cursorVisible = true
    @State private var dragStartHeight: CGFloat = 0
    @State private var blinkEpoch = 0
    @State private var tableHeights: [Int: CGFloat] = [:]

    private var termBg: Color {
        colorScheme == .dark
            ? Color(red: 0.05, green: 0.08, blue: 0.05)
            : Color(red: 0.93, green: 0.97, blue: 0.93)
    }
    private var termText: Color {
        colorScheme == .dark
            ? Color(red: 0.2, green: 0.9, blue: 0.3)
            : Color(red: 0.05, green: 0.35, blue: 0.1)
    }
    private var termDim: Color {
        colorScheme == .dark
            ? Color(red: 0.15, green: 0.4, blue: 0.2)
            : Color(red: 0.3, green: 0.6, blue: 0.35)
    }
    private var termBorder: Color {
        colorScheme == .dark
            ? Color(red: 0.15, green: 0.4, blue: 0.2).opacity(0.5)
            : Color(red: 0.3, green: 0.6, blue: 0.35).opacity(0.4)
    }
    private var handleBg: Color {
        colorScheme == .dark
            ? Color(red: 0.15, green: 0.2, blue: 0.15)
            : Color(red: 0.85, green: 0.92, blue: 0.85)
    }

    private let minHeight: CGFloat = 80

    /// Convert URLs in text to clickable underlined links, keeping the rest in termText color.
    private static func linkify(_ text: String, color: Color) -> AttributedString {
        var result = AttributedString(text)
        result.foregroundColor = color

        let detector = try? NSDataDetector(types: NSTextCheckingResult.CheckingType.link.rawValue)
        let nsText = text as NSString
        let matches = detector?.matches(in: text, range: NSRange(location: 0, length: nsText.length)) ?? []

        for match in matches.reversed() {
            guard let url = match.url,
                  let range = Range(match.range, in: result) else { continue }
            result[range].link = url
            result[range].underlineStyle = .single
        }
        return result
    }

    // MARK: - Terminal Table Rendering

    /// Build an AttributedString that renders markdown tables as box-drawn terminal tables.
    private static func richText(_ text: String, color: Color, dimColor: Color, headerColor: Color) -> AttributedString {
        let lines = text.split(separator: "\n", omittingEmptySubsequences: false).map(String.init)
        var result = AttributedString()
        var i = 0
        while i < lines.count {
            // Detect table block: header | sep | 1+ data rows
            if i + 2 < lines.count,
               lines[i].trimmingCharacters(in: .whitespaces).hasPrefix("|"),
               TableRendering.isTableSeparator(lines[i + 1])
            {
                let tableStart = i
                var tableEnd = i + 2
                while tableEnd < lines.count,
                      lines[tableEnd].trimmingCharacters(in: .whitespaces).hasPrefix("|"),
                      !TableRendering.isTableSeparator(lines[tableEnd])
                {
                    tableEnd += 1
                }
                // Parse all rows
                let headerCells = TableRendering.parseTableRow(lines[tableStart])
                var dataRows: [[String]] = []
                for r in (tableStart + 2)..<tableEnd {
                    dataRows.append(TableRendering.parseTableRow(lines[r]))
                }
                let colCount = headerCells.count
                // Compute column widths
                var widths = headerCells.map(\.count)
                for row in dataRows {
                    for (c, cell) in row.enumerated() where c < colCount {
                        widths[c] = max(widths[c], cell.count)
                    }
                }
                // Ensure minimum width of 3
                widths = widths.map { max($0, 3) }

                // Build box-drawing lines
                let topLine = "┌" + widths.map { String(repeating: "─", count: $0 + 2) }.joined(separator: "┬") + "┐"
                let midLine = "├" + widths.map { String(repeating: "─", count: $0 + 2) }.joined(separator: "┼") + "┤"
                let botLine = "└" + widths.map { String(repeating: "─", count: $0 + 2) }.joined(separator: "┴") + "┘"

                func padCell(_ s: String, width: Int) -> String {
                    s + String(repeating: " ", count: max(0, width - s.count))
                }

                func rowLine(_ cells: [String]) -> String {
                    var parts: [String] = []
                    for (c, cell) in cells.enumerated() where c < colCount {
                        parts.append(" " + padCell(cell, width: widths[c]) + " ")
                    }
                    // Fill missing columns
                    for c in cells.count..<colCount {
                        parts.append(" " + String(repeating: " ", count: widths[c]) + " ")
                    }
                    return "│" + parts.joined(separator: "│") + "│"
                }

                // Add newline before table if not at start
                if !result.characters.isEmpty {
                    result.append(AttributedString("\n"))
                }

                // Top border (dim)
                var border = AttributedString(topLine + "\n")
                border.foregroundColor = dimColor
                result.append(border)

                // Header row (bright/bold)
                var hdr = AttributedString(rowLine(headerCells) + "\n")
                hdr.foregroundColor = headerColor
                result.append(hdr)

                // Mid border (dim)
                var mid = AttributedString(midLine + "\n")
                mid.foregroundColor = dimColor
                result.append(mid)

                // Data rows
                for row in dataRows {
                    var r = AttributedString(rowLine(row) + "\n")
                    r.foregroundColor = color
                    result.append(r)
                }

                // Bottom border (dim)
                var bot = AttributedString(botLine)
                bot.foregroundColor = dimColor
                result.append(bot)

                // Add newline after table unless at end
                if tableEnd < lines.count {
                    result.append(AttributedString("\n"))
                }

                i = tableEnd
                continue
            }

            // Regular line — linkify it
            if i > 0 { result.append(AttributedString("\n")) }
            result.append(linkify(lines[i], color: color))
            i += 1
        }
        return result
    }

    /// Maximum height: leave room for header, status bar, and input area.
    /// IMPORTANT: keyWindow/mainWindow return nil when the app deactivates — must
    /// fall back to our own visible window, NOT NSScreen, otherwise the cap balloons
    /// to screen size and the view fills the entire window on focus loss.
    private var maxHeight: CGFloat {
        let windowH = NSApp.keyWindow?.frame.height
            ?? NSApp.mainWindow?.frame.height
            ?? NSApp.windows.first(where: { $0.isVisible && $0.contentView != nil && !$0.isFloatingPanel })?.frame.height
            ?? 800
        return windowH * 0.50 - 100
    }

    var body: some View {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        // Hide cursor only while actively inside a table (last line starts with |)
        let lastLine = trimmed.components(separatedBy: "\n").last?.trimmingCharacters(in: .whitespaces) ?? ""
        let inTable = lastLine.hasPrefix("|") || lastLine.allSatisfy({ $0 == "-" || $0 == ":" || $0 == "|" || $0 == " " }) && lastLine
            .contains("-")
        let cursor = inTable ? "" : (cursorVisible ? "█" : " ")
        let displayText = trimmed.isEmpty ? "" : trimmed + cursor
        VStack(spacing: 0) {
            ZStack(alignment: .bottomTrailing) {
                if !displayText.isEmpty {
                    LLMOutputTextView(text: displayText, isStreaming: isStreaming) { h in
                        guard dragStartHeight == 0 else { return }
                        // Pre-size from raw stream (ahead of drip) to prevent stutter
                        let lineCount = CGFloat(rawText.components(separatedBy: "\n").count)
                        let lineEstimate = lineCount * 24 + 24
                        // Use the larger of: line-count estimate (pre-size), or actual rendered
                        // height from the package (which accounts for word-wrap and tables).
                        let proposed = min(max(minHeight, max(lineEstimate, h + 4)), maxHeight)
                        height = proposed
                    }
                    .overlay {
                        if showScanlines {
                            ScanlineOverlay(spacing: 2, color: .black, opacity: 0.375, blurRadius: 0.005)
                            ScanlineOverlay(spacing: 4, color: .green, opacity: 0.112, blurRadius: 0.25)
                        }
                    }
                    .frame(height: min(height, maxHeight))
                } else {
                    VStack(spacing: 0) {
                        HStack(spacing: 0) {
                            Text("AGENT! > ")
                                .font(.system(size: 16.5, design: .monospaced))
                                .foregroundColor(termText)
                            Text(cursorVisible ? "█" : " ")
                                .font(.system(size: 16.5, design: .monospaced))
                                .foregroundColor(termText)
                            Spacer()
                        }
                        .padding(10)
                        Spacer(minLength: 0)
                    }
                    .overlay {
                        if showScanlines {
                            ScanlineOverlay(spacing: 2, color: .black, opacity: 0.375, blurRadius: 0.005)
                            ScanlineOverlay(spacing: 4, color: .green, opacity: 0.112, blurRadius: 0.25)
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .topLeading)
                    .frame(height: min(max(height, 40), maxHeight))
                    .onAppear { height = minHeight }
                }

                // Dismiss button — overlaid bottom right, no extra space
                if showDismiss {
                    Button {
                        onDismiss?()
                    } label: {
                        Text("Dismiss")
                            .font(.system(size: 11, design: .monospaced).bold())
                            .foregroundColor(dismissEnabled ? termText : termDim)
                            .padding(.horizontal, 10)
                            .padding(.vertical, 4)
                            .background(termBg.opacity(0.9))
                            .overlay(RoundedRectangle(cornerRadius: 4).stroke(termBorder, lineWidth: 1))
                    }
                    .buttonStyle(.plain)
                    .disabled(!dismissEnabled)
                    .padding(8)
                }
            }

            // Drag handle
            Rectangle()
                .fill(handleBg)
                .frame(height: 6)
                .overlay(
                    RoundedRectangle(cornerRadius: 2)
                        .fill(termDim)
                        .frame(width: 40, height: 3)
                )
                .onHover { inside in
                    if inside { NSCursor.resizeUpDown.push() } else { NSCursor.pop() }
                }
                .contentShape(Rectangle())
                .highPriorityGesture(
                    DragGesture(minimumDistance: 2, coordinateSpace: .global)
                        .onChanged { value in
                            if dragStartHeight == 0 {
                                dragStartHeight = height
                            }
                            height = min(max(40, dragStartHeight + value.translation.height), maxHeight)
                        }
                        .onEnded { _ in dragStartHeight = 0 }
                )
        }
        .background(termBg)
        .cornerRadius(6)
        .overlay(RoundedRectangle(cornerRadius: 6).stroke(termBorder, lineWidth: 1))
        .task {
            // Blink cursor at ~2Hz — always running, seamless streaming→idle
            while !Task.isCancelled {
                try? await Task.sleep(for: .milliseconds(500))
                cursorVisible.toggle()
            }
        }
    }
}

/// CRT scanline overlay — horizontal lines like a 1980s phosphor monitor
private struct ScanlineOverlay: View {
    var spacing: CGFloat = 2.5
    var color: Color = .black
    var opacity: Double = 0.5
    var blurRadius: CGFloat = 0

    var body: some View {
        Canvas { context, size in
            for y in stride(from: 0, to: size.height, by: spacing) {
                let rect = CGRect(x: 0, y: y, width: size.width, height: 1)
                context.fill(Path(rect), with: .color(color.opacity(opacity)))
            }
        }
        .blur(radius: blurRadius)
        .allowsHitTesting(false)
    }
}

private struct ContentHeightKey: PreferenceKey {
    nonisolated(unsafe) static var defaultValue: CGFloat = 40
    static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) {
        value = max(value, nextValue())
    }
}

// MARK: - Tool Steps View

/// Collapsible list of tool invocations for the current task.
///
/// `isExpanded` is a Binding (not local @State) so the disclosure state
/// survives the view being torn down and rebuilt — which happens whenever
/// the user toggles the LLM HUD with Cmd+B, switches tabs, or when steps
/// transition from empty → non-empty between LLM iterations. Without this,
/// every newly arriving step would collapse the list.
struct ToolStepsView: View {
    let steps: [AgentViewModel.ToolStep]
    @Binding var isExpanded: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Button {
                withAnimation(.easeInOut(duration: 0.2)) {
                    isExpanded.toggle()
                }
            } label: {
                HStack(spacing: 4) {
                    Image(systemName: "chevron.right")
                        .font(.system(size: 8, weight: .bold))
                        .rotationEffect(.degrees(isExpanded ? 90 : 0))
                    Text("Steps (\(steps.count))")
                        .font(.caption)
                    Spacer()
                }
                .foregroundStyle(.secondary)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)

            if isExpanded {
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 0) {
                        ForEach(Array(steps.enumerated()), id: \.element.id) { idx, step in
                            HStack(spacing: 6) {
                                Text("\(idx + 1)")
                                    .font(.system(size: 9, design: .monospaced))
                                    .foregroundStyle(.tertiary)
                                    .frame(width: 20, alignment: .trailing)
                                switch step.status {
                                case .running:
                                    ProgressView().controlSize(.mini)
                                case .success:
                                    Image(systemName: "checkmark.circle.fill")
                                        .font(.system(size: 9))
                                        .foregroundStyle(.green)
                                case .error:
                                    Image(systemName: "xmark.circle.fill")
                                        .font(.system(size: 9))
                                        .foregroundStyle(.red)
                                }
                                Text(step.name)
                                    .font(.system(size: 10, weight: .medium, design: .monospaced))
                                    .foregroundStyle(.primary)
                                if !step.detail.isEmpty {
                                    Text(step.detail)
                                        .font(.system(size: 10, design: .monospaced))
                                        .foregroundStyle(.secondary)
                                        .lineLimit(1)
                                        .truncationMode(.middle)
                                }
                                Spacer()
                                if let d = step.duration {
                                    Text(String(format: "%.1fs", d))
                                        .font(.system(size: 9, design: .monospaced))
                                        .foregroundStyle(.tertiary)
                                }
                            }
                            .padding(.vertical, 1)
                        }
                    }
                    .padding(.top, 2)
                }
                .frame(maxHeight: min(CGFloat(steps.count) * 18, 200))
            }
        }
        .padding(.vertical, 4)
    }
}

// MARK: - Split text into plain chunks and table blocks

extension LLMOutputBox {
    struct TextChunk {
        let text: String
        let isTable: Bool
    }

    static func splitByTables(_ text: String) -> [TextChunk] {
        let lines = text.components(separatedBy: "\n")
        var chunks: [TextChunk] = []
        var plainLines: [String] = []
        var i = 0

        while i < lines.count {
            let trimmed = lines[i].trimmingCharacters(in: .whitespaces)
            // Detect table: line starts with |, next line is separator
            if i + 2 < lines.count,
               trimmed.hasPrefix("|"),
               TerminalNeoRenderer.isTableSeparator(lines[i + 1])
            {
                // Flush plain text
                if !plainLines.isEmpty {
                    chunks.append(TextChunk(text: plainLines.joined(separator: "\n"), isTable: false))
                    plainLines = []
                }
                // Collect table lines
                var tableLines: [String] = [lines[i], lines[i + 1]]
                var j = i + 2
                while j < lines.count,
                      lines[j].trimmingCharacters(in: .whitespaces).hasPrefix("|"),
                      !TerminalNeoRenderer.isTableSeparator(lines[j])
                {
                    tableLines.append(lines[j])
                    j += 1
                }
                chunks.append(TextChunk(text: tableLines.joined(separator: "\n"), isTable: true))
                i = j
            } else {
                plainLines.append(lines[i])
                i += 1
            }
        }
        if !plainLines.isEmpty {
            chunks.append(TextChunk(text: plainLines.joined(separator: "\n"), isTable: false))
        }
        return chunks
    }
}

// MARK: - NSTextView wrapper for inline NSTextTable

private struct NSTextViewWrapper: NSViewRepresentable {
    let attributedString: NSAttributedString
    @Binding var measuredHeight: CGFloat

    final class Coordinator {
        var lastLength = 0
    }

    func makeCoordinator() -> Coordinator { Coordinator() }

    func makeNSView(context: Context) -> NSTextView {
        let tv = NSTextView()
        tv.isEditable = false
        tv.isSelectable = true
        tv.drawsBackground = false
        tv.isVerticallyResizable = false
        tv.isHorizontallyResizable = false
        tv.textContainerInset = NSSize(width: 0, height: 4)
        tv.textContainer?.widthTracksTextView = true
        tv.textContainer?.lineFragmentPadding = 0
        return tv
    }

    func updateNSView(_ tv: NSTextView, context: Context) {
        let len = attributedString.length
        guard len != context.coordinator.lastLength else { return }
        context.coordinator.lastLength = len

        tv.textStorage?.setAttributedString(attributedString)
        tv.layoutManager?.ensureLayout(for: tv.textContainer!)
        let h = tv.layoutManager?.usedRect(for: tv.textContainer!).height ?? 40
        let total = h + 8
        DispatchQueue.main.async {
            measuredHeight = total
        }
    }
}
