import Foundation
import AppKit

extension WebAutomationService {
    // MARK: - Selenium Helpers Note: Selenium operations are handled via Selenium AgentScript through tool handlers
    // These methods are placeholders - actual Selenium calls go through run_agent_script

    func seleniumClick(selector: String) async throws -> String {
        throw WebAutomationError.seleniumError("Selenium operations should be called via selenium_click tool")
    }

    func seleniumType(selector: String, text: String) async throws -> String {
        throw WebAutomationError.seleniumError("Selenium operations should be called via selenium_type tool")
    }

    func seleniumExecute(script: String) async throws -> Any? {
        throw WebAutomationError.seleniumError("Selenium operations should be called via selenium_execute tool")
    }

    // MARK: - Fuzzy Matching

    /// Calculate fuzzy match score between two strings
    func fuzzyMatch(text: String, pattern: String) -> Double {
        let textLower = text.lowercased()
        let patternLower = pattern.lowercased()

        // Exact match
        if textLower == patternLower { return 1.0 }

        // Contains
        if textLower.contains(patternLower) { return 0.9 }

        // Prefix match
        if textLower.hasPrefix(patternLower) { return 0.85 }

        // Suffix match
        if textLower.hasSuffix(patternLower) { return 0.8 }

        // Levenshtein distance ratio
        let distance = levenshtein(textLower, patternLower)
        let maxLength = max(textLower.count, patternLower.count)
        let ratio = 1.0 - Double(distance) / Double(maxLength)

        return max(0, ratio)
    }

    /// Levenshtein distance between two strings
    func levenshtein(_ s1: String, _ s2: String) -> Int {
        let a = Array(s1)
        let b = Array(s2)
        let m = a.count
        let n = b.count

        if m == 0 { return n }
        if n == 0 { return m }

        var dp = Array(repeating: Array(repeating: 0, count: n + 1), count: m + 1)

        for i in 0...m { dp[i][0] = i }
        for j in 0...n { dp[0][j] = j }

        for i in 1...m {
            for j in 1...n {
                if a[i-1] == b[j-1] {
                    dp[i][j] = dp[i-1][j-1]
                } else {
                    dp[i][j] = 1 + min(dp[i-1][j], dp[i][j-1], dp[i-1][j-1])
                }
            }
        }

        return dp[m][n]
    }

    // MARK: - Selector Parsing

    /// Parse a selector string into role/title/value hints
    func parseSelector(_ selector: String) -> (role: String?, title: String?, value: String?) {
        // Handle CSS-style selectors
        if selector.hasPrefix("#") {
            // ID selector
            return (role: nil, title: nil, value: String(selector.dropFirst()))
        }

        if selector.hasPrefix(".") {
            // Class selector - use as title hint
            return (role: nil, title: String(selector.dropFirst()), value: nil)
        }

        // Handle attribute selectors like [title="Submit"]
        if selector.hasPrefix("[") && selector.hasSuffix("]") {
            let inner = String(selector.dropFirst().dropLast())
            if let eqRange = inner.range(of: "=") {
                let attr = String(inner[..<eqRange.lowerBound])
                let value = String(inner[inner.index(after: eqRange.lowerBound)...])
                    .trimmingCharacters(in: CharacterSet(charactersIn: "\"'"))

                if attr.lowercased() == "title" || attr.lowercased() == "aria-label" {
                    return (role: nil, title: value, value: nil)
                }
                if attr.lowercased() == "role" {
                    return (role: "AX\(value.capitalized)", title: nil, value: nil)
                }
                if attr.lowercased() == "value" || attr.lowercased() == "placeholder" {
                    return (role: nil, title: nil, value: value)
                }
            }
        }

        // Handle accessibility role selectors like AXButton, AXTextField
        if selector.hasPrefix("AX") {
            return (role: selector, title: nil, value: nil)
        }

        // Handle text content selectors like text:Submit
        if selector.hasPrefix("text:") {
            return (role: nil, title: nil, value: String(selector.dropFirst(5)))
        }

        // Default: treat as title
        return (role: nil, title: selector, value: nil)
    }

    // MARK: - Browser Detection

    /// Detect the currently active browser
    func detectActiveBrowser() -> String? {
        let apps = NSWorkspace.shared.runningApplications
        let browsers = ["com.apple.Safari", "com.google.Chrome", "org.mozilla.firefox", "com.microsoft.edgemac"]

        // Find frontmost browser
        for app in apps where app.activationPolicy == .regular {
            if let bundleId = app.bundleIdentifier, browsers.contains(bundleId) {
                return bundleId
            }
        }

        return nil
    }

    // MARK: - Cache Management

    func cacheElement(key: String, element: [String: Any], source: ElementSource) {
        cacheLock.lock()
        defer { cacheLock.unlock() }

        elementCache[key] = CachedElement(
            element: element,
            timestamp: Date(),
            role: element["role"] as? String,
            title: element["title"] as? String,
            value: element["value"] as? String,
            bounds: parseBounds(element["bounds"]),
            source: source
        )

        // Clean expired entries
        cleanExpiredCache()
    }

    func getCachedElement(key: String) -> [String: Any]? {
        cacheLock.lock()
        defer { cacheLock.unlock() }

        guard let cached = elementCache[key] else { return nil }

        let elapsed = Date().timeIntervalSince(cached.timestamp)
        if elapsed > cacheTTL {
            elementCache.removeValue(forKey: key)
            return nil
        }

        return cached.element
    }

    func cleanExpiredCache() {
        let now = Date()
        elementCache = elementCache.filter { now.timeIntervalSince($0.value.timestamp) <= cacheTTL }
    }

    func parseBounds(_ value: Any?) -> CGRect {
        guard let dict = value as? [String: Double],
              let x = dict["x"],
              let y = dict["y"],
              let w = dict["width"],
              let h = dict["height"] else
        {
            return .zero
        }
        return CGRect(x: x, y: y, width: w, height: h)
    }

}
