import Foundation

/// Curated MCP server presets — well-known third-party MCP endpoints that
/// users can add with one click instead of typing the URL and headers by
/// hand. Selecting a preset opens the standard MCP server edit sheet with
/// the URL/headers pre-populated; the user fills in their API key (or it's
/// auto-filled from the keychain if we already have one for that provider)
/// and saves.
///
/// To add a new preset: append a new `MCPPreset` to `all`, supply a unique
/// id and a label, and return a fully-formed `MCPServerConfig` from
/// `makeConfig()`. The `userKeyResolver` closure can pull a stored API key
/// from KeychainService so the user doesn't have to paste it again.
struct MCPPreset: Identifiable {
    let id: String
    let menuLabel: String
    let makeConfig: () -> MCPServerConfig
}

@MainActor
enum MCPPresets {
    /// Sentinel string the user is expected to replace with their API key
    /// when no key is on file. We surface it in the URL so it's visually
    /// obvious in the edit sheet.
    static let placeholderKey = "YOUR_ZAI_API_KEY"

    static let all: [MCPPreset] = [
        MCPPreset(
            id: "zai.web_search",
            menuLabel: "Z.AI Web Search"
        ) {
            // Z.AI's MCP server passes the API key as an Authorization query
            // parameter (not a header). If the user already saved a Z.AI key
            // in Settings, paste it directly into the URL so the preset is
            // ready to connect with no further editing.
            let key = KeychainService.shared.getZAIAPIKey()?.trimmingCharacters(in: .whitespacesAndNewlines)
            let token = (key?.isEmpty == false) ? key! : placeholderKey
            return MCPServerConfig(
                name: "Z.AI Web Search",
                url: "https://api.z.ai/api/mcp/web_search/sse?Authorization=\(token)",
                headers: [:],
                enabled: true,
                autoStart: true
            )
        },
    ]
}
