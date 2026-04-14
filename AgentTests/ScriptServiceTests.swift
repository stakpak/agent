import Testing
import Foundation
@testable import Agent_

@Suite("ScriptService")
@MainActor
struct ScriptServiceTests {
    let service = ScriptService()

    // MARK: - Create

    @Test("Create script produces Sources/Scripts/{name}.swift")
    func createScript() {
        let result = service.createScript(name: "test_hello", content: "print(\"hello\")")
        #expect(result.contains("Created test_hello"))

        let source = service.readScript(name: "test_hello")
        #expect(source == "print(\"hello\")")

        _ = service.deleteScript(name: "test_hello")
    }

    @Test("Create script strips .swift suffix from name")
    func createScriptStripsSuffix() {
        let result = service.createScript(name: "suffix_test.swift", content: "/
        #expect(result.contains("Created suffix_test"))

        let source = service.readScript(name: "suffix_test")
        #expect(source == "// test")

        _ = service.deleteScript(name: "suffix_test")
    }

    @Test("Create duplicate script returns error")
    func createDuplicateScript() {
        _ = service.createScript(name: "dup_test", content: "// first")
        let result = service.createScript(name: "dup_test", content: "// second"
        #expect(result.contains("already exists"))

        _ = service.deleteScript(name: "dup_test")
    }

    // MARK: - Read

    @Test("Read nonexistent script returns nil")
    func readNonexistent() {
        let source = service.readScript(name: "does_not_exist_\(UUID().uuidString)")
        #expect(source == nil)
    }

    // MARK: - Update

    @Test("Update existing script changes content")
    func updateScript() {
        _ = service.createScript(name: "update_test", content: "// v1")
        let result = service.updateScript(name: "update_test", content: "// v2")
        #expect(result.contains("Updated update_test"))

        let source = service.readScript(name: "update_test")
        #expect(source == "// v2")

        _ = service.deleteScript(name: "update_test")
    }

    @Test("Update nonexistent script returns error")
    func updateNonexistent() {
        let result = service.updateScript(name: "no_such_script_\(UUID().uuidStr
        #expect(result.contains("not found"))
    }

    // MARK: - Delete

    @Test("Delete existing script succeeds")
    func deleteScript() {
        _ = service.createScript(name: "delete_me", content: "// bye")
        let result = service.deleteScript(name: "delete_me")
        #expect(result.contains("Deleted delete_me"))

        let source = service.readScript(name: "delete_me")
        #expect(source == nil)
    }

    @Test("Delete nonexistent script returns error")
    func deleteNonexistent() {
        let result = service.deleteScript(name: "ghost_script_\(UUID().uuidString)")
        #expect(result.contains("not found"))
    }

    // MARK: - List

    @Test("List scripts includes created script")
    func listScripts() {
        _ = service.createScript(name: "list_test", content: "// listed")
        let scripts = service.listScripts()
        let names = scripts.map(\.name)
        #expect(names.contains("list_test"))

        _ = service.deleteScript(name: "list_test")
    }

    // MARK: - Compile Command

    @Test("compileCommand returns swift build command")
    func compileCommand() {
        _ = service.createScript(name: "cmd_test", content: "print(\"hi\")")
        let cmd = service.compileCommand(name: "cmd_test")
        #expect(cmd?.contains("swift build --product 'cmd_test'") == true)

        _ = service.deleteScript(name: "cmd_test")
    }

    @Test("compileCommand returns nil for missing script")
    func compileCommandMissing() {
        let cmd = service.compileCommand(name: "no_such_\(UUID().uuidString)")
        #expect(cmd == nil)
    }

    @Test("dylibPath returns path with lib prefix and .dylib extension")
    func dylibPathFormat() {
        let path = service.dylibPath(name: "MyScript")
        #expect(path.contains("libMyScript.dylib"))
        #expect(path.contains(".build/debug/"))
    }

    // MARK: - AGENT_SCRIPT_ARGS

    @Test("loadAndRunScript passes arguments via AGENT_SCRIPT_ARGS env var")
    func argsPassedViaEnvVar() async {
        let script = """
        import Foundation

        @_cdecl("script_main")
        public func scriptMain() -> Int32 {
            printArgs()
            return 0
        }

        func printArgs() {
            if let args = ProcessInfo.processInfo.environment["AGENT_SCRIPT_ARGS"] {
                print("ARGS:\\(args)")
            } else {
                print("ARGS:none")
            }
        }
        """
        _ = service.createScript(name: "test_args", content: script)
        defer { _ = service.deleteScript(name: "test_args") }

        // Compile
        guard let cmd = service.compileCommand(name: "test_args") else {
            Issue.record("compileCommand returned nil")
            return
        }
        let compileResult = shell(cmd)
        guard compileResult.status == 0 else {
            Issue.record("Compile failed: \(compileResult.output)")
            return
        }

        // Run with arguments
        let result = await service.loadAndRunScript(name: "test_args", arguments: "/Applications/Safari.app")
        #expect(result.output.contains("ARGS:/Applications/Safari.app"))
        #expect(result.status == 0)
    }

    @Test("loadAndRunScript with empty arguments does not set AGENT_SCRIPT_ARGS")
    func emptyArgsNotSet() async {
        let script = """
        import Foundation

        @_cdecl("script_main")
        public func scriptMain() -> Int32 {
            checkArgs()
            return 0
        }

        func checkArgs() {
            if let args = ProcessInfo.processInfo.environment["AGENT_SCRIPT_ARGS"] {
                print("ARGS:\\(args)")
            } else {
                print("ARGS:none")
            }
        }
        """
        _ = service.createScript(name: "test_noargs", content: script)
        defer { _ = service.deleteScript(name: "test_noargs") }

        guard let cmd = service.compileCommand(name: "test_noargs") else {
            Issue.record("compileCommand returned nil")
            return
        }
        let compileResult = shell(cmd)
        guard compileResult.status == 0 else {
            Issue.record("Compile failed: \(compileResult.output)")
            return
        }

        let result = await service.loadAndRunScript(name: "test_noargs", arguments: "")
        #expect(result.output.contains("ARGS:none"))
        #expect(result.status == 0)
    }

    @Test("AGENT_SCRIPT_ARGS is cleaned up after script runs")
    func argsCleanedUp() async {
        let script = """
        import Foundation

        @_cdecl("script_main")
        public func scriptMain() -> Int32 {
            print("ok")
            return 0
        }
        """
        _ = service.createScript(name: "test_cleanup", content: script)
        defer { _ = service.deleteScript(name: "test_cleanup") }

        guard let cmd = service.compileCommand(name: "test_cleanup") else {
            Issue.record("compileCommand returned nil")
            return
        }
        let compileResult = shell(cmd)
        guard compileResult.status == 0 else {
            Issue.record("Compile failed: \(compileResult.output)")
            return
        }

        _ = await service.loadAndRunScript(name: "test_cleanup", arguments: "secret_data")

        // After the call, env var should be unset
        let envVal = ProcessInfo.processInfo.environment["AGENT_SCRIPT_ARGS"]
        #expect(envVal == nil)
    }

    // MARK: - JSON I/O

    @Test("Script reads JSON input and writes JSON output")
    func jsonInputOutput() async {
        let agentDir = ScriptService.agentDir.path
        let inputPath = "\(agentDir)/test_json_io_input.json"
        let outputPath = "\(agentDir)/test_json_io_output.json"

        // Clean up any previous run
        try? FileManager.default.removeItem(atPath: inputPath)
        try? FileManager.default.removeItem(atPath: outputPath)
        defer {
            try? FileManager.default.removeItem(atPath: inputPath)
            try? FileManager.default.removeItem(atPath: outputPath)
            _ = service.deleteScript(name: "test_json_io")
        }

        // Write input JSON
        let input: [String: Any] = ["greeting": "hello", "count": 42]
        guard let inputData = try? JSONSerialization.data(withJSONObject: input, options: .prettyPrinted) else {
            Issue.record("Could not serialize input JSON")
            return
        }
        try? inputData.write(to: URL(fileURLWithPath: inputPath))

        // Create script that reads input and writes output
        let script = """
        import Foundation

        @_cdecl("script_main")
        public func scriptMain() -> Int32 {
            processJSON()
            return 0
        }

        func processJSON() {
            let home = NSHomeDirectory()
            let inputPath = "\\(home)/Documents/AgentScript/test_json_io_input.json"
            let outputPath = "\\(home)/Documents/AgentScript/test_json_io_output.json"

            guard let data = FileManager.default.contents(atPath: inputPath),
                  let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
                print("Failed to read input")
                return
            }

            let greeting = json["greeting"] as? String ?? "unknown"
            let count = json["count"] as? Int ?? 0

            let result: [String: Any] = [
                "success": true,
                "echo_greeting": greeting,
                "echo_count": count,
                "doubled": count * 2
            ]

            guard let outData = try? JSONSerialization.data(withJSONObject: result, options: .prettyPrinted) else { return }
            try? outData.write(to: URL(fileURLWithPath: outputPath))
            print("JSON processed")
        }
        """
        _ = service.createScript(name: "test_json_io", content: script)

        guard let cmd = service.compileCommand(name: "test_json_io") else {
            Issue.record("compileCommand returned nil")
            return
        }
        let compileResult = shell(cmd)
        guard compileResult.status == 0 else {
            Issue.record("Compile failed: \(compileResult.output)")
            return
        }

        let result = await service.loadAndRunScript(name: "test_json_io")
        #expect(result.status == 0)
        #expect(result.output.contains("JSON processed"))

        // Read and verify output JSON
        guard let outData = FileManager.default.contents(atPath: outputPath),
              let outJSON = try? JSONSerialization.jsonObject(with: outData) as? [String: Any] else {
            Issue.record("Could not read output JSON at \(outputPath)")
            return
        }

        #expect(outJSON["success"] as? Bool == true)
        #expect(outJSON["echo_greeting"] as? String == "hello")
        #expect(outJSON["echo_count"] as? Int == 42)
        #expect(outJSON["doubled"] as? Int == 84)
    }

    @Test("Script handles missing JSON input gracefully")
    func jsonMissingInput() async {
        let agentDir = ScriptService.agentDir.path
        let inputPath = "\(agentDir)/test_missing_json_input.json"

        // Make sure input doesn't exist
        try? FileManager.default.removeItem(atPath: inputPath)
        defer { _ = service.deleteScript(name: "test_missing_json") }

        let script = """
        import Foundation

        @_cdecl("script_main")
        public func scriptMain() -> Int32 {
            checkInput()
        }

        func checkInput() -> Int32 {
            let home = NSHomeDirectory()
            let inputPath = "\\(home)/Documents/AgentScript/test_missing_json_input.json"

            guard let _ = FileManager.default.contents(atPath: inputPath) else {
                print("ERROR:input_not_found")
                return 1
            }
            return 0
        }
        """
        _ = service.createScript(name: "test_missing_json", content: script)

        guard let cmd = service.compileCommand(name: "test_missing_json") else {
            Issue.record("compileCommand returned nil")
            return
        }
        let compileResult = shell(cmd)
        guard compileResult.status == 0 else {
            Issue.record("Compile failed: \(compileResult.output)")
            return
        }

        let result = await service.loadAndRunScript(name: "test_missing_json")
        #expect(result.output.contains("ERROR:input_not_found"))
        #expect(result.status == 1)
    }

    // MARK: - Helper

    private func shell(_ command: String) -> (output: String, status: Int32) {
        let process = Process()
        let pipe = Pipe()
        process.standardOutput = pipe
        process.standardError = pipe
        process.executableURL = URL(fileURLWithPath: "/bin/zsh")
        process.arguments = ["-c", command]
        try? process.run()
        process.waitUntilExit()
        let data = pipe.fileHandleForReading.readDataToEndOfFile()
        let output = String(data: data, encoding: .utf8) ?? ""
        return (output, process.terminationStatus)
    }
}
