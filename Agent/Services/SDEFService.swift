import Foundation

/// Parses bundled JSON SDEF files so the LLM and AppleEventService
/// can look up an app's commands, classes, properties, and enums.
final class SDEFService: @unchecked Sendable {
    static let shared = SDEFService()

    // MARK: - JSON Models

    struct SDEFDocument: Codable {
        var suites: [Suite]
    }

    struct Suite: Codable {
        var name: String
        var commands: [Command]?
        var classes: [SDEFClass]?
        var enumerations: [Enumeration]?
    }

    struct Command: Codable {
        var name: String
        var description: String?
        var directParameter: DirectParameter?
        var parameters: [Parameter]?
        var result: String?

        enum CodingKeys: String, CodingKey {
            case name, description
            case directParameter = "direct_parameter"
            case parameters, result
        }
    }

    struct DirectParameter: Codable {
        var type: String?
        var description: String?
    }

    struct Parameter: Codable {
        var name: String
        var type: String?
        var description: String?
        var optional: Bool?
    }

    struct SDEFClass: Codable {
        var name: String
        var inherits: String?
        var description: String?
        var properties: [Property]?
        var elements: [String]?
        var respondsTo: [String]?

        enum CodingKeys: String, CodingKey {
            case name, inherits, description, properties, elements
            case respondsTo = "responds_to"
        }
    }

    struct Property: Codable {
        var name: String
        var type: String?
        var readonly: Bool?
        var description: String?
    }

    struct Enumeration: Codable {
        var name: String
        var values: [EnumValue]
    }

    struct EnumValue: Codable {
        var name: String
        var description: String?
    }

    // MARK: - Cache

    /// Map from filename (e.g. "Mail") to parsed document
    private var cache: [String: SDEFDocument] = [:]

    /// Map from bundle ID to filename for quick lookup
    private let bundleIDMap: [String: String] = [
        "com.apple.AppleScriptUtility": "AppleScriptUtility",
        "com.apple.Automator": "Automator",
        "com.apple.Automator.Automator-Application-Stub": "AutomatorApplicationStub",
        "com.apple.BluetoothFileExchange": "BluetoothFileExchange",
        "com.apple.iCal": "Calendar",
        "com.apple.Console": "Console",
        "com.apple.AddressBook": "Contacts",
        "com.apple.databaseevents": "DatabaseEvents",
        "developer.apple.wwdc-Release": "Developer",
        "com.apple.FinalCutApp": "FinalCutProCreatorStudio",
        "com.apple.finder": "Finder",
        "org.mozilla.firefox": "Firefox",
        "com.apple.FolderActionsDispatcher": "FolderActionsDispatcher",
        "com.apple.FolderActionsSetup": "FolderActionsSetup",
        "com.google.Chrome": "GoogleChrome",
        "com.apple.imageevents": "ImageEvents",
        "com.apple.dt.Instruments": "Instruments",
        "com.apple.Keynote": "Keynote",
        "com.apple.mobilelogic": "LogicProCreatorStudio",
        "com.apple.mail": "Mail",
        "com.apple.MobileSMS": "Messages",
        "com.microsoft.edgemac": "MicrosoftEdge",
        "com.apple.Music": "Music",
        "com.apple.Notes": "Notes",
        "com.apple.iWork.Numbers": "Numbers",
        "com.apple.Numbers": "NumbersCreatorStudio",
        "com.apple.iWork.Pages": "Pages",
        "com.apple.Pages": "PagesCreatorStudio",
        "com.apple.Photos": "Photos",
        "com.apple.pixelmator": "PixelmatorPro",
        "com.apple.Preview": "Preview",
        "com.apple.QuickTimePlayerX": "QuickTimePlayer",
        "com.apple.reminders": "Reminders",
        "com.apple.Safari": "Safari",
        "com.apple.ScreenSharing": "ScreenSharing",
        "com.apple.ScriptEditor2": "ScriptEditor",
        "com.apple.shortcuts": "Shortcuts",
        "com.apple.shortcuts.events": "ShortcutsEvents",
        "com.apple.iphonesimulator": "Simulator",
        "com.apple.systemevents": "SystemEvents",
        "com.apple.SystemProfiler": "SystemInformation",
        "com.apple.systempreferences": "SystemSettings",
        "com.apple.Terminal": "Terminal",
        "com.apple.TextEdit": "TextEdit",
        "com.apple.TV": "TV",
        "com.utmapp.UTM": "UTM",
        "com.apple.VoiceOver": "VoiceOver",
        "com.tcltk.wish": "Wish",
        "com.apple.dt.Xcode": "Xcode",
    ]

    // MARK: - Loading

    /// Load a parsed SDEF by filename (without extension).
    func load(_ name: String) -> SDEFDocument? {
        if let cached = cache[name] { return cached }

        guard let url = Bundle.main.url(forResource: name, withExtension: "json", subdirectory: "SDEFs") else {
            return nil
        }
        guard let data = try? Data(contentsOf: url),
              let doc = try? JSONDecoder().decode(SDEFDocument.self, from: data) else
        {
            return nil
        }
        cache[name] = doc
        return doc
    }

    /// Load a parsed SDEF by bundle identifier.
    func loadByBundleID(_ bundleID: String) -> SDEFDocument? {
        guard let name = bundleIDMap[bundleID] else { return nil }
        return load(name)
    }

    // MARK: - Queries

    /// List all available SDEF names.
    func availableSDEFs() -> [String] {
        guard let url = Bundle.main.url(forResource: "SDEFs", withExtension: nil),
              let files = try? FileManager.default.contentsOfDirectory(atPath: url.path) else
        {
            return []
        }
        return files.filter { $0.hasSuffix(".json") }
            .map { ($0 as NSString).deletingPathExtension }
            .sorted()
    }

    /// Get all commands for an app.
    func commands(for bundleID: String) -> [Command] {
        guard let doc = loadByBundleID(bundleID) else { return [] }
        return doc.suites.flatMap { $0.commands ?? [] }
    }

    /// Get all classes for an app.
    func classes(for bundleID: String) -> [SDEFClass] {
        guard let doc = loadByBundleID(bundleID) else { return [] }
        return doc.suites.flatMap { $0.classes ?? [] }
    }

    /// Get all properties of a specific class.
    func properties(for bundleID: String, className: String) -> [Property] {
        let allClasses = classes(for: bundleID)
        return allClasses.first { $0.name == className }?.properties ?? []
    }

    /// Get all enumerations for an app.
    func enumerations(for bundleID: String) -> [Enumeration] {
        guard let doc = loadByBundleID(bundleID) else { return [] }
        return doc.suites.flatMap { $0.enumerations ?? [] }
    }

    /// Get all elements (child collections) for a specific class.
    func elements(for bundleID: String, className: String) -> [String] {
        let allClasses = classes(for: bundleID)
        return allClasses.first { $0.name == className }?.elements ?? []
    }

    // MARK: - LLM Summary

    /// Generate a concise summary for the LLM of an app's scripting dictionary.
    func summary(for bundleID: String) -> String {
        guard let doc = loadByBundleID(bundleID) else {
            return "No SDEF found for \(bundleID)"
        }

        var lines: [String] = []
        let name = bundleIDMap[bundleID] ?? bundleID
        lines.append("# \(name) Scripting Dictionary")

        for suite in doc.suites {
            lines.append("\n## \(suite.name)")

            if let commands = suite.commands, !commands.isEmpty {
                lines.append("Commands:")
                for cmd in commands {
                    var line = "  \(cmd.name)"
                    if let dp = cmd.directParameter?.type { line += " <\(dp)>" }
                    if let params = cmd.parameters, !params.isEmpty {
                        let paramStr = params.map { p in
                            let opt = p.optional == true ? "?" : ""
                            return "\(p.name)\(opt):\(p.type ?? "any")"
                        }.joined(separator: ", ")
                        line += " [\(paramStr)]"
                    }
                    if let r = cmd.result { line += " → \(r)" }
                    if let d = cmd.description { line += "  // \(d)" }
                    lines.append(line)
                }
            }

            if let classes = suite.classes, !classes.isEmpty {
                lines.append("Classes:")
                for cls in classes {
                    var header = "  \(cls.name)"
                    if let inh = cls.inherits { header += " : \(inh)" }
                    lines.append(header)
                    if let props = cls.properties {
                        for p in props {
                            let ro = p.readonly == true ? " (r)" : ""
                            lines.append("    .\(Self.toCamelCase(p.name)): \(p.type ?? "any")\(ro)")
                        }
                    }
                    if let elems = cls.elements {
                        lines.append("    elements: \(elems.joined(separator: ", "))")
                    }
                }
            }

            if let enums = suite.enumerations, !enums.isEmpty {
                lines.append("Enums:")
                for e in enums {
                    let vals = e.values.map { $0.name }.joined(separator: " | ")
                    lines.append("  \(e.name): \(vals)")
                }
            }
        }
        return lines.joined(separator: "\n")
    }

    /// Convert SDEF space-separated name to camelCase for KVC / value(forKey:).
    /// "current track" → "currentTrack", "AirPlay enabled" → "AirPlayEnabled"
    static func toCamelCase(_ name: String) -> String {
        let words = name.components(separatedBy: " ")
        guard let first = words.first else { return name }
        let rest = words.dropFirst().map { $0.prefix(1).uppercased() + $0.dropFirst() }
        return first + rest.joined()
    }

    /// Lookup valid property/element keys for SDEF queries.
    /// Returns camelCase keys the AI can use with `get`, `iterate`, etc.
    func aeKeys(for bundleID: String, className: String = "application") -> (properties: [String], elements: [String]) {
        let allClasses = classes(for: bundleID)

        // Collect keys from the class and its inheritance chain
        var propNames: [String] = []
        var elemNames: [String] = []
        var current: String? = className

        while let name = current {
            if let cls = allClasses.first(where: { $0.name == name }) {
                propNames.append(contentsOf: cls.properties?.map { Self.toCamelCase($0.name) } ?? [])
                elemNames.append(contentsOf: cls.elements?.map { Self.toCamelCase($0) } ?? [])
                current = cls.inherits
            } else {
                current = nil
            }
        }
        return (propNames, elemNames)
    }
}
