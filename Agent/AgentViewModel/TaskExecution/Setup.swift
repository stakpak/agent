
@preconcurrency import Foundation
import AgentTools
import AgentMCP
import AgentD1F
import AgentSwift
import Cocoa

// MARK: - Task Execution — Provider / Model / Service Setup

extension AgentViewModel {

    /// / Bundled LLM services built for a single task iteration. Exactly one / of these four services is non-nil after
    /// `buildLLMServices` returns / (matching the original inline closure's invariant).
    struct LLMServiceBundle {
        var claude: ClaudeService?
        var openAICompatible: OpenAICompatibleService?
        var ollama: OllamaService?
        var foundationModel: FoundationModelService?
    }

    /// / Resolves the initial `(provider, modelName, isVision)` triple for a new task / from the currently-selected
    /// provider and per-provider model/vision settings. / Matches the original inline switch exactly.
    func resolveInitialProviderConfig() -> (provider: APIProvider, modelName: String, isVision: Bool) {
        let provider = selectedProvider
        let modelName: String
        var isVision: Bool
        switch provider {
        case .claude:
            modelName = selectedModel
            isVision = true // Claude Sonnet/Opus/Haiku all support vision
        case .openAI:
            modelName = openAIModel
            isVision = true // GPT-4o, GPT-4 Turbo support vision
        case .deepSeek:
            modelName = deepSeekModel
            isVision = Self.isVisionModel(deepSeekModel)
        case .huggingFace:
            modelName = huggingFaceModel
            isVision = Self.isVisionModel(huggingFaceModel)
        case .ollama:
            modelName = ollamaModel
            isVision = selectedOllamaSupportsVision || Self.isVisionModel(ollamaModel)
        case .localOllama:
            modelName = localOllamaModel
            isVision = selectedLocalOllamaSupportsVision || Self.isVisionModel(localOllamaModel)
        case .vLLM:
            modelName = vLLMModel
            isVision = Self.isVisionModel(vLLMModel)
        case .lmStudio:
            modelName = lmStudioModel
            isVision = Self.isVisionModel(lmStudioModel)
        case .zAI:
            isVision = zAIModel.hasSuffix(":v")
            modelName = zAIModel.replacingOccurrences(of: ":v", with: "")
        case .bigModel:
            isVision = bigModelModel.hasSuffix(":v")
            modelName = bigModelModel.replacingOccurrences(of: ":v", with: "")
        case .qwen:
            modelName = qwenModel
            isVision = Self.isVisionModel(qwenModel)
        case .gemini:
            modelName = geminiModel
            isVision = true // Gemini supports vision
        case .grok:
            modelName = grokModel
            isVision = Self.isVisionModel(grokModel)
        case .mistral:
            modelName = mistralModel
            isVision = true
        case .codestral:
            modelName = codestralModel
            isVision = false
        case .vibe:
            modelName = vibeModel
            isVision = false
        case .foundationModel:
            modelName = "Apple Intelligence"
            isVision = false // Apple Intelligence doesn't support image input
        }
        if forceVision { isVision = true }
        return (provider, modelName, isVision)
    }

    /// / Builds the LLM service bundle for a given provider/model/vision combo. / Called at task start and again
    /// whenever the fallback chain swaps providers / mid-task. Mirrors the original inline `buildLLMServices` closure exactly.
    func buildLLMServiceBundle(
        provider: APIProvider,
        modelName: String,
        isVision: Bool,
        historyContext: String,
        maxTokens mt: Int
    ) -> LLMServiceBundle {
        var claude: ClaudeService?
        var openAICompatible: OpenAICompatibleService?
        var ollama: OllamaService?
        var foundationModelService: FoundationModelService?

        if provider == .claude {
            claude = ClaudeService(
                apiKey: apiKey, model: modelName,
                historyContext: historyContext,
                projectFolder: projectFolder, maxTokens: mt
            )
        } else if provider == .lmStudio && lmStudioProtocol == .anthropic {
            claude = ClaudeService(
                apiKey: lmStudioAPIKey, model: lmStudioModel,
                historyContext: historyContext,
                projectFolder: projectFolder,
                baseURL: lmStudioEndpoint, maxTokens: mt
            )
        } else {
            claude = nil
        }
        // OpenAI-compatible service — URLs from LLMRegistry (single source of truth)
        switch provider {
        case .claude, .ollama, .localOllama, .foundationModel:
            openAICompatible = nil
        case .lmStudio where lmStudioProtocol == .anthropic:
            openAICompatible = nil
        case .lmStudio:
            let key = lmStudioProtocol == .lmStudio ? "input" : "messages"
            openAICompatible = OpenAICompatibleService(
                apiKey: apiKeyForProvider(provider), model: modelName,
                baseURL: lmStudioEndpoint, historyContext: historyContext,
                projectFolder: projectFolder, provider: provider,
                messagesKey: key, maxTokens: mt
            )
        case .vLLM:
            openAICompatible = OpenAICompatibleService(
                apiKey: apiKeyForProvider(provider), model: modelName,
                baseURL: vLLMEndpoint, historyContext: historyContext,
                projectFolder: projectFolder, provider: provider,
                maxTokens: mt
            )
        default:
            let url = chatURLForProvider(provider)
            openAICompatible = url.isEmpty ? nil : OpenAICompatibleService(
                apiKey: apiKeyForProvider(provider), model: modelName,
                baseURL: url, supportsVision: isVision,
                historyContext: historyContext, projectFolder: projectFolder,
                provider: provider, maxTokens: mt
            )
        }
        switch provider {
        case .ollama:
            ollama = OllamaService(
                apiKey: ollamaAPIKey, model: modelName,
                endpoint: ollamaEndpoint, supportsVision: isVision,
                historyContext: historyContext, projectFolder: projectFolder,
                provider: .ollama
            )
        case .localOllama:
            ollama = OllamaService(
                apiKey: "", model: modelName, endpoint: localOllamaEndpoint,
                supportsVision: isVision, historyContext: historyContext,
                projectFolder: projectFolder, provider: .localOllama,
                contextSize: localOllamaContextSize
            )
        default:
            ollama = nil
        }
        foundationModelService = provider == .foundationModel
            ? FoundationModelService(historyContext: historyContext, projectFolder: projectFolder) : nil

        // Set temperature per provider
        claude?.temperature = temperatureForProvider(provider == .claude ? .claude : provider)
        ollama?.temperature = temperatureForProvider(provider)
        openAICompatible?.temperature = temperatureForProvider(provider)

        return LLMServiceBundle(
            claude: claude,
            openAICompatible: openAICompatible,
            ollama: ollama,
            foundationModel: foundationModelService
        )
    }
}
