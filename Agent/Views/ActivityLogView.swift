import SwiftUI
import AppKit
import Combine
import AgentColorSyntax
import AgentTerminalNeo

/// NSTextView-backed activity log — avoids SwiftUI Text layout storms on large/streaming content.
/// Detects image/HTML file paths in log output and shows clickable links that open in Preview/Browser.
/// Optimized for smooth streaming with incremental updates and debouncing.
struct ActivityLogView: NSViewRepresentable {
    @Environment(\.colorScheme) private var colorScheme
    let text: String
    var tabID: UUID?  // nil = main tab
    var isActive: Bool = false  // true when tab/task is running — skip truncation
    var textProvider: (@MainActor () -> String)? = nil  // polled for live updates
    var searchText: String = ""
    var caseSensitive: Bool = false
    var currentMatchIndex: Int = 0
    var onMatchCount: ((Int) -> Void)? = nil

    func makeNSView(context: Context) -> NSScrollView {
        let scrollView = NSTextView.scrollableTextView()
        guard let textView = scrollView.documentView as? NSTextView else { return scrollView }
        textView.isEditable = false
        textView.isSelectable = true
        textView.font = .monospacedSystemFont(ofSize: NSFont.systemFontSize, weight: .regular)
        textView.backgroundColor = .clear
        textView.textContainerInset = NSSize(width: 12, height: 12)
        textView.isAutomaticQuoteSubstitutionEnabled = false
        textView.isAutomaticDashSubstitutionEnabled = false
        textView.isAutomaticTextReplacementEnabled = false
        scrollView.hasVerticalScroller = true
        scrollView.autohidesScrollers = true
        scrollView.scrollerStyle = .overlay
        scrollView.drawsBackground = false
        // Improve text rendering performance
        textView.usesFontPanel = false
        textView.usesRuler = false
        textView.isRichText = true
        textView.allowsUndo = false
        // Enable link detection and clicking
        textView.isAutomaticLinkDetectionEnabled = false
        textView.delegate = context.coordinator
        context.coordinator.startObservingScroll(scrollView)
        context.coordinator.latestTextView = textView
        context.coordinator.latestScrollView = scrollView
        context.coordinator.textProvider = textProvider
        context.coordinator.latestTabID = tabID
        context.coordinator.startObservingLogChanges()
        return scrollView
    }

    func updateNSView(_ scrollView: NSScrollView, context: Context) {
        let coord = context.coordinator
        coord.textProvider = textProvider
        let len = (text as NSString).length
        let tabChanged = tabID != coord.latestTabID
        let textChanged = len != coord.updateNSViewLastLength
        let searchChanged = searchText != coord.latestSearchText || currentMatchIndex != coord.latestMatchIndex || caseSensitive != coord.latestCaseSensitive
        guard tabChanged || textChanged || searchChanged else { return }

        coord.latestText = text
        coord.updateNSViewLastLength = len
        coord.latestTabID = tabID
        if tabChanged {
            coord.forceTabSwitch = true
            // Hide, snap at 100ms, fade in
            coord.latestTextView?.alphaValue = 0
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.1) { [weak coord] in
                guard let coord, let tv = coord.latestTextView else { return }
                coord.snapToEnd(tv)
                NSAnimationContext.runAnimationGroup { ctx in
                    ctx.duration = 0.2
                    tv.animator().alphaValue = 1
                }
            }
        }
        coord.latestSearchText = searchText
        coord.latestCaseSensitive = caseSensitive
        coord.isActive = isActive
        coord.latestMatchIndex = currentMatchIndex
        coord.latestMatchCallback = onMatchCount
        coord.scheduleRender()
    }

    func makeCoordinator() -> Coordinator { Coordinator() }

    @MainActor class Coordinator: NSObject, NSTextViewDelegate, @unchecked Sendable {
        let renderSubject = PassthroughSubject<Void, Never>()
        var renderCancellable: AnyCancellable?
        var lastRenderTime: CFAbsoluteTime = 0
        var pendingRenderWork: DispatchWorkItem?
        var scheduledRenderWork: DispatchWorkItem?
        var lastLength = 0
        var showingPlaceholder = true
        var lastSearch = ""
        var lastMatchIndex = -1
        nonisolated(unsafe) let font = NSFont.monospacedSystemFont(ofSize: NSFont.systemFontSize, weight: .regular)
        /// Latest state from updateNSView
        var latestText = ""
        var latestSearchText = ""
        var latestCaseSensitive = false
        nonisolated(unsafe) var isActive = false
        var latestMatchIndex = 0
        var latestMatchCallback: ((Int) -> Void)?
        /// Weak reference to text view for debounced search callbacks
        weak var latestTextView: NSTextView?
        weak var latestScrollView: NSScrollView?
        var latestTabID: UUID?
        /// Track the last fully rendered text for incremental updates
        var lastRenderedText = ""
        /// Track which tab we last rendered — forces full rebuild on tab switch
        var lastTabID: UUID?
        /// Set by updateNSView when tab changes — consumed by performRender
        var forceTabSwitch = false
        /// Separate length tracker for updateNSView dedup (independent of performRender's lastLength)
        var updateNSViewLastLength = 0
        /// Polls the text source directly — bypasses SwiftUI observation
        var textProvider: (@MainActor () -> String)?
        /// Notification observer for activityLog changes
        nonisolated(unsafe) var logObserver: NSObjectProtocol?

        override init() {
            super.init()
            renderCancellable = renderSubject
                .debounce(for: .milliseconds(100), scheduler: DispatchQueue.main)
                .sink { [weak self] in
                    self?.performRender()
                }
        }

        /// React to activityLog changes — no polling, instant response
        func startObservingLogChanges() {
            guard logObserver == nil else { return }
            logObserver = NotificationCenter.default.addObserver(
                forName: .activityLogDidChange,
                object: nil,
                queue: .main
            ) { [weak self] notification in
                let notifTabID = notification.object as? UUID
                MainActor.assumeIsolated {
                    guard let self, let provider = self.textProvider else { return }
                    guard notifTabID == self.latestTabID else { return }
                    self.latestText = provider()
                    self.performRender()
                }
            }
        }

        /// Schedule rendering AFTER SwiftUI's layout pass completes.
        /// If already scheduled, marks dirty so a follow-up render fires.
        private var renderDirty = false
        func scheduleRender() {
            if scheduledRenderWork != nil {
                renderDirty = true
                return
            }
            let work = DispatchWorkItem { [weak self] in
                guard let self else { return }
                self.scheduledRenderWork = nil
                self.performRender()
                // If new data arrived during render, schedule again
                if self.renderDirty {
                    self.renderDirty = false
                    self.scheduleRender()
                }
            }
            scheduledRenderWork = work
            DispatchQueue.main.async(execute: work)
        }

        /// All rendering logic — runs on main thread but OUTSIDE SwiftUI's layout pass
        func performRender() {
            guard let textView = latestTextView, let scrollView = latestScrollView else {

                return
            }
            let text = latestText
            let searchText = latestSearchText
            let caseSensitive = latestCaseSensitive
            let currentMatchIndex = latestMatchIndex
            let onMatchCount = latestMatchCallback
            let tabID = latestTabID

            if text.isEmpty {
                guard !showingPlaceholder else { return }
                textView.alphaValue = 0
                NSAnimationContext.runAnimationGroup { ctx in
                    ctx.duration = 0.3
                    textView.animator().alphaValue = 1
                }
                textView.textStorage?.setAttributedString(
                    NSAttributedString(string: "Ready. Enter a task below to begin.",
                                       attributes: [.font: font, .foregroundColor: NSColor.secondaryLabelColor])
                )
                showingPlaceholder = true
                lastLength = 0
                lastSearch = ""
                lastMatchIndex = -1
                clearCache()
                if let tabID { invalidateCache(for: tabID) }
                onMatchCount?(0)
                return
            }

            let len = (text as NSString).length
            let searchChanged = searchText != lastSearch || currentMatchIndex != lastMatchIndex
            let tabSwitched = forceTabSwitch || tabID != lastTabID
            forceTabSwitch = false

            let currentAppearance = scrollView.effectiveAppearance.bestMatch(from: [.darkAqua, .aqua])
            let appearanceChanged = currentAppearance != lastAppearanceName
            if appearanceChanged {
                lastAppearanceName = currentAppearance
                lastLength = 0
                lastRenderedText = ""
                clearCache()
                invalidateAllCaches()
                CodeBlockTheme.updateAppearance()
                TerminalNeoTheme.updateAppearance()
            }

            guard len != lastLength || showingPlaceholder || searchChanged || tabSwitched || appearanceChanged else { return }

            let textChanged = len != lastLength || showingPlaceholder
            let textGrew = len > lastLength
            let searchCleared = searchText.isEmpty && !lastSearch.isEmpty
            showingPlaceholder = false

            if tabSwitched {
                if let storage = textView.textStorage, lastLength > 0, !lastRenderedText.isEmpty {
                    cacheAttributedString(NSAttributedString(attributedString: storage), for: lastTabID, text: lastRenderedText)
                }
                lastTabID = tabID
                clearCache()
                // Reset lastLength to 0 so the textChanged path treats this as fresh content
                lastLength = 0
                lastRenderedText = ""
                userIsAtBottom = true
                // Fall through to textChanged path — same scroll behavior as first load
            }

            if textChanged || searchCleared {
                let isAppending = len > lastLength && lastLength > 0 && !searchCleared

                if isAppending {
                    let prevLen = lastLength
                    let newText = (text as NSString).substring(from: prevLen)
                    // Auto-scroll to bottom when a new task starts
                    if newText.contains(AgentViewModel.newTaskMarker) {
                        userIsAtBottom = true
                    }
                    let newLines = newText.components(separatedBy: "\n")
                    let hasTableLines = newLines.contains { $0.trimmingCharacters(in: .whitespaces).hasPrefix("|") }
                    let prevTail = (text as NSString).substring(to: prevLen).components(separatedBy: "\n").suffix(3)
                    let prevHasTableLines = prevTail.contains { $0.trimmingCharacters(in: .whitespaces).hasPrefix("|") }

                    if hasTableLines || prevHasTableLines {
                        let savedOrigin = scrollView.contentView.bounds.origin
                        let wasAtBottom = isNearBottom(textView)
                        let t0 = CFAbsoluteTimeGetCurrent()
                        textView.textStorage?.beginEditing()
                        textView.textStorage?.setAttributedString(buildAttributedString(from: text, cap: renderCap))
                        textView.textStorage?.endEditing()
                        adaptRenderCap(elapsed: CFAbsoluteTimeGetCurrent() - t0)
                        lastLength = len
                        lastRenderedText = text
                        if !wasAtBottom {
                            scrollView.contentView.scroll(to: savedOrigin)
                            scrollView.reflectScrolledClipView(scrollView.contentView)
                        }
                    } else {
                        // Freeze scroll position during text mutation to prevent tearing
                        let wasAtBottom = userIsAtBottom
                        let savedY = scrollView.contentView.bounds.origin.y

                        CATransaction.begin()
                        CATransaction.setDisableActions(true)
                        textView.textStorage?.beginEditing()
                        textView.textStorage?.append(renderMarkdownOnly(newText))
                        // Trim from top if textStorage exceeds adaptive cap
                        if let storage = textView.textStorage, storage.length > renderCap {
                            let trim = storage.length - renderCap
                            let snapRange = NSRange(location: trim, length: min(200, storage.length - trim))
                            let snippet = storage.string as NSString
                            let nlRange = snippet.range(of: "\n", range: snapRange)
                            let cutPoint = nlRange.location != NSNotFound ? nlRange.location + 1 : trim
                            storage.deleteCharacters(in: NSRange(location: 0, length: cutPoint))
                            let banner = NSAttributedString(string: "··· earlier output trimmed ···\n\n", attributes: [
                                .font: font,
                                .foregroundColor: NSColor.secondaryLabelColor
                            ])
                            storage.insert(banner, at: 0)
                        }
                        textView.textStorage?.endEditing()
                        CATransaction.commit()

                        // Restore scroll position if user was NOT at bottom
                        if !wasAtBottom {
                            isProgrammaticScroll = true
                            scrollView.contentView.scroll(to: NSPoint(x: 0, y: savedY))
                            scrollView.reflectScrolledClipView(scrollView.contentView)
                            isProgrammaticScroll = false
                        }

                        lastLength = len
                        lastRenderedText = text
                    }
                } else {
                    let savedOrigin = scrollView.contentView.bounds.origin
                    let wasAtBottom = tabSwitched || isNearBottom(textView)
                    // Try instant swap from cached TextStorage (no re-layout)
                    if tabSwitched, swapToCachedStorage(for: tabID, text: text, textView: textView, scrollView: scrollView) {
                        // Cache hit — layout preserved, scroll restored
                    } else {
                        let t0 = CFAbsoluteTimeGetCurrent()
                        textView.textStorage?.beginEditing()
                        textView.textStorage?.setAttributedString(buildAttributedString(from: text, cap: renderCap))
                        textView.textStorage?.endEditing()
                        adaptRenderCap(elapsed: CFAbsoluteTimeGetCurrent() - t0)
                        if !wasAtBottom {
                            scrollView.contentView.scroll(to: savedOrigin)
                            scrollView.reflectScrolledClipView(scrollView.contentView)
                        }
                    }
                    lastLength = len
                    lastRenderedText = text
                }
            }

            if !searchText.isEmpty || !lastSearch.isEmpty {
                if searchChanged {
                    pendingRenderWork?.cancel()
                    applySearchHighlighting(textView: textView, searchText: searchText, caseSensitive: caseSensitive, currentMatch: currentMatchIndex, onMatchCount: onMatchCount)
                } else if textChanged && !searchText.isEmpty {
                    pendingRenderWork?.cancel()
                    let work = DispatchWorkItem { [weak self] in
                        guard let self, let tv = self.latestTextView else { return }
                        self.applySearchHighlighting(textView: tv, searchText: self.latestSearchText, caseSensitive: self.latestCaseSensitive, currentMatch: self.latestMatchIndex, onMatchCount: self.latestMatchCallback)
                    }
                    pendingRenderWork = work
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.3, execute: work)
                }
            }
            lastSearch = searchText
            lastMatchIndex = currentMatchIndex

            if textGrew {
                throttledScrollToEnd(textView)
            }
        }
        /// Throttle scrollToEnd to avoid hyper-scrolling during fast streaming
        var lastScrollTime: CFAbsoluteTime = 0
        var pendingScrollWork: DispatchWorkItem?
        /// Minimum time between full renders during streaming (ms)
        private static let minRenderInterval: CFAbsoluteTime = 50

        /// Tracks whether user is at/near bottom — updated continuously via scroll notifications
        var userIsAtBottom = true
        /// Suppresses scroll tracking during programmatic scrolls
        var isProgrammaticScroll = false
        /// Observation token for scroll notifications
        nonisolated(unsafe) var scrollObserver: NSObjectProtocol?
        /// Last known appearance name — used to detect light/dark mode changes
        var lastAppearanceName: NSAppearance.Name?

        /// Start observing scroll position changes and appearance changes
        private var scrollThrottled = false

        func startObservingScroll(_ scrollView: NSScrollView) {
            scrollView.contentView.postsBoundsChangedNotifications = true
            scrollObserver = NotificationCenter.default.addObserver(
                forName: NSView.boundsDidChangeNotification,
                object: scrollView.contentView,
                queue: .main
            ) { [weak self, weak scrollView] _ in
                MainActor.assumeIsolated {
                    guard let self, !self.scrollThrottled, let scrollView else { return }
                    guard !self.isProgrammaticScroll else { return }
                    guard let textView = scrollView.documentView as? NSTextView else { return }
                    self.userIsAtBottom = self.isNearBottom(textView)
                    self.scrollThrottled = true
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) { [weak self] in
                        self?.scrollThrottled = false
                    }
                }
            }
            lastAppearanceName = NSApp.effectiveAppearance.bestMatch(from: [.darkAqua, .aqua])
        }

        deinit {
            if let observer = scrollObserver {
                NotificationCenter.default.removeObserver(observer)
            }
            if let observer = logObserver {
                NotificationCenter.default.removeObserver(observer)
            }
        }

        /// Check if scroll view is near the bottom
        func isNearBottom(_ textView: NSTextView) -> Bool {
            guard let scrollView = textView.enclosingScrollView else { return true }
            let visibleBottom = scrollView.contentView.bounds.origin.y + scrollView.contentView.bounds.height
            let contentHeight = textView.frame.height
            return (contentHeight - visibleBottom) < 300
        }

        /// Instant scroll to end — no animation
        func snapToEnd(_ textView: NSTextView) {
            guard let scrollView = textView.enclosingScrollView,
                  let textContainer = textView.textContainer else {
                textView.scrollToEndOfDocument(nil)
                return
            }
            isProgrammaticScroll = true
            textView.layoutManager?.ensureLayout(for: textContainer)
            textView.scrollToEndOfDocument(nil)
            scrollView.reflectScrolledClipView(scrollView.contentView)
            isProgrammaticScroll = false
            userIsAtBottom = true
        }

        /// Smooth animated scroll to end
        func smoothScrollToEnd(_ textView: NSTextView) {
            guard let scrollView = textView.enclosingScrollView,
                  let textContainer = textView.textContainer else {
                textView.scrollToEndOfDocument(nil)
                return
            }
            textView.layoutManager?.ensureLayout(for: textContainer)
            let contentHeight = textView.frame.height
            let clipHeight = scrollView.contentView.bounds.height
            let targetY = max(0, contentHeight - clipHeight)
            isProgrammaticScroll = true
            NSAnimationContext.runAnimationGroup { ctx in
                ctx.duration = 0.25
                ctx.timingFunction = CAMediaTimingFunction(name: .easeInEaseOut)
                scrollView.contentView.animator().setBoundsOrigin(NSPoint(x: 0, y: targetY))
            } completionHandler: {
                MainActor.assumeIsolated { [weak self] in
                    textView.scrollToEndOfDocument(nil)
                    scrollView.reflectScrolledClipView(scrollView.contentView)
                    self?.isProgrammaticScroll = false
                }
            }
        }

        /// Throttled scroll — at most once per 0.1s, skipped if user scrolled away from bottom.
        /// Uses snap (no animation) to avoid fighting with layout during streaming.
        func throttledScrollToEnd(_ textView: NSTextView) {
            guard userIsAtBottom else { return }
            let now = CFAbsoluteTimeGetCurrent()
            let interval: CFAbsoluteTime = 0.1
            pendingScrollWork?.cancel()
            if now - lastScrollTime >= interval {
                lastScrollTime = now
                snapToEnd(textView)
            } else {
                let work = DispatchWorkItem { [weak self, weak textView] in
                    guard let self, let textView else { return }
                    guard self.userIsAtBottom else { return }
                    self.lastScrollTime = CFAbsoluteTimeGetCurrent()
                    self.snapToEnd(textView)
                }
                pendingScrollWork = work
                DispatchQueue.main.asyncAfter(deadline: .now() + interval, execute: work)
            }
        }

        /// Track previous search highlight ranges so we can remove only those
        var lastSearchRanges: [NSRange] = []
        /// Saved original foreground colors per highlighted range so we can restore them
        var savedForegroundColors: [(range: NSRange, color: NSColor?)] = []
        /// Debounce timer for search highlighting during streaming
        var pendingSearchWork: DispatchWorkItem?

        /// Highlight search matches in the text view's text storage
        func applySearchHighlighting(textView: NSTextView, searchText: String, caseSensitive: Bool = false, currentMatch: Int, onMatchCount: ((Int) -> Void)?) {
            guard let storage = textView.textStorage else { return }

            let highlightColor = NSColor.systemYellow.withAlphaComponent(0.3)
            let currentColor = NSColor.systemYellow.withAlphaComponent(0.8)

            // Batch all attribute changes in a single editing pass
            storage.beginEditing()

            // Remove previous highlights and restore original foreground colors
            for range in lastSearchRanges {
                if range.location + range.length <= storage.length {
                    storage.removeAttribute(.backgroundColor, range: range)
                }
            }
            for entry in savedForegroundColors {
                if entry.range.location + entry.range.length <= storage.length {
                    if let color = entry.color {
                        storage.addAttribute(.foregroundColor, value: color, range: entry.range)
                    } else {
                        storage.removeAttribute(.foregroundColor, range: entry.range)
                    }
                }
            }
            savedForegroundColors.removeAll()
            lastSearchRanges.removeAll()

            guard !searchText.isEmpty else {
                storage.endEditing()
                onMatchCount?(0)
                return
            }

            // Search only the visible portion + buffer for large texts to avoid beach ball
            let text = storage.string
            let textLength = (text as NSString).length
            let searchNeedle = caseSensitive ? searchText : searchText.lowercased()

            // For very large texts, limit search to last 60K chars (matches render cap)
            let maxSearchChars = 60_000
            let searchStart = textLength > maxSearchChars ? textLength - maxSearchChars : 0
            let searchableText = caseSensitive ? text as NSString : text.lowercased() as NSString

            var matchRanges: [NSRange] = []
            var searchRange = NSRange(location: searchStart, length: textLength - searchStart)
            while searchRange.location < textLength {
                let found = searchableText.range(of: searchNeedle, options: [], range: searchRange)
                guard found.location != NSNotFound else { break }
                matchRanges.append(found)
                searchRange.location = found.location + found.length
                searchRange.length = textLength - searchRange.location
            }

            onMatchCount?(matchRanges.count)
            lastSearchRanges = matchRanges

            for (i, range) in matchRanges.enumerated() {
                let color = (i == currentMatch) ? currentColor : highlightColor
                // Save original foreground color for this range so we can restore it later
                let originalFg = storage.attribute(.foregroundColor, at: range.location, effectiveRange: nil) as? NSColor
                savedForegroundColors.append((range: range, color: originalFg))
                storage.addAttribute(.backgroundColor, value: color, range: range)
                storage.addAttribute(.foregroundColor, value: NSColor.black, range: range)
            }

            storage.endEditing()

            // Scroll to current match
            if !matchRanges.isEmpty, currentMatch < matchRanges.count {
                let targetRange = matchRanges[currentMatch]
                textView.scrollRangeToVisible(targetRange)
                textView.showFindIndicator(for: targetRange)
            }
        }

        // Matches image files
        private static let imagePathPattern: NSRegularExpression? = try? NSRegularExpression(
            pattern: #"(/[^\n"'<>]+\.(?:jpg|jpeg|png|gif|tiff|bmp|webp|heic|ico|icon))"#,
            options: .caseInsensitive
        )
        // Matches HTML files
        private static let htmlPathPattern: NSRegularExpression? = try? NSRegularExpression(
            pattern: #"(/[^\n"'<>]+\.html?)"#,
            options: .caseInsensitive
        )

        private static let fencePattern: NSRegularExpression? = try? NSRegularExpression(
            pattern: #"```(\w*)\n([\s\S]*?)\n```(?=\n|$)"#, options: []
        )

        private static let headerPattern: NSRegularExpression? = try? NSRegularExpression(
            pattern: #"^(#{1,6})\s+(.*)"#, options: []
        )
        private static let bulletPattern: NSRegularExpression? = try? NSRegularExpression(
            pattern: #"^(\s*)[-*+]\s+(.*)"#, options: []
        )
        private static let hrPattern: NSRegularExpression? = try? NSRegularExpression(
            pattern: #"^\s*([-*_]\s*){3,}$"#, options: []
        )
        private static let blockquotePattern: NSRegularExpression? = try? NSRegularExpression(
            pattern: #"^>\s?(.*)"#, options: []
        )

        /// Strip ANSI escape sequences so they don't appear as garbage
        private static let ansiEscapePattern: NSRegularExpression? = try? NSRegularExpression(
            pattern: #"\x1B\[[0-9;]*[A-Za-z]"#, options: []
        )

        /// Fast render for incremental text updates - detects image/HTML paths and creates clickable links
        nonisolated func renderMarkdownOnly(_ text: String) -> NSAttributedString {
            // Check for image or HTML file paths in this chunk
            let nsText = text as NSString
            let fullRange = NSRange(location: 0, length: nsText.length)
            let imageMatches = MarkdownPatterns.imagePathPattern?.matches(in: text, range: fullRange) ?? []
            let htmlMatches = MarkdownPatterns.htmlPathPattern?.matches(in: text, range: fullRange) ?? []

            guard !imageMatches.isEmpty || !htmlMatches.isEmpty else {
                return renderMarkdown(text)
            }

            // Same logic as buildAttributedString for path-to-link conversion
            struct FileMatch {
                let range: NSRange
                let path: String
                let isHTML: Bool
            }
            let fm = FileManager.default
            var allMatches: [FileMatch] = []
            for m in imageMatches {
                let r = m.range(at: 1)
                let p = nsText.substring(with: r)
                if fm.fileExists(atPath: p) {
                    allMatches.append(FileMatch(range: r, path: p, isHTML: false))
                }
            }
            for m in htmlMatches {
                let r = m.range(at: 1)
                let p = nsText.substring(with: r)
                if fm.fileExists(atPath: p) {
                    allMatches.append(FileMatch(range: r, path: p, isHTML: true))
                }
            }
            allMatches.sort { $0.range.location < $1.range.location }

            guard !allMatches.isEmpty else { return renderMarkdown(text) }

            let baseAttrs: [NSAttributedString.Key: Any] = [
                .font: font,
                .foregroundColor: NSColor.labelColor
            ]
            let result = NSMutableAttributedString()
            var lastEnd = 0

            for match in allMatches {
                if match.range.location > lastEnd {
                    let beforeRange = NSRange(location: lastEnd, length: match.range.location - lastEnd)
                    let beforeText = nsText.substring(with: beforeRange)
                    result.append(renderMarkdown(beforeText))
                }

                let path = match.path
                let linkText = match.isHTML ? "📄 \(path)" : "🖼️ \(path)"
                let linkAttrs: [NSAttributedString.Key: Any] = [
                    .font: font,
                    .foregroundColor: NSColor.linkColor,
                    .link: URL(fileURLWithPath: path).absoluteString,
                    .underlineStyle: NSUnderlineStyle.single.rawValue
                ]
                result.append(NSAttributedString(string: linkText, attributes: linkAttrs))
                result.append(NSAttributedString(string: "\n", attributes: baseAttrs))
                lastEnd = match.range.location + match.range.length
            }

            if lastEnd < nsText.length {
                let remainingRange = NSRange(location: lastEnd, length: nsText.length - lastEnd)
                result.append(renderMarkdown(nsText.substring(with: remainingRange)))
            }

            return result
        }

        // MARK: - Beach Ball Predictor
        /// Tracks render time to dynamically adjust truncation threshold.
        /// Starts generous (200K), shrinks when renders get slow, grows when fast.
        var renderCap = 500_000
        /// Threshold in seconds — if a render takes longer, shrink the cap
        private static let beachBallThreshold: CFAbsoluteTime = 0.08  // 80ms

        /// Call after a render completes to adapt the cap for next time.
        func adaptRenderCap(elapsed: CFAbsoluteTime) {
            if elapsed > Self.beachBallThreshold {
                // Slow render — shrink cap by 25%, floor at 100K
                renderCap = max(100_000, renderCap * 3 / 4)
            } else if elapsed < Self.beachBallThreshold / 2, renderCap < 1_000_000 {
                // Fast render — grow cap by 10%, ceiling at 1M
                renderCap = min(1_000_000, renderCap * 11 / 10)
            }
        }

        /// Build attributed string from text. Converts image/HTML paths to clickable links.
        nonisolated func buildAttributedString(from text: String, cap: Int = 500_000) -> NSAttributedString {
            let baseAttrs: [NSAttributedString.Key: Any] = [
                .font: font,
                .foregroundColor: NSColor.labelColor
            ]

            // Truncate from the front if text exceeds adaptive render cap
            var renderText = text
            var wasTruncated = false
            if renderText.count > cap {
                let drop = renderText.count - cap
                renderText = String(renderText.dropFirst(drop))
                // Snap to next newline so we don't start mid-line
                if let nl = renderText.firstIndex(of: "\n") {
                    renderText = String(renderText[renderText.index(after: nl)...])
                }
                wasTruncated = true
            }

            // Strip ANSI escape codes from the text
            let cleanText: String
            if let rx = MarkdownPatterns.ansiEscapePattern {
                cleanText = rx.stringByReplacingMatches(in: renderText, range: NSRange(location: 0, length: (renderText as NSString).length), withTemplate: "")
            } else {
                cleanText = renderText
            }

            let nsText = cleanText as NSString
            let fullRange = NSRange(location: 0, length: nsText.length)
            let imageMatches = MarkdownPatterns.imagePathPattern?.matches(in: cleanText, range: fullRange) ?? []
            let htmlMatches = MarkdownPatterns.htmlPathPattern?.matches(in: cleanText, range: fullRange) ?? []

            // Build truncation banner if needed
            let truncationBanner: NSAttributedString? = wasTruncated ? {
                let bannerAttrs: [NSAttributedString.Key: Any] = [
                    .font: NSFont.monospacedSystemFont(ofSize: font.pointSize - 1, weight: .medium),
                    .foregroundColor: NSColor.secondaryLabelColor,
                    .backgroundColor: NSColor.systemYellow.withAlphaComponent(0.15)
                ]
                return NSAttributedString(string: "--- Log truncated (showing last 50K characters) ---\n", attributes: bannerAttrs)
            }() : nil

            guard !imageMatches.isEmpty || !htmlMatches.isEmpty else {
                let rendered = renderMarkdown(cleanText)
                guard let banner = truncationBanner else { return rendered }
                let combined = NSMutableAttributedString(attributedString: banner)
                combined.append(rendered)
                return combined
            }

            // Merge all matches sorted by location
            struct FileMatch {
                let range: NSRange
                let path: String
                let isHTML: Bool
            }
            let fm = FileManager.default
            var allMatches: [FileMatch] = []
            for m in imageMatches {
                let r = m.range(at: 1)
                let p = nsText.substring(with: r)
                if fm.fileExists(atPath: p) {
                    allMatches.append(FileMatch(range: r, path: p, isHTML: false))
                }
            }
            for m in htmlMatches {
                let r = m.range(at: 1)
                let p = nsText.substring(with: r)
                if fm.fileExists(atPath: p) {
                    allMatches.append(FileMatch(range: r, path: p, isHTML: true))
                }
            }
            allMatches.sort { $0.range.location < $1.range.location }

            guard !allMatches.isEmpty else {
                let rendered = renderMarkdown(cleanText)
                guard let banner = truncationBanner else { return rendered }
                let combined = NSMutableAttributedString(attributedString: banner)
                combined.append(rendered)
                return combined
            }

            let result = NSMutableAttributedString()
            var lastEnd = 0

            for match in allMatches {
                // Add text before this match
                if match.range.location > lastEnd {
                    let beforeRange = NSRange(location: lastEnd, length: match.range.location - lastEnd)
                    let beforeText = nsText.substring(with: beforeRange)
                    result.append(renderMarkdown(beforeText))
                }

                // Add the path as a clickable link
                let path = match.path
                let linkText = match.isHTML ? "📄 \(path)" : "🖼️ \(path)"
                let linkAttrs: [NSAttributedString.Key: Any] = [
                    .font: font,
                    .foregroundColor: NSColor.linkColor,
                    .link: URL(fileURLWithPath: path).absoluteString,
                    .underlineStyle: NSUnderlineStyle.single.rawValue
                ]
                result.append(NSAttributedString(string: linkText, attributes: linkAttrs))
                result.append(NSAttributedString(string: "\n", attributes: baseAttrs))
                lastEnd = match.range.location + match.range.length
            }

            // Add remaining text after last match
            if lastEnd < nsText.length {
                let remainingRange = NSRange(location: lastEnd, length: nsText.length - lastEnd)
                result.append(renderMarkdown(nsText.substring(with: remainingRange)))
            }

            if let banner = truncationBanner {
                let combined = NSMutableAttributedString(attributedString: banner)
                combined.append(result)
                return combined
            }
            return result
        }

        nonisolated private func renderMarkdown(_ text: String) -> NSAttributedString {
            let baseAttrs: [NSAttributedString.Key: Any] = [
                .font: font,
                .foregroundColor: NSColor.labelColor
            ]

            // Check if the text is read_file output (strictly matches "NN |" at the start of lines)
            // This check MUST come before markdown processing to preserve backticks in code
            let readFilePattern = #"^\s*\d+\s*\|\s"#
            let lines = text.components(separatedBy: "\n").filter { !$0.isEmpty }
            let isReadFileOutput = !lines.isEmpty
                && lines.allSatisfy { line in
                    line.range(of: readFilePattern, options: .regularExpression) != nil
                }

            if isReadFileOutput {
                let hl = CodeBlockHighlighter.highlight(code: text, language: "swift", font: font)
                let block = NSMutableAttributedString(attributedString: hl)
                block.addAttribute(.backgroundColor, value: CodeBlockTheme.bg,
                                   range: NSRange(location: 0, length: block.length))
                return block
            }

            // Detect source code output (e.g. from cat command) — look for Swift/code patterns
            // Skip this heuristic if text contains markdown indicators (headers, fences, bullets)
            // to avoid treating markdown summaries with embedded code as raw code output
            let hasMarkdownStructure = lines.contains { line in
                let t = line.trimmingCharacters(in: .whitespaces)
                return t.hasPrefix("#") || t.hasPrefix("```") || t.hasPrefix("- ") || t.hasPrefix("* ")
            }
            let codeIndicators = ["import ", "func ", "class ", "struct ", "enum ", "protocol ", "@MainActor", "@Observable", "let ", "var ", "private ", "public ", "extension "]
            let codeLineCount = lines.filter { line in codeIndicators.contains(where: { line.trimmingCharacters(in: .whitespaces).hasPrefix($0) }) }.count
            let isCodeOutput = !hasMarkdownStructure && lines.count >= 3 && codeLineCount >= 2

            if isCodeOutput {
                let hl = CodeBlockHighlighter.highlight(code: text, language: "swift", font: font)
                let block = NSMutableAttributedString(attributedString: hl)
                block.addAttribute(.backgroundColor, value: CodeBlockTheme.bg,
                                   range: NSRange(location: 0, length: block.length))
                return block
            }

            // Handle code fences (```lang\n...\n```) first
            guard let fenceRx = MarkdownPatterns.fencePattern else { return NSAttributedString(string: text, attributes: baseAttrs) }
            let nsText = text as NSString
            let fullRange = NSRange(location: 0, length: nsText.length)
            let fences = fenceRx.matches(in: text, range: fullRange)

            guard !fences.isEmpty else {
                return renderInlineMarkdown(text)
            }

            let result = NSMutableAttributedString()
            var cursor = 0

            for fence in fences {
                if fence.range.location > cursor {
                    let seg = nsText.substring(with: NSRange(location: cursor, length: fence.range.location - cursor))
                    result.append(renderInlineMarkdown(seg))
                }

                let lang = fence.range(at: 1).length > 0 ? nsText.substring(with: fence.range(at: 1)) : nil
                var code = nsText.substring(with: fence.range(at: 2))
                if code.hasSuffix("\n") { code = String(code.dropLast()) }

                // Copy button only for actual source code blocks (not shell output or file reads)
                let shellLangs: Set<String> = ["bash", "sh", "zsh", "shell", "console", "terminal"]
                let firstLine = code.components(separatedBy: "\n").first ?? ""
                let looksLikeNumberedOutput = firstLine.range(of: #"^\s*\d+\s+"#, options: .regularExpression) != nil
                let isSourceCode = (lang.map { !shellLangs.contains($0.lowercased()) } ?? false) && !looksLikeNumberedOutput
                if isSourceCode {
                    let attach = NSTextAttachment()
                    attach.attachmentCell = MainActor.assumeIsolated { CopyButtonCell(codeText: code) }
                    let rightPara = NSMutableParagraphStyle()
                    rightPara.alignment = .right
                    let copyStr = NSMutableAttributedString(attachment: attach)
                    copyStr.addAttribute(.paragraphStyle, value: rightPara, range: NSRange(location: 0, length: copyStr.length))
                    result.append(copyStr)
                }

                // Syntax-highlighted code with background
                let hl = CodeBlockHighlighter.highlight(code: code, language: lang, font: font)
                let block = NSMutableAttributedString(string: "\n", attributes: baseAttrs)
                block.append(hl)
                block.append(NSAttributedString(string: "\n", attributes: baseAttrs))
                block.addAttribute(.backgroundColor, value: CodeBlockTheme.bg,
                                   range: NSRange(location: 0, length: block.length))
                result.append(block)

                cursor = fence.range.location + fence.range.length
            }

            if cursor < nsText.length {
                result.append(renderInlineMarkdown(nsText.substring(with: NSRange(location: cursor, length: nsText.length - cursor))))
            }

            return result
        }

        /// Splits text into lines and renders block-level markdown (headers, lists, rules, tables)
        /// then delegates inline rendering (bold, italic, code) per line.
        nonisolated private func renderInlineMarkdown(_ text: String) -> NSAttributedString {
            guard !text.isEmpty else { return NSAttributedString() }

            let result = NSMutableAttributedString()
            let lines = text.components(separatedBy: "\n")
            var i = 0

            while i < lines.count {
                // Detect markdown table blocks (consecutive lines starting with |)
                if lines[i].trimmingCharacters(in: .whitespaces).hasPrefix("|") {
                    var tableLines: [String] = []
                    var j = i
                    while j < lines.count && lines[j].trimmingCharacters(in: .whitespaces).hasPrefix("|") {
                        tableLines.append(lines[j])
                        j += 1
                    }
                    if tableLines.count >= 3, isTableSeparator(tableLines[1]),
                       let tableAttr = renderMarkdownTable(tableLines) {
                        result.append(tableAttr)
                        i = j
                        continue
                    }
                }

                // Regular line
                result.append(renderMarkdownLine(lines[i]))
                if i < lines.count - 1 {
                    result.append(NSAttributedString(string: "\n", attributes: [.font: font]))
                }
                i += 1
            }

            return result
        }

        // MARK: - Markdown Table Rendering (NSTextTable)

        nonisolated private func isTableSeparator(_ line: String) -> Bool {
            let t = line.trimmingCharacters(in: .whitespaces)
            guard t.hasPrefix("|") else { return false }
            var inner = t[t.index(after: t.startIndex)...]
            if inner.hasSuffix("|") { inner = inner.dropLast() }
            let cells = inner.split(separator: "|", omittingEmptySubsequences: false)
            guard !cells.isEmpty else { return false }
            return cells.allSatisfy { cell in
                let s = cell.trimmingCharacters(in: .whitespaces)
                return !s.isEmpty && s.allSatisfy { $0 == "-" || $0 == ":" }
            }
        }

        nonisolated private func parseTableRow(_ line: String) -> [String] {
            let t = line.trimmingCharacters(in: .whitespaces)
            guard t.hasPrefix("|") else { return [] }
            var inner = t[t.index(after: t.startIndex)...]
            if inner.hasSuffix("|") { inner = inner.dropLast() }
            return inner.split(separator: "|", omittingEmptySubsequences: false)
                .map { $0.trimmingCharacters(in: .whitespaces) }
        }

        nonisolated private func renderMarkdownTable(_ lines: [String]) -> NSAttributedString? {
            let headerCells = parseTableRow(lines[0])
            guard !headerCells.isEmpty else { return nil }

            let sepCells = parseTableRow(lines[1])
            let alignments: [NSTextAlignment] = sepCells.map { cell in
                let left = cell.hasPrefix(":")
                let right = cell.hasSuffix(":")
                if left && right { return .center }
                if right { return .right }
                return .left
            }

            var dataRows: [[String]] = []
            for idx in 2..<lines.count {
                let cells = parseTableRow(lines[idx])
                if !cells.isEmpty { dataRows.append(cells) }
            }

            let colCount = headerCells.count
            let table = NSTextTable()
            table.numberOfColumns = colCount
            table.layoutAlgorithm = .automaticLayoutAlgorithm
            table.collapsesBorders = true
            table.hidesEmptyCells = false

            let result = NSMutableAttributedString()
            let borderColor = NSColor.separatorColor
            let headerBg = NSColor.controlAccentColor.withAlphaComponent(0.15)
            let boldFont = NSFont.monospacedSystemFont(ofSize: font.pointSize, weight: .bold)

            for (col, cell) in headerCells.prefix(colCount).enumerated() {
                let align = col < alignments.count ? alignments[col] : .left
                result.append(makeTableCell(
                    text: cell, table: table, row: 0, column: col,
                    bg: headerBg, cellFont: boldFont, align: align, border: borderColor))
            }

            let evenBg = NSColor.controlBackgroundColor
            let oddBg = NSColor.windowBackgroundColor
            for (rowIdx, row) in dataRows.enumerated() {
                let bg = (rowIdx % 2 == 0) ? evenBg : oddBg
                for col in 0..<colCount {
                    let cellText = col < row.count ? row[col] : ""
                    let align = col < alignments.count ? alignments[col] : .left
                    result.append(makeTableCell(
                        text: cellText, table: table, row: rowIdx + 1, column: col,
                        bg: bg, cellFont: font, align: align, border: borderColor))
                }
            }

            return result
        }

        nonisolated private func makeTableCell(
            text: String, table: NSTextTable, row: Int, column: Int,
            bg: NSColor, cellFont: NSFont, align: NSTextAlignment, border: NSColor
        ) -> NSAttributedString {
            let block = NSTextTableBlock(
                table: table, startingRow: row, rowSpan: 1,
                startingColumn: column, columnSpan: 1)
            block.backgroundColor = bg
            block.setBorderColor(border)
            block.setWidth(0.5, type: .absoluteValueType, for: .border)
            block.setWidth(5.0, type: .absoluteValueType, for: .padding)

            let style = NSMutableParagraphStyle()
            style.textBlocks = [block]
            style.alignment = align

            let rendered = renderInlineElements(text, baseFont: cellFont)
            let cell = NSMutableAttributedString(attributedString: rendered)
            cell.append(NSAttributedString(string: "\n"))
            cell.addAttribute(.paragraphStyle, value: style, range: NSRange(location: 0, length: cell.length))
            return cell
        }

        /// Renders a single line, detecting block-level elements first, then inline.
        nonisolated private func renderMarkdownLine(_ line: String) -> NSAttributedString {
            let nsLine = line as NSString
            let fullRange = NSRange(location: 0, length: nsLine.length)

            // Horizontal rule (check before bullet since --- could conflict)
            if MarkdownPatterns.hrPattern?.firstMatch(in: line, range: fullRange) != nil {
                let result = NSMutableAttributedString()
                let attachment = NSTextAttachment()
                attachment.attachmentCell = MainActor.assumeIsolated { HRLineCell(color: .separatorColor) }
                result.append(NSAttributedString(attachment: attachment))
                return result
            }

            // Header
            if let match = MarkdownPatterns.headerPattern?.firstMatch(in: line, range: fullRange) {
                let level = nsLine.substring(with: match.range(at: 1)).count
                let content = nsLine.substring(with: match.range(at: 2))
                let size: CGFloat
                switch level {
                case 1: size = font.pointSize * 1.5
                case 2: size = font.pointSize * 1.3
                case 3: size = font.pointSize * 1.15
                default: size = font.pointSize
                }
                let headerFont = NSFont.monospacedSystemFont(ofSize: size, weight: .bold)
                return renderInlineElements(content, baseFont: headerFont)
            }

            // Bullet list
            if let match = MarkdownPatterns.bulletPattern?.firstMatch(in: line, range: fullRange) {
                let indent = nsLine.substring(with: match.range(at: 1))
                let content = nsLine.substring(with: match.range(at: 2))
                let result = NSMutableAttributedString()
                result.append(NSAttributedString(
                    string: indent + "  \u{2022} ",
                    attributes: [.font: font, .foregroundColor: NSColor.secondaryLabelColor]
                ))
                result.append(renderInlineElements(content, baseFont: font))
                return result
            }

            // Blockquote
            if let match = MarkdownPatterns.blockquotePattern?.firstMatch(in: line, range: fullRange) {
                let content = nsLine.substring(with: match.range(at: 1))
                let result = NSMutableAttributedString()
                result.append(NSAttributedString(
                    string: "\u{258E} ",
                    attributes: [.font: font, .foregroundColor: NSColor.systemBlue]
                ))
                let rendered = renderInlineElements(content, baseFont: font)
                let mutableRendered = NSMutableAttributedString(attributedString: rendered)
                let rRange = NSRange(location: 0, length: mutableRendered.length)
                mutableRendered.addAttribute(.foregroundColor, value: NSColor.secondaryLabelColor, range: rRange)
                result.append(mutableRendered)
                return result
            }

            // Activity log output (timestamps, grep results) — bypass markdown parser but still linkify URLs
            if let highlighted = CodeBlockHighlighter.highlightActivityLogLine(line: line, font: font) {
                return linkifyURLs(highlighted)
            }

            // Regular line — inline elements only
            return renderInlineElements(line, baseFont: font)
        }

        /// Parses inline markdown (bold, italic, inline code) using Apple's AttributedString.
        nonisolated private func renderInlineElements(_ text: String, baseFont: NSFont) -> NSAttributedString {
            guard !text.isEmpty else { return NSAttributedString() }

            let plainAttrs: [NSAttributedString.Key: Any] = [
                .font: baseFont,
                .foregroundColor: NSColor.labelColor
            ]

            // Fast path: skip the markdown parser for lines with no markdown syntax.
            let hasMarkdownChars = text.contains("*") || text.contains("_") || text.contains("`")
                || text.contains("[") || text.contains("~")
            guard hasMarkdownChars else {
                return linkifyURLs(NSAttributedString(string: text, attributes: plainAttrs))
            }

            // SAFETY: Skip markdown parsing if text contains Swift raw strings with backticks
            // (e.g., #"...`..."#). Apple's markdown parser mangles these.
            // Also skip if text looks like numbered code output (e.g., "1 | code")
            let hasRawStringWithBacktick = text.contains("#\"") && text.contains("\"#") && text.contains("`")
            let looksLikeNumberedCode = text.contains(#"\d+\s*\|"#) && text.split(separator: "\n").allSatisfy {
                $0.trimmingCharacters(in: .whitespaces).isEmpty || $0.range(of: #"^\s*\d+\s*\|"#, options: .regularExpression) != nil
            }
            if hasRawStringWithBacktick || looksLikeNumberedCode {
                return NSAttributedString(string: text, attributes: plainAttrs)
            }

            do {
                var options = AttributedString.MarkdownParsingOptions()
                options.interpretedSyntax = .inlineOnlyPreservingWhitespace
                let parsed = try AttributedString(markdown: text, options: options)

                let nsAttr = NSMutableAttributedString(parsed)
                let fullRange = NSRange(location: 0, length: nsAttr.length)

                // Set base monospaced font and color
                nsAttr.addAttribute(.font, value: baseFont, range: fullRange)
                nsAttr.addAttribute(.foregroundColor, value: NSColor.labelColor, range: fullRange)

                // Apply bold/italic/code from inline presentation intents
                nsAttr.enumerateAttributes(in: fullRange, options: []) { attrs, range, _ in
                    if let intentValue = attrs[.inlinePresentationIntent] as? Int {
                        let intent = InlinePresentationIntent(rawValue: UInt(intentValue))
                        var styledFont = baseFont
                        if intent.contains(.stronglyEmphasized) {
                            styledFont = NSFontManager.shared.convert(styledFont, toHaveTrait: .boldFontMask)
                        }
                        if intent.contains(.emphasized) {
                            styledFont = NSFontManager.shared.convert(styledFont, toHaveTrait: .italicFontMask)
                        }
                        nsAttr.addAttribute(.font, value: styledFont, range: range)
                        if intent.contains(.code) {
                            nsAttr.addAttribute(.backgroundColor, value: NSColor.quaternaryLabelColor, range: range)
                        }
                    }
                }

                // Manual fallback: apply **bold** and *italic* that Apple's parser missed
                applyManualBoldItalic(nsAttr, baseFont: baseFont)
                return nsAttr
            } catch {
                // Parser failed entirely — do manual bold/italic on plain text
                let nsAttr = NSMutableAttributedString(string: text, attributes: plainAttrs)
                applyManualBoldItalic(nsAttr, baseFont: baseFont)
                return nsAttr
            }
        }

        /// Detect https/http URLs in attributed text and make them clickable links.
        nonisolated private func linkifyURLs(_ input: NSAttributedString) -> NSAttributedString {
            let text = input.string
            let result = NSMutableAttributedString(attributedString: input)

            // 1. Convert markdown links [text](url) → clickable "text" with link
            if text.contains("](") {
                let mdPattern = try? NSRegularExpression(pattern: #"\[([^\]]+)\]\((https?://[^\)]+)\)"#)
                let mdMatches = mdPattern?.matches(in: text, range: NSRange(location: 0, length: (text as NSString).length)) ?? []
                for match in mdMatches.reversed() {
                    let displayRange = match.range(at: 1)
                    let urlRange = match.range(at: 2)
                    let displayText = (text as NSString).substring(with: displayRange)
                    let urlString = (text as NSString).substring(with: urlRange)
                    let linked = NSMutableAttributedString(string: displayText, attributes: result.attributes(at: match.range.location, effectiveRange: nil))
                    linked.addAttribute(.link, value: urlString, range: NSRange(location: 0, length: displayText.count))
                    linked.addAttribute(.foregroundColor, value: NSColor.linkColor, range: NSRange(location: 0, length: displayText.count))
                    linked.addAttribute(.underlineStyle, value: NSUnderlineStyle.single.rawValue, range: NSRange(location: 0, length: displayText.count))
                    result.replaceCharacters(in: match.range, with: linked)
                }
            }

            // 2. Linkify bare URLs not already in markdown links
            let updatedText = result.string
            guard updatedText.contains("http") else { return result }
            let detector = try? NSDataDetector(types: NSTextCheckingResult.CheckingType.link.rawValue)
            let matches = detector?.matches(in: updatedText, range: NSRange(location: 0, length: (updatedText as NSString).length)) ?? []
            for match in matches.reversed() {
                guard let url = match.url else { continue }
                // Skip if this range already has a link attribute
                var existingLink: Any?
                if match.range.location < result.length {
                    existingLink = result.attribute(.link, at: match.range.location, effectiveRange: nil)
                }
                if existingLink != nil { continue }
                result.addAttribute(.link, value: url.absoluteString, range: match.range)
                result.addAttribute(.foregroundColor, value: NSColor.linkColor, range: match.range)
                result.addAttribute(.underlineStyle, value: NSUnderlineStyle.single.rawValue, range: match.range)
            }

            // 3. Linkify Xcode build errors: /path/file.swift:42:10: error/warning: message
            let resultText = result.string
            if resultText.contains(": error:") || resultText.contains(": warning:") || resultText.contains(": note:") {
                let errorPattern = try? NSRegularExpression(pattern: #"(/[^\s:]+\.\w+):(\d+):(\d+): (error|warning|note):"#)
                let errorMatches = errorPattern?.matches(in: resultText, range: NSRange(location: 0, length: (resultText as NSString).length)) ?? []
                for match in errorMatches.reversed() {
                    let filePath = (resultText as NSString).substring(with: match.range(at: 1))
                    let line = (resultText as NSString).substring(with: match.range(at: 2))
                    let col = (resultText as NSString).substring(with: match.range(at: 3))
                    let severity = (resultText as NSString).substring(with: match.range(at: 4))
                    // Encode as xcode:// URL for clickedOnLink handler
                    let xcodeURL = "xcode://open?file=\(filePath)&line=\(line)&col=\(col)"
                    let color: NSColor = severity == "error" ? .systemRed : severity == "warning" ? .systemOrange : .systemBlue
                    // Only linkify the file:line:col portion
                    let fileRange = match.range(at: 1)
                    let colonAfterCol = match.range(at: 3).location + match.range(at: 3).length
                    let linkRange = NSRange(location: fileRange.location, length: colonAfterCol - fileRange.location)
                    result.addAttribute(.link, value: xcodeURL, range: linkRange)
                    result.addAttribute(.foregroundColor, value: color, range: linkRange)
                    result.addAttribute(.underlineStyle, value: NSUnderlineStyle.single.rawValue, range: linkRange)
                }
            }

            return result
        }

        /// Manually apply **bold** and *italic* markers that Apple's markdown parser missed.
        nonisolated private func applyManualBoldItalic(_ attrStr: NSMutableAttributedString, baseFont: NSFont) {
            let text = attrStr.string
            // Bold: **text**
            if let regex = try? NSRegularExpression(pattern: #"\*\*(.+?)\*\*"#) {
                let matches = regex.matches(in: text, range: NSRange(text.startIndex..., in: text))
                let boldFont = NSFontManager.shared.convert(baseFont, toHaveTrait: .boldFontMask)
                for match in matches.reversed() {
                    let contentRange = match.range(at: 1)
                    let content = (text as NSString).substring(with: contentRange)
                    let styled = NSAttributedString(string: content, attributes: [
                        .font: boldFont,
                        .foregroundColor: NSColor.labelColor
                    ])
                    attrStr.replaceCharacters(in: match.range, with: styled)
                }
            }
            // Italic: *text* (but not inside **)
            let updatedText = attrStr.string
            if let regex = try? NSRegularExpression(pattern: #"(?<!\*)\*(?!\*)(.+?)(?<!\*)\*(?!\*)"#) {
                let matches = regex.matches(in: updatedText, range: NSRange(updatedText.startIndex..., in: updatedText))
                let italicFont = NSFontManager.shared.convert(baseFont, toHaveTrait: .italicFontMask)
                for match in matches.reversed() {
                    let contentRange = match.range(at: 1)
                    let content = (updatedText as NSString).substring(with: contentRange)
                    let styled = NSAttributedString(string: content, attributes: [
                        .font: italicFont,
                        .foregroundColor: NSColor.labelColor
                    ])
                    attrStr.replaceCharacters(in: match.range, with: styled)
                }
            }
        }

        // Open image/HTML file links — images in default app (Preview), HTML in browser
        func textView(_ textView: NSTextView, clickedOnLink link: Any, at charIndex: Int) -> Bool {
            let urlString: String
            if let url = link as? URL {
                urlString = url.absoluteString
            } else if let str = link as? String {
                urlString = str
            } else {
                return false
            }
            // Handle xcode:// links — open file at line in Xcode
            if urlString.hasPrefix("xcode://open?") {
                if let comps = URLComponents(string: urlString),
                   let file = comps.queryItems?.first(where: { $0.name == "file" })?.value,
                   let line = comps.queryItems?.first(where: { $0.name == "line" })?.value {
                    let script = "tell application \"Xcode\" to open \"\(file)\""
                    // Open file in Xcode, then jump to line via xed
                    Task { @MainActor in
                        NSAppleScript(source: script)?.executeAndReturnError(nil)
                        let p = Process()
                        p.executableURL = URL(fileURLWithPath: "/usr/bin/xed")
                        p.arguments = ["--line", line, file]
                        try? p.run()
                        p.waitUntilExit()
                    }
                }
                return true
            }

            guard let url = URL(string: urlString), url.isFileURL else { return false }
            let ext = url.pathExtension.lowercased()
            let htmlExtensions: Set<String> = ["html", "htm"]
            if htmlExtensions.contains(ext) {
                // HTML → open in default browser
                if let browserURL = NSWorkspace.shared.urlForApplication(toOpen: URL(string: "https://example.com")!) {
                    let config = NSWorkspace.OpenConfiguration()
                    NSWorkspace.shared.open([url], withApplicationAt: browserURL, configuration: config)
                } else {
                    NSWorkspace.shared.open(url)
                }
            } else {
                // Images → open in default app (Preview)
                NSWorkspace.shared.open(url)
            }
            return true
        }

        // MARK: - Per-Tab TextStorage Cache

        /// Cached NSTextStorage per tab — swapping avoids re-layout entirely.
        private struct TabCache {
            let textStorage: NSTextStorage
            let textLength: Int
            let textHash: Int
            let scrollY: CGFloat
        }
        private var tabCaches: [UUID?: TabCache] = [:]

        /// Returns cached text storage if the text hasn't changed, otherwise nil
        func cachedAttributedString(for tabID: UUID?, text: String) -> NSAttributedString? {
            guard let cache = tabCaches[tabID] else { return nil }
            let len = (text as NSString).length
            let hash = text.hashValue
            guard cache.textLength == len, cache.textHash == hash else { return nil }
            return cache.textStorage
        }

        /// Swap to a cached NSTextStorage for instant tab switch (no re-layout).
        /// Returns true if cache was used.
        func swapToCachedStorage(for tabID: UUID?, text: String, textView: NSTextView, scrollView: NSScrollView) -> Bool {
            guard let cache = tabCaches[tabID] else { return false }
            let len = (text as NSString).length
            let hash = text.hashValue
            guard cache.textLength == len, cache.textHash == hash else { return false }
            // Swap the textStorage on the layout manager — instant, no re-layout
            textView.layoutManager?.replaceTextStorage(cache.textStorage)
            // Restore scroll position
            scrollView.contentView.scroll(to: NSPoint(x: 0, y: cache.scrollY))
            scrollView.reflectScrolledClipView(scrollView.contentView)
            return true
        }

        /// Save current textStorage and scroll position for a tab
        func cacheAttributedString(_ attrStr: NSAttributedString, for tabID: UUID?, text: String) {
            guard let scrollView = latestScrollView else { return }
            let len = (text as NSString).length
            let hash = text.hashValue
            let scrollY = scrollView.contentView.bounds.origin.y
            // Copy into a new NSTextStorage so the cached one is independent
            let storage = NSTextStorage(attributedString: attrStr)
            tabCaches[tabID] = TabCache(textStorage: storage, textLength: len, textHash: hash, scrollY: scrollY)
        }

        /// Invalidate cache for a specific tab
        func invalidateCache(for tabID: UUID?) {
            tabCaches.removeValue(forKey: tabID)
        }

        /// Invalidate all tab caches (e.g. on appearance change)
        func invalidateAllCaches() {
            tabCaches.removeAll()
        }

        func clearCache() {
            lastSearch = ""
            lastMatchIndex = -1
            lastSearchRanges.removeAll()
            savedForegroundColors.removeAll()
        }
    }
}
