import Foundation

extension CodingService {
    // MARK: - Split File

    /// / Split a Swift file into separate files.
    static func splitFile(path: String, mode: String = "declarations") -> String {
        let expanded = (path as NSString).expandingTildeInPath
        guard let data = FileManager.default.contents(atPath: expanded),
              let source = String(data: data, encoding: .utf8) else
        {
            return "Error: cannot read \(path)"
        }

        if mode == "handlers" {
            return splitToolHandlers(source: source, path: expanded)
        }

        let lines = source.components(separatedBy: "\n")
        let baseName = (expanded as NSString).lastPathComponent.replacingOccurrences(of: ".swift", with: "")
        let dir = (expanded as NSString).deletingLastPathComponent

        // Collect ALL imports from entire file (not just top)
        var imports = [String]()
        var seenImports = Set<String>()
        // Collect file-level variables/constants (Logger, etc.) at brace depth
        var fileLevelVars = [String]()
        for line in lines {
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            if trimmed.hasPrefix("import ") || trimmed.hasPrefix("@preconcurrency import ") {
                if seenImports.insert(trimmed).inserted { imports.append(trimmed) }
            }
        }
        // Scan for file-level let/var at brace depth 0
        var scanDepth = 0
        for line in lines {
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            if scanDepth == 0 && !trimmed.hasPrefix("import ") && !trimmed.hasPrefix("@preconcurrency") {
                if trimmed.hasPrefix("private let ") || trimmed.hasPrefix("private var ") ||
                    trimmed.hasPrefix("fileprivate let ") || trimmed.hasPrefix("fileprivate var ") ||
                    trimmed.hasPrefix("let ") || trimmed.hasPrefix("var ") ||
                    trimmed.hasPrefix("nonisolated(unsafe)")
                {
                    // File-level variable — needs to be in every split file Cha
                    let fixed = line.replacingOccurrences(of: "private let ", with: "let ")
                        .replacingOccurrences(of: "private var ", with: "var ")
                        .replacingOccurrences(of: "fileprivate let ", with: "let ")
                        .replacingOccurrences(of: "fileprivate var ", with: "var ")
                    fileLevelVars.append(fixed)
                }
            }
            scanDepth += line.filter({ $0 == "{" }).count
            scanDepth -= line.filter({ $0 == "}" }).count
            scanDepth = max(0, scanDepth)
        }

        var headerComments = [String]()
        var declarations: [(name: String, startLine: Int, lines: [String])] = []
        var currentDecl: (name: String, startLine: Int, lines: [String])?
        var braceDepth = 0
        var inHeader = true

        for (i, line) in lines.enumerated() {
            let trimmed = line.trimmingCharacters(in: .whitespaces)

            // Skip imports (already collected)
            if trimmed.hasPrefix("import ") || trimmed.hasPrefix("@preconcurrency import ") {
                inHeader = false
                continue
            }

            // Skip file-level vars (already collected)
            if braceDepth == 0 && (
                trimmed.hasPrefix("private let ") || trimmed.hasPrefix("private var ") ||
                    trimmed.hasPrefix("fileprivate let ") || trimmed.hasPrefix("fileprivate var ") ||
                    trimmed.hasPrefix("nonisolated(unsafe)")
            )
            {
                if !trimmed.hasPrefix("extension ") && !trimmed.hasPrefix("class ") &&
                    !trimmed.hasPrefix("struct ") && !trimmed.hasPrefix("enum ")
                {
                    continue
                }
            }

            // Collect header comments before any declaration
            /// ") || trimmed.hasPrefix("*") || trimmed.isEmpty) { headerComment
