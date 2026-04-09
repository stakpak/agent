import SwiftUI
import AppKit

// MARK: - Coordinator: Inline Markdown Rendering Bold, italic, inline code, link detection, and manual bold/italic
// fallbacks for content Apple's markdown parser misses.

extension ActivityLogView.Coordinator {
    /// Parses inline markdown (bold, italic, inline code) using Apple's AttributedString.
    nonisolated func renderInlineElements(_ text: String, baseFont: NSFont) -> NSAttributedString {
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

        // SAFETY: Skip markdown parsing if text contains Swift raw strings with backticks (e.g., #"...`..."#). Apple's
        // markdown parser mangles these. Also skip if text looks like numbered code output (e.g., "1 | code")
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
    nonisolated func linkifyURLs(_ input: NSAttributedString) -> NSAttributedString {
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
                let linked = NSMutableAttributedString(
                    string: displayText,
                    attributes: result.attributes(at: match.range.location, effectiveRange: nil)
                )
                linked.addAttribute(.link, value: urlString, range: NSRange(location: 0, length: displayText.count))
                linked.addAttribute(.foregroundColor, value: NSColor.linkColor, range: NSRange(location: 0, length: displayText.count))
                linked.addAttribute(
                    .underlineStyle,
                    value: NSUnderlineStyle.single.rawValue,
                    range: NSRange(location: 0, length: displayText.count)
                )
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
            let errorMatches = errorPattern?.matches(
                in: resultText,
                range: NSRange(location: 0, length: (resultText as NSString).length)
            ) ?? []
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
    nonisolated func applyManualBoldItalic(_ attrStr: NSMutableAttributedString, baseFont: NSFont) {
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
}
