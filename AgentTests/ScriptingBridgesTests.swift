import Testing
import Foundation
@testable import Agent_

@Suite("ScriptingBridges Bundle")
struct ScriptingBridgesTests {

    // MARK: - Bundle Presence

    @Test("Scripts folder exists in bundle Sources")
    func scriptsFolderExists() {
        guard let resourcePath = Bundle.main.resourcePath else {
            #expect(Bool(false), "No resource path")
            return
        }
        let path = resourcePath + "/Sources/Scripts"
        #expect(FileManager.default.fileExists(atPath: path),
                "Expected Scripts folder at \(path)")
    }

    @Test("Package.swift exists in bundle")
    func packageSwiftExists() {
        guard let resourcePath = Bundle.main.resourcePath else {
            #expect(Bool(false), "No resource path")
            return
        }
        let path = resourcePath + "/Package.swift"
        #expect(FileManager.default.fileExists(atPath: path),
                "Expected Package.swift at \(path)")
    }
}
