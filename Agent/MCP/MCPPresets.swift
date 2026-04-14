import Foundation

/// / Curated MCP server presets
struct MCPPreset: Identifiable {
    let id: String
    let menuLabel: String
    let makeConfig: () -> MCPServerConfig
}

@MainActor
enum MCPPresets {
    /// / Curated MCP server presets.
    static let all: [MCPPreset] = [
        MCPPreset(id: "xcf", menuLabel: "xcf.ai — Xcode Build & Run") {
            MCPServerConfig(
                name: "xcf",
                command: "/Applications/xcf.app/Contents/MacOS/xcf server"
            )
        },
    ]
}
