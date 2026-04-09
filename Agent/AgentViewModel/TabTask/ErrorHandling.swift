
@preconcurrency import Foundation
import AgentTools
import AgentLLM


// MARK: - Tab Task Error Handling

extension AgentViewModel {

    /// / Action the caller should take in response to a thrown error during the / tab task LLM loop. Mirrors the
    /// `continue`/`break`/fallback behavior of / the legacy monolithic executeTabTask catch block.
    enum TabTaskErrorAction {
        /// Retry the current iteration (equivalent to `continue`)
        case retry
        /// Give up and exit the LLM loop (equivalent to `break`)
        case giveUp
        /// / Fallback chain recorded a failure; caller should reset / timeoutRetryCount and continue. If a
        /// provider/model pair is / attached, the caller should also swap providers and rebuild / services, then log "Now using ...". Mirrors the legacy behavior / where a fallback-record reset+continue even when the provider / raw value failed to decode.
        case fallback(APIProvider?, String?)
    }

    /// / Handle an error thrown from the streaming LLM call. Applies the same / timeout/rate-limit/network/fallback
    /// logic the legacy executeTabTask / catch block used.
    func handleTabTaskError(
        tab: ScriptTab,
        error: Error,
        hasClaude: Bool,
        hasOpenAICompatible: Bool,
        hasOllama: Bool,
        hasFoundationModel: Bool,
        provider: APIProvider,
        timeoutRetryCount: inout Int
    ) async -> TabTaskErrorAction {
        if Task.isCancelled { return .giveUp }
        let mediator = AppleIntelligenceMediator.shared
        let errMsg = error.localizedDescription

        // Detect timeout errors
        let isNetworkTimeout = errMsg.lowercased().contains("timeout") || errMsg.lowercased().contains("timed out")

        // Determine error source for better logging
        var errorSource = "Unknown"
        if hasClaude {
            errorSource = "Claude API"
        } else if hasOpenAICompatible {
            errorSource = "\(provider.displayName) API"
        } else if hasOllama {
            errorSource = "Ollama API"
        } else if hasFoundationModel {
            errorSource = "Apple Intelligence"
        }

        let maxTimeoutRetries = maxRetries

        // Handle timeout errors with retry logic
        if isNetworkTimeout {
            // Check if we've already retried this timeout
            if timeoutRetryCount < maxTimeoutRetries {
                timeoutRetryCount += 1

                // Special handling for Ollama timeouts - check server health
                if errorSource == "Ollama API" || errorSource == "Local Ollama" {
                    tab.appendLog("🔍 Checking Ollama server health...")

                    // Run Ollama health check in background
                    let healthCheckResult = await Self.offMain {
                        let healthCheckTask = Process()
                        healthCheckTask.executableURL = URL(fileURLWithPath: "/usr/bin/curl")
                        healthCheckTask.arguments = ["-s", "-f", "http://localhost:11434/api/tags", "--max-time", "5"]
                        healthCheckTask.currentDirectoryURL = URL(fileURLWithPath: NSHomeDirectory())

                        let pipe = Pipe()
                        healthCheckTask.standardOutput = pipe
                        healthCheckTask.standardError = pipe

                        do {
                            try healthCheckTask.run()
                            healthCheckTask.waitUntilExit()
                            return healthCheckTask.terminationStatus
                        } catch {
                            return -1
                        }
                    }

                    if healthCheckResult != 0 {
                        tab.appendLog("⚠️ Ollama server not responding. Attempting to restart...")

                        // Restart Ollama via UserService XPC
                        _ = await userService
                            .execute(command: "pkill -f 'ollama serve' && sleep 2 && open /Applications/Ollama.app")
                        tab.appendLog("🔄 Restart command executed")

                        // Wait longer for Ollama startup
                        let startupDelay = TimeInterval(min(10 * timeoutRetryCount, 30)) // Exponential backoff up to 30 seconds
                        let retryMessage =
                            """
                            \(errorSource) timeout detected \
                            (attempt \(timeoutRetryCount)/\(maxTimeoutRetries)) — \
                            Ollama restart attempted, \
                            waiting \(Int(startupDelay)) seconds...
                            """
                        tab.appendLog(retryMessage)

                        try? await Task.sleep(for: .seconds(startupDelay))
                        if Task.isCancelled { return .giveUp }
                        return .retry
                    } else {
                        tab.appendLog("✅ Ollama server is running but API timed out")
                    }
                }

                let retryDelay = TimeInterval(min(10 * timeoutRetryCount, 30)) // Exponential backoff up to 30 seconds
                let retryMessage =
                    """
                    \(errorSource) timeout detected \
                    (attempt \(timeoutRetryCount)/\(maxTimeoutRetries)) — \
                    retrying in \(Int(retryDelay)) seconds...
                    """
                tab.appendLog(retryMessage)

                // Log to task log for debugging

                try? await Task.sleep(for: .seconds(retryDelay))
                if Task.isCancelled { return .giveUp }
                return .retry
            } else {
                // Max retries reached - try final Ollama restart if applicable
                if (errorSource == "Ollama API" || errorSource == "Local Ollama") && timeoutRetryCount == maxTimeoutRetries {
                    tab.appendLog("🔄 Max retries reached. Attempting final Ollama restart...")

                    // Restart Ollama via UserService XPC
                    _ = await userService
                        .execute(command: "pkill -f 'ollama serve' && sleep 3 && open /Applications/Ollama.app && sleep 10")
                    tab.appendLog("🔄 Ollama restart attempted. Check Ollama status.")
                }

                let timeoutMessage =
                    """
                    \(errorSource) timeout after \(maxTimeoutRetries) \
                    retries. Please check your network connection \
                    or try a different LLM provider.
                    """
                tab.appendLog(timeoutMessage)
                return .giveUp
            }
        } else if let agentErr = error as? AgentError, agentErr.isRateLimited, timeoutRetryCount < maxTimeoutRetries {
            // 429 rate-limit / "service overloaded" — exponential backoff up to 60s, matching the main task loop in
            // AgentViewModel+TaskExecution.swift. Z.ai returns this with body code 1305 ("service may be temporarily overloaded"); the previous one-shot 30s retry gave up too quickly when the service stayed congested.
            timeoutRetryCount += 1
            let retryDelay = TimeInterval(min(15 * timeoutRetryCount, 60))
            tab.appendLog(
                """
                ⏳ \(errorSource) rate limited (429) — retrying in \(Int(retryDelay))s \
                (attempt \(timeoutRetryCount)/\(maxTimeoutRetries))
                """
            )
            tab.flush()
            try? await Task.sleep(for: .seconds(retryDelay))
            if Task.isCancelled { return .giveUp }
            return .retry
        } else if let agentErr = error as? AgentError, agentErr.isRecoverable, timeoutRetryCount < maxTimeoutRetries {
            // Server/network error — retry every 10 seconds
            timeoutRetryCount += 1
            let retryDelay: TimeInterval = 10
            tab
                .appendLog(
                    """
                    \(errorSource) recoverable error \
                    (attempt \(timeoutRetryCount)/\(maxTimeoutRetries)) — \
                    retrying in \(Int(retryDelay))s...
                    """
                )
            tab.flush()
            try? await Task.sleep(for: .seconds(retryDelay))
            if Task.isCancelled { return .giveUp }
            return .retry
        } else if errMsg.lowercased().contains("network")
            || errMsg.lowercased().contains("connection")
            || errMsg.lowercased().contains("internet")
            || (error as? URLError)?.code == .networkConnectionLost
            || (error as? URLError)?.code == .notConnectedToInternet
        {
            // Network lost — retry in 60 seconds
            timeoutRetryCount += 1
            if timeoutRetryCount <= maxTimeoutRetries {
                let delay = networkRetryDelay
                tab
                    .appendLog(
                        """
                        🌐 Network connection lost — \
                        retrying in \(delay)s \
                        (attempt \(timeoutRetryCount)/\(maxTimeoutRetries))...
                        """
                    )
                tab.flush()
                try? await Task.sleep(for: .seconds(Double(delay)))
                if Task.isCancelled { return .giveUp }
                return .retry
            } else {
                tab.appendLog("🌐 Network connection lost after \(maxTimeoutRetries) retries. Check your connection.")
                tab.flush()
                return .giveUp
            }
        } else {
            // Try fallback chain before giving up
            if let fallback = FallbackChainService.shared.recordFailure() {
                tab.appendLog("🔄 Switching to fallback: \(fallback.displayName)")
                tab.flush()
                if let fbProvider = APIProvider(rawValue: fallback.provider) {
                    // Caller is responsible for rebuilding services, resetting timeoutRetryCount, and logging "Now
                    // using ..." AFTER the rebuild — matches the legacy executeTabTask order.
                    return .fallback(fbProvider, fallback.model)
                }
                // Fallback was recorded but the provider raw value failed to decode. Legacy behavior still reset the
                // counter and did a `continue` in this case — preserve that.
                return .fallback(nil, nil)
            }
            // Non-recoverable error — don't retry (400 bad request, auth errors, etc.)
            tab.appendLog("\(errorSource) Error: \(errMsg)")
            tab.flush()

            if mediator.isEnabled && mediator.showAnnotationsToUser {
                if let errorAnnotation = await mediator.explainError(toolName: "LLM request", error: errMsg) {
                    tab.appendLog(errorAnnotation.formatted)
                    tab.flush()
                }
            }
            return .giveUp
        }
    }
}
