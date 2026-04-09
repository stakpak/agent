import SwiftUI
import AppKit
import Combine
import AgentColorSyntax
import AgentTerminalNeo

/// NSTextView with arrow cursor instead of I-beam. Selection still works.
final class ArrowCursorTextView: NSTextView {
    private var arrowTrackingArea: NSTrackingArea?

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        for area in trackingAreas where area.options.contains(.cursorUpdate) {
            removeTrackingArea(area)
        }
        if let existing = arrowTrackingArea { removeTrackingArea(existing) }
        let area = NSTrackingArea(
            rect: bounds,
            options: [.cursorUpdate, .activeAlways, .inVisibleRect],
            owner: self,
            userInfo: nil
        )
        addTrackingArea(area)
        arrowTrackingArea = area
    }

    override func cursorUpdate(with event: NSEvent) { NSCursor.arrow.set() }

    override func resetCursorRects() {
        discardCursorRects()
        addCursorRect(visibleRect, cursor: .arrow)
    }
}

/// NSTextView-backed activity log — avoids SwiftUI Text layout storms.
/// Detects image/HTML paths and shows clickable links. Optimized for streaming.
/// Rendering, scroll, search, markdown, and cache live on `Coordinator`, split across:
/// ActivityLogView+Update.swift, +Scroll, +Search, +Markdown, +MarkdownBlock,
/// +MarkdownInline, +Cache, +Rendering.
struct ActivityLogView: NSViewRepresentable {
    @Environment(\.colorScheme) private var colorScheme
    let text: String
    var tabID: UUID? // nil = main tab
    var isActive: Bool = false // true when tab/task is running — skip truncation
    var textProvider: (@MainActor () -> String)? = nil // polled for live updates
    var searchText: String = ""
    var caseSensitive: Bool = false
    var currentMatchIndex: Int = 0
    var onMatchCount: ((Int) -> Void)? = nil

    func makeNSView(context: Context) -> NSScrollView {
        let scrollView = NSScrollView()
        let contentSize = scrollView.contentSize
        let textContainer = NSTextContainer(
            containerSize: NSSize(width: contentSize.width, height: CGFloat.greatestFiniteMagnitude)
        )
        textContainer.widthTracksTextView = true
        let layoutManager = NSLayoutManager()
        layoutManager.addTextContainer(textContainer)
        let textStorage = NSTextStorage()
        textStorage.addLayoutManager(layoutManager)
        let textView = ArrowCursorTextView(
            frame: NSRect(x: 0, y: 0, width: contentSize.width, height: contentSize.height),
            textContainer: textContainer
        )
        textView.minSize = NSSize(width: 0, height: 0)
        textView.maxSize = NSSize(width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
        textView.isVerticallyResizable = true
        textView.isHorizontallyResizable = false
        textView.autoresizingMask = [.width]
        scrollView.documentView = textView
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
        let searchChanged = searchText != coord.latestSearchText || currentMatchIndex != coord.latestMatchIndex || caseSensitive != coord
            .latestCaseSensitive
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

        // MARK: - Scroll-state props (consumed by ActivityLogView+Scroll.swift)

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

        // MARK: - Search-state props (consumed by ActivityLogView+Search.swift)

        /// Track previous search highlight ranges so we can remove only those
        var lastSearchRanges: [NSRange] = []
        /// Saved original foreground colors per highlighted range so we can restore them
        var savedForegroundColors: [(range: NSRange, color: NSColor?)] = []
        /// Debounce timer for search highlighting during streaming
        var pendingSearchWork: DispatchWorkItem?

        // MARK: - Per-Tab TextStorage Cache (consumed by ActivityLogView+Cache.swift)

        /// Cached NSTextStorage per tab — swapping avoids re-layout entirely.
        struct TabCache {
            let textStorage: NSTextStorage
            let textLength: Int
            let textHash: Int
            let scrollY: CGFloat
        }
        var tabCaches: [UUID?: TabCache] = [:]

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

        // MARK: - Link Clicks

        /// Open image/HTML file links — images in default app (Preview), HTML in browser
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
                   let line = comps.queryItems?.first(where: { $0.name == "line" })?.value
                {
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
                if let probeURL = URL(string: "https://example.com"),
                   let browserURL = NSWorkspace.shared.urlForApplication(toOpen: probeURL) {
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
    }
}
