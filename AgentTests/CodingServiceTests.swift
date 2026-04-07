import Testing
import Foundation
@testable import Agent_

@Suite("CodingService")
struct CodingServiceTests {

    /// Temp directory for test files, cleaned up after each test via defer.
    private func makeTempDir() throws -> String {
        let dir = NSTemporaryDirectory() + "agent_coding_tests_\(UUID().uuidString)"
        try FileManager.default.createDirectory(atPath: dir, withIntermediateDirectories: true)
        return dir
    }

    private func cleanup(_ path: String) {
        try? FileManager.default.removeItem(atPath: path)
    }

    // MARK: - readFile

    @Test("readFile returns numbered lines")
    func readFileBasic() throws {
        let dir = try makeTempDir()
        defer { cleanup(dir) }
        let file = "\(dir)/hello.txt"
        try "line1\nline2\nline3\n".write(toFile: file, atomically: true, encoding: .utf8)

        let result = CodingService.readFile(path: file, offset: nil, limit: nil)
        #expect(result.contains("1\tline1"))
        #expect(result.contains("2\tline2"))
        #expect(result.contains("3\tline3"))
    }

    @Test("readFile with offset and limit")
    func readFileOffsetLimit() throws {
        let dir = try makeTempDir()
        defer { cleanup(dir) }
        let file = "\(dir)/lines.txt"
        let content = (1...20).map { "line\($0)" }.joined(separator: "\n")
        try content.write(toFile: file, atomically: true, encoding: .utf8)

        let result = CodingService.readFile(path: file, offset: 5, limit: 3)
        #expect(result.contains("line5"))
        #expect(result.contains("line6"))
        #expect(result.contains("line7"))
        #expect(!result.contains("line4"))
        #expect(!result.contains("line8"))
    }

    @Test("readFile returns error for missing file")
    func readFileMissing() {
        let result = CodingService.readFile(path: "/nonexistent/path/file.txt", offset: nil, limit: nil)
        #expect(result.contains("Error"))
    }

    @Test("readFile handles offset beyond file length")
    func readFileOffsetBeyondEnd() throws {
        let dir = try makeTempDir()
        defer { cleanup(dir) }
        let file = "\(dir)/short.txt"
        try "one\ntwo\n".write(toFile: file, atomically: true, encoding: .utf8)

        let result = CodingService.readFile(path: file, offset: 100, limit: 5)
        #expect(result.contains("Error") || result.isEmpty)
    }

    @Test("readFile handles limit larger than remaining lines")
    func readFileLimitLarger() throws {
        let dir = try makeTempDir()
        defer { cleanup(dir) }
        let file = "\(dir)/small.txt"
        let content = "alpha\nbeta\n"
        try content.write(toFile: file, atomically: true, encoding: .utf8)

        let result = CodingService.readFile(path: file, offset: 1, limit: 100)
        #expect(result.contains("2\tbeta"))
    }

    // MARK: - writeFile

    @Test("writeFile creates file and returns success")
    func writeFileBasic() throws {
        let dir = try makeTempDir()
        defer { cleanup(dir) }
        let file = "\(dir)/output.txt"
        let result = CodingService.writeFile(path: file, content: "hello world")
        #expect(result.contains("Success"))

        let contents = try String(contentsOfFile: file, encoding: .utf8)
        #expect(contents == "hello world")
    }

    @Test("writeFile creates intermediate directories")
    func writeFileWithDirs() throws {
        let dir = try makeTempDir()
        defer { cleanup(dir) }
        let file = "\(dir)/sub/deep/output.txt"
        let result = CodingService.writeFile(path: file, content: "nested")
        #expect(result.contains("Success"))

        let contents = try String(contentsOfFile: file, encoding: .utf8)
        #expect(contents == "nested")
    }

    @Test("writeFile overwrites existing file")
    func writeFileOverwrite() throws {
        let dir = try makeTempDir()
        defer { cleanup(dir) }
        let file = "\(dir)/overwrite.txt"
        try "old content".write(toFile: file, atomically: true, encoding: .utf8)

        let result = CodingService.writeFile(path: file, content: "new content")
        #expect(result.contains("Success"))

        let contents = try String(contentsOfFile: file, encoding: .utf8)
        #expect(contents == "new content")
    }

    // MARK: - editFile

    @Test("editFile replaces exact string")
    func editFileBasic() throws {
        let dir = try makeTempDir()
        defer { cleanup(dir) }
        let file = "\(dir)/edit.txt"
        try "Hello World".write(toFile: file, atomically: true, encoding: .utf8)

        let result = CodingService.editFile(path: file, oldString: "World", newString: "Swift")
        #expect(result.contains("Success"))

        let contents = try String(contentsOfFile: file, encoding: .utf8)
        #expect(contents == "Hello Swift")
    }

    @Test("editFile replaces all occurrences")
    func editFileReplaceAll() throws {
        let dir = try makeTempDir()
        defer { cleanup(dir) }
        let file = "\(dir)/editall.txt"
        try "foo bar foo baz foo".write(toFile: file, atomically: true, encoding: .utf8)

        let result = CodingService.editFile(path: file, oldString: "foo", newString: "qux", replaceAll: true)
        #expect(result.contains("Success"))

        let contents = try String(contentsOfFile: file, encoding: .utf8)
        #expect(contents == "qux bar qux baz qux")
    }

    @Test("editFile returns error when oldString not found")
    func editFileNotFound() throws {
        let dir = try makeTempDir()
        defer { cleanup(dir) }
        let file = "\(dir)/nofind.txt"
        try "aaa bbb aaa".write(toFile: file, atomically: true, encoding: .utf8)

        let result = CodingService.editFile(path: file, oldString: "zzz", newString: "xxx")
        #expect(result.contains("Error") || result.contains("not found"))
    }

    // MARK: - runCommand

    @Test("runCommand captures stdout")
    func runCommandBasic() throws {
        let dir = try makeTempDir()
        defer { cleanup(dir) }
        let file = "\(dir)/cmd.txt"
        try "some content".write(toFile: file, atomically: true, encoding: .utf8)

        let result = CodingService.runCommand(command: "cat \(file)")
        #expect(result.contains("some content"))
    }

    @Test("runCommand captures exit code on failure")
    func runCommandFailure() {
        let result = CodingService.runCommand(command: "ls /nonexistent_directory_xyz")
        #expect(result.contains("No such file") || result.contains("error") || result.contains("Error") || result.contains("not found"))
    }

    // MARK: - diff_apply

    @Test("applyDiff applies unified diff")
    func applyDiffBasic() throws {
        let dir = try makeTempDir()
        defer { cleanup(dir) }
        let file = "\(dir)/diff.txt"
        try "hello world".write(toFile: file, atomically: true, encoding: .utf8)

        let diff = """
        --- a/\(file)
        +++ b/\(file)
        @@ -1 +1 @@
        -hello world
        +hello swift
        """
        let result = CodingService.applyDiff(filePath: file, diff: diff)
        #expect(result.contains("Success") || result.contains("Applied"))

        let contents = try String(contentsOfFile: file, encoding: .utf8)
        #expect(contents == "hello swift")
    }

    @Test("applyDiff returns error for bad diff")
    func applyDiffBad() throws {
        let dir = try makeTempDir()
        defer { cleanup(dir) }
        let file = "\(dir)/baddiff.txt"
        try "original".write(toFile: file, atomically: true, encoding: .utf8)

        let diff = "not a valid diff"
        let result = CodingService.applyDiff(filePath: file, diff: diff)
        #expect(result.contains("Error") || result.contains("error") || result.contains("Failed"))
    }

    // MARK: - MultiLineDiff integration

    @Test("MultiLineDiff applyDiff via CodingService")
    func multiLineDiffApply() throws {
        let source = "line1\nline2\nline3\n"
        let destination = "line1\nmodified\nline3\n"
        let diff = try MultiLineDiff.createDiff(source: source, destination: destination)
        let result = try MultiLineDiff.applyDiff(to: source, diff: diff)
        #expect(result == destination)
    }

    // MARK: - listFiles

    @Test("listFiles returns directory contents")
    func listFilesBasic() throws {
        let dir = try makeTempDir()
        defer { cleanup(dir) }
        try "a".write(toFile: "\(dir)/file1.txt", atomically: true, encoding: .utf8)
        try "b".write(toFile: "\(dir)/file2.txt", atomically: true, encoding: .utf8)

        let result = CodingService.listFiles(path: dir)
        #expect(result.contains("file1.txt"))
        #expect(result.contains("file2.txt"))
    }

    @Test("listFiles returns error for missing directory")
    func listFilesMissing() {
        let result = CodingService.listFiles(path: "/nonexistent_dir_xyz")
        #expect(result.contains("Error"))
    }

    // MARK: - buildGitCommitCommand

    @Test("buildGitCommitCommand with files")
    func buildGitCommitWithFiles() {
        let cmd = CodingService.buildGitCommitCommand(path: "/tmp/repo", message: "initial", files: "file1.txt")
        #expect(cmd.contains("git commit -m 'initial'"))
        #expect(cmd.contains("file1.txt"))
    }

    @Test("buildGitCommitCommand without files uses git add -A")
    func buildGitCommitNoFiles() {
        let cmd = CodingService.buildGitCommitCommand(path: "/tmp/repo", message: "wip", files: nil)
        #expect(cmd.contains("git add -A"))
    }

    @Test("buildGitCommitCommand escapes single quotes in message")
    func buildGitCommitQuotedMessage() {
        let cmd = CodingService.buildGitCommitCommand(path: "/tmp/repo", message: "it's done", files: nil)
        #expect(cmd.contains("'it'\\''s done'"))
    }

    @Test("buildGitBranchCommand with checkout")
    func buildGitBranchCheckout() {
        let cmd = CodingService.buildGitBranchCommand(path: "/tmp/repo", name: "feature/new", checkout: true)
        #expect(cmd.contains("git checkout -b"))
        #expect(cmd.contains("'feature/new'"))
    }

    @Test("buildGitBranchCommand without checkout")
    func buildGitBranchNoCheckout() {
        let cmd = CodingService.buildGitBranchCommand(path: "/tmp/repo", name: "bugfix", checkout: false)
        #expect(cmd.contains("git branch 'bugfix'"))
        #expect(!cmd.contains("checkout"))
    }

    // MARK: - Preview helper

    @MainActor @Test("preview returns full text when under limit")
    func previewShort() {
        let text = "line1\nline2"
        let result = AgentViewModel.preview(text, lines: 3)
        #expect(result == "line1\nline2")
    }

    @MainActor @Test("preview truncates with ... when over limit")
    func previewLong() {
        let text = "line1\nline2\nline3\nline4\nline5"
        let result = AgentViewModel.preview(text, lines: 3)
        #expect(result == "line1\nline2\nline3\n...")
    }

    @MainActor @Test("preview handles single line")
    func previewSingle() {
        let result = AgentViewModel.preview("hello", lines: 3)
        #expect(result == "hello")
    }

    @MainActor @Test("preview handles empty string")
    func previewEmpty() {
        let result = AgentViewModel.preview("", lines: 3)
        #expect(result == "")
    }
}
