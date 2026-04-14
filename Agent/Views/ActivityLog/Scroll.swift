import SwiftUI
import AppKit

// MARK: - Coordinator: Scroll Helpers

extension ActivityLogView.Coordinator {
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
              let textContainer = textView.textContainer else
        {
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
              let textContainer = textView.textContainer else
        {
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
}
