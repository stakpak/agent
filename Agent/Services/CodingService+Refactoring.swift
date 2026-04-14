import Foundation

extension CodingService {
    // MARK: - Convert If-Chains to Switch/Case

    /// Convert `if name == "xxx" { ...
    static func convertIfToSwitch(path: String) -> String {
        let expanded = (path as NSString).expandingTildeInPath
        guard let data = FileManager.default.contents(atPath: expanded),
              let source = String(data: data, encoding: .utf8) else
        {
            return "Error: cannot read \(path)"
        }

        let lines = source.components(separatedBy: "\n")
        var newLines = [String]()
        var i = 0
        var switchOpen = false
        var converted = 0

        while i < lines.count {
            let line = lines[i]
            let stripped = line.trimmingCharacters(in: .whitespaces)

            // Match: if name == "xxx" {
            if stripped.hasPrefix("if name == \"") {
                let indent = String(line.prefix(while: { $0 == " " || $0 == "\t" }))

                // Extract all tool names from the line
                var names = [String]()
                let searchLine = line as NSString
                guard let namePattern = try? NSRegularExpression(pattern: #"name == "([^"]+)""#) else { continue }
                let matches = namePattern.matches(in: line, range: NSRange(location: 0, length: searchLine.length))
                for m in matches {
                    if m.numberOfRanges >= 2 {
                        names.append(searchLine.substring(with: m.range(at: 1)))
                    }
                }

                if !names.isEmpty {
                    if !switchOpen {
                        newLines.append("\(indent)switch name {")
                        switchOpen = true
                    }

                    let caseLabel = names.map { "\"\($0)\"" }.joined(separator: ", ")
                    newLines.append("\(indent)case \(caseLabel):")

                    // Track braces to find end of if block
                    var depth = line.filter({ $0 == "{" }).count - line.filter({ $0 == "}" }).count
                    i += 1
                    while i < lines.count && depth > 0 {
                        depth += lines[i].filter({ $0 == "{" }).count - lines[i].filter({ $0 == "}" }).count
                        if depth <= 0 { break } // closing brace — skip it
                        newLines.append(lines[i])
                        i += 1
                    }
                    newLines.append("")
                    converted += 1
                    i += 1
                    continue
                }
            }

            // Detect fallback/default section and close switch
            if switchOpen && stripped.hasPrefix("// Fallback") {
                let indent = String(line.prefix(while: { $0 == " " || $0 == "\t" }))
                newLines.append("\(indent)default:")
                i += 1
                while i < lines.count {
                    let fl = lines[i]
                    if fl.trimmingCharacters(in: .whitespaces) == "}" {
                        newLines.append("\(indent)}") // close switch
                        let outerIndent = String(indent.dropLast(4))
                        newLines.append("\(outerIndent)}") // close func
                        switchOpen = false
                        i += 1
                        break
                    }
                    newLines.append(fl)
                    i += 1
                }
                continue
            }

            newLines.append(line)
            i += 1
        }

        if converted == 0 {
            return "No `if name ==` blocks found to convert"
        }

        let result = newLines.joined(separator: "\n")
        do {
            try result.write(toFile: expanded, atomically: true, encoding: .utf8)
            return "Converted \(converted) if-blocks to switch/case in \((expanded as NSString).lastPathComponent)"
        } catch {
            return "Error writing: \(error.localizedDescription)"
        }
    }

    // MARK: - Extract to File

    /// Extract a function (by name) from a Swift file into a new file.
    static func extractFunctionToFile(sourcePath: String, functionName: String, newFileName: String) -> String {
        let expanded = (sourcePath as NSString).expandingTildeInPath
        guard let data = FileManager.default.contents(atPath: expanded),
              let source = String(data: data, encoding: .utf8) else
        {
            return "Error: cannot read \(sourcePath)"
        }

        let lines = source.components(separatedBy: "\n")
        let dir = (expanded as NSString).deletingLastPathComponent

        // Collect imports
        var imports = [String]()
        var seenImports = Set<String>()
        for line in lines {
            let t = line.trimmingCharacters(in: .whitespaces)
            if t.hasPrefix("import ") || t.hasPrefix("@preconcurrency import ") {
                if seenImports.insert(t).inserted { imports.append(t) }
            }
        }

        // Find the extension/class wrapper
        var extensionLine = ""
        for line in lines {
            let t = line.trimmingCharacters(in: .whitespaces)
            if t.hasPrefix("extension ") || t.hasPrefix("public extension ") || t.hasPrefix("class ") {
                extensionLine = t
                break
            }
        }

        // Find the function by name
        var funcStart = -1
        var funcEnd = -1
        var braceDepth = 0

        for (idx, line) in lines.enumerated() {
            let t = line.trimmingCharacters(in: .whitespaces)
            if t.contains("func \(functionName)") && funcStart == -1 {
                funcStart = idx
                // Include preceding comments/attributes
                var commentStart = idx
                while commentStart > 0 {
                    let prev = lines[commentStart - 1].trimmingCharacters(in: .whitespaces)
                    if prev.hasPrefix("///") || prev.hasPrefix("//") || prev.has
                        commentStart -= 1
                    } else { break }
                }
                funcStart = commentStart
                braceDepth = 0
            }
            if funcStart != -1 && funcEnd == -1 {
                braceDepth += line.filter({ $0 == "{" }).count
                braceDepth -= line.filter({ $0 == "}" }).count
                if braceDepth <= 0 && idx > funcStart {
                    funcEnd = idx
                }
            }
        }

        guard funcStart != -1 && funcEnd != -1 else {
            return "Error: function '\(functionName)' not found in \(sourcePath)"
        }

        let extractedLines = Array(lines[funcStart...funcEnd])

        // Build new file
        var newContent = imports.joined(separator: "\n") + "\n\n"
        if !extensionLine.isEmpty {
            newContent += (extensionLine.hasSuffix("{") ? extensionLine : extensionLine + " {") + "\n\n"
        }
        // Fix private → internal
        for line in extractedLines {
            let t = line.trimmingCharacters(in: .whitespaces)
            var fixed = line
            if t.hasPrefix("private func ") { fixed = line.replacingOccurrences(of: "private func ", with: "func ") }
            if t.hasPrefix("private static func ") { fixed = line.replacingOccurrences(of: "private static func ", with: "static func ") }
            newContent += fixed + "\n"
        }
        if !extensionLine.isEmpty { newContent += "}\n" }

        // Write new file
        let newPath = (dir as NSString).appendingPathComponent(newFileName)
        do {
            try newContent.write(toFile: newPath, atomically: true, encoding: .utf8)
        } catch {
            return "Error writing \(newFileName): \(error.localizedDescription)"
        }

        // Remove from original
        var remaining = lines
        remaining.removeSubrange(funcStart...funcEnd)
        do {
            try remaining.joined(separator: "\n").write(toFile: expanded, atomically: true, encoding: .utf8)
        } catch {
            return "Error updating original: \(error.localizedDescription)"
        }

        return
            "Extracted '\(functionName)' (\(extractedLines.count) lines) "
            + "→ \(newFileName)\n"
            + "Original: \(lines.count) → \(remaining.count) lines\n"
            + "Use xcode (action: add_file) to add \(newPath) "
            + "to project, then build."
    }
}
