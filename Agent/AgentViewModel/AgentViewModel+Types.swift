@preconcurrency import Foundation
import AgentTools
import AgentLLM
import AppKit
import SwiftUI

/// Per-tab LLM configuration for multi-main-tab support
struct LLMConfig: Codable {
    var provider: APIProvider
    var model: String
    var displayName: String
}

enum LMStudioProtocol: String, CaseIterable, Codable {
    case openAI = "openAI"
    case anthropic = "anthropic"
    case lmStudio = "lmStudio"

    var displayName: String {
        switch self {
        case .openAI: "OpenAI Compatible"
        case .anthropic: "Anthropic Compatible"
        case .lmStudio: "LM Studio Native"
        }
    }

    var defaultEndpoint: String {
        switch self {
        case .openAI: "http://localhost:1234/v1/chat/completions"
        case .anthropic: "http://localhost:1234/v1/messages"
        case .lmStudio: "http://localhost:1234/api/v1/chat"
        }
    }
}

enum PromptStyle: String, CaseIterable, Codable {
    case full
    case compact

    var displayName: String {
        switch self {
        case .full: "Full"
        case .compact: "Compact"
        }
    }
}

extension AgentViewModel {
    // MARK: - Tool Steps (structured tool call tracking)

    /// A single tool invocation step for structured display
    struct ToolStep: Identifiable {
        let id = UUID()
        let name: String
        let detail: String
        let startTime: Date
        var duration: TimeInterval?
        var status: Status = .running

        enum Status {
            case running, success, error
        }
    }

    // MARK: - Model Info Types

    struct OpenAIModelInfo: Identifiable {
        let id: String
        let name: String
    }

    struct OllamaModelInfo: Identifiable {
        let id: String // same as name
        let name: String
        let supportsVision: Bool
    }

    struct ClaudeModelInfo: Identifiable, Codable {
        let id: String
        let name: String
        let displayName: String
        let createdAt: String?
        let description: String?

        var formattedDisplayName: String {
            if let created = createdAt {
                let dateStr = String(created.prefix(10))
                return "\(displayName) (\(dateStr))"
            }
            return displayName
        }
    }

    // MARK: - Terminal Speed

    enum TerminalSpeed: Int, CaseIterable {
        case current = 22
        case fast = 15
        case faster = 10
        case blazing = 5
        case ludicrous = 1

        var label: String {
            switch self {
            case .current: "Normal"
            case .fast: "Fast"
            case .faster: "Faster"
            case .blazing: "Blazing"
            case .ludicrous: "Ludicrous"
            }
        }
    }

    // MARK: - Static option arrays

    static let iterationOptions = [25, 50, 100, 200, 400, 800, 1600]
    static let retryOptions = [1, 2, 3, 5, 10, 15, 20]
    static let outputLineOptions = [10, 50, 75, 100, 150, 200, 250, 500, 750, 1000, 1500]
    static let readPreviewOptions = [3, 10, 50, 100, 250, 500, 750, 1000]
    static let maxLogSize = 60_000

    // MARK: - Static helpers

    /// Detect vision-capable models by name patterns
    /// Auto-detect vision-capable models by name keywords.
    /// Sources: ollama.com/search?c=vision, OpenAI docs, Anthropic docs
    nonisolated static func isVisionModel(_ model: String) -> Bool {
        let lower = model.lowercased()
        let visionKeywords = [
            // Ollama vision models (from ollama.com/search?c=vision)
            "llava", "llava-llama3", "bakllava", "minicpm-v",
            "gemma3", "gemma4", "gemma-3", "gemma-4",
            "qwen-vl", "qwen2.5vl", "qwen2.5-vl", "qwen3-vl", "qwen3.5",
            "llama3.2-vision", "llama-3.2-vision", "llama4",
            "mistral-small3.1", "mistral-small3.2", "mistral-large-3",
            "kimi-k2.5", "gemini-3-flash", "glm-ocr", "deepseek-ocr",
            "ministral-3", "devstral-small-2",
            // General vision keywords
            "vision", "-vl", "cogvlm", "internvl", "pixtral", "molmo",
            "phi-3-vision", "phi-3.5-vision", "phi-4", "idefics", "fuyu",
            // Cloud API vision models
            "gpt-4o", "gpt-4-turbo", "gpt-4-vision", "claude",
            "glm-4v", "glm-4.5v", "glm-4.6v", "glm-5v", "deepseek-vl",
        ]
        return visionKeywords.contains { lower.contains($0) }
    }

    // MARK: - Off-Main-Thread Helper

    /// Run synchronous work off the main thread to avoid blocking the UI.
    static func offMain<T: Sendable>(_ work: @Sendable @escaping () -> T) async -> T {
        await Task.detached { work() }.value
    }
}
