import SwiftUI
import AgentAudit

struct MCPServersView: View {
    @Bindable var registry = MCPServerRegistry.shared
    @Bindable var mcpService = MCPService.shared
    @State private var showingAddServer = false
    @State private var editingServer: MCPServerConfig?
    @State private var showingImport = false
    @State private var importText = ""
    @State private var connectingIds: Set<UUID> = []
    @State private var renderKey = false
    @State private var addError: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            // Header
            Text("MCP Servers")
                .font(.headline)

            Text("Connect external tools via Model Context Protocol.")
                .font(.caption)
                .foregroundStyle(.secondary)

            HStack {
                Spacer()
                Button {
                    Task { await mcpService.refreshState() }
                } label: {
                    Image(systemName: "arrow.clockwise")
                }
                .buttonStyle(.bordered)
                .controlSize(.small)
                .help("Refresh server status")

                Button {
                    showingImport = true
                } label: {
                    Image(systemName: "square.and.arrow.down")
                }
                .buttonStyle(.bordered)
                .controlSize(.small)
                .help("Import server configuration")

                Button {
                    showingAddServer = true
                } label: {
                    Image(systemName: "plus")
                }
                .buttonStyle(.bordered)
                .controlSize(.small)
                .help("Add MCP server")
            }

            if registry.servers.isEmpty {
                VStack(spacing: 12) {
                    Image(systemName: "server.rack")
                        .font(.system(size: 32))
                        .foregroundStyle(.secondary)
                    Text("No MCP servers configured")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                    Text("Add servers to expose tools to Agent!")
                        .font(.caption)
                        .foregroundStyle(.tertiary)
                }
                .frame(maxWidth: .infinity)
                .padding(.vertical, 20)
            } else {
                ScrollView {
                    LazyVStack(spacing: 8) {
                        ForEach(registry.servers) { server in
                            serverRow(server)
                        }
                    }
                }
                .id(renderKey)
            }

            Divider()

            VStack(alignment: .leading, spacing: 4) {
                Text("MCP (Model Context Protocol) servers provide tools to Agent!")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Text("Supports stdio and HTTP/HTTPS transport.")
                    .font(.caption)
                    .foregroundStyle(.tertiary)
            }
        }
        .padding(16)
        .padding(.bottom, 15)
        .frame(width: 420)
        .frame(maxHeight: 500)
        .sheet(isPresented: $showingAddServer) {
            MCPServerEditView(server: nil) { newServer in
                if let err = registry.add(newServer) {
                    addError = err
                } else {
                    showingAddServer = false
                    // Auto-connect the new server
                    let config = newServer
                    Task {
                        connectingIds.insert(config.id)
                        try? await mcpService.connect(to: config)
                        connectingIds.remove(config.id)
                    }
                }
            }
        }
        .alert("Add Failed", isPresented: Binding(get: { addError != nil }, set: { if !$0 { addError = nil } })) {
            Button("OK") { addError = nil }
        } message: {
            Text(addError ?? "")
        }
        .sheet(item: $editingServer) { server in
            MCPServerEditView(server: server) { updatedServer in
                registry.update(updatedServer)
                editingServer = nil
            }
        }
        .sheet(isPresented: $showingImport) {
            MCPImportView(registry: registry, isPresented: $showingImport)
        }
        .onAppear {
            // Force toggles to re-render with correct thumb after popover appears
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.05) {
                renderKey.toggle()
            }
        }
    }

    // MARK: - Status (single source of truth: MCPService)

    private func statusFor(_ id: UUID) -> ServerStatus {
        if connectingIds.contains(id) { return .connecting }
        if mcpService.connectedServerIds.contains(id) { return .connected }
        if let err = mcpService.connectionErrors[id] { return .error(err) }
        return .disconnected
    }

    enum ServerStatus {
        case disconnected, connecting, connected, error(String)
    }

    // MARK: - Actions

    /// Connect or disconnect based on the NEW enabled state (already set by the toggle binding).
    private func connectOrDisconnect(_ serverId: UUID, enable: Bool) async {
        if enable {
            guard let server = registry.servers.first(where: { $0.id == serverId }) else { return }
            connectingIds.insert(serverId)
            do {
                try await mcpService.connect(to: server)
            } catch {
                mcpService.connectionErrors[serverId] = error.localizedDescription
            }
            connectingIds.remove(serverId)
        } else {
            connectingIds.remove(serverId)
            await mcpService.disconnect(serverId: serverId)
        }
    }

    // MARK: - Row

    @ViewBuilder
    private func serverRow(_ server: MCPServerConfig) -> some View {
        let status = statusFor(server.id)

        HStack(spacing: 12) {
            VStack(alignment: .leading, spacing: 2) {
                Toggle("", isOn: Binding(
                    get: { registry.servers.first(where: { $0.id == server.id })?.enabled ?? false },
                    set: { newValue in
                        registry.setEnabled(server.id, newValue)
                        Task { await connectOrDisconnect(server.id, enable: newValue) }
                    }
                ))
                .toggleStyle(.switch)
                .controlSize(.mini)

                HStack(spacing: 3) {
                    switch status {
                    case .connected:
                        Circle().fill(.green).frame(width: 6, height: 6)
                        Text("Connected").font(.caption2).foregroundStyle(.green)
                    case .connecting:
                        ProgressView().controlSize(.mini)
                        Text("Connecting...").font(.caption2).foregroundStyle(.secondary)
                    case .disconnected:
                        Circle().fill(.secondary).frame(width: 6, height: 6)
                        Text("Disconnected").font(.caption2).foregroundStyle(.secondary)
                    case .error(let message):
                        Circle().fill(.red).frame(width: 6, height: 6)
                        Text(message).font(.caption2).foregroundStyle(.red).lineLimit(1)
                    }
                }
            }

            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 6) {
                    Text(server.name).font(.subheadline).fontWeight(.medium)
                    if server.autoStart {
                        Text("auto").font(.caption2).foregroundStyle(.white)
                            .padding(.horizontal, 4).padding(.vertical, 1)
                            .background(.blue).clipShape(Capsule())
                    }
                }
                Text(server.displayAddress).font(.caption).foregroundStyle(.secondary)
                    .lineLimit(1).truncationMode(.middle)
                // Show discovered tools as toggleable tags
                let tools = mcpService.discoveredTools.filter { $0.serverId == server.id }
                if !tools.isEmpty {
                    FlowLayout(spacing: 4) {
                        ForEach(tools) { tool in
                            let enabled = mcpService.isToolEnabled(serverName: server.name, toolName: tool.name)
                            Button {
                                mcpService.toggleTool(serverName: server.name, toolName: tool.name)
                            } label: {
                                Text(tool.name)
                                    .font(.caption2)
                                    .padding(.horizontal, 6)
                                    .padding(.vertical, 2)
                                    .background(enabled ? Color.accentColor.opacity(0.2) : Color.secondary.opacity(0.1))
                                    .foregroundStyle(enabled ? .primary : .tertiary)
                                    .clipShape(Capsule())
                                    .overlay(Capsule().stroke(enabled ? Color.accentColor.opacity(0.5) : Color.clear, lineWidth: 0.5))
                            }
                            .buttonStyle(.plain)
                            .help(tool.description)
                        }
                    }
                }
            }

            Spacer()

            HStack(spacing: 4) {
                Button { editingServer = server } label: {
                    Image(systemName: "pencil")
                }
                .buttonStyle(.bordered).controlSize(.mini)

                Button(role: .destructive) {
                    let serverId = server.id
                    Task {
                        await mcpService.disconnect(serverId: serverId)
                        registry.remove(at: serverId)
                    }
                } label: {
                    Image(systemName: "trash")
                }
                .buttonStyle(.bordered).controlSize(.mini)
            }
        }
        .padding(8)
        .background(.secondary.opacity(0.05))
        .clipShape(RoundedRectangle(cornerRadius: 8))
    }
}

// MARK: - Edit View

struct MCPServerEditView: View {
    let server: MCPServerConfig?
    let onSave: (MCPServerConfig) -> Void

    @State private var name: String
    @State private var useHTTP: Bool
    // Stdio fields
    @State private var command: String
    @State private var argumentsText: String
    @State private var environmentText: String
    // HTTP fields
    @State private var urlText: String
    @State private var headersText: String
    @State private var sseEndpointText: String
    @State private var httpEndpointText: String
    // Common
    @State private var enabled: Bool
    @State private var autoStart: Bool
    @State private var jsonText: String
    @State private var jsonError: String?
    @State private var updatingFromJSON = false
    @State private var updatingFromFields = false
    @State private var currentUnsupportedFieldsJSON: String? = nil

    private func syncFieldsToJSON() {
        guard !updatingFromJSON else { return }
        updatingFromFields = true
        jsonText = previewJSON
        // Keep flag alive across the SwiftUI update cycle
        DispatchQueue.main.async { updatingFromFields = false }
    }

    @Environment(\.dismiss) private var dismiss

    init(server: MCPServerConfig?, onSave: @escaping (MCPServerConfig) -> Void) {
        self.server = server
        self.onSave = onSave
        _name = State(initialValue: server?.name ?? "")
        _useHTTP = State(initialValue: server?.isHTTP ?? false)
        _command = State(initialValue: server?.command ?? "")
        _argumentsText = State(initialValue: server?.arguments.joined(separator: "\n") ?? "")
        _environmentText = State(initialValue: server?.environment.map { "\($0.key)=\($0.value)" }.joined(separator: "\n") ?? "")
        _urlText = State(initialValue: server?.url ?? "")
        _headersText = State(initialValue: server?.headers.map { "\($0.key): \($0.value)" }.joined(separator: "\n") ?? "")
        _sseEndpointText = State(initialValue: server?.sseEndpoint ?? "")
        _httpEndpointText = State(initialValue: server?.httpEndpoint ?? "")
        _enabled = State(initialValue: server?.enabled ?? true)
        _autoStart = State(initialValue: server?.autoStart ?? true)
        _currentUnsupportedFieldsJSON = State(initialValue: server?.unsupportedFieldsJSON)
        let initial = Self.buildConfig(from: server)
        _jsonText = State(initialValue: initial.toJSON())
        _jsonError = State(initialValue: nil)
    }

    private static func buildConfig(
        from server: MCPServerConfig?,
        nameOverride: String? = nil,
        useHTTPOverride: Bool? = nil,
        commandOverride: String? = nil,
        argsOverride: [String]? = nil,
        envOverride: [String: String]? = nil,
        urlOverride: String? = nil,
        headersOverride: [String: String]? = nil,
        sseOverride: String? = nil,
        httpOverride: String? = nil,
        enabledOverride: Bool? = nil,
        autoStartOverride: Bool? = nil,
        unsupportedJSON: String? = nil
    ) -> MCPServerConfig {
        let useHTTP = useHTTPOverride ?? (server?.isHTTP ?? false)
        let name = nameOverride ?? server?.name ?? ""

        if useHTTP {
            var config = MCPServerConfig(
                id: server?.id ?? UUID(),
                name: name,
                url: urlOverride ?? server?.url ?? "",
                headers: headersOverride ?? server?.headers ?? [:],
                enabled: enabledOverride ?? server?.enabled ?? true,
                autoStart: autoStartOverride ?? server?.autoStart ?? true
            )
            config.sseEndpoint = sseOverride ?? server?.sseEndpoint
            config.httpEndpoint = httpOverride ?? server?.httpEndpoint
            config.unsupportedFieldsJSON = unsupportedJSON ?? server?.unsupportedFieldsJSON
            return config
        } else {
            var config = MCPServerConfig(
                id: server?.id ?? UUID(),
                name: name,
                command: commandOverride ?? server?.command ?? "",
                arguments: argsOverride ?? server?.arguments ?? [],
                environment: envOverride ?? server?.environment ?? [:],
                enabled: enabledOverride ?? server?.enabled ?? true,
                autoStart: autoStartOverride ?? server?.autoStart ?? true
            )
            config.unsupportedFieldsJSON = unsupportedJSON ?? server?.unsupportedFieldsJSON
            return config
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                Text(server == nil ? "Add MCP Server" : "Edit MCP Server")
                    .font(.headline)
                Spacer()
                Button("Cancel") { dismiss() }
                    .buttonStyle(.bordered).controlSize(.small)
            }

            Divider()

            VStack(alignment: .leading, spacing: 12) {
                VStack(alignment: .leading, spacing: 4) {
                    Text("Name").font(.caption).foregroundStyle(.secondary)
                    TextField("My MCP Server", text: $name).textFieldStyle(.roundedBorder)
                }

                // Transport picker
                Picker("Transport", selection: $useHTTP) {
                    Text("Stdio").tag(false)
                    Text("HTTP").tag(true)
                }
                .pickerStyle(.segmented)

                if useHTTP {
                    // HTTP fields
                    VStack(alignment: .leading, spacing: 4) {
                        Text("URL").font(.caption).foregroundStyle(.secondary)
                        TextField("https://example.com/mcp", text: $urlText).textFieldStyle(.roundedBorder)
                    }
                    VStack(alignment: .leading, spacing: 4) {
                        Text("SSE Endpoint (optional)").font(.caption).foregroundStyle(.secondary)
                        TextField("/sse", text: $sseEndpointText).textFieldStyle(.roundedBorder)
                    }
                    VStack(alignment: .leading, spacing: 4) {
                        Text("HTTP Endpoint (optional)").font(.caption).foregroundStyle(.secondary)
                        TextField("/message", text: $httpEndpointText).textFieldStyle(.roundedBorder)
                    }
                    VStack(alignment: .leading, spacing: 4) {
                        Text("Headers (Name: Value, one per line)").font(.caption).foregroundStyle(.secondary)
                        TextField("Authorization: Bearer ...", text: $headersText, axis: .vertical)
                            .font(.system(.caption, design: .monospaced))
                            .textFieldStyle(.roundedBorder)
                            .lineLimit(3...6)
                    }
                } else {
                    // Stdio fields
                    VStack(alignment: .leading, spacing: 4) {
                        Text("Command").font(.caption).foregroundStyle(.secondary)
                        TextField("/usr/local/bin/my-mcp-server", text: $command).textFieldStyle(.roundedBorder)
                    }
                    VStack(alignment: .leading, spacing: 4) {
                        Text("Arguments (one per line)").font(.caption).foregroundStyle(.secondary)
                        TextField("arg1", text: $argumentsText, axis: .vertical)
                            .font(.system(.caption, design: .monospaced))
                            .textFieldStyle(.roundedBorder)
                            .lineLimit(3...6)
                    }
                    VStack(alignment: .leading, spacing: 4) {
                        Text("Environment Variables (KEY=value, one per line)").font(.caption).foregroundStyle(.secondary)
                        TextField("API_KEY=abc123", text: $environmentText, axis: .vertical)
                            .font(.system(.caption, design: .monospaced))
                            .textFieldStyle(.roundedBorder)
                            .lineLimit(3...6)
                            .disableAutocorrection(true)
                    }
                }

                HStack(spacing: 20) {
                    Toggle("Enabled", isOn: $enabled).toggleStyle(.switch).controlSize(.mini)
                    Toggle("Auto-start", isOn: $autoStart).toggleStyle(.switch).controlSize(.mini)
                }
                .onChange(of: name) { syncFieldsToJSON() }
                .onChange(of: useHTTP) { syncFieldsToJSON() }
                .onChange(of: command) { syncFieldsToJSON() }
                .onChange(of: argumentsText) { syncFieldsToJSON() }
                .onChange(of: environmentText) { syncFieldsToJSON() }
                .onChange(of: urlText) { syncFieldsToJSON() }
                .onChange(of: sseEndpointText) { syncFieldsToJSON() }
                .onChange(of: httpEndpointText) { syncFieldsToJSON() }
                .onChange(of: headersText) { syncFieldsToJSON() }
                .onChange(of: enabled) { syncFieldsToJSON() }
                .onChange(of: autoStart) { syncFieldsToJSON() }
            }

            Divider()

            VStack(alignment: .leading, spacing: 4) {
                Text("JSON").font(.caption).foregroundStyle(.secondary)
                PlainTextEditor(text: $jsonText)
                    .frame(height: 120)
                    .clipShape(RoundedRectangle(cornerRadius: 6))
                    .onChange(of: jsonText) {
                        if !updatingFromFields { applyJSON(jsonText) }
                    }
                if let jsonError {
                    Text(jsonError)
                        .font(.caption2)
                        .foregroundStyle(.red)
                }
            }

            HStack {
                Button("Revert") {
                    jsonText = previewJSON
                    jsonError = nil
                }
                .buttonStyle(.bordered).controlSize(.small)
                Spacer()
                Button("Save") {
                    let config: MCPServerConfig
                    if useHTTP {
                        var httpConfig = MCPServerConfig(
                            id: server?.id ?? UUID(),
                            name: name,
                            url: urlText,
                            headers: parseHeaders(headersText),
                            enabled: enabled,
                            autoStart: autoStart
                        )
                        httpConfig.sseEndpoint = sseEndpointText.isEmpty ? nil : sseEndpointText
                        httpConfig.httpEndpoint = httpEndpointText.isEmpty ? nil : httpEndpointText
                        httpConfig.unsupportedFieldsJSON = currentUnsupportedFieldsJSON
                        config = httpConfig
                    } else {
                        var stdioConfig = MCPServerConfig(
                            id: server?.id ?? UUID(),
                            name: name,
                            command: command,
                            arguments: argumentsText.split(separator: "\n").map(String.init),
                            environment: parseEnvironment(environmentText),
                            enabled: enabled,
                            autoStart: autoStart
                        )
                        stdioConfig.unsupportedFieldsJSON = currentUnsupportedFieldsJSON
                        config = stdioConfig
                    }
                    onSave(config)
                    dismiss()
                }
                .buttonStyle(.borderedProminent).controlSize(.small)
                .disabled(name.isEmpty || (useHTTP ? urlText.isEmpty : command.isEmpty))
            }
        }
        .padding(16)
        .frame(width: 400)
    }

    private var previewJSON: String {
        if useHTTP {
            var httpConfig = MCPServerConfig(
                id: server?.id ?? UUID(),
                name: name,
                url: urlText,
                headers: parseHeaders(headersText),
                enabled: enabled,
                autoStart: autoStart
            )
            httpConfig.sseEndpoint = sseEndpointText.isEmpty ? nil : sseEndpointText
            httpConfig.httpEndpoint = httpEndpointText.isEmpty ? nil : httpEndpointText
            httpConfig.unsupportedFieldsJSON = currentUnsupportedFieldsJSON
            return httpConfig.toJSON()
        } else {
            var stdioConfig = MCPServerConfig(
                id: server?.id ?? UUID(),
                name: name,
                command: command,
                arguments: argumentsText.split(separator: "\n").map(String.init),
                environment: parseEnvironment(environmentText),
                enabled: enabled,
                autoStart: autoStart
            )
            stdioConfig.unsupportedFieldsJSON = currentUnsupportedFieldsJSON
            return stdioConfig.toJSON()
        }
    }

    /// Debug helper: dump all fields from a JSON dict
    private func debugDump(_ dict: [String: Any], prefix: String = "") {
        for (key, value) in dict {
            if let nested = value as? [String: Any] {
                AuditLog.log(.mcp, "\(prefix)\(key):")
                debugDump(nested, prefix: "  \(prefix)")
            } else if let nested = value as? [Any] {
                AuditLog.log(.mcp, "\(prefix)\(key): [\(nested.count) items]")
            } else {
                AuditLog.log(.mcp, "\(prefix)\(key): \(value)")
            }
        }
    }

    /// Parse edited JSON back into the form fields.
    /// Accepts standard MCP format: {"mcpServers":{"name":{...}}} or {"name":{...}} or bare {command, args, ...}
    /// Preserves unsupported fields for export round-tripping
    private func applyJSON(_ json: String) {
        guard let data = json.data(using: .utf8),
              let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else
        {
            jsonError = "Invalid JSON"
            return
        }

        // Unwrap standard MCP format to find the server name and inner dict
        var serverName: String?
        var innerDict: [String: Any]?

        if let mcpServers = obj["mcpServers"] as? [String: Any] {
            // { "mcpServers": { "name": { ... } } } - standard format
            // Find the first server entry (there could be multiple, but edit view handles one at a time)
            for (key, value) in mcpServers {
                if let serverDict = value as? [String: Any] {
                    serverName = key
                    innerDict = serverDict
                    break // Take first server
                }
            }
        } else if obj["command"] != nil || obj["url"] != nil || obj["transport"] != nil {
            // Bare config: { "command": "...", "args": [...] }
            innerDict = obj
        } else {
            // { "name": { "command": "...", ... } } - single server format
            // Filter out non-server keys like "globalShortcut"
            let knownNonServerKeys = Set(["globalShortcut"])
            for (key, value) in obj {
                if !knownNonServerKeys.contains(key), let serverDict = value as? [String: Any] {
                    serverName = key
                    innerDict = serverDict
                    break // Take first server
                }
            }
        }

        guard let dict = innerDict,
              var config = MCPServerConfig(from: dict) else
        {
            jsonError = "Invalid MCP server config"
            return
        }

        if let sn = serverName {
            config.name = sn
        }

        jsonError = nil
        updatingFromJSON = true
        if !config.name.isEmpty {
            name = config.name
        }
        if config.isHTTP {
            useHTTP = true
            urlText = config.url ?? ""
            headersText = config.headers.map { "\($0.key): \($0.value)" }.joined(separator: "\n")
            sseEndpointText = config.sseEndpoint ?? ""
            httpEndpointText = config.httpEndpoint ?? ""
        } else {
            useHTTP = false
            command = config.command
            argumentsText = config.arguments.joined(separator: "\n")
            environmentText = config.environment.map { "\($0.key)=\($0.value)" }.joined(separator: "\n")
        }
        enabled = config.enabled
        autoStart = config.autoStart

        // Store unsupported fields for round-tripping
        currentUnsupportedFieldsJSON = config.unsupportedFieldsJSON

        updatingFromJSON = false
    }

    private func parseEnvironment(_ text: String) -> [String: String] {
        var result: [String: String] = [:]
        for line in text.split(separator: "\n") {
            let parts = line.split(separator: "=", maxSplits: 1)
            if parts.count == 2 {
                result[String(parts[0])] = String(parts[1])
            }
        }
        return result
    }

    private func parseHeaders(_ text: String) -> [String: String] {
        var result: [String: String] = [:]
        for line in text.split(separator: "\n") {
            let parts = line.split(separator: ":", maxSplits: 1)
            if parts.count == 2 {
                result[String(parts[0]).trimmingCharacters(in: .whitespaces)] = String(parts[1]).trimmingCharacters(in: .whitespaces)
            }
        }
        return result
    }
}

// MARK: - Plain Text Editor (no smart quotes)

private struct PlainTextEditor: NSViewRepresentable {
    @Binding var text: String

    func makeNSView(context: Context) -> NSScrollView {
        let scrollView = NSTextView.scrollableTextView()
        guard let textView = scrollView.documentView as? NSTextView else { return scrollView }
        textView.font = .monospacedSystemFont(ofSize: NSFont.smallSystemFontSize, weight: .regular)
        textView.isAutomaticQuoteSubstitutionEnabled = false
        textView.isAutomaticDashSubstitutionEnabled = false
        textView.isAutomaticTextReplacementEnabled = false
        textView.isAutomaticSpellingCorrectionEnabled = false
        textView.isRichText = false
        textView.backgroundColor = NSColor.textBackgroundColor.withAlphaComponent(0.05)
        textView.textContainerInset = NSSize(width: 4, height: 4)
        textView.delegate = context.coordinator
        textView.string = text
        scrollView.hasVerticalScroller = true
        scrollView.drawsBackground = false
        return scrollView
    }

    func updateNSView(_ scrollView: NSScrollView, context: Context) {
    }

    func makeCoordinator() -> Coordinator { Coordinator(text: $text) }

    class Coordinator: NSObject, NSTextViewDelegate {
        var text: Binding<String>
        init(text: Binding<String>) { self.text = text }

        func textDidChange(_ notification: Notification) {
            guard let tv = notification.object as? NSTextView else { return }
            text.wrappedValue = tv.string
        }
    }
}

// MARK: - Import View (proper sheet with text editor)

struct MCPImportView: View {
    let registry: MCPServerRegistry
    @Binding var isPresented: Bool
    @State private var jsonText = ""
    @State private var errorText: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Import MCP Server")
                .font(.headline)
            Text("Paste standard MCP JSON configuration:")
                .font(.caption)
                .foregroundStyle(.secondary)

            PlainTextEditor(text: $jsonText)
                .frame(height: 200)
                .clipShape(RoundedRectangle(cornerRadius: 6))

            if let errorText {
                Text(errorText)
                    .font(.caption)
                    .foregroundStyle(.red)
            }

            HStack {
                Button("Cancel") {
                    isPresented = false
                }
                .buttonStyle(.bordered)
                .controlSize(.small)
                Spacer()
                Button("Import") {
                    if registry.importFrom(jsonText) {
                        isPresented = false
                    } else {
                        errorText = "Invalid JSON. Expected format: {\"mcpServers\": {\"name\": {\"command\": \"...\", \"args\": [...]}}}"
                    }
                }
                .buttonStyle(.borderedProminent)
                .controlSize(.small)
                .disabled(jsonText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
        }
        .padding(16)
        .frame(width: 450)
    }
}

// MARK: - Flow Layout (wrapping horizontal layout for tool tags)

private struct FlowLayout: Layout {
    var spacing: CGFloat = 4

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let rows = computeRows(proposal: proposal, subviews: subviews)
        var height: CGFloat = 0
        for (i, row) in rows.enumerated() {
            let rowHeight = row.map { $0.sizeThatFits(.unspecified).height }.max() ?? 0
            height += rowHeight + (i > 0 ? spacing : 0)
        }
        return CGSize(width: proposal.width ?? 0, height: height)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let rows = computeRows(proposal: proposal, subviews: subviews)
        var y = bounds.minY
        for row in rows {
            let rowHeight = row.map { $0.sizeThatFits(.unspecified).height }.max() ?? 0
            var x = bounds.minX
            for subview in row {
                let size = subview.sizeThatFits(.unspecified)
                subview.place(at: CGPoint(x: x, y: y), proposal: ProposedViewSize(size))
                x += size.width + spacing
            }
            y += rowHeight + spacing
        }
    }

    private func computeRows(proposal: ProposedViewSize, subviews: Subviews) -> [[LayoutSubviews.Element]] {
        let maxWidth = proposal.width ?? .infinity
        var rows: [[LayoutSubviews.Element]] = [[]]
        var x: CGFloat = 0
        for subview in subviews {
            let size = subview.sizeThatFits(.unspecified)
            if x + size.width > maxWidth && !rows[rows.count - 1].isEmpty {
                rows.append([])
                x = 0
            }
            rows[rows.count - 1].append(subview)
            x += size.width + spacing
        }
        return rows
    }
}

#Preview {
    MCPServersView()
}
