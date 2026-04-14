import SwiftUI
import AppKit
import AgentColorSyntax

// MARK: - Coordinator: Block-Level Markdown Rendering Fenced code blocks

extension ActivityLogView.Coordinator {
    nonisolated func renderMarkdown(_ text: String) -> NSAttributedString {
        let baseAttrs: [NSAttributedString.Key: Any] = [
            .font: font,
            .foregroundColor: NSColor.labelColor
        ]

        // Check if the text is read_file output
        let readFilePattern = #"^\s*\d+\s*\|\s"#
        let lines = text.components(separatedBy: "\n").filter { !$0.isEmpty }
        let isReadFileOutput = !lines.isEmpty
            && lines.allSatisfy { line in
                line.range(of: readFilePattern, options: .regularExpression) != nil
            }

        if isReadFileOutput {
            let hl = CodeBlockHighlighter.highlight(code: text, language: "swift", font: font)
            let block = NSMutableAttributedString(attributedString: hl)
            block.addAttribute(
                .backgroundColor,
                value: CodeBlockTheme.bg,
                range: NSRange(location: 0, length: block.length)
            )
            return block
        }

        // Detect source code output (e.g. from cat command)
        let hasMarkdownStructure = lines.contains { line in
            let t = line.trimmingCharacters(in: .whitespaces)
            return t.hasPrefix("#") || t.hasPrefix("```") || t.hasPrefix("- ") || t.hasPrefix("* ")
        }
        let codeIndicators = [
            "import ",
            "func ",
            "class ",
            "struct ",
            "enum ",
            "protocol ",
            "@MainActor",
            "@Observable",
            "let ",
            "var ",
            "private ",
            "public ",
            "extension "
        ]
        let codeLineCount = lines
            .filter { line in codeIndicators.contains(where: { line.trimmingCharacters(in: .whitespaces).hasPrefix($0) }) }.count
        let isCodeOutput = !hasMarkdownStructure && lines.count >= 3 && codeLineCount >= 2

        if isCodeOutput {
            let hl = CodeBlockHighlighter.highlight(code: text, language: "swift", font: font)
            let block = NSMutableAttributedString(attributedString: hl)
            block.addAttribute(
                .backgroundColor,
                value: CodeBlockTheme.bg,
                range: NSRange(location: 0, length: block.length)
            )
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

            // Copy button only for actual source code blocks
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
            block.addAttribute(
                .backgroundColor,
                value: CodeBlockTheme.bg,
                range: NSRange(location: 0, length: block.length)
            )
            result.append(block)

            cursor = fence.range.location + fence.range.length
        }

        if cursor < nsText.length {
            result.append(renderInlineMarkdown(nsText.substring(with: NSRange(location: cursor, length: nsText.length - cursor))))
        }

        return result
    }

    /// Splits text into lines and renders block-level markdown (headers
    nonisolated func renderInlineMarkdown(_ text: String) -> NSAttributedString {
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
                   let tableAttr = renderMarkdownTable(tableLines)
                {
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

    nonisolated func isTableSeparator(_ line: String) -> Bool {
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

    nonisolated func parseTableRow(_ line: String) -> [String] {
        let t = line.trimmingCharacters(in: .whitespaces)
        guard t.hasPrefix("|") else { return [] }
        var inner = t[t.index(after: t.startIndex)...]
        if inner.hasSuffix("|") { inner = inner.dropLast() }
        return inner.split(separator: "|", omittingEmptySubsequences: false)
            .map { $0.trimmingCharacters(in: .whitespaces) }
    }

    nonisolated func renderMarkdownTable(_ lines: [String]) -> NSAttributedString? {
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
                bg: headerBg, cellFont: boldFont, align: align, border: borderColor
            ))
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
                    bg: bg, cellFont: font, align: align, border: borderColor
                ))
            }
        }

        return result
    }

    nonisolated func makeTableCell(
        text: String, table: NSTextTable, row: Int, column: Int,
        bg: NSColor, cellFont: NSFont, align: NSTextAlignment, border: NSColor
    ) -> NSAttributedString {
        let block = NSTextTableBlock(
            table: table, startingRow: row, rowSpan: 1,
            startingColumn: column, columnSpan: 1
        )
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

    /// Renders a single line, detecting block-level elements first, then inline
    nonisolated func renderMarkdownLine(_ line: String) -> NSAttributedString {
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

        // Activity log output (timestamps, grep results)
        if let highlighted = CodeBlockHighlighter.highlightActivityLogLine(line: line, font: font) {
            return linkifyURLs(highlighted)
        }

        // Regular line — inline elements only
        return renderInlineElements(line, baseFont: font)
    }
}
