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
    /// Curated MCP server presets. Currently empty — see history for the
    /// removed Z.AI Web Search preset and why it doesn't work.
    ///
    /// REMOVED 2026-04-08:
    /// - "Z.AI Web Search" (https://api.z.ai/api/mcp/web_search/sse?...)
    ///   AgentMCP 1.6.1 implements the legacy HTTP+SSE transport
    ///   (LegacyHTTPSSEConnection) end-to-end correctly — bytes are
    ///   pulled via URLSessionDataDelegate, the parser handles partial
    ///   chunks, and the endpoint discovery race is fixed.
    ///
    ///   But Z.AI's MCP server itself returns a TRUNCATED tools/list
    ///   response: ~1914 bytes of JSON with 16 unbalanced opening
    ///   braces and only 10 closing braces, ending mid-Chinese-character
    ///   with no terminator. Confirmed by raw curl over HTTP/2 after
    ///   3 minutes of waiting — same truncated stream every time.
    ///   The bytes are unparseable as JSON because the JSON itself is
    ///   incomplete. There's nothing the client can do.
    ///
    ///   Z.AI users wanting web search should set their Z.AI API key
    ///   in Settings — the LLM's `web_search` tool routes through
    ///   POST https://api.z.ai/api/paas/v4/web_search natively, using
    ///   the same key, returning the same `search-prime` results. See
    ///   AgentViewModel+TaskUtilities.performZAIWebSearchInternal.
    ///
    ///   Re-enable this preset only after Z.AI fixes their MCP server.
    static let all: [MCPPreset] = []
}
