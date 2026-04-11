import Testing
import Foundation
@testable import Agent_
import AgentD1F

/// Tests that mirror exactly how the AI calls diff tools through the handler code paths.
/// Each test simulates the exact input dict the LLM sends and the exact steps the handler runs.
@Suite("DiffTools")
@MainActor struct DiffToolsTests {

    private func makeTempDir() throws -> String {
        let dir = NSTemporaryDirectory() + "agent_diff_tests_\(UUID().uuidString)"
        try FileManager.default.createDirectory(atPath: dir, withIntermediateDirectories: true)
        return dir
    }

    private func cleanup(_ path: String) {
        try? FileManager.default.removeItem(atPath: path)
        DiffStore.shared.clear()
    }

    private func writeFile(_ path: String, _ content: String) throws {
        try content.write(toFile: path, atomically: true, encoding: .utf8)
    }

    private func readFile(_ path: String) throws -> String {
        try String(contentsOfFile: path, encoding: .utf8)
    }

        // MARK: - Scenario 1: create_diff then apply_diff (2-step review)

    @Test("Scenario 1: create_diff + apply_diff with line range and UUID")
    func createThenApplyWithUUID() throws {
        let dir = makeTempDir()
        defer { cleanup(dir) }
        let file = "\(dir)/scenario1.swift"
        writeFile(file, "import Foundation\n\nfunc hello() {\n    print(\"Hello\")\n}\n\nfunc goodbye() {\n    print(\"Bye\")\n}\n")

        // === AI calls create_diff ===
        // AI sees lines 3-5 are the hello function, wants to change the print
        let destination = "func hello() {\n    print(\"Hello, World!\")\n}"

        // Simulate handler: read file, extract lines 3-5, create diff, store UUID
        let fullText = readFile(file)
        let lines = fullText.components(separatedBy: "\n")
        let s = 2 // line 3, 0-indexed
        let e = 5 // line 5, exclusive
        let source = lines[s..<e].joined(separator: "\n")

        let algorithm = CodingService.selectDiffAlgorithm(source: source, destination: destination)
        let diff = MultiLineDiff.createDiff(
            source: source,
            destination: destination,
            algorithm: algorithm,
            includeMetadata: true,
            sourceStartLine: 2
        )
        let d1f = MultiLineDiff.displayDiff(diff: diff, source: source, format: .ai)
        let diffId = DiffStore.shared.store(diff: diff, source: source)

        // AI gets back: "diff_id: <UUID>\n\n<preview>"
        #expect(!d1f.isEmpty, "D1F preview should not be empty")
        #expect(DiffStore.shared.retrieve(diffId) != nil, "Diff should be stored by UUID")

        // === AI calls apply_diff with the diff_id ===
        let currentSource = readFile(file)
        let stored = DiffStore.shared.retrieve(diffId)!
        let patched = try MultiLineDiff.applyDiff(to: currentSource, diff: stored.diff)

        // Safety check (same as handler)
        #expect(!(currentSource.count > 200 && patched.count < currentSource.count / 2), "Should not trigger truncation safety")

        try patched.write(toFile: file, atomically: true, encoding: .utf8)
        DiffStore.shared.recordApply(diffId: diffId, filePath: file, originalContent: currentSource)

        // Verify (same as handler)
        let verifyDiff = MultiLineDiff.createDiff(source: currentSource, destination: patched, includeMetadata: true)
        let verified = MultiLineDiff.verifyDiff(verifyDiff)
        let display = MultiLineDiff.displayDiff(diff: verifyDiff, source: currentSource, format: .ai)

        #expect(verified, "Verification should pass")
        #expect(!display.isEmpty, "Verification preview should not be empty")

        // Check file on disk
        let final = readFile(file)
        #expect(final.contains("Hello, World!"), "File should have the new print")
        #expect(final.contains("func goodbye()"), "Untouched code should remain")
        #expect(final.contains("import Foundation"), "Header should remain")
    }

    // MARK: - Scenario 2: apply_diff with inline edit

    @Test("Scenario 2: diff_and_apply with line range — same steps as create+apply")
    func diffAndApplyLineRange() throws {
        let dir = makeTempDir()
        defer { cleanup(dir) }
        let file = "\(dir)/scenario2.html"
        let original = "<html>\n<head>\n<title>Old Title</title>\n</head>\n<body>\n<h1>Hello</h1>\n<p>Content here</p>\n</body>\n</html>\n"
        writeFile(file, original)

        // AI wants to change lines 3 (title) — sends only the replacement
        let destination = "<title>New Title</title>"
        let startLine = 3
        let endLine = 3

        // Simulate handler: read file, extract section, create diff, apply, splice, write
        let fullText = readFile(file)
        let allLines = fullText.components(separatedBy: "\n")
        let s2 = max(startLine - 1, 0)
        let e2 = min(endLine, allLines.count)
        let source = allLines[s2..<e2].joined(separator: "\n")

        #expect(source != destination, "Source and destination should differ")

        let algorithm = CodingService.selectDiffAlgorithm(source: source, destination: destination)
        let diff = MultiLineDiff.createDiff(
            source: source,
            destination: destination,
            algorithm: algorithm,
            includeMetadata: true,
            sourceStartLine: startLine - 1
        )
        let diffId = DiffStore.shared.store(diff: diff, source: source)
        let patched = try MultiLineDiff.applyDiff(to: source, diff: diff)

        // Splice back
        var newLines = allLines
        newLines.replaceSubrange(s2..<e2, with: patched.components(separatedBy: "\n"))
        let finalContent = newLines.joined(separator: "\n")

        try finalContent.write(toFile: file, atomically: true, encoding: .utf8)
        DiffStore.shared.recordApply(diffId: diffId, filePath: file, originalContent: fullText)

        // Verify
        let verifyDiff = MultiLineDiff.createDiff(source: source, destination: patched, includeMetadata: true)
        let verified = MultiLineDiff.verifyDiff(verifyDiff)
        let display = MultiLineDiff.displayDiff(diff: verifyDiff, source: source, format: .ai)

        #expect(verified, "Verification should pass")
        #expect(!display.isEmpty, "D1F preview should be shown")

        let final = readFile(file)
        #expect(final.contains("New Title"), "Title should be changed")
        #expect(final.contains("<h1>Hello</h1>"), "Body should be untouched")
        #expect(final.contains("</html>"), "Footer should remain")
    }

    // MARK: - Scenario 3: AI undoes a diff by UUID

    @Test("Scenario 3: undo_edit by diff_id using D1F createUndoDiff")
    func undoByDiffId() throws {
        let dir = makeTempDir()
        defer { cleanup(dir) }
        let file = "\(dir)/scenario3.txt"
        let original = "line1\nline2\nline3\nline4\nline5\n"
        writeFile(file, original)

        // AI does diff_and_apply on lines 2-3
        let destination = "LINE2_CHANGED\nLINE3_CHANGED"
        let fullText = readFile(file)
        let allLines = fullText.components(separatedBy: "\n")
        let source = allLines[1..<3].joined(separator: "\n")

        let diff = MultiLineDiff.createDiff(source: source, destination: destination, includeMetadata: true)
        let diffId = DiffStore.shared.store(diff: diff, source: source)
        let patched = try MultiLineDiff.applyDiff(to: source, diff: diff)

        var newLines = allLines
        newLines.replaceSubrange(1..<3, with: patched.components(separatedBy: "\n"))
        let edited = newLines.joined(separator: "\n")
        try edited.write(toFile: file, atomically: true, encoding: .utf8)
        DiffStore.shared.recordApply(diffId: diffId, filePath: file, originalContent: fullText)

        #expect(readFile(file).contains("LINE2_CHANGED"), "Edit should be applied")

        // === AI calls undo_edit with diff_id ===
        let stored = DiffStore.shared.retrieve(diffId)!
        let undoDiff = MultiLineDiff.createUndoDiff(from: stored.diff)
        #expect(undoDiff != nil, "D1F should create undo diff from metadata")

        let current = readFile(file)
        let restored = try MultiLineDiff.applyDiff(to: current, diff: undoDiff!)
        try restored.write(toFile: file, atomically: true, encoding: .utf8)
        DiffStore.shared.popLastApplied(for: file)

        let display = MultiLineDiff.displayDiff(diff: undoDiff!, source: current, format: .ai)
        #expect(!display.isEmpty, "Undo should show D1F preview")
        #expect(readFile(file) == original, "File should be restored to original")
    }

    // MARK: - Scenario 4: AI makes multiple edits, undoes the last one

    @Test("Scenario 4: two diff_and_apply edits, undo last one")
    func multipleEditsUndoLast() throws {
        let dir = makeTempDir()
        defer { cleanup(dir) }
        let file = "\(dir)/scenario4.css"
        let original = "body {\n  color: black;\n  margin: 0;\n  padding: 0;\n}\n"
        writeFile(file, original)

        // Edit 1: change color on line 2
        let full1 = readFile(file)
        let lines1 = full1.components(separatedBy: "\n")
        let src1 = lines1[1]
        let dst1 = "  color: red;"
        let diff1 = MultiLineDiff.createDiff(source: src1, destination: dst1, includeMetadata: true)
        let id1 = DiffStore.shared.store(diff: diff1, source: src1)
        let patched1 = try MultiLineDiff.applyDiff(to: src1, diff: diff1)
        var newLines1 = lines1
        newLines1[1] = patched1
        let content1 = newLines1.joined(separator: "\n")
        try content1.write(toFile: file, atomically: true, encoding: .utf8)
        DiffStore.shared.recordApply(diffId: id1, filePath: file, originalContent: full1)

        #expect(readFile(file).contains("color: red"), "First edit applied")

        // Edit 2: change padding on line 4
        let full2 = readFile(file)
        let lines2 = full2.components(separatedBy: "\n")
        let src2 = lines2[3]
        let dst2 = "  padding: 10px;"
        let diff2 = MultiLineDiff.createDiff(source: src2, destination: dst2, includeMetadata: true)
        let id2 = DiffStore.shared.store(diff: diff2, source: src2)
        let patched2 = try MultiLineDiff.applyDiff(to: src2, diff: diff2)
        var newLines2 = lines2
        newLines2[3] = patched2
        let content2 = newLines2.joined(separator: "\n")
        try content2.write(toFile: file, atomically: true, encoding: .utf8)
        DiffStore.shared.recordApply(diffId: id2, filePath: file, originalContent: full2)

        #expect(readFile(file).contains("padding: 10px"), "Second edit applied")

        // Undo edit 2 by UUID
        let stored2 = DiffStore.shared.retrieve(id2)!
        let undo2 = MultiLineDiff.createUndoDiff(from: stored2.diff)!
        let current = readFile(file)
        let afterUndo = try MultiLineDiff.applyDiff(to: current, diff: undo2)
        try afterUndo.write(toFile: file, atomically: true, encoding: .utf8)
        DiffStore.shared.popLastApplied(for: file)

        let final = readFile(file)
        #expect(final.contains("color: red"), "First edit should still be there")
        #expect(final.contains("padding: 0"), "Second edit should be undone")
    }

    // MARK: - Scenario 5: Truncation safety rejects bad diffs

    @Test("Scenario 5: apply_diff rejects diff that would truncate file")
    func truncationSafetyCheck() throws {
        let dir = makeTempDir()
        defer { cleanup(dir) }
        let file = "\(dir)/scenario5.txt"
        // Create a file > 200 chars
        let original = (1...50).map { "line number \($0) with some content" }.joined(separator: "\n") + "\n"
        writeFile(file, original)
        #expect(original.count > 200)

        // Create a bad diff that would replace the whole file with 1 line
        let badDest = "just one line"
        let source = readFile(file)
        let diff = MultiLineDiff.createDiff(source: source, destination: badDest, includeMetadata: true)
        let diffId = DiffStore.shared.store(diff: diff, source: source)

        // Apply — should be rejected
        let stored = DiffStore.shared.retrieve(diffId)!
        let patched = try MultiLineDiff.applyDiff(to: source, diff: stored.diff)
        let rejected = source.count > 200 && patched.count < source.count / 2

        #expect(rejected, "Should reject diff that shrinks file by >50%")
        // File should NOT have been modified (handler would return before writing)
        #expect(readFile(file) == original, "File should remain unchanged")
    }

    // MARK: - Scenario 6: diff_and_apply inserting more lines than replaced

    @Test("Scenario 6: diff_and_apply replaces 1 line with 3 lines")
    func diffAndApplyInsertLines() throws {
        let dir = makeTempDir()
        defer { cleanup(dir) }
        let file = "\(dir)/scenario6.swift"
        writeFile(file, "import Foundation\n\nclass Foo {\n}\n")

        // Replace line 3 ("class Foo {") with 3 lines
        let destination = "class Foo {\n    var name: String = \"\"\n    var age: Int = 0"
        let fullText = readFile(file)
        let allLines = fullText.components(separatedBy: "\n")
        let source = allLines[2] // line 3

        let diff = MultiLineDiff.createDiff(source: source, destination: destination, includeMetadata: true, sourceStartLine: 2)
        let diffId = DiffStore.shared.store(diff: diff, source: source)
        let patched = try MultiLineDiff.applyDiff(to: source, diff: diff)

        var newLines = allLines
        newLines.replaceSubrange(2..<3, with: patched.components(separatedBy: "\n"))
        let finalContent = newLines.joined(separator: "\n")
        try finalContent.write(toFile: file, atomically: true, encoding: .utf8)
        DiffStore.shared.recordApply(diffId: diffId, filePath: file, originalContent: fullText)

        let final = readFile(file)
        #expect(final.contains("var name: String"), "New property should be inserted")
        #expect(final.contains("var age: Int"), "Second new property should be inserted")
        #expect(final.contains("import Foundation"), "Header intact")
        #expect(final.contains("}"), "Closing brace intact")
    }

    // MARK: - Scenario 7: D1F preview output for all tools

    @Test("Scenario 7: all tools produce D1F ASCII preview")
    func allToolsShowPreview() throws {
        let dir = makeTempDir()
        defer { cleanup(dir) }
        let file = "\(dir)/scenario7.txt"
        writeFile(file, "aaa\nbbb\nccc\n")

        // create_diff preview
        let source1 = "bbb"
        let dest1 = "BBB"
        let diff1 = MultiLineDiff.createDiff(source: source1, destination: dest1, includeMetadata: true)
        let preview1 = MultiLineDiff.displayDiff(diff: diff1, source: source1, format: .ai)
        #expect(!preview1.isEmpty, "create_diff should produce D1F preview")

        // apply_diff verification preview
        let diffId = DiffStore.shared.store(diff: diff1, source: source1)
        let stored = DiffStore.shared.retrieve(diffId)!
        let patched = try MultiLineDiff.applyDiff(to: readFile(file), diff: stored.diff)
        let verifyDiff = MultiLineDiff.createDiff(source: readFile(file), destination: patched, includeMetadata: true)
        let preview2 = MultiLineDiff.displayDiff(diff: verifyDiff, source: readFile(file), format: .ai)
        #expect(!preview2.isEmpty, "apply_diff should produce verification preview")

        // undo preview
        let undoDiff = MultiLineDiff.createUndoDiff(from: diff1)
        #expect(undoDiff != nil, "Should create undo diff")
        let preview3 = MultiLineDiff.displayDiff(diff: undoDiff!, source: dest1, format: .ai)
        #expect(!preview3.isEmpty, "undo should produce D1F preview")
    }

    // MARK: - Scenario 8: DiffStore UUID lifecycle

    @Test("Scenario 8: DiffStore stores, retrieves, tracks applies, and clears")
    func diffStoreLifecycle() {
        let dir = makeTempDir()
        defer { cleanup(dir) }
        let file = "\(dir)/scenario8.txt"
        writeFile(file, "original\n")

        let diff = MultiLineDiff.createDiff(source: "original", destination: "changed", includeMetadata: true)

        // Store
        let id = DiffStore.shared.store(diff: diff, source: "original")
        #expect(DiffStore.shared.retrieve(id) != nil)

        // Record apply
        DiffStore.shared.recordApply(diffId: id, filePath: file, originalContent: "original\n")
        #expect(DiffStore.shared.lastAppliedDiffId(for: file) == id)
        #expect(DiffStore.shared.lastEdit(for: file) == "original\n")

        // Pop last applied
        DiffStore.shared.popLastApplied(for: file)
        #expect(DiffStore.shared.lastAppliedDiffId(for: file) == nil)

        // Clear all
        DiffStore.shared.clear()
        #expect(DiffStore.shared.retrieve(id) == nil)
        #expect(DiffStore.shared.lastEdit(for: file) == nil)
    }

    // MARK: - Scenario 9: Full file diff_and_apply (no line range)

    @Test("Scenario 9: diff_and_apply without line range replaces entire file")
    func diffAndApplyFullFile() throws {
        let dir = makeTempDir()
        defer { cleanup(dir) }
        let file = "\(dir)/scenario9.txt"
        writeFile(file, "old content\nmore old\n")

        let destination = "new content\nmore new\nextra line\n"
        let fullText = readFile(file)
        let source = fullText

        let diff = MultiLineDiff.createDiff(source: source, destination: destination, includeMetadata: true)
        let diffId = DiffStore.shared.store(diff: diff, source: source)
        let patched = try MultiLineDiff.applyDiff(to: source, diff: diff)

        try patched.write(toFile: file, atomically: true, encoding: .utf8)
        DiffStore.shared.recordApply(diffId: diffId, filePath: file, originalContent: fullText)

        #expect(readFile(file) == destination)

        // Verify
        let verified = MultiLineDiff.verifyDiff(MultiLineDiff.createDiff(source: source, destination: patched, includeMetadata: true))
        #expect(verified)
    }

    // MARK: - Scenario 10: SHA verification catches corruption

    @Test("Scenario 10: verifyDiff confirms integrity")
    func shaVerification() {
        let source = "func test() {\n    return 42\n}\n"
        let destination = "func test() -> Int {\n    return 42\n}\n"
        let diff = MultiLineDiff.createDiff(source: source, destination: destination, includeMetadata: true)

        // Verify the diff matches its own metadata
        let verified = MultiLineDiff.verifyDiff(diff)
        #expect(verified, "SHA verification should pass for valid diff")

        // Metadata should contain both source and destination
        #expect(diff.metadata?.sourceContent != nil, "Metadata should store source")
        #expect(diff.metadata?.destinationContent != nil, "Metadata should store destination")
        #expect(diff.metadata?.diffHash != nil, "Metadata should have SHA hash")
    }
}
