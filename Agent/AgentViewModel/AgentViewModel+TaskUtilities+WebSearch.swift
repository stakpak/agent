
@preconcurrency import Foundation
import AgentTools
import AgentAudit
import AgentMCP
import AgentD1F
import Cocoa




// MARK: - Task Utilities: Web Search

extension AgentViewModel {

    // MARK: - Web Search (forwarding to WebSearch extension)

    /// Perform web search using the appropriate API based on provider.
    /// This delegates to the implementation in AgentViewModel+WebSearch.swift.
    nonisolated static func performWebSearchForTask(query: String, apiKey: String, provider: APIProvider) async -> String {
        // Fallback chain — each step tries a more universal backend:
        // 1. Ollama+key → Ollama Web Search
        // 2. Z.AI/BigModel+key → Z.AI search-prime
        // 3. Tavily key → Tavily
        // 4. DuckDuckGo HTML scrape (no key, always available)
        if provider == .ollama || provider == .localOllama {
            if let ollamaKey = KeychainService.shared.getOllamaAPIKey(), !ollamaKey.isEmpty {
                let ollamaResult = await performOllamaWebSearchInternal(query: query, apiKey: ollamaKey)
                if !ollamaResult.hasPrefix("Error:") {
                    return ollamaResult
                }
            }
        }
        // Z.AI / BigModel providers: try native web_search API first. Returns
        // structured results like Tavily. Not all accounts include it, so fall
        // through on error.
        if provider == .zAI || provider == .bigModel {
            if let zKey = KeychainService.shared.getZAIAPIKey(), !zKey.isEmpty {
                let zResult = await performZAIWebSearchInternal(query: query, apiKey: zKey)
                if !zResult.hasPrefix("Error:") {
                    return zResult
                }
            }
        }
        // Tavily — only attempt if a key is configured. Missing key → skip,
        // fall through to keyless DuckDuckGo instead of dead-ending.
        if !apiKey.isEmpty {
            let tavilyResult = await performTavilySearchForTask(query: query, apiKey: apiKey)
            if !tavilyResult.hasPrefix("Error:") {
                return tavilyResult
            }
        }
        // Universal keyless fallback — always-on safety net.
        return await performDuckDuckGoSearchInternal(query: query)
    }

    /// Calls Z.AI's /web_search endpoint (search-prime engine). Returns structured
    /// results. Used as Tavily replacement when user has a Z.AI API key configured.
    nonisolated private static func performZAIWebSearchInternal(query: String, apiKey: String) async -> String {
        guard !apiKey.isEmpty else { return "Error: Z.AI API key not set." }
        guard let url = URL(string: "https://api.z.ai/api/paas/v4/web_search") else {
            return "Error: Invalid Z.AI search URL"
        }
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.setValue("Bearer \(apiKey)", forHTTPHeaderField: "Authorization")
        request.timeoutInterval = llmAPITimeout
        let body: [String: Any] = [
            "search_engine": "search-prime",
            "search_query": query,
            "count": 10,
            "search_recency_filter": "noLimit",
        ]
        do {
            request.httpBody = try JSONSerialization.data(withJSONObject: body)
            let (data, response) = try await URLSession.shared.data(for: request)
            guard let httpResponse = response as? HTTPURLResponse else {
                return "Error: Invalid response from Z.AI"
            }
            guard httpResponse.statusCode == 200 else {
                let errorBody = String(data: data, encoding: .utf8) ?? "Unknown error"
                return "Error: Z.AI API returned \(httpResponse.statusCode): \(errorBody)"
            }
            guard let json = try JSONSerialization.jsonObject(with: data) as? [String: Any] else {
                return "Error: Failed to parse Z.AI response"
            }
            guard let results = json["search_result"] as? [[String: Any]], !results.isEmpty else {
                return "No results found for: \(query)"
            }
            var output = ""
            for (i, result) in results.enumerated() {
                let title = result["title"] as? String ?? "Untitled"
                let resultUrl = result["link"] as? String ?? ""
                let content = result["content"] as? String ?? ""
                let media = result["media"] as? String ?? ""
                let publishDate = result["publish_date"] as? String ?? ""
                output += "\(i + 1). \(title)\n   \(resultUrl)\n"
                if !media.isEmpty || !publishDate.isEmpty {
                    let metaParts = [media, publishDate].filter { !$0.isEmpty }
                    output += "   [\(metaParts.joined(separator: " · "))]\n"
                }
                if !content.isEmpty {
                    output += "   \(content)\n"
                }
                output += "\n"
            }
            return output.trimmingCharacters(in: .whitespacesAndNewlines)
        } catch {
            return "Error: Z.AI web search failed: \(error.localizedDescription)"
        }
    }

    /// Universal keyless web search via DuckDuckGo HTML endpoint. No API key needed.
    /// Parses result rows with regex; extracts real URLs from DDG click-tracker links.
    nonisolated private static func performDuckDuckGoSearchInternal(query: String) async -> String {
        guard let encoded = query.addingPercentEncoding(withAllowedCharacters: .urlQueryAllowed) else {
            return "Error: failed to encode query"
        }
        guard let url = URL(string: "https://html.duckduckgo.com/html/?q=\(encoded)") else {
            return "Error: failed to build DuckDuckGo URL"
        }
        var request = URLRequest(url: url)
        // Real browser UA — DDG serves a stripped-down or empty page to
        // obvious bots. Matches the same UA the keyless web_fetch tool uses.
        request.setValue(
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) "
            + "AppleWebKit/605.1.15 (KHTML, like Gecko) "
            + "Version/17.0 Safari/605.1.15",
            forHTTPHeaderField: "User-Agent"
        )
        request.setValue("text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8", forHTTPHeaderField: "Accept")
        request.setValue("en-US,en;q=0.9", forHTTPHeaderField: "Accept-Language")
        request.timeoutInterval = llmAPITimeout
        do {
            let (data, response) = try await URLSession.shared.data(for: request)
            guard let httpResponse = response as? HTTPURLResponse else {
                return "Error: invalid response from DuckDuckGo"
            }
            guard (200..<400).contains(httpResponse.statusCode) else {
                return "Error: DuckDuckGo returned HTTP \(httpResponse.statusCode)"
            }
            guard let html = String(data: data, encoding: .utf8) else {
                return "Error: failed to decode DuckDuckGo response"
            }
            let results = parseDuckDuckGoHTML(html, limit: 10)
            if results.isEmpty {
                return "No results found for: \(query)"
            }
            var output = ""
            for (i, result) in results.enumerated() {
                output += "\(i + 1). \(result.title)\n   \(result.url)\n"
                if !result.snippet.isEmpty {
                    output += "   \(result.snippet)\n"
                }
                output += "\n"
            }
            return output.trimmingCharacters(in: .whitespacesAndNewlines)
        } catch {
            return "Error: DuckDuckGo web search failed: \(error.localizedDescription)"
        }
    }

    /// Parse the HTML result page returned by html.duckduckgo.com. Each
    /// result row exposes a `result__a` anchor for the title+URL and a
    /// `result__snippet` element for the description. The href is wrapped
    /// in DDG's click-tracker (`/l/?uddg=ENCODED_URL`) so we extract and
    /// decode the `uddg` query parameter to get the real destination.
    nonisolated private static func parseDuckDuckGoHTML(_ html: String, limit: Int) -> [(title: String, url: String, snippet: String)] {
        // Match: <a class="result__a" href="HREF" ... >TITLE</a>
        let titlePattern = #"<a[^>]*class="[^"]*result__a[^"]*"[^>]*href="([^"]+)"[^>]*>([\s\S]*?)</a>"#
        // Match: class="result__snippet" ...>SNIPPET</...>
        let snippetPattern = #"class="[^"]*result__snippet[^"]*"[^>]*>([\s\S]*?)</a>"#

        guard let titleRegex = try? NSRegularExpression(pattern: titlePattern, options: []),
              let snippetRegex = try? NSRegularExpression(pattern: snippetPattern, options: [])
        else {
            return []
        }

        let nsHtml = html as NSString
        let titleMatches = titleRegex.matches(in: html, range: NSRange(location: 0, length: nsHtml.length))
        let snippetMatches = snippetRegex.matches(in: html, range: NSRange(location: 0, length: nsHtml.length))

        var results: [(title: String, url: String, snippet: String)] = []
        for (i, match) in titleMatches.enumerated() where match.numberOfRanges > 2 {
            if results.count >= limit { break }
            let rawHref = nsHtml.substring(with: match.range(at: 1))
            let rawTitle = nsHtml.substring(with: match.range(at: 2))
            let resolvedURL = decodeDuckDuckGoHref(rawHref)
            let title = stripHTMLTags(rawTitle).trimmingCharacters(in: .whitespacesAndNewlines)
            var snippet = ""
            if i < snippetMatches.count, snippetMatches[i].numberOfRanges > 1 {
                let raw = nsHtml.substring(with: snippetMatches[i].range(at: 1))
                snippet = stripHTMLTags(raw).trimmingCharacters(in: .whitespacesAndNewlines)
            }
            if !title.isEmpty && !resolvedURL.isEmpty {
                results.append((title, resolvedURL, snippet))
            }
        }
        return results
    }

    /// DDG wraps every outbound link in `/l/?uddg=ENCODED_REAL_URL&rut=...`.
    /// Pull the `uddg` parameter and percent-decode it to recover the real
    /// destination. Falls back to the raw href if there's no `uddg` param.
    nonisolated private static func decodeDuckDuckGoHref(_ href: String) -> String {
        let normalized = href.hasPrefix("//") ? "https:" + href : href
        guard let comps = URLComponents(string: normalized) else { return href }
        if let uddg = comps.queryItems?.first(where: { $0.name == "uddg" })?.value {
            return uddg.removingPercentEncoding ?? uddg
        }
        return normalized
    }

    /// Lightweight HTML tag stripper for DDG result text. Result rows
    /// contain `<b>` highlighting and stray entities — strip the markup
    /// and decode the common entities so the LLM gets clean text.
    nonisolated private static func stripHTMLTags(_ s: String) -> String {
        var out = s.replacingOccurrences(of: #"<[^>]+>"#, with: "", options: .regularExpression)
        out = out
            .replacingOccurrences(of: "&amp;", with: "&")
            .replacingOccurrences(of: "&lt;", with: "<")
            .replacingOccurrences(of: "&gt;", with: ">")
            .replacingOccurrences(of: "&quot;", with: "\"")
            .replacingOccurrences(of: "&#39;", with: "'")
            .replacingOccurrences(of: "&nbsp;", with: " ")
        return out
    }

    nonisolated private static func performOllamaWebSearchInternal(query: String, apiKey: String) async -> String {
        guard !apiKey.isEmpty else { return "Error: Ollama API key not set. Add it in Settings." }
        guard let url = URL(string: "https://ollama.com/api/web_search") else { return "Error: Invalid Ollama search URL" }
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.setValue("Bearer \(apiKey)", forHTTPHeaderField: "Authorization")
        request.timeoutInterval = llmAPITimeout
        let body: [String: Any] = ["query": query, "max_results": 5]
        do {
            request.httpBody = try JSONSerialization.data(withJSONObject: body)
            let (data, response) = try await URLSession.shared.data(for: request)
            guard let httpResponse = response as? HTTPURLResponse else { return "Error: Invalid response from Ollama" }
            guard httpResponse.statusCode == 200 else {
                let errorBody = String(data: data, encoding: .utf8) ?? "Unknown error"
                return "Error: Ollama API returned \(httpResponse.statusCode): \(errorBody)"
            }
            guard let json = try JSONSerialization.jsonObject(with: data) as? [String: Any]
            else { return "Error: Failed to parse Ollama response" }
            if let results = json["results"] as? [[String: Any]], !results.isEmpty {
                var output = ""
                for (i, result) in results.enumerated() {
                    let title = result["title"] as? String ?? "Untitled"
                    let resultUrl = result["url"] as? String ?? ""
                    let content = result["content"] as? String ?? result["snippet"] as? String ?? ""
                    output += "\(i + 1). \(title)\n   \(resultUrl)\n   \(content)\n\n"
                }
                return output.trimmingCharacters(in: .whitespacesAndNewlines)
            }
            if let results = json["web_search_results"] as? [[String: Any]], !results.isEmpty {
                var output = ""
                for (i, result) in results.enumerated() {
                    let title = result["title"] as? String ?? "Untitled"
                    let resultUrl = result["url"] as? String ?? ""
                    let content = result["content"] as? String ?? result["snippet"] as? String ?? ""
                    output += "\(i + 1). \(title)\n   \(resultUrl)\n   \(content)\n\n"
                }
                return output.trimmingCharacters(in: .whitespacesAndNewlines)
            }
            return "No search results found for '\(query)'"
        } catch { return "Error: \(error.localizedDescription)" }
    }

    nonisolated private static func performTavilySearchForTask(query: String, apiKey: String) async -> String {
        guard !apiKey.isEmpty else { return "Error: Tavily API key not set. Add it in Settings." }
        guard let url = URL(string: "https://api.tavily.com/search") else { return "Error: Invalid Tavily URL" }
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.setValue("Bearer \(apiKey)", forHTTPHeaderField: "Authorization")
        request.timeoutInterval = llmAPITimeout
        let body: [String: Any] = ["query": query, "max_results": 5]
        do {
            request.httpBody = try JSONSerialization.data(withJSONObject: body)
            let (data, response) = try await URLSession.shared.data(for: request)
            guard let httpResponse = response as? HTTPURLResponse else { return "Error: Invalid response from Tavily" }
            guard httpResponse.statusCode == 200 else {
                let errorBody = String(data: data, encoding: .utf8) ?? "Unknown error"
                return "Error: Tavily API returned \(httpResponse.statusCode): \(errorBody)"
            }
            guard let json = try JSONSerialization.jsonObject(with: data) as? [String: Any],
                  let results = json["results"] as? [[String: Any]] else { return "Error: Failed to parse Tavily response" }
            if results.isEmpty { return "No search results found for '\(query)'" }
            var output = ""
            for (i, result) in results.enumerated() {
                let title = result["title"] as? String ?? "Untitled"
                let resultUrl = result["url"] as? String ?? ""
                let content = result["content"] as? String ?? ""
                output += "\(i + 1). \(title)\n   \(resultUrl)\n   \(content)\n\n"
            }
            return output.trimmingCharacters(in: .whitespacesAndNewlines)
        } catch { return "Error: \(error.localizedDescription)" }
    }
}
