import Foundation
import AgentAudit

// MARK: - MCP Server Configuration

struct MCPServerConfig: Codable, Identifiable, Hashable {
    var id: UUID
    var name: String
    // Stdio transport
    var command: String
    var arguments: [String]
    var environment: [String: String]
    // HTTP transport
    var url: String?
    var headers: [String: String]

    // Agent-specific fields — stored in UserDefaults, NOT in JSON
    var enabled: Bool
    var autoStart: Bool

    /// True if this server uses HTTP/HTTPS transport
    var isHTTP: Bool {
        guard let url = url, !url.isEmpty else { return false }
        return true
    }

    /// Display string for the server's connection target
    var displayAddress: String { isHTTP ? (url ?? "") : command }

    init(id: UUID = UUID(), name: String, command: String, arguments: [String] = [], environment: [String: String] = [:], enabled: Bool = true, autoStart: Bool = true) {
        self.id = id
        self.name = name
        self.command = command
        self.arguments = arguments
        self.environment = environment
        self.url = nil
        self.headers = [:]
        self.enabled = enabled
        self.autoStart = autoStart
    }

    init(id: UUID = UUID(), name: String, url: String, headers: [String: String] = [:], enabled: Bool = true, autoStart: Bool = true) {
        self.id = id
        self.name = name
        self.command = ""
        self.arguments = []
        self.environment = [:]
        self.url = url
        self.headers = headers
        self.enabled = enabled
        self.autoStart = autoStart
    }

    // SSE/HTTP transport endpoint paths (for servers that use separate endpoints)
    var sseEndpoint: String?
    var httpEndpoint: String?
    
    /// Unsupported fields that aren't part of the standard MCP spec but are preserved for export.
    /// Stored as JSON string for Hashable conformance.
    var unsupportedFieldsJSON: String? = nil
    
    /// Get unsupported fields as a dictionary
    var unsupportedFields: [String: Any] {
        guard let json = unsupportedFieldsJSON,
              let data = json.data(using: .utf8),
              let dict = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            return [:]
        }
        return dict
    }
    
    /// Set unsupported fields from a dictionary
    mutating func setUnsupportedFields(_ dict: [String: Any]) {
        guard !dict.isEmpty,
              let data = try? JSONSerialization.data(withJSONObject: dict, options: [.sortedKeys]),
              let json = String(data: data, encoding: .utf8) else {
            unsupportedFieldsJSON = nil
            return
        }
        unsupportedFieldsJSON = json
    }

    // Only encode/decode MCP-standard fields in JSON
    // Only MCP-standard fields in JSON; name is the dictionary key, not a field
    private enum CodingKeys: String, CodingKey {
        case transport, command, args, env, url, headers
        case sseEndpoint, httpEndpoint
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let transport = try c.decodeIfPresent(String.self, forKey: .transport)
        command = try c.decodeIfPresent(String.self, forKey: .command) ?? ""
        arguments = try c.decodeIfPresent([String].self, forKey: .args) ?? []
        environment = try c.decodeIfPresent([String: String].self, forKey: .env) ?? [:]
        url = try c.decodeIfPresent(String.self, forKey: .url)
        headers = try c.decodeIfPresent([String: String].self, forKey: .headers) ?? [:]
        sseEndpoint = try c.decodeIfPresent(String.self, forKey: .sseEndpoint)
        httpEndpoint = try c.decodeIfPresent(String.self, forKey: .httpEndpoint)
        // If transport is explicitly "http"/"https" but url is missing, clear command
        if let transport, (transport == "http" || transport == "https"), url != nil {
            command = ""
        }
        name = ""
        id = UUID()
        enabled = true
        autoStart = true
        unsupportedFieldsJSON = nil
    }
    
    /// Initialize from a dictionary, capturing unsupported fields
    init?(from dict: [String: Any]) {
        // Determine transport type first
        let transport = dict["transport"] as? String
        let hasURL = (dict["url"] as? String) != nil
        let isHTTPTransport = (transport == "http" || transport == "https") || hasURL
        
        // Extract all fields
        command = dict["command"] as? String ?? ""
        arguments = dict["args"] as? [String] ?? []
        environment = dict["env"] as? [String: String] ?? [:]
        url = dict["url"] as? String
        headers = dict["headers"] as? [String: String] ?? [:]
        sseEndpoint = dict["sseEndpoint"] as? String
        httpEndpoint = dict["httpEndpoint"] as? String
        name = ""
        id = UUID()
        enabled = true
        autoStart = true
        
        // Standard fields for the ACTUAL transport type
        // HTTP transport uses: transport, url, headers, sseEndpoint, httpEndpoint
        // Stdio transport uses: transport, command, args, env
        let httpStandardFields = Set(["transport", "url", "headers", "sseEndpoint", "httpEndpoint"])
        let stdioStandardFields = Set(["transport", "command", "args", "env"])
        
        var unsupported: [String: Any] = [:]
        
        if isHTTPTransport {
            // HTTP server: preserve stdio fields as unsupported for round-tripping
            if !command.isEmpty { unsupported["command"] = command }
            if !arguments.isEmpty { unsupported["args"] = arguments }
            if !environment.isEmpty { unsupported["env"] = environment }
            // Clear stdio fields since this is HTTP
            command = ""
            arguments = []
            environment = [:]
            
            // Capture any other non-standard fields
            for (key, value) in dict {
                if !httpStandardFields.contains(key) {
                    unsupported[key] = value
                }
            }
        } else {
            // Stdio server: preserve HTTP fields as unsupported for round-tripping
            if let u = url { unsupported["url"] = u }
            if !headers.isEmpty { unsupported["headers"] = headers }
            if let sse = sseEndpoint { unsupported["sseEndpoint"] = sse }
            if let http = httpEndpoint { unsupported["httpEndpoint"] = http }
            // Clear HTTP fields since this is stdio
            url = nil
            headers = [:]
            sseEndpoint = nil
            httpEndpoint = nil
            
            // Capture any other non-standard fields
            for (key, value) in dict {
                if !stdioStandardFields.contains(key) {
                    unsupported[key] = value
                }
            }
        }
        
        setUnsupportedFields(unsupported)
    }

    func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        if isHTTP {
            try c.encode("http", forKey: .transport)
            try c.encode(url, forKey: .url)
            if !headers.isEmpty {
                try c.encode(headers, forKey: .headers)
            }
            if let sseEndpoint, !sseEndpoint.isEmpty {
                try c.encode(sseEndpoint, forKey: .sseEndpoint)
            }
            if let httpEndpoint, !httpEndpoint.isEmpty {
                try c.encode(httpEndpoint, forKey: .httpEndpoint)
            }
        } else {
            try c.encode("stdio", forKey: .transport)
            try c.encode(command, forKey: .command)
            try c.encode(arguments, forKey: .args)
            try c.encode(environment, forKey: .env)
        }
    }
    
    /// Convert to dictionary including unsupported fields
    func toDictionary() -> [String: Any] {
        var dict: [String: Any] = [:]
        
        if isHTTP {
            dict["transport"] = "http"
            dict["url"] = url ?? ""
            if !headers.isEmpty { dict["headers"] = headers }
            if let sseEndpoint, !sseEndpoint.isEmpty { dict["sseEndpoint"] = sseEndpoint }
            if let httpEndpoint, !httpEndpoint.isEmpty { dict["httpEndpoint"] = httpEndpoint }
        } else {
            dict["transport"] = "stdio"
            dict["command"] = command
            if !arguments.isEmpty { dict["args"] = arguments }
            if !environment.isEmpty { dict["env"] = environment }
        }
        
        // Merge in unsupported fields (preserved for export)
        for (key, value) in unsupportedFields {
            dict[key] = value
        }
        return dict
    }

    /// Create from a JSON string (for importing)
    static func fromJSON(_ jsonString: String) -> MCPServerConfig? {
        guard let data = jsonString.data(using: .utf8),
              let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            return nil
        }
        return MCPServerConfig(from: obj)
    }

    /// Export as MCP-standard dict: { "mcpServers": { "ServerName": { ... } } }
    func toJSON() -> String {
        let inner = toDictionary()
        let wrapper: [String: Any] = ["mcpServers": [name: inner]]
        guard let wrapperData = try? JSONSerialization.data(withJSONObject: wrapper, options: [.prettyPrinted]),
              let json = String(data: wrapperData, encoding: .utf8) else {
            return "{}"
        }
        return json
    }
}

// MARK: - Agent Preferences (UserDefaults, keyed by server name)

extension MCPServerConfig {
    private static let enabledPrefix = "mcp.enabled."
    private static let autoStartPrefix = "mcp.autoStart."

    /// Load Agent-specific prefs from UserDefaults
    mutating func loadPrefs() {
        guard !name.isEmpty else { return }
        let defs = UserDefaults.standard
        if defs.object(forKey: Self.enabledPrefix + name) != nil {
            enabled = defs.bool(forKey: Self.enabledPrefix + name)
        }
        if defs.object(forKey: Self.autoStartPrefix + name) != nil {
            autoStart = defs.bool(forKey: Self.autoStartPrefix + name)
        }
    }

    /// Save Agent-specific prefs to UserDefaults
    func savePrefs() {
        guard !name.isEmpty else { return }
        UserDefaults.standard.set(enabled, forKey: Self.enabledPrefix + name)
        UserDefaults.standard.set(autoStart, forKey: Self.autoStartPrefix + name)
    }

    /// Remove Agent-specific prefs from UserDefaults
    func removePrefs() {
        guard !name.isEmpty else { return }
        UserDefaults.standard.removeObject(forKey: Self.enabledPrefix + name)
        UserDefaults.standard.removeObject(forKey: Self.autoStartPrefix + name)
    }
}

// MARK: - MCP Server Registry

@MainActor @Observable
final class MCPServerRegistry {
    static let shared = MCPServerRegistry()

    private let configFileURL: URL = {
        guard let appSupport = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first else {
            return URL(fileURLWithPath: NSTemporaryDirectory()).appendingPathComponent("mcp_servers.json")
        }
        let dir = appSupport.appendingPathComponent("Agent", isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir.appendingPathComponent("mcp_servers.json")
    }()

    private(set) var servers: [MCPServerConfig] = []

    private init() {
        load()
    }

    // MARK: - CRUD Operations

    @discardableResult
    func add(_ config: MCPServerConfig) -> String? {
        if config.isHTTP {
            guard let url = URL(string: config.url ?? ""), url.scheme != nil, url.host != nil else {
                return "Invalid URL: \(config.url ?? "")"
            }
        } else {
            guard Self.validateCommandPath(config.command) else {
                return "Command not found: \(config.command)"
            }
        }
        config.savePrefs()
        servers.append(config)
        save()
        return nil
    }

    func update(_ config: MCPServerConfig) {
        if let index = servers.firstIndex(where: { $0.id == config.id }) {
            if config.isHTTP {
                guard let url = URL(string: config.url ?? ""), url.scheme != nil, url.host != nil else {
                    AuditLog.log(.mcp, "[MCPConfig] Refusing to update server: invalid URL \(config.url ?? "")")
                    return
                }
            } else {
                guard Self.validateCommandPath(config.command) else {
                    AuditLog.log(.mcp, "[MCPConfig] Refusing to update server: command not found at \(config.command)")
                    return
                }
            }
            config.savePrefs()
            servers[index] = config
            save()
        }
    }

    /// Validate that a command exists — supports both absolute paths and bare names resolved via PATH.
    /// macOS apps don't inherit the user's shell PATH, so we check common tool directories.
    private static func validateCommandPath(_ command: String) -> Bool {
        let fm = FileManager.default
        // Absolute or relative path — check directly
        if command.contains("/") {
            var isDir: ObjCBool = false
            guard fm.fileExists(atPath: command, isDirectory: &isDir) else { return false }
            return !isDir.boolValue
        }
        // Bare command name — check common dirs + PATH
        let home = fm.homeDirectoryForCurrentUser.path
        let searchDirs = [
            "\(home)/.local/bin",
            "/opt/homebrew/bin",
            "/usr/local/bin",
            "\(home)/.cargo/bin",
            "\(home)/.nvm/current/bin",
            "/usr/bin",
            "/bin",
        ]
        for dir in searchDirs {
            let full = "\(dir)/\(command)"
            if fm.fileExists(atPath: full) { return true }
        }
        // Also check process PATH as fallback
        let pathDirs = (ProcessInfo.processInfo.environment["PATH"] ?? "")
            .split(separator: ":")
        for dir in pathDirs {
            let full = "\(dir)/\(command)"
            if fm.fileExists(atPath: full) { return true }
        }
        return false
    }

    func remove(at id: UUID) {
        if let server = servers.first(where: { $0.id == id }) {
            server.removePrefs()
        }
        servers.removeAll { $0.id == id }
        save()
    }

    func toggleEnabled(_ id: UUID) {
        if let index = servers.firstIndex(where: { $0.id == id }) {
            servers[index].enabled.toggle()
            servers[index].savePrefs()
        }
    }

    func setEnabled(_ id: UUID, _ enabled: Bool) {
        if let index = servers.firstIndex(where: { $0.id == id }) {
            servers[index].enabled = enabled
            servers[index].savePrefs()
        }
    }

    // MARK: - Persistence

    private func load() {
        guard FileManager.default.fileExists(atPath: configFileURL.path),
              let data = try? Data(contentsOf: configFileURL),
              let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let mcpServers = json["mcpServers"] as? [String: Any] else {
            servers = Self.defaultServers
            return
        }
        var result: [MCPServerConfig] = []
        for (name, value) in mcpServers {
            guard let serverDict = value as? [String: Any],
                  var config = MCPServerConfig(from: serverDict) else { continue }
            config.name = name
            config.loadPrefs()
            result.append(config)
        }
        servers = result.sorted { $0.name.localizedCaseInsensitiveCompare($1.name) == .orderedAscending }
    }

    private func save() {
        var mcpServers: [String: Any] = [:]
        for server in servers {
            mcpServers[server.name] = server.toDictionary()
        }
        let root: [String: Any] = ["mcpServers": mcpServers]
        guard let data = try? JSONSerialization.data(withJSONObject: root, options: [.prettyPrinted, .sortedKeys]) else { return }

        Task.detached(priority: .background) { [url = configFileURL, data] in
            try? data.write(to: url, options: .atomic)
            // Set restrictive permissions (owner read/write only)
            try? FileManager.default.setAttributes(
                [.posixPermissions: 0o600],
                ofItemAtPath: url.path
            )
        }
    }

    // MARK: - Default Servers

    static let defaultServers: [MCPServerConfig] = []

    // MARK: - Import/Export

    func exportAll() -> String {
        var mcpServers: [String: Any] = [:]
        for server in servers {
            mcpServers[server.name] = server.toDictionary()
        }
        let root: [String: Any] = ["mcpServers": mcpServers]
        guard let data = try? JSONSerialization.data(withJSONObject: root, options: [.prettyPrinted, .sortedKeys]),
              let json = String(data: data, encoding: .utf8) else { return "{}" }
        return json
    }

    func importFrom(_ jsonString: String) -> Bool {
        guard let data = jsonString.data(using: .utf8),
              let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { return false }

        // Accept { "mcpServers": { ... } } or { "name": { ... } } directly
        let serverDict: [String: Any]
        if let mcp = json["mcpServers"] as? [String: Any] {
            serverDict = mcp
        } else {
            // Treat top-level keys as server names (but filter out non-server keys like globalShortcut)
            let knownNonServerKeys = Set(["globalShortcut"])
            serverDict = json.filter { key, value in
                !knownNonServerKeys.contains(key) && value is [String: Any]
            }
        }
        guard !serverDict.isEmpty else { return false }

        for (name, value) in serverDict {
            guard let entry = value as? [String: Any],
                  var config = MCPServerConfig(from: entry) else { continue }
            config.name = name
            config.loadPrefs()
            if let index = servers.firstIndex(where: { $0.name == name }) {
                config.id = servers[index].id
                servers[index] = config
            } else {
                servers.append(config)
            }
        }
        save()
        return true
    }
}
