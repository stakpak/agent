
@preconcurrency import Foundation
import AgentTools
import AgentMCP
import AgentD1F
import AgentSwift
import Cocoa

// MARK: - Task Execution — LLM Error

extension AgentViewModel {

    /// Result of handling a thrown error inside the LLM task loop.
    enum TaskLoopErrorOutcome {
        /// Caller should `continue` the outer while loop
        case continueLoop
        /// Caller should `break` out of the outer while loop.
        case breakLoop
        /// Caller should switch to the named fallback provider/model and `conti
        case fallbackRequested(provider: APIProvider, modelName: String, isVision: Bool)
    }

    /// / Which LLM service a given task-loop iteration is talking to.
    enum ActiveLLMService {
        case claude
        case openAICompatible
        case ollama
        case foundationModel
        case none
    }

    /// / Handles an error from LLM streaming: context-overflow pruning
    func handleTaskLoopError(
        _ error: Error,
        activeService: ActiveLLMService,
        providerDisplayName: String,
        messages: inout [[String: Any]],
        timeoutRetryCount: inout Int,
        maxTimeoutRetries: Int
    ) async -> TaskLoopErrorOutcome {
        if Task.isCancelled { return .breakLoop }
        let errMsg = error.localizedDescription

        // Context overflow — prune messages aggressively and retry
        let isOverflow = errMsg.contains("max_tokens") || errMsg.contains("context_length") || errMsg
            .contains("too many tokens") || errMsg.contains("prompt is too long")
        if isOverflow {
            appendLog("⚠️ Context overflow — pruning messages and retrying")
            flushLog()
            Self.pruneMessages(&messages, keepRecent: 4)
            Self.stripOldImages(&messages)
            return .continueLoop
        }

        // Stale connection — retry with fresh request
        let isStaleConnection = errMsg.contains("ECONNRESET") || errMsg.contains("EPIPE")
            || errMsg.contains("connection reset") || errMsg.contains("broken pipe")
        if isStaleConnection && timeoutRetryCount < maxTimeoutRetries {
            timeoutRetryCount += 1
            appendLog("🔌 Connection reset — retrying (\(timeoutRetryCount)/\(maxTimeoutRetries))")
            flushLog()
            try? await Task.sleep(for: .seconds(2))
            return .continueLoop
        }

        // Detect timeout errors
        let isNetworkTimeout = errMsg.lowercased().contains("timeout") || errMsg.lowercased().contains("timed out")

        // Determine error source for better logging
        var errorSource = "Unknown"
        switch activeService {
        case .claude:
            errorSource = "Claude API"
        case .openAICompatible:
            errorSource = "\(providerDisplayName) API"
        case .ollama:
            errorSource = "Ollama API"
        case .foundationModel:
            errorSource = "Apple Intelligence"
        case .none:
            errorSource = "Unknown"
        }

        // Handle timeout errors with retry logic
        if isNetworkTimeout {
            // Check if we've already retried this timeout
            if timeoutRetryCount < maxTimeoutRetries {
                timeoutRetryCount += 1

                // Special handling for Ollama timeouts - check server health
                if errorSource == "Ollama API" || errorSource == "Local Ollama" {
                    appendLog("🔍 Checking Ollama server health...")
                    flushLog()

                    // Run Ollama health check in background
                    let healthCheckResult = await Self.offMain {
                        let healthCheckTask = Process()
                        healthCheckTask.executableURL = URL(fileURLWithPath: "/usr/bin/curl")
                        healthCheckTask.arguments = ["-s", "-f", "http://localho
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
                        appendLog("⚠️ Ollama server not responding. Attempting to restart...")
                        flushLog()

                        // Restart Ollama via UserService XPC
                        _ = await userService
                            .execute(command: "pkill -f 'ollama serve' && sleep 2 && open /Applications/Ollama.app")
                        appendLog("🔄 Restart command executed")
                        flushLog()

                        // Wait longer for Ollama startup
                        let startupDelay = TimeInterval(min(10 * timeoutRetryCou
                        let retryMessage =
                            """
                            \(errorSource) timeout detected \
                            (attempt \(timeoutRetryCount)/\(maxTimeoutRetries)) — \
                            Ollama restart attempted, \
                            waiting \(Int(startupDelay)) seconds...
                            """
                        appendLog(retryMessage)
                        flushLog()
                        if agentReplyHandle != nil {
                            sendProgressUpdate(retryMessage)
                        }

                        try? await Task.sleep(for: .seconds(startupDelay))
                        if Task.isCancelled { return .breakLoop }
                        return .continueLoop
                    } else {
                        appendLog("✅ Ollama server is running but API timed out")
                        flushLog()
                    }
                }

                let retryDelay = TimeInterval(min(10 * timeoutRetryCount, 30)) /
                let retryMessage =
                    """
                    \(errorSource) timeout detected \
                    (attempt \(timeoutRetryCount)/\(maxTimeoutRetries)) — \
                    retrying in \(Int(retryDelay)) seconds...
                    """
                appendLog(retryMessage)
                flushLog()
                if agentReplyHandle != nil {
                    sendProgressUpdate(retryMessage)
                }

                // Log to task log for debugging

                try? await Task.sleep(for: .seconds(retryDelay))
                if Task.isCancelled { return .breakLoop }
                return .continueLoop
            } else {
                // Max retries reached - try final Ollama restart if applicable
                if (errorSource == "Ollama API" || errorSource == "Local Ollama") && timeoutRetryCount == maxTimeoutRetries {
                    appendLog("🔄 Max retries reached. Attempting final Ollama restart...")
                    flushLog()

                    // Restart Ollama via UserService XPC
                    _ = await userService
                        .execute(command: "pkill -f 'ollama serve' && sleep 3 && open /Applications/Ollama.app && sleep 10")
                    appendLog("Ollama restart attempted. Please check Ollama application status.")
                    flushLog()
                }

                // Retry budget exhausted on the same provider
                if let fallback = await tryFallbackChain(reason: "\(errorSource) timeout after \(maxTimeoutRetries) retries") {
                    timeoutRetryCount = 0
                    return fallback
                }

                let timeoutMessage =
                    """
                    \(errorSource) timeout after \(maxTimeoutRetries) \
                    retries. Please check your network connection \
                    or try a different LLM provider.
                    """
                appendLog(timeoutMessage)
                flushLog()
                if agentReplyHandle != nil {
                    sendProgressUpdate(timeoutMessage)
                }
                return .breakLoop
            }
        } else if let agentErr = error as? AgentError, agentErr.isRateLimited, timeoutRetryCount < maxTimeoutRetries {
            // 429 rate-limit / "service overloaded"
            timeoutRetryCount += 1
            let apiBody: String
            if case .apiError(_, let msg) = error as? AgentError { apiBody = msg } else { apiBody = errMsg }
            // Record every 429 with the fallback chain
            if let fallback = await tryFallbackChain(reason: "429 (\(timeoutRetryCount))") {
                timeoutRetryCount = 0
                return fallback
            }
            let retryDelay: TimeInterval = 10
            appendLog(
                """
                ⏳ \(errorSource) 429: \(apiBody.prefix(200))
                Retrying in \(Int(retryDelay))s (attempt \(timeoutRetryCount)/\(maxTimeoutRetries))
                """
            )
            flushLog()
            if agentReplyHandle != nil {
                sendProgressUpdate("\(errorSource) rate limited — waiting \(Int(retryDelay))s")
            }
            try? await Task.sleep(for: .seconds(retryDelay))
            if Task.isCancelled { return .breakLoop }
            return .continueLoop
        } else if let agentErr = error as? AgentError, agentErr.isRecoverable, timeoutRetryCount < maxTimeoutRetries {
            // Server/network error — retry every 10 seconds
            timeoutRetryCount += 1
            let retryDelay: TimeInterval = 10
            appendLog(
                """
                \(errorSource) recoverable error \
                (attempt \(timeoutRetryCount)/\(maxTimeoutRetries)) — \
                retrying in \(Int(retryDelay))s...
                \(errMsg)
                """
            )
            flushLog()
            try? await Task.sleep(for: .seconds(retryDelay))
            if Task.isCancelled { return .breakLoop }
            return .continueLoop
        } else if errMsg.lowercased().contains("network")
            || errMsg.lowercased().contains("connection")
            || errMsg.lowercased().contains("internet")
            || (error as? URLError)?.code == .networkConnectionLost
            || (error as? URLError)?.code == .notConnectedToInternet
        {
            timeoutRetryCount += 1
            if timeoutRetryCount <= maxTimeoutRetries {
                let delay = networkRetryDelay
                appendLog(
                    """
                    🌐 Network connection lost — retrying in \(delay)s \
                    (attempt \(timeoutRetryCount)/\(maxTimeoutRetries))...
                    """
                )
                flushLog()
                try? await Task.sleep(for: .seconds(Double(delay)))
                if Task.isCancelled { return .breakLoop }
                return .continueLoop
            } else {
                // Network retry budget exhausted — try fallback chain before gi
                if let fallback = await tryFallbackChain(reason: "network connection lost after \(maxTimeoutRetries) retries") {
                    timeoutRetryCount = 0
                    return fallback
                }
                appendLog("🌐 Network connection lost after \(maxTimeoutRetries) retries.")
                flushLog()
                return .breakLoop
            }
        } else {
            // Try fallback chain before giving up
            if let fallback = await tryFallbackChain(reason: "\(errorSource) error: \(errMsg)") {
                timeoutRetryCount = 0
                return fallback
            }

            // Non-recoverable error — no fallback available
            appendLog("\(errorSource) Error: \(errMsg)")
            flushLog()

            // Apple Intelligence error explanation
            let mediator = AppleIntelligenceMediator.shared
            if mediator.isEnabled && mediator.showAnnotationsToUser {
                if let errorAnnotation = await mediator.explainError(toolName: "LLM request", error: errMsg) {
                    appendLog(errorAnnotation.formatted)
                    flushLog()
                }
            }
            return .breakLoop
        }
    }

    /// / Shared helper used by every error branch in handleTaskLoopError.
    private func tryFallbackChain(reason: String) async -> TaskLoopErrorOutcome? {
        guard let fallback = FallbackChainService.shared.recordFailure() else { return nil }
        appendLog("🔄 Fallback triggered (\(reason))")
        appendLog("🔄 Switching to fallback: \(fallback.displayName)")
        flushLog()
        guard let fbProvider = APIProvider(rawValue: fallback.provider) else { return .continueLoop }
        var newIsVision = Self.isVisionModel(fallback.model)
        if forceVision { newIsVision = true }
        appendLog("✅ Now using \(fbProvider.displayName) / \(fallback.model)")
        flushLog()
        return .fallbackRequested(provider: fbProvider, modelName: fallback.model, isVision: newIsVision)
    }
}
