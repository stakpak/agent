import SwiftUI
import AppKit

// MARK: - Text Attachment Cells

/// Draws a solid horizontal line for markdown thematic breaks (---).
class HRLineCell: NSTextAttachmentCell {
    let color: NSColor

    init(color: NSColor) {
        self.color = color
        super.init(textCell: "")
    }

    @available(*, unavailable)
    required init(coder: NSCoder) { fatalError() }

    override func cellSize() -> NSSize { NSSize(width: 400, height: 8) }

    override func draw(withFrame cellFrame: NSRect, in controlView: NSView?) {
        let lineY = cellFrame.midY
        let path = NSBezierPath()
        path.move(to: NSPoint(x: cellFrame.minX, y: lineY))
        path.line(to: NSPoint(x: cellFrame.maxX, y: lineY))
        path.lineWidth = 0.5
        color.setStroke()
        path.stroke()
    }
}

/// Clickable copy-to-clipboard button for code blocks.
class CopyButtonCell: NSTextAttachmentCell {
    let codeText: String

    init(codeText: String) {
        self.codeText = codeText
        super.init(textCell: "")
    }

    @available(*, unavailable)
    required init(coder: NSCoder) { fatalError() }

    override func cellSize() -> NSSize { NSSize(width: 20, height: 16) }

    override func draw(withFrame cellFrame: NSRect, in controlView: NSView?) {
        let emoji = "📋" as NSString
        let attrs: [NSAttributedString.Key: Any] = [
            .font: NSFont.systemFont(ofSize: 12)
        ]
        let size = emoji.size(withAttributes: attrs)
        let x = cellFrame.midX - size.width / 2
        let y = cellFrame.midY - size.height / 2
        emoji.draw(at: NSPoint(x: x, y: y), withAttributes: attrs)
    }

    override func wantsToTrackMouse(
        for theEvent: NSEvent,
        in cellFrame: NSRect,
        of controlView: NSView?,
        atCharacterIndex charIndex: Int
    ) -> Bool { true }

    override func trackMouse(
        with theEvent: NSEvent,
        in cellFrame: NSRect,
        of controlView: NSView?,
        atCharacterIndex charIndex: Int,
        untilMouseUp flag: Bool
    ) -> Bool
    {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(codeText, forType: .string)
        // Brief flash feedback
        if let tv = controlView as? NSTextView {
            let orig = tv.backgroundColor
            tv.backgroundColor = NSColor.controlAccentColor.withAlphaComponent(0.1)
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.15) {
                tv.backgroundColor = orig
            }
        }
        return true
    }
}

// MARK: - Markdown Rendering Patterns

/// Static regex patterns used for markdown rendering
enum MarkdownPatterns {
    /// Matches image file paths
    static let imagePathPattern: NSRegularExpression? = try? NSRegularExpression(
        pattern: #"(/[^\n"'<>]+\.(?:jpg|jpeg|png|gif|tiff|bmp|webp|heic|ico|icon))"#,
        options: .caseInsensitive
    )
    /// Matches HTML files
    static let htmlPathPattern: NSRegularExpression? = try? NSRegularExpression(
        pattern: #"(/[^\n"'<>]+\.html?)"#,
        options: .caseInsensitive
    )
    /// Matches code fences
    static let fencePattern: NSRegularExpression? = try? NSRegularExpression(
        pattern: #"```(\w*)\n([\s\S]*?)\n```(?=\n|$)"#, options: []
    )
    /// Matches markdown headers
    static let headerPattern: NSRegularExpression? = try? NSRegularExpression(
        pattern: #"^(#{1,6})\s+(.*)"#, options: []
    )
    /// Matches bullet lists
    static let bulletPattern: NSRegularExpression? = try? NSRegularExpression(
        pattern: #"^(\s*)[-*+]\s+(.*)"#, options: []
    )
    /// Matches horizontal rules
    static let hrPattern: NSRegularExpression? = try? NSRegularExpression(
        pattern: #"^\s*([-*_]\s*){3,}$"#, options: []
    )
    /// Matches blockquotes
    static let blockquotePattern: NSRegularExpression? = try? NSRegularExpression(
        pattern: #"^>\s?(.*)"#, options: []
    )
    /// Strip ANSI escape sequences
    static let ansiEscapePattern: NSRegularExpression? = try? NSRegularExpression(
        pattern: #"\x1B\[[0-9;]*[A-Za-z]"#, options: []
    )
}

// MARK: - Table Rendering Helpers

/// Helper functions for markdown table parsing and rendering
enum TableRendering {
    /// Check if a line is a table separator (|---|---|---|)
    static func isTableSeparator(_ line: String) -> Bool {
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

    /// Parse a table row into cell strings
    static func parseTableRow(_ line: String) -> [String] {
        let t = line.trimmingCharacters(in: .whitespaces)
        guard t.hasPrefix("|") else { return [] }
        var inner = t[t.index(after: t.startIndex)...]
        if inner.hasSuffix("|") { inner = inner.dropLast() }
        return inner.split(separator: "|", omittingEmptySubsequences: false)
            .map { $0.trimmingCharacters(in: .whitespaces) }
    }
}
