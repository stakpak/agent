import Foundation

/// / Curated MCP server presets — one-click add with pre-populated URL/headers. / User fills in their API key (or
/// auto-filled from keychain). / / To add a preset: append `MCPPreset` to `all` with unique id, label, and `makeConfig()`. / `userKeyResolver` can pull a stored API key from KeychainService.
struct MCPPreset: Identifiable {
    let id: String
    let menuLabel: String
    let makeConfig: () -> MCPServerConfig
}

@MainActor
enum MCPPresets {
    /// / Curated MCP server presets. Currently empty — the Z.AI Web Search preset / was removed 2026-04-08 because
    /// Z.AI's MCP server returns truncated tools/list / JSON (unbalanced braces, mid-character cutoff). Use `web_search` tool via / Settings → Z.AI API key instead (see performZAIWebSearchInternal). / Re-enable only after Z.AI fixes their MCP server.
    static let all: [MCPPreset] = [
        MCPPreset(id: "xcf", menuLabel: "xcf.ai — Xcode Build & Run") {
            MCPServerConfig(
                name: "xcf",
                command: "/Applications/xcf.app/Contents/MacOS/xcf server"
            )
        },
    ]
}
