import SwiftUI
import AppKit

// MARK: - Coordinator: Search Highlighting

extension ActivityLogView.Coordinator {
    /// Highlight search matches in the text view's text storage
    func applySearchHighlighting(
        textView: NSTextView,
        searchText: String,
        caseSensitive: Bool = false,
        currentMatch: Int,
        onMatchCount: ((Int) -> Void)?
    ) {
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

        // Search only the visible portion + buffer for large texts to avoid bea
        let text = storage.string
        let textLength = (text as NSString).length
        let searchNeedle = caseSensitive ? searchText : searchText.lowercased()

        // For very large texts, limit search to last 60K chars (matches render
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
            // Save original foreground color for this range so we can restore i
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
}
