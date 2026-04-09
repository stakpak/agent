import SwiftUI
import AppKit
import AgentTerminalNeo

/// NSTextView with no-op scrollRangeToVisible (we drive scroll manually) and arrow cursor. Selection still works.
final class FollowTextView: NSTextView {
    private var arrowTrackingArea: NSTrackingArea?

    override func scrollRangeToVisible(_ range: NSRange) { /* no-op */ }

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

  /// NSScrollView subclass that fires callbacks on scroll/hover. Intercepts scrollWheel directly so auto-follow disables immediately.
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

/// Local NSScrollView/NSTextView wrapper for the LLM Output HUD. Renders via TerminalNeoRenderer.
/// autoFollowDisabled is single truth. Jitter: incremental append, no animations, cursor blink via setAttributes.
struct LLMOutputTextView: NSViewRepresentable {
    let text: String
    /// True while streaming — when false, mouse-exit won't force snap-to-bottom; the view stays where the user parked it.
    var isStreaming: Bool = false
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
        // Selection enabled — click+drag to select, Cmd+C to copy. Arrow cursor forced via FollowTextView overrides.
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
        // Hover disables auto-follow. On mouse-exit while streaming, re-enable + snap to bottom. When done, leave view where parked.
        scrollView.onHoverChange = { [weak coord] hovering in
            guard let coord else { return }
            coord.isHovering = hovering
            if hovering {
                coord.autoFollowDisabled = true
            } else if coord.isStreaming {
                coord.autoFollowDisabled = false
                if let tv = coord.textView {
                    coord.snapToEnd(tv, force: true)
                }
            }
            // Operation done — leave autoFollowDisabled as-is, don't snap. User's scroll position preserved.
        }

        return scrollView
    }

    func updateNSView(_ scrollView: NSScrollView, context: Context) {
        let coord = context.coordinator
        coord.onContentHeight = onContentHeight
        coord.isStreaming = isStreaming
        guard let tv = coord.textView, let storage = tv.textStorage else { return }

        // Decompose input into "real content" + "cursor state". Upstream appends "█"/" " for blink on/off.
        let cursorVisible = text.hasSuffix("█")
        let hasCursor = cursorVisible || text.hasSuffix(" ")
        let contentText = hasCursor ? String(text.dropLast()) : text
        let contentLen = contentText.count

        let isDark = tv.effectiveAppearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
        let textColor: NSColor = isDark
            ? NSColor(red: 0.2, green: 0.9, blue: 0.3, alpha: 1)
            : NSColor(red: 0.05, green: 0.35, blue: 0.1, alpha: 1)
        let font = NSFont.monospacedSystemFont(ofSize: 16.5, weight: .regular)
        let cursorAttrs: [NSAttributedString.Key: Any] = [
            .font: font,
            .foregroundColor: cursorVisible ? textColor : NSColor.clear
        ]

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
                // FAST PATH: strip previous cursor glyph, append new content delta, then append fresh "█" cursor. Color switches via setAttributes, not replaceCharacters.
                let attrLen = storage.length
                if attrLen > 0 {
                    let lastChar = storage.string.suffix(1)
                    if lastChar == "█" || lastChar == " " {
                        storage.deleteCharacters(in: NSRange(location: attrLen - 1, length: 1))
                    }
                }
                let startIdx = storage.length
                storage.beginEditing()
                if startIdx < contentText.count {
                    let newPart = String(
                        contentText[contentText.index(contentText.startIndex, offsetBy: startIdx)...]
                    )
                    storage.append(NSAttributedString(string: newPart, attributes: [
                        .font: font, .foregroundColor: textColor
                    ]))
                }
                if hasCursor {
                    storage.append(NSAttributedString(string: "█", attributes: cursorAttrs))
                }
                storage.endEditing()
            } else {
                // SLOW PATH: full markdown re-render of contentText (no cursor), then append cursor as separate run.
                storage.beginEditing()
                storage.setAttributedString(TerminalNeoRenderer.render(contentText))
                if hasCursor {
                    storage.append(NSAttributedString(string: "█", attributes: cursorAttrs))
                }
                storage.endEditing()
                tv.layoutManager?.ensureLayout(for: tv.textContainer!)
            }

            CATransaction.commit()
            coord.lastContentLength = contentLen

            // Latch table-render mode while the tail looks like a table row
            let lastNonEmpty = contentText.components(separatedBy: "\n")
                .last(where: { !$0.trimmingCharacters(in: .whitespaces).isEmpty }) ?? ""
            if lastNonEmpty.trimmingCharacters(in: .whitespaces).hasPrefix("|") {
                coord.needsTableRender = true
            }

            // Follow-bottom: only scroll when autoFollowDisabled is false. Content extends below naturally when user scrolls away.
            if !coord.autoFollowDisabled {
                coord.snapToEnd(tv)
            }
        } else {
            // Cursor blink — only color changes via setAttributes, no replaceCharacters. Surrounding text stays pixel-stable.
            guard !coord.needsTableRender, hasCursor else { return }
            let attrLen = storage.length
            guard attrLen > 0 else { return }
            let lastRange = NSRange(location: attrLen - 1, length: 1)
            let lastChar = (storage.string as NSString).substring(with: lastRange)
            guard lastChar == "█" else { return }  // not in cursor mode
            storage.setAttributes(cursorAttrs, range: lastRange)
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
        /// Latched once we see a markdown table — stays on so we keep doing full re-renders instead of incremental appends.
        var needsTableRender: Bool = false

        /// HARD SWITCH. When true, no auto-follow regardless of position. Set on scroll/hover-enter; cleared on hover-exit-at-bottom or scrolling back to bottom.
        var autoFollowDisabled: Bool = false
        /// Mouse currently hovering over the scroll view.
        var isHovering: Bool = false
        /// Mirror of isStreaming. Mouse-exit uses this to decide: force-snap while streaming, or leave parked when done.
        var isStreaming: Bool = false
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
                    // Re-enable auto-follow only when scrolled to bottom AND not hovering.
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

        /// True iff the visible bottom is within 5pt of the content end —
        /// tight threshold so we only re-engage when truly at bottom.
        func isAtBottom(_ textView: NSTextView) -> Bool {
            guard let scrollView = textView.enclosingScrollView else { return true }
            let visibleBottom = scrollView.contentView.bounds.origin.y + scrollView.contentView.bounds.height
            let contentHeight = textView.frame.height
            return (contentHeight - visibleBottom) < 5
        }

        /// True iff cursor is inside the scroll view's frame, polled synchronously.
        /// More reliable than NSTrackingArea — just asks "where is the cursor RIGHT NOW?".
        func isMouseInside(_ scrollView: NSScrollView) -> Bool {
            guard let window = scrollView.window else { return false }
            let pointInWindow = window.mouseLocationOutsideOfEventStream
            let pointInView = scrollView.convert(pointInWindow, from: nil)
            return scrollView.bounds.contains(pointInView)
        }

    /// Scroll to bottom. Skips if hovering during stream. `force` bypasses hover check. Brackets with isProgrammaticScroll.
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
