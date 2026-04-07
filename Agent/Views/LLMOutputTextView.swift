import SwiftUI
import AppKit
import AgentTerminalNeo

/// NSTextView subclass whose scrollRangeToVisible is a no-op. Stock NSTextView
/// implicitly scrolls the new range into view on every text mutation
/// (storage.append, replaceCharacters, etc.) — that's the source of the
/// remaining fight when streaming chunks arrive while the user is scrolled up.
/// We disable that path entirely and drive scroll only via snapToEnd, which
/// talks to the clip view directly.
final class FollowTextView: NSTextView {
    override func scrollRangeToVisible(_ range: NSRange) { /* no-op */ }
}

/// NSScrollView subclass that fires callbacks on user scroll and on hover
/// enter/exit. We need this because the boundsDidChangeNotification observer
/// has a perceptible lag (it fires AFTER the scroll lands), which lets a
/// streaming chunk's snapToEnd race against the user's in-progress gesture.
/// Intercepting scrollWheel directly disables auto-follow on the very first
/// event, before any fight can happen.
final class FollowScrollView: NSScrollView {
    var onUserScroll: (() -> Void)?
    var onHoverChange: ((Bool) -> Void)?
    private var hoverTrackingArea: NSTrackingArea?

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let area = hoverTrackingArea { removeTrackingArea(area) }
        let area = NSTrackingArea(
            rect: bounds,
            options: [.mouseEnteredAndExited, .activeAlways, .inVisibleRect],
            owner: self,
            userInfo: nil
        )
        addTrackingArea(area)
        hoverTrackingArea = area
    }

    override func scrollWheel(with event: NSEvent) {
        onUserScroll?()
        super.scrollWheel(with: event)
    }

    override func mouseEntered(with event: NSEvent) {
        super.mouseEntered(with: event)
        onHoverChange?(true)
    }

    override func mouseExited(with event: NSEvent) {
        super.mouseExited(with: event)
        onHoverChange?(false)
    }
}

/// Local NSScrollView/NSTextView wrapper for the LLM Output HUD.
/// Renders text via TerminalNeoRenderer for markdown/table styling.
///
/// Scroll policy — hard switch model:
/// - `autoFollowDisabled` is the single source of truth.
/// - It flips to TRUE the moment the user does ANY of:
///     • scrollWheel/trackpad event (caught instantly via FollowScrollView)
///     • mouse hover-enter over the scroll view
/// - It flips back to FALSE when:
///     • user scrolls back to the very bottom AND mouse is not hovering
///     • text shrinks (new task / reset) — fresh content always follows
///
/// Jitter avoidance (untouched from the smooth version):
/// - Incremental append for non-table streaming chunks — no full re-layout.
/// - CATransaction.setDisableActions(true) wrap suppresses implicit animations.
/// - Full TerminalNeoRenderer re-render only for tables, shrinks, or first load.
/// - Cursor-blink path mutates only the trailing char with no scroll calls.
struct LLMOutputTextView: NSViewRepresentable {
    let text: String
    var onContentHeight: ((CGFloat) -> Void)?

    func makeCoordinator() -> Coordinator { Coordinator() }

    func makeNSView(context: Context) -> NSScrollView {
        let scrollView = FollowScrollView()
        scrollView.hasVerticalScroller = true
        scrollView.hasHorizontalScroller = false
        scrollView.autohidesScrollers = true
        scrollView.drawsBackground = false
        scrollView.borderType = .noBorder

        let contentSize = scrollView.contentSize
        let textView = FollowTextView(frame: NSRect(x: 0, y: 0, width: contentSize.width, height: contentSize.height))
        textView.minSize = NSSize(width: 0, height: 0)
        textView.maxSize = NSSize(width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
        textView.isVerticallyResizable = true
        textView.isHorizontallyResizable = false
        textView.autoresizingMask = [.width]
        textView.textContainer?.containerSize = NSSize(width: contentSize.width, height: CGFloat.greatestFiniteMagnitude)
        textView.textContainer?.widthTracksTextView = true

        textView.isEditable = false
        textView.isSelectable = true
        textView.backgroundColor = .clear
        textView.drawsBackground = false
        textView.textContainerInset = NSSize(width: 10, height: 10)
        textView.isAutomaticQuoteSubstitutionEnabled = false
        textView.isAutomaticDashSubstitutionEnabled = false
        textView.isAutomaticLinkDetectionEnabled = false
        textView.usesFontPanel = false
        textView.usesRuler = false
        textView.isRichText = true
        textView.allowsUndo = false
        textView.layoutManager?.allowsNonContiguousLayout = true

        scrollView.documentView = textView

        let coord = context.coordinator
        coord.textView = textView
        coord.onContentHeight = onContentHeight
        coord.startObservingScroll(scrollView)

        // Hard switch: any user scroll wheel/trackpad event disables auto-follow
        // immediately, before the bounds observer would have a chance to fire.
        scrollView.onUserScroll = { [weak coord] in
            coord?.autoFollowDisabled = true
        }
        // Hover over the scroll view disables auto-follow. On mouse-exit,
        // unconditionally re-enable AND force a snap to the bottom so the
        // view catches up to whatever streamed in while the user was hovering.
        scrollView.onHoverChange = { [weak coord] hovering in
            guard let coord else { return }
            coord.isHovering = hovering
            if hovering {
                coord.autoFollowDisabled = true
            } else {
                coord.autoFollowDisabled = false
                if let tv = coord.textView {
                    coord.snapToEnd(tv, force: true)
                }
            }
        }

        return scrollView
    }

    func updateNSView(_ scrollView: NSScrollView, context: Context) {
        let coord = context.coordinator
        coord.onContentHeight = onContentHeight
        guard let tv = coord.textView, let storage = tv.textStorage else { return }

        // Strip cursor char to detect content changes vs cursor blink
        let contentText = text.hasSuffix("█") ? String(text.dropLast()) : (text.hasSuffix(" ") ? String(text.dropLast()) : text)
        let contentLen = contentText.count

        if contentLen != coord.lastContentLength {
            // Text shrank → new task / reset → re-arm auto-follow
            if contentLen < coord.lastContentLength {
                coord.autoFollowDisabled = false
            }
            let isAppend = contentLen > coord.lastContentLength && coord.lastContentLength > 0
            let hasTable = contentText.contains("|\n") && contentText.contains("---")
            if hasTable { coord.needsTableRender = true }

            CATransaction.begin()
            CATransaction.setDisableActions(true)

            if isAppend && !coord.needsTableRender {
                // FAST PATH: incremental append. No layout reflow above the
                // appended range, so no jitter. Strip trailing cursor first if
                // present, then append the delta with terminal styling.
                let attrLen = storage.length
                if attrLen > 0 {
                    let lastChar = storage.string.suffix(1)
                    if lastChar == "█" || lastChar == " " {
                        storage.deleteCharacters(in: NSRange(location: attrLen - 1, length: 1))
                    }
                }
                let startIdx = storage.length
                if startIdx < text.count {
                    let newPart = String(text[text.index(text.startIndex, offsetBy: startIdx)...])
                    let isDark = tv.effectiveAppearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
                    let color: NSColor = isDark
                        ? NSColor(red: 0.2, green: 0.9, blue: 0.3, alpha: 1)
                        : NSColor(red: 0.05, green: 0.35, blue: 0.1, alpha: 1)
                    let font = NSFont.monospacedSystemFont(ofSize: 16.5, weight: .regular)
                    storage.beginEditing()
                    storage.append(NSAttributedString(string: newPart, attributes: [
                        .font: font, .foregroundColor: color
                    ]))
                    storage.endEditing()
                }
            } else {
                // SLOW PATH: full markdown re-render. Used for tables, shrinks
                // (text reset), and first render. Wrapped in CATransaction so
                // implicit animations don't fire.
                storage.setAttributedString(TerminalNeoRenderer.render(text))
                tv.layoutManager?.ensureLayout(for: tv.textContainer!)
            }

            CATransaction.commit()
            coord.lastContentLength = contentLen

            // Latch table-render mode while the tail looks like a table row
            let lastNonEmpty = contentText.components(separatedBy: "\n").last(where: { !$0.trimmingCharacters(in: .whitespaces).isEmpty }) ?? ""
            if lastNonEmpty.trimmingCharacters(in: .whitespaces).hasPrefix("|") {
                coord.needsTableRender = true
            }

            // Follow-bottom: only scroll when the hard switch says we may.
            // If the user has scrolled away or is hovering, autoFollowDisabled
            // is true and we leave the clip view origin alone — the appended
            // content extends the document below their view naturally.
            if !coord.autoFollowDisabled {
                coord.snapToEnd(tv)
            }
        } else {
            // Cursor blink: swap last char only. No scroll calls. Skip during
            // table-render mode to avoid mutating freshly rendered table layout.
            guard !coord.needsTableRender else { return }
            let attrLen = storage.length
            if attrLen > 0 {
                let cursorChar = text.hasSuffix("█") ? "█" : " "
                let lastChar = String(storage.string.suffix(1))
                if lastChar != cursorChar {
                    let isDark = tv.effectiveAppearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
                    let color: NSColor = isDark
                        ? NSColor(red: 0.2, green: 0.9, blue: 0.3, alpha: 1)
                        : NSColor(red: 0.05, green: 0.35, blue: 0.1, alpha: 1)
                    let font = NSFont.monospacedSystemFont(ofSize: 16.5, weight: .regular)
                    CATransaction.begin()
                    CATransaction.setDisableActions(true)
                    storage.beginEditing()
                    storage.replaceCharacters(in: NSRange(location: attrLen - 1, length: 1),
                        with: NSAttributedString(string: cursorChar, attributes: [
                            .font: font, .foregroundColor: color
                        ]))
                    storage.endEditing()
                    CATransaction.commit()
                }
            }
        }

        // Report content height back to SwiftUI for box sizing
        let h = (tv.layoutManager?.usedRect(for: tv.textContainer!).height ?? 40) + tv.textContainerInset.height * 2
        if abs(h - coord.lastReportedHeight) > 1 {
            coord.lastReportedHeight = h
            let cb = coord.onContentHeight
            DispatchQueue.main.async { cb?(h) }
        }
    }

    @MainActor final class Coordinator: @unchecked Sendable {
        weak var textView: NSTextView?
        var onContentHeight: ((CGFloat) -> Void)?
        var lastContentLength: Int = 0
        var lastReportedHeight: CGFloat = 0
        /// Latched on once we see a markdown table — stays on so we keep doing
        /// full re-renders instead of incremental appends (tables can't be
        /// extended by simple character append).
        var needsTableRender: Bool = false

        /// HARD SWITCH. When true, no auto-follow regardless of position.
        /// Set true on: scrollWheel/trackpad event, hover-enter.
        /// Cleared on: hover-exit while at bottom, text shrink (new task),
        /// or user scrolling back to the bottom while not hovering.
        var autoFollowDisabled: Bool = false
        /// Mouse currently hovering over the scroll view.
        var isHovering: Bool = false
        /// Suppresses bounds-tracking during our own programmatic scrolls.
        var isProgrammaticScroll: Bool = false
        private var scrollThrottled: Bool = false

        nonisolated(unsafe) var scrollObserver: NSObjectProtocol?

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
                    // Re-enable auto-follow only when the user has manually
                    // scrolled all the way back to the bottom AND isn't hovering.
                    if !self.isHovering && self.isAtBottom(textView) {
                        self.autoFollowDisabled = false
                    }
                    self.scrollThrottled = true
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.1) { [weak self] in
                        self?.scrollThrottled = false
                    }
                }
            }
        }

        deinit {
            if let observer = scrollObserver {
                NotificationCenter.default.removeObserver(observer)
            }
        }

        /// True iff the visible bottom is within 5pt of the content end.
        /// Tight threshold so we only re-engage when the user really lands at
        /// the bottom — not just somewhere near it.
        func isAtBottom(_ textView: NSTextView) -> Bool {
            guard let scrollView = textView.enclosingScrollView else { return true }
            let visibleBottom = scrollView.contentView.bounds.origin.y + scrollView.contentView.bounds.height
            let contentHeight = textView.frame.height
            return (contentHeight - visibleBottom) < 5
        }

        /// True iff the cursor is currently inside the given scroll view's
        /// frame, polled synchronously via mouseLocationOutsideOfEventStream.
        /// More reliable than NSTrackingArea callbacks because it doesn't
        /// depend on AppKit having already delivered a mouseEntered event —
        /// it just asks "where is the cursor RIGHT NOW?".
        func isMouseInside(_ scrollView: NSScrollView) -> Bool {
            guard let window = scrollView.window else { return false }
            let pointInWindow = window.mouseLocationOutsideOfEventStream
            let pointInView = scrollView.convert(pointInWindow, from: nil)
            return scrollView.bounds.contains(pointInView)
        }

        /// Instant scroll to end. Drives the clip view directly so it works
        /// even though FollowTextView's scrollRangeToVisible is a no-op.
        /// Brackets the call with isProgrammaticScroll so the bounds observer
        /// doesn't misread it.
        ///
        /// HOVER CHECK: before scrolling, polls the cursor position right now.
        /// If the mouse is over the scroll view the user is reading — skip
        /// this snap (no latching). The next chunk will re-poll, so as soon
        /// as the mouse leaves, snaps resume naturally.
        ///
        /// `force: true` bypasses the hover check. Used by the mouse-exit
        /// handler to catch up after the user moves away.
        func snapToEnd(_ textView: NSTextView, force: Bool = false) {
            guard let scrollView = textView.enclosingScrollView else { return }
            if !force && isMouseInside(scrollView) {
                return
            }
            // Make sure layout is up to date so the document height is correct.
            if let container = textView.textContainer {
                textView.layoutManager?.ensureLayout(for: container)
            }
            let docHeight = textView.frame.height
            let visibleHeight = scrollView.contentView.bounds.height
            let bottomY = max(0, docHeight - visibleHeight)
            isProgrammaticScroll = true
            scrollView.contentView.scroll(to: NSPoint(x: 0, y: bottomY))
            scrollView.reflectScrolledClipView(scrollView.contentView)
            isProgrammaticScroll = false
        }
    }
}
