import Testing
import Foundation
@testable import Agent_

@Suite("Package.swift Auto-Discovery")
@MainActor
struct PackageAutoDiscoveryTests {
    let service = ScriptService()

    /// Helper: read Package.swift content after creating/deleting scripts
    private func packageContent() -> String? {
        let path = ScriptService.agentsDir.appendingPathComponent("Package.swift").path
        return try? String(contentsOfFile: path, encoding: .utf8)
    }

    /// Helper: run `swift package dump-package` and return parsed JSON
    private func dumpPackage() -> [String: Any]? {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/env")
        process.arguments = ["swift", "package", "dump-package"]
        process.currentDirectoryURL = ScriptService.agentsDir
        let pipe = Pipe()
        process.standardOutput = pipe
        process.standardError = FileHandle.nullDevice
        try? process.run()
        process.waitUntilExit()
        guard process.terminationStatus == 0 else { return nil }
        let data = pipe.fileHandleForReading.readDataToEndOfFile()
        return try? JSONSerialization.jsonObject(with: data) as? [String: Any]
    }

    /// Helper: extract target names from dump-package JSON
    private func targetNames(from json: [String: Any]?) -> [String] {
        guard let targets = json?["targets"] as? [[String: Any]] else { return [] }
        return targets.compactMap { $0["name"] as? String }
    }

    // MARK: - Auto-Discovery Tests

    @Test("New script is auto-discovered without Package.swift edits")
    func newScriptAutoDiscovered() {
        let name = "auto_discover_test_\(Int.random(in: 1000...9999))"
        _ = service.createScript(name: name, content: "import Foundation\nprint(\"hello\")")
        defer { _ = service.deleteScript(name: name) }

        let json = dumpPackage()
        let names = targetNames(from: json)
        #expect(names.contains(name), "Expected \(name) in targets: \(names)")
    }

    @Test("Deleted script is removed from auto-discovery")
    func deletedScriptRemoved() {
        let name = "auto_remove_test_\(Int.random(in: 1000...9999))"
        _ = service.createScript(name: name, content: "print(\"bye\")")
        _ = service.deleteScript(name: name)

        let json = dumpPackage()
        let names = targetNames(from: json)
        #expect(!names.contains(name), "Expected \(name) NOT in targets: \(names)")
    }

    @Test("Script with bridge import gets correct dependency")
    func bridgeImportDetected() {
        let name = "auto_dep_test_\(Int.random(in: 1000...9999))"
        let content = """
        import Foundation
        import MusicBridge

        print("music test")
        """
        _ = service.createScript(name: name, content: content)
        defer { _ = service.deleteScript(name: name) }

        let json = dumpPackage()
        guard let targets = json?["targets"] as? [[String: Any]] else {
            #expect(Bool(false), "No targets in package dump")
            return
        }
        guard let target = targets.first(where: { $0["name"] as? String == name }) else {
            #expect(Bool(false), "\(name) not found in targets")
            return
        }
        let deps = target["dependencies"] as? [[String: Any]] ?? []
        let depNames = deps.compactMap { dep -> String? in
            if let byName = dep["byName"] as? [Any?] {
                return byName.first as? String
            }
            return nil
        }
        #expect(depNames.contains("MusicBridge"), "Expected MusicBridge in deps: \(depNames)")
    }

    @Test("Script with multiple bridge imports gets all dependencies")
    func multipleBridgeImports() {
        let name = "auto_multi_dep_\(Int.random(in: 1000...9999))"
        let content = """
        import Foundation
        import MailBridge
        import CalendarBridge

        print("multi")
        """
        _ = service.createScript(name: name, content: content)
        defer { _ = service.deleteScript(name: name) }

        let json = dumpPackage()
        guard let targets = json?["targets"] as? [[String: Any]],
              let target = targets.first(where: { $0["name"] as? String == name }) else {
            #expect(Bool(false), "\(name) not found in targets")
            return
        }
        let deps = target["dependencies"] as? [[String: Any]] ?? []
        let depNames = deps.compactMap { dep -> String? in
            if let byName = dep["byName"] as? [Any?] {
                return byName.first as? String
            }
            return nil
        }
        #expect(depNames.contains("MailBridge"), "Expected MailBridge in deps: \(depNames)")
        #expect(depNames.contains("CalendarBridge"), "Expected CalendarBridge in deps: \(depNames)")
    }

    @Test("Script with no bridge imports has no bridge dependencies")
    func noBridgeImports() {
        let name = "auto_no_dep_\(Int.random(in: 1000...9999))"
        let content = """
        import Foundation

        print("no deps")
        """
        _ = service.createScript(name: name, content: content)
        defer { _ = service.deleteScript(name: name) }

        let json = dumpPackage()
        guard let targets = json?["targets"] as? [[String: Any]],
              let target = targets.first(where: { $0["name"] as? String == name }) else {
            #expect(Bool(false), "\(name) not found in targets")
            return
        }
        let deps = target["dependencies"] as? [[String: Any]] ?? []
        let depNames = deps.compactMap { dep -> String? in
            if let byName = dep["byName"] as? [Any?] {
                return byName.first as? String
            }
            return nil
        }
        let bridgeDeps = depNames.filter { $0.hasSuffix("Bridge") }
        #expect(bridgeDeps.isEmpty, "Expected no bridge deps but got: \(bridgeDeps)")
    }

    @Test("Script with ScriptingBridgeCommon import gets common dependency")
    func commonImportDetected() {
        let name = "auto_common_dep_\(Int.random(in: 1000...9999))"
        let content = """
        import Foundation
        import ScriptingBridgeCommon
        import MailBridge

        print("common test")
        """
        _ = service.createScript(name: name, content: content)
        defer { _ = service.deleteScript(name: name) }

        let json = dumpPackage()
        guard let targets = json?["targets"] as? [[String: Any]],
              let target = targets.first(where: { $0["name"] as? String == name }) else {
            #expect(Bool(false), "\(name) not found in targets")
            return
        }
        let deps = target["dependencies"] as? [[String: Any]] ?? []
        let depNames = deps.compactMap { dep -> String? in
            if let byName = dep["byName"] as? [Any?] {
                return byName.first as? String
            }
            return nil
        }
        #expect(depNames.contains("ScriptingBridgeCommon"), "Expected ScriptingBridgeCommon in deps: \(depNames)")
        #expect(depNames.contains("MailBridge"), "Expected MailBridge in deps: \(depNames)")
    }
}
