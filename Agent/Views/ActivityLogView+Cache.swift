import SwiftUI
import AppKit

// MARK: - Coordinator: Per-Tab TextStorage Cache
//
// Swapping a cached `NSTextStorage` on a tab switch avoids re-layout entirely.

extension ActivityLogView.Coordinator {
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
        tabCaches[tabID] = ActivityLogView.Coordinator.TabCache(textStorage: storage, textLength: len, textHash: hash, scrollY: scrollY)
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
