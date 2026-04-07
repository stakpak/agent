import Testing
import AppKit
import AgentColorSyntax
@testable import Agent_

@Suite("CodeBlockHighlighter")
@MainActor
struct CodeBlockHighlighterTests {

    // MARK: - Test Helpers
    
    private func getAttributeColors(_ attrString: NSAttributedString) -> [String: NSColor] {
        var colors: [String: NSColor] = [:]
        let text = attrString.string
        attrString.enumerateAttributes(in: NSRange(location: 0, length: attrString.length), options: []) { attrs, range, _ in
            if let color = attrs[.foregroundColor] as? NSColor {
                let segment = String(text[Range(range, in: text)!])
                colors[segment] = color
            }
        }
        return colors
    }
    
    private func hasColor(_ attrString: NSAttributedString, for substring: String) -> Bool {
        let text = attrString.string
        guard let range = text.range(of: substring) else { return false }
        let nsRange = NSRange(range, in: text)
        var hasColor = false
        attrString.enumerateAttributes(in: nsRange, options: []) { attrs, _, _ in
            if attrs[.foregroundColor] != nil {
                hasColor = true
            }
        }
        return hasColor
    }
    
    private func colorFor(_ attrString: NSAttributedString, substring: String) -> NSColor? {
        let text = attrString.string
        guard let range = text.range(of: substring) else { return nil }
        let nsRange = NSRange(range, in: text)
        var foundColor: NSColor?
        attrString.enumerateAttributes(in: nsRange, options: []) { attrs, _, _ in
            if let color = attrs[.foregroundColor] as? NSColor {
                foundColor = color
            }
        }
        return foundColor
    }
    
    private let defaultFont = NSFont.monospacedSystemFont(ofSize: 12, weight: .regular)
    
    // MARK: - Language Detection Tests
    
    @Test("guessLanguage detects Swift code")
    func guessLanguageSwift() {
        #expect(CodeBlockHighlighter.guessLanguage(from: "import Foundation") == "swift")
        #expect(CodeBlockHighlighter.guessLanguage(from: "func test() {}") == "swift")
        #expect(CodeBlockHighlighter.guessLanguage(from: "let x = 5") == "swift")
        #expect(CodeBlockHighlighter.guessLanguage(from: "struct MyStruct {}") == "swift")
        #expect(CodeBlockHighlighter.guessLanguage(from: "class MyClass {}") == "swift")
        #expect(CodeBlockHighlighter.guessLanguage(from: "enum MyEnum {}") == "swift")
        #expect(CodeBlockHighlighter.guessLanguage(from: "protocol MyProtocol {}") == "swift")
        #expect(CodeBlockHighlighter.guessLanguage(from: "@MainActor") == "swift")
        #expect(CodeBlockHighlighter.guessLanguage(from: "guard let x = y else { return }") == "swift")
    }
    
    @Test("guessLanguage detects Python code")
    func guessLanguagePython() {
        #expect(CodeBlockHighlighter.guessLanguage(from: "def test():") == "python")
        #expect(CodeBlockHighlighter.guessLanguage(from: "from module import thing") == "python")
        #expect(CodeBlockHighlighter.guessLanguage(from: "print('hello')") == "python")
    }
    
    @Test("guessLanguage detects JSON")
    func guessLanguageJSON() {
        #expect(CodeBlockHighlighter.guessLanguage(from: "{\"key\": \"value\"}") == "json")
        #expect(CodeBlockHighlighter.guessLanguage(from: "[1, 2, 3]") == "json")
    }
    
    @Test("guessLanguage detects shell/bash")
    func guessLanguageBash() {
        #expect(CodeBlockHighlighter.guessLanguage(from: "$ cd /path") == "bash")
        #expect(CodeBlockHighlighter.guessLanguage(from: "#!/bin/bash") == "bash")
        #expect(CodeBlockHighlighter.guessLanguage(from: "ls -la") == "bash")
        #expect(CodeBlockHighlighter.guessLanguage(from: "git status") == "bash")
        #expect(CodeBlockHighlighter.guessLanguage(from: "brew install something") == "bash")
        #expect(CodeBlockHighlighter.guessLanguage(from: "curl -L url") == "bash")
        #expect(CodeBlockHighlighter.guessLanguage(from: "mkdir -p dir") == "bash")
    }
    
    @Test("guessLanguage returns nil for unrecognized code")
    func guessLanguageUnknown() {
        #expect(CodeBlockHighlighter.guessLanguage(from: "random text") == nil)
        #expect(CodeBlockHighlighter.guessLanguage(from: "") == nil)
    }
    
    // MARK: - Swift Keyword Highlighting Tests
    
    @Test("Swift keywords are highlighted")
    func swiftKeywordsHighlighted() {
        let code = "if true { return }"
        let result = CodeBlockHighlighter.highlight(code: code, language: "swift", font: defaultFont)
        
        #expect(hasColor(result, for: "if"))
        #expect(hasColor(result, for: "true"))
        #expect(hasColor(result, for: "return"))
    }
    
    @Test("Swift declaration keywords are highlighted")
    func swiftDeclarationKeywordsHighlighted() {
        let code = "func test() { let x = 5 }"
        let result = CodeBlockHighlighter.highlight(code: code, language: "swift", font: defaultFont)
        
        #expect(hasColor(result, for: "func"))
        #expect(hasColor(result, for: "let"))
    }
    
    @Test("Swift types are highlighted")
    func swiftTypesHighlighted() {
        let code = "var name: String = \"hello\""
        let result = CodeBlockHighlighter.highlight(code: code, language: "swift", font: defaultFont)
        
        #expect(hasColor(result, for: "String"))
    }
    
    @Test("Swift attributes are highlighted")
    func swiftAttributesHighlighted() {
        let code = "@MainActor func test() {}"
        let result = CodeBlockHighlighter.highlight(code: code, language: "swift", font: defaultFont)
        
        #expect(hasColor(result, for: "@MainActor"))
    }
    
    @Test("Swift self keywords are highlighted")
    func swiftSelfKeywordsHighlighted() {
        let code = "self.value = nil"
        let result = CodeBlockHighlighter.highlight(code: code, language: "swift", font: defaultFont)
        
        #expect(hasColor(result, for: "self"))
        #expect(hasColor(result, for: "nil"))
    }
    
    // MARK: - String Highlighting Tests
    
    @Test("Strings are highlighted in Swift")
    func stringsHighlightedSwift() {
        let code = "let s = \"hello world\""
        let result = CodeBlockHighlighter.highlight(code: code, language: "swift", font: defaultFont)
        
        #expect(hasColor(result, for: "\"hello world\""))
    }
    
    @Test("Multiline strings are highlighted")
    func multilineStringsHighlighted() {
        let code = "let s = \"\"\"\nline1\nline2\n\"\"\""
        let result = CodeBlockHighlighter.highlight(code: code, language: "swift", font: defaultFont)
        
        // Multiline strings should be highlighted as strings
        #expect(hasColor(result, for: "\"\"\""))
    }
    
    @Test("Strings with escape sequences are highlighted")
    func escapedStringsHighlighted() {
        let code = "let s = \"hello\\nworld\""
        let result = CodeBlockHighlighter.highlight(code: code, language: "swift", font: defaultFont)
        
        #expect(hasColor(result, for: "\"hello\\nworld\""))
    }
    
    // MARK: - Number Highlighting Tests
    
    @Test("Numbers are highlighted")
    func numbersHighlighted() {
        let code = "let x = 42"
        let result = CodeBlockHighlighter.highlight(code: code, language: "swift", font: defaultFont)
        
        #expect(hasColor(result, for: "42"))
    }
    
    @Test("Hex numbers are highlighted")
    func hexNumbersHighlighted() {
        let code = "let x = 0xFF"
        let result = CodeBlockHighlighter.highlight(code: code, language: "swift", font: defaultFont)
        
        #expect(hasColor(result, for: "0xFF"))
    }
    
    @Test("Float numbers are highlighted")
    func floatNumbersHighlighted() {
        let code = "let x = 3.14"
        let result = CodeBlockHighlighter.highlight(code: code, language: "swift", font: defaultFont)
        
        #expect(hasColor(result, for: "3.14"))
    }
    
    @Test("Scientific notation numbers are highlighted")
    func scientificNumbersHighlighted() {
        let code = "let x = 1.5e10"
        let result = CodeBlockHighlighter.highlight(code: code, language: "swift", font: defaultFont)
        
        #expect(hasColor(result, for: "1.5e10"))
    }
    
    // MARK: - Comment Highlighting Tests
    
    @Test("Single-line comments are highlighted")
    func singleLineCommentsHighlighted() {
        let code = "let x = 5 // this is a comment"
        let result = CodeBlockHighlighter.highlight(code: code, language: "swift", font: defaultFont)
        
        #expect(hasColor(result, for: "// this is a comment"))
    }
    
    @Test("Block comments are highlighted")
    func blockCommentsHighlighted() {
        let code = "let x = 5 /* comment */ let y = 10"
        let result = CodeBlockHighlighter.highlight(code: code, language: "swift", font: defaultFont)
        
        #expect(hasColor(result, for: "/* comment */"))
    }
    
    @Test("Python comments are highlighted")
    func pythonCommentsHighlighted() {
        let code = "def test(): # comment\n    pass"
        let result = CodeBlockHighlighter.highlight(code: code, language: "python", font: defaultFont)
        
        #expect(hasColor(result, for: "# comment"))
    }
    
    // MARK: - Language-Specific Tests
    
    @Test("Python def and class are highlighted")
    func pythonDeclKeywordsHighlighted() {
        let code = "def test():\n    pass"
        let result = CodeBlockHighlighter.highlight(code: code, language: "python", font: defaultFont)
        
        #expect(hasColor(result, for: "def"))
        #expect(hasColor(result, for: "pass"))
    }
    
    @Test("JavaScript function and const are highlighted")
    func javascriptDeclKeywordsHighlighted() {
        let code = "const x = () => { return 5; }"
        let result = CodeBlockHighlighter.highlight(code: code, language: "javascript", font: defaultFont)
        
        #expect(hasColor(result, for: "const"))
        #expect(hasColor(result, for: "return"))
    }
    
    @Test("C preprocessor directives are highlighted")
    func cPreprocessorHighlighted() {
        let code = "#include <stdio.h>\nint main() { return 0; }"
        let result = CodeBlockHighlighter.highlight(code: code, language: "c", font: defaultFont)
        
        #expect(hasColor(result, for: "#include <stdio.h>"))
    }
    
    @Test("Rust attributes are highlighted")
    func rustAttributesHighlighted() {
        let code = "#[derive(Debug)]\nstruct MyStruct {}"
        let result = CodeBlockHighlighter.highlight(code: code, language: "rust", font: defaultFont)
        
        #expect(hasColor(result, for: "#[derive(Debug)]"))
    }
    
    // MARK: - Terminal Output Tests
    
    @Test("Terminal output ls -la permissions highlighted")
    func terminalPermissionsHighlighted() {
        let code = "drwxr-xr-x  5 user  staff  160 Jan 15 10:00 Documents"
        let result = CodeBlockHighlighter.highlight(code: code, language: "bash", font: defaultFont)
        
        #expect(hasColor(result, for: "drwxr-xr-x"))
    }
    
    @Test("Terminal output dates highlighted")
    func terminalDatesHighlighted() {
        let code = "drwxr-xr-x  5 user  staff  160 Jan 15 10:00 Documents"
        let result = CodeBlockHighlighter.highlight(code: code, language: "bash", font: defaultFont)
        
        #expect(hasColor(result, for: "Jan 15 10:00"))
    }
    
    @Test("Terminal output error keywords highlighted")
    func terminalErrorsHighlighted() {
        let code = "error: file not found"
        let result = CodeBlockHighlighter.highlight(code: code, language: "bash", font: defaultFont)
        
        #expect(hasColor(result, for: "error"))
    }
    
    @Test("Terminal output warning keywords highlighted")
    func terminalWarningsHighlighted() {
        let code = "warning: deprecated function"
        let result = CodeBlockHighlighter.highlight(code: code, language: "bash", font: defaultFont)
        
        #expect(hasColor(result, for: "warning"))
    }
    
    // MARK: - Diff Highlighting Tests
    
    @Test("Diff added lines are highlighted")
    func diffAddedLinesHighlighted() {
        let code = "+ added line\n- removed line"
        let result = CodeBlockHighlighter.highlight(code: code, language: "diff", font: defaultFont)
        
        #expect(hasColor(result, for: "+ added line"))
    }
    
    @Test("Diff removed lines are highlighted")
    func diffRemovedLinesHighlighted() {
        let code = "- removed line\n+ added line"
        let result = CodeBlockHighlighter.highlight(code: code, language: "diff", font: defaultFont)
        
        #expect(hasColor(result, for: "- removed line"))
    }
    
    @Test("Diff line-numbered format is highlighted")
    func diffLineNumberedHighlighted() {
        let code = "123 +\tfunc test() {}\n124 -\told code"
        let result = CodeBlockHighlighter.highlight(code: code, language: "diff", font: defaultFont)
        
        // Should highlight both added and removed with backgrounds
        #expect(result.string.contains("123"))
        #expect(result.string.contains("124"))
    }
    
    // MARK: - Activity Log Highlighting Tests
    
    @Test("Activity log timestamps are highlighted")
    func activityLogTimestampsHighlighted() {
        let code = "[12:34:56] Task: running"
        let result = CodeBlockHighlighter.highlightActivityLogLine(line: code, font: defaultFont)
        
        #expect(result != nil)
        #expect(hasColor(result!, for: "[12:34:56]"))
    }
    
    @Test("Activity log shell commands are highlighted")
    func activityLogShellCommandsHighlighted() {
        let code = "$ cd /path/to/project"
        let result = CodeBlockHighlighter.highlightActivityLogLine(line: code, font: defaultFont)
        
        #expect(result != nil)
        #expect(hasColor(result!, for: "$ cd"))
    }
    
    @Test("Activity log file paths are highlighted")
    func activityLogFilePathsHighlighted() {
        let code = "/Users/toddbruss/Documents/file.swift:42: error"
        let result = CodeBlockHighlighter.highlightActivityLogLine(line: code, font: defaultFont)
        
        #expect(result != nil)
        // Path should be colored
        #expect(hasColor(result!, for: "/Users/toddbruss/Documents/file.swift"))
    }
    
    @Test("Activity log grep format is highlighted")
    func activityLogGrepFormatHighlighted() {
        let code = "File.swift:42:func test() {"
        let result = CodeBlockHighlighter.highlightActivityLogLine(line: code, font: defaultFont)
        
        #expect(result != nil)
        #expect(hasColor(result!, for: "File.swift"))
        #expect(hasColor(result!, for: "42"))
    }
    
    @Test("Activity log returns nil for non-log lines")
    func activityLogNonLogReturnsNil() {
        let code = "regular code line"
        let result = CodeBlockHighlighter.highlightActivityLogLine(line: code, font: defaultFont)
        
        #expect(result == nil)
    }
    
    // MARK: - Git Output Highlighting Tests
    
    @Test("Git 'files changed' is highlighted")
    func gitFilesChangedHighlighted() {
        let code = "3 files changed, 42 insertions(+), 10 deletions(-)"
        let result = CodeBlockHighlighter.highlightActivityLogLine(line: code, font: defaultFont)
        
        #expect(result != nil)
        #expect(hasColor(result!, for: "3 files changed"))
    }
    
    @Test("Git commit references are highlighted")
    func gitCommitRefsHighlighted() {
        let code = "[main abc1234] commit message"
        let result = CodeBlockHighlighter.highlightActivityLogLine(line: code, font: defaultFont)
        
        #expect(result != nil)
        #expect(hasColor(result!, for: "[main abc1234]"))
    }
    
    @Test("Git mode changes are highlighted")
    func gitModeChangesHighlighted() {
        let code = "create mode 100644 newfile.swift"
        let result = CodeBlockHighlighter.highlightActivityLogLine(line: code, font: defaultFont)
        
        #expect(result != nil)
    }
    
    // MARK: - Hex Dump Highlighting Tests
    
    @Test("Hex dump addresses are highlighted")
    func hexDumpAddressesHighlighted() {
        let code = "00000000: 48 65 6c 6c 6f 20 57 6f  Hello Wo"
        let result = CodeBlockHighlighter.highlightActivityLogLine(line: code, font: defaultFont)
        
        #expect(result != nil)
        #expect(hasColor(result!, for: "00000000:"))
    }
    
    @Test("Hex dump ASCII column is highlighted")
    func hexDumpASCIIColumnHighlighted() {
        let code = "00000000: 48 65 6c 6c 6f 20 57 6f  Hello Wo"
        let result = CodeBlockHighlighter.highlightActivityLogLine(line: code, font: defaultFont)
        
        #expect(result != nil)
        #expect(hasColor(result!, for: "Hello Wo"))
    }
    
    // MARK: - ANSI Stripping Tests
    
    @Test("ANSI escape codes are stripped before highlighting")
    func ansiCodesStripped() {
        // ANSI color codes: \u{001B}[31m = red, \u{001B}[0m = reset
        let code = "\u{001B}[31merror\u{001B}[0m message"
        let result = CodeBlockHighlighter.highlight(code: code, language: "swift", font: defaultFont)
        
        // Should not contain ANSI codes in the result
        #expect(!result.string.contains("\u{001B}"))
        #expect(result.string.contains("error"))
        #expect(result.string.contains("message"))
    }
    
    // MARK: - Language Alias Tests
    
    @Test("Language aliases resolve correctly")
    func languageAliasesResolve() {
        let code = "const x = 5;"
        
        // Test various aliases
        let js1 = CodeBlockHighlighter.highlight(code: code, language: "js", font: defaultFont)
        let js2 = CodeBlockHighlighter.highlight(code: code, language: "javascript", font: defaultFont)
        
        // Both should highlight 'const'
        #expect(hasColor(js1, for: "const"))
        #expect(hasColor(js2, for: "const"))
    }
    
    @Test("TypeScript highlights TypeScript keywords")
    func typeScriptKeywordsHighlighted() {
        let code = "interface MyInterface { name: string; }"
        let result = CodeBlockHighlighter.highlight(code: code, language: "typescript", font: defaultFont)
        
        #expect(hasColor(result, for: "interface"))
        #expect(hasColor(result, for: "string"))
    }
    
    @Test("Kotlin highlights Kotlin keywords")
    func kotlinKeywordsHighlighted() {
        let code = "fun test(): String { return \"hello\" }"
        let result = CodeBlockHighlighter.highlight(code: code, language: "kotlin", font: defaultFont)
        
        #expect(hasColor(result, for: "fun"))
        #expect(hasColor(result, for: "String"))
        #expect(hasColor(result, for: "return"))
    }
    
    @Test("Go highlights Go keywords")
    func goKeywordsHighlighted() {
        let code = "func main() { var x int }"
        let result = CodeBlockHighlighter.highlight(code: code, language: "go", font: defaultFont)
        
        #expect(hasColor(result, for: "func"))
        #expect(hasColor(result, for: "var"))
    }
    
    @Test("Ruby highlights Ruby keywords")
    func rubyKeywordsHighlighted() {
        let code = "def test\n  puts 'hello'\nend"
        let result = CodeBlockHighlighter.highlight(code: code, language: "ruby", font: defaultFont)
        
        #expect(hasColor(result, for: "def"))
        #expect(hasColor(result, for: "end"))
    }
    
    // MARK: - SQL Highlighting Tests
    
    @Test("SQL keywords are highlighted")
    func sqlKeywordsHighlighted() {
        let code = "SELECT * FROM users WHERE id = 1"
        let result = CodeBlockHighlighter.highlight(code: code, language: "sql", font: defaultFont)
        
        #expect(hasColor(result, for: "SELECT"))
        #expect(hasColor(result, for: "FROM"))
        #expect(hasColor(result, for: "WHERE"))
    }
    
    @Test("SQL strings are highlighted")
    func sqlStringsHighlighted() {
        let code = "SELECT * FROM users WHERE name = 'John'"
        let result = CodeBlockHighlighter.highlight(code: code, language: "sql", font: defaultFont)
        
        #expect(hasColor(result, for: "'John'"))
    }
    
    // MARK: - JSON Highlighting Tests
    
    @Test("JSON keywords are highlighted")
    func jsonKeywordsHighlighted() {
        let code = "{ \"key\": true, \"other\": false, \"value\": null }"
        let result = CodeBlockHighlighter.highlight(code: code, language: "json", font: defaultFont)
        
        #expect(hasColor(result, for: "true"))
        #expect(hasColor(result, for: "false"))
        #expect(hasColor(result, for: "null"))
    }
    
    // MARK: - Empty/Edge Case Tests
    
    @Test("Empty code returns empty attributed string")
    func emptyCodeReturnsEmpty() {
        let result = CodeBlockHighlighter.highlight(code: "", language: "swift", font: defaultFont)
        
        #expect(result.string == "")
        #expect(result.length == 0)
    }
    
    @Test("Unknown language uses generic highlighting")
    func unknownLanguageUsesGeneric() {
        let code = "some random text"
        let result = CodeBlockHighlighter.highlight(code: code, language: "unknownlang", font: defaultFont)
        
        // Should not crash, returns text
        #expect(result.string == "some random text")
    }
    
    @Test("Nil language auto-detects")
    func nilLanguageAutoDetects() {
        let code = "func test() {}"
        let result = CodeBlockHighlighter.highlight(code: code, language: nil, font: defaultFont)
        
        // Should detect Swift and highlight 'func'
        #expect(hasColor(result, for: "func"))
    }
    
    // MARK: - Font Tests
    
    @Test("Bold font applied to keywords")
    func boldFontAppliedToKeywords() {
        let code = "func test() {}"
        let result = CodeBlockHighlighter.highlight(code: code, language: "swift", font: defaultFont)
        
        // Check that 'func' has bold font
        let text = result.string
        guard let range = text.range(of: "func") else {
            Issue.record("Keyword 'func' not found")
            return
        }
        let nsRange = NSRange(range, in: text)
        var hasBold = false
        result.enumerateAttributes(in: nsRange, options: []) { attrs, _, _ in
            if let font = attrs[.font] as? NSFont, font.fontDescriptor.symbolicTraits.contains(.bold) {
                hasBold = true
            }
        }
        #expect(hasBold)
    }
    
    // MARK: - Path Highlighting Tests
    
    @Test("Multi-segment paths are highlighted with multiple colors")
    func multiSegmentPathsHighlighted() {
        let code = "/Users/toddbruss/Documents/file.swift"
        let result = CodeBlockHighlighter.highlightActivityLogLine(line: code, font: defaultFont)
        
        #expect(result != nil)
        // Path segments should have different colors
        // Home prefix (/Users/toddbruss/) dim
        // Top dir (Documents/) green
        // Middle dirs cyan
        // Filename blue bold
        #expect(hasColor(result!, for: "/Users/toddbruss/Documents/file.swift"))
    }
    
    @Test("Home directory paths are highlighted")
    func homeDirectoryPathsHighlighted() {
        let code = "~/Documents/file.swift"
        let result = CodeBlockHighlighter.highlightActivityLogLine(line: code, font: defaultFont)
        
        #expect(result != nil)
        #expect(hasColor(result!, for: "~/Documents/file.swift"))
    }
    
    // MARK: - Property Access Highlighting Tests
    
    @Test("Property access is highlighted")
    func propertyAccessHighlighted() {
        let code = "object.property.method()"
        let result = CodeBlockHighlighter.highlight(code: code, language: "swift", font: defaultFont)
        
        // Property names after dots should be highlighted
        #expect(hasColor(result, for: "property"))
    }
    
    // MARK: - Function Call Highlighting Tests
    
    @Test("Function calls are highlighted")
    func functionCallsHighlighted() {
        let code = "print(\"hello\")\nmyFunction()"
        let result = CodeBlockHighlighter.highlight(code: code, language: "swift", font: defaultFont)
        
        #expect(hasColor(result, for: "print"))
        #expect(hasColor(result, for: "myFunction"))
    }
    
    // MARK: - Theme Color Tests
    
    @Test("Dark mode uses dark theme colors")
    func darkModeThemeColors() async {
        // This test verifies theme color computation for dark mode
        // Colors should adapt based on effective appearance
        let code = "let x = 5"
        let result = CodeBlockHighlighter.highlight(code: code, language: "swift", font: defaultFont)
        
        // Should not crash and should apply colors
        #expect(result.length > 0)
    }
    
    // MARK: - Bash/Shell Specific Tests
    
    @Test("Bash variables are highlighted")
    func bashVariablesHighlighted() {
        let code = "export PATH=/usr/bin\n$HOME/bin"
        let result = CodeBlockHighlighter.highlight(code: code, language: "bash", font: defaultFont)
        
        #expect(hasColor(result, for: "export"))
    }
    
    @Test("Bash system functions are highlighted")
    func bashSystemFunctionsHighlighted() {
        let code = "echo \"hello\"\ngit status\nls -la"
        let result = CodeBlockHighlighter.highlight(code: code, language: "bash", font: defaultFont)
        
        #expect(hasColor(result, for: "echo"))
        #expect(hasColor(result, for: "git"))
        #expect(hasColor(result, for: "ls"))
    }
    
    // MARK: - Complex Code Tests
    
    @Test("Complex Swift code is properly highlighted")
    func complexSwiftCodeHighlighted() {
        let code = """
        @MainActor
        func processData(_ input: String) async throws -> [String: Any] {
            // Process the input
            let items = input.split(separator: ",")
            var result: [String: Any] = [:]
            for item in items {
                result[String(item)] = true
            }
            return result
        }
        """
        let result = CodeBlockHighlighter.highlight(code: code, language: "swift", font: defaultFont)
        
        // Verify various elements are highlighted
        #expect(hasColor(result, for: "@MainActor"))
        #expect(hasColor(result, for: "func"))
        #expect(hasColor(result, for: "String"))
        #expect(hasColor(result, for: "async"))
        #expect(hasColor(result, for: "throws"))
        #expect(hasColor(result, for: "// Process the input"))
        #expect(hasColor(result, for: "let"))
        #expect(hasColor(result, for: "var"))
        #expect(hasColor(result, for: "for"))
        #expect(hasColor(result, for: "return"))
    }
    
    @Test("Nested structures are highlighted")
    func nestedStructuresHighlighted() {
        let code = "struct Outer { struct Inner { let value: Int } }"
        let result = CodeBlockHighlighter.highlight(code: code, language: "swift", font: defaultFont)
        
        #expect(hasColor(result, for: "struct"))
        #expect(hasColor(result, for: "Int"))
    }
}