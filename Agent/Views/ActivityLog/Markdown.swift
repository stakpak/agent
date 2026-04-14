import SwiftUI
import AppKit

// MARK: - Coordinator: Markdown Build Pipeline Top-level attributed-string buil

extension ActivityLogView.Coordinator {
    /// Fast render for incremental text updates
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

    /// / Build attributed string from text.
    nonisolated func buildAttributedString(from text: String) -> NSAttributedString {
        let baseAttrs: [NSAttributedString.Key: Any] = [
            .font: font,
            .foregroundColor: NSColor.labelColor
        ]

        // Strip ANSI escape codes from the text
        let cleanText: String
        if let rx = MarkdownPatterns.ansiEscapePattern {
            cleanText = rx.stringByReplacingMatches(
                in: text,
                range: NSRange(location: 0, length: (text as NSString).length),
                withTemplate: ""
            )
        } else {
            cleanText = text
        }

        let nsText = cleanText as NSString
        let fullRange = NSRange(location: 0, length: nsText.length)
        let imageMatches = MarkdownPatterns.imagePathPattern?.matches(in: cleanText, range: fullRange) ?? []
        let htmlMatches = MarkdownPatterns.htmlPathPattern?.matches(in: cleanText, range: fullRange) ?? []

        guard !imageMatches.isEmpty || !htmlMatches.isEmpty else {
            let rendered = renderMarkdown(cleanText)
            return Self.styleTrimBanner(rendered, font: font)
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
            return Self.styleTrimBanner(rendered, font: font)
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

        return Self.styleTrimBanner(result, font: font)
    }

    /// Apply yellow background + medium-weight styling to the trim-banner liter
    nonisolated static func styleTrimBanner(_ rendered: NSAttributedString, font: NSFont) -> NSAttributedString {
        let bannerLiteral = ScriptTab.trimBanner.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !bannerLiteral.isEmpty else { return rendered }
        let nsString = rendered.string as NSString
        let searchRange = NSRange(location: 0, length: nsString.length)
        let found = nsString.range(of: bannerLiteral, options: [], range: searchRange)
        guard found.location != NSNotFound else { return rendered }
        let mutable = NSMutableAttributedString(attributedString: rendered)
        mutable.addAttributes([
            .font: NSFont.monospacedSystemFont(ofSize: font.pointSize - 1, weight: .medium),
            .foregroundColor: NSColor.secondaryLabelColor,
            .backgroundColor: NSColor.systemYellow.withAlphaComponent(0.15)
        ], range: found)
        return mutable
    }
}
