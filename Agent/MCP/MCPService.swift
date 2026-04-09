import AgentAudit
import Foundation
import AgentMCP

/// / Service for managing MCP server connections / Acts as a bridge between the UI (MCPServersView) and the underlying
/// MCP client / The MCPClient is imported from the AgentMCP package
@MainActor @Observable
final class MCPService: @unchecked Sendable {
    static let shared = MCPService()

    private let client = MCPClient()
    private(set) var connectedServerIds: Set<UUID> = []
    var connectionErrors: [UUID: String] = [:]
    private(set) var discoveredTools: [MCPToolInfo] = []
    private(set) var discoveredResources: [MCPResourceInfo] = []
    /// Tool names disabled by the user, keyed by server name. Stored in UserDefaults.
    var disabledTools: Set<String> = [] {
        didSet { saveDisabledTools() }
    }

    private static let disabledToolsKey = "mcp.disabledTools"

    private func loadDisabledTools() {
        let arr = UserDefaults.standard.stringArray(forKey: Self.disabledToolsKey) ?? []
        disabledTools = Set(arr)
    }

    private func saveDisabledTools() {
        UserDefaults.standard.set(Array(disabledTools), forKey: Self.disabledToolsKey)
    }

    /// Unique key for a tool: "serverName.toolName"
    static func toolKey(serverName: String, toolName: String) -> String {
        "\(serverName).\(toolName)"
    }

    /// Check if a tool is enabled
    func isToolEnabled(serverName: String, toolName: String) -> Bool {
        !disabledTools.contains(Self.toolKey(serverName: serverName, toolName: toolName))
    }

    /// Toggle a tool's enabled state
    func toggleTool(serverName: String, toolName: String) {
        let key = Self.toolKey(serverName: serverName, toolName: toolName)
        if disabledTools.contains(key) {
            disabledTools.remove(key)
        } else {
            disabledTools.insert(key)
        }
    }

    struct MCPToolInfo: Identifiable, Sendable {
        let id: UUID
        let serverId: UUID
        let serverName: String
        let name: String
        let description: String
        let inputSchemaJSON: String
    }

    struct MCPResourceInfo: Identifiable, Sendable {
        let id: UUID
        let serverId: UUID
        let serverName: String
        let uri: String
        let name: String
    }

    private init() { loadDisabledTools() }

    /// Connect to an MCP server (stdio or HTTP)
    func connect(to config: MCPServerConfig) async throws {
        let serverConfig: MCPClient.ServerConfig
        if config.isHTTP {
            serverConfig = MCPClient.ServerConfig(
                id: config.id,
                name: config.name,
                url: config.url ?? "",
                headers: config.headers,
                sseEndpoint: config.sseEndpoint,
                httpEndpoint: config.httpEndpoint,
                enabled: config.enabled,
                autoStart: config.autoStart
            )
        } else {
            // Resolve bare command names (e.g. "uvx") to full paths since
            // macOS apps don't inherit the user's shell PATH
            let resolvedCommand = Self.resolveCommand(config.command)

            // Merge user's PATH into the server environment so child processes can find tools
            var env = config.environment
            if env["PATH"] == nil {
                env["PATH"] = Self.userShellPATH()
            }

            serverConfig = MCPClient.ServerConfig(
                id: config.id,
                name: config.name,
                command: resolvedCommand,
                arguments: config.arguments,
                env: env,
                enabled: config.enabled,
                autoStart: config.autoStart
            )
        }

        try await client.addServer(serverConfig)
        connectedServerIds.insert(config.id)
        connectionErrors.removeValue(forKey: config.id)

        // Refresh state
        await refreshState()
    }

    /// Disconnect from an MCP server (with timeout to prevent beach ball)
    func disconnect(serverId: UUID) async {
        // Timeout the removeServer call to prevent hangs on unresponsive servers
        await withTaskGroup(of: Void.self) { group in
            group.addTask {
                await self.client.removeServer(serverId)
            }
            group.addTask {
                try? await Task.sleep(for: .seconds(5))
            }
            // Whichever finishes first, cancel the other
            await group.next()
            group.cancelAll()
        }
        connectedServerIds.remove(serverId)
        connectionErrors.removeValue(forKey: serverId)

        await refreshState()
    }

    /// Disconnect all connected MCP servers (called on app quit)
    func disconnectAll() async {
        for serverId in connectedServerIds {
            await client.removeServer(serverId)
        }
        connectedServerIds.removeAll()
        connectionErrors.removeAll()
    }

    /// Start all servers marked with autoStart
    func startAutoStartServers() async {
        let autoStartConfigs = MCPServerRegistry.shared.servers
            .filter { $0.autoStart && $0.enabled && !connectedServerIds.contains($0.id) }

        for config in autoStartConfigs {
            do {
                try await connect(to: config)
            } catch {
                connectionErrors[config.id] = error.localizedDescription
            }
        }
    }

    /// Check if a server is connected
    func isConnected(_ serverId: UUID) async -> Bool {
        await client.isConnected(serverId)
    }

    /// Get error for a server
    func getError(_ serverId: UUID) async -> String? {
        // Check both local errors and client-side errors
        if let localError = connectionErrors[serverId] {
            return localError
        }
        return await client.getError(serverId)
    }

    /// Refresh local state from client
    func refreshState() async {
        let state = await client.getConnectionState()

        discoveredTools = state.discoveredTools.map { tool in
            MCPToolInfo(
                id: tool.id,
                serverId: tool.serverId,
                serverName: tool.serverName,
                name: tool.name,
                description: tool.description,
                inputSchemaJSON: tool.inputSchemaJSON
            )
        }

        discoveredResources = state.discoveredResources.map { resource in
            MCPResourceInfo(
                id: resource.id,
                serverId: resource.serverId,
                serverName: resource.serverName,
                uri: resource.uri,
                name: resource.name
            )
        }
    }

    /// Call a tool on a specific server
    func callTool(serverId: UUID, name: String, arguments: [String: JSONValue]) async throws -> MCPClient.ToolResult {
        try await client.callTool(serverId: serverId, name: name, arguments: arguments)
    }

    /// Read a resource from a server
    func readResource(serverId: UUID, uri: String) async throws -> MCPClient.ResourceContent {
        try await client.readResource(serverId: serverId, uri: uri)
    }

    /// Refresh a server connection
    func refreshConnection(serverId: UUID) async throws {
        guard let config = MCPServerRegistry.shared.servers.first(where: { $0.id == serverId }) else {
            return
        }

        await disconnect(serverId: serverId)
        try await connect(to: config)
    }

    // MARK: - PATH Resolution

    /// Resolve a bare command name to its full path via common directories.
    /// macOS apps don't inherit the user's shell PATH, so "uvx" won't be found.
    private static func resolveCommand(_ command: String) -> String {
        guard !command.contains("/") else { return command }
        let home = FileManager.default.homeDirectoryForCurrentUser.path
        let searchDirs = [
            "\(home)/.local/bin",
            "/usr/local/bin",
            "/opt/homebrew/bin",
            "\(home)/.cargo/bin",
            "\(home)/.nvm/current/bin",
            "/usr/bin",
            "/bin",
        ]
        let fm = FileManager.default
        for dir in searchDirs {
            let full = "\(dir)/\(command)"
            if fm.fileExists(atPath: full) { return full }
        }
        // Also check PATH env
        let pathDirs = (ProcessInfo.processInfo.environment["PATH"] ?? "")
            .split(separator: ":")
        for dir in pathDirs {
            let full = "\(dir)/\(command)"
            if fm.fileExists(atPath: full) { return full }
        }
        return command // Return as-is if not found
    }

    /// Get a reasonable PATH string including common user tool directories.
    private static func userShellPATH() -> String {
        let home = FileManager.default.homeDirectoryForCurrentUser.path
        let extra = [
            "\(home)/.local/bin",
            "/opt/homebrew/bin",
            "/usr/local/bin",
            "\(home)/.cargo/bin",
        ]
        let existing = ProcessInfo.processInfo.environment["PATH"] ?? "/usr/bin:/bin"
        return (extra + [existing]).joined(separator: ":")
    }
}
