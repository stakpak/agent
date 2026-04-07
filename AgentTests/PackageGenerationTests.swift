import Testing
import Foundation
@testable import Agent_

@Suite("Package.swift Generation")
@MainActor
struct PackageGenerationTests {
    let service = ScriptService()

    @Test("Package.swift exists after ensurePackage via create")
    func packageSwiftCreated() {
        _ = service.createScript(name: "pkg_test", content: "print(\"pkg\")")

        let packagePath = ScriptService.agentsDir.appendingPathComponent("Package.swift").path
        #expect(FileManager.default.fileExists(atPath: packagePath))

        let content = try? String(contentsOfFile: packagePath, encoding: .utf8)
        #expect(content?.contains("swift-tools-version") == true)
        #expect(content?.contains("AppleEventBridges") == true)

        _ = service.deleteScript(name: "pkg_test")
    }

    @Test("Package.swift lists created script as dynamic library target")
    func packageIncludesScriptTarget() {
        _ = service.createScript(name: "target_test", content: "// target")

        let packagePath = ScriptService.agentsDir.appendingPathComponent("Package.swift").path
        let content = try? String(contentsOfFile: packagePath, encoding: .utf8)
        #expect(content?.contains("\"target_test\"") == true)

        _ = service.deleteScript(name: "target_test")
    }

    @Test("Package.swift removes deleted script target")
    func packageRemovesDeletedTarget() {
        _ = service.createScript(name: "remove_test", content: "// remove")
        _ = service.deleteScript(name: "remove_test")

        let packagePath = ScriptService.agentsDir.appendingPathComponent("Package.swift").path
        let content = try? String(contentsOfFile: packagePath, encoding: .utf8)
        #expect(content?.contains("\"remove_test\"") != true)
    }
}
