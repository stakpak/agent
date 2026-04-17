@preconcurrency import Foundation

extension AgentViewModel {
    // MARK: - OpenAI

    nonisolated static let defaultOpenAIModels: [OpenAIModelInfo] = [
        OpenAIModelInfo(id: "gpt-4.1-nano", name: "GPT-4.1 Nano"),
        OpenAIModelInfo(id: "gpt-4.1-mini", name: "GPT-4.1 Mini"),
        OpenAIModelInfo(id: "gpt-4.1", name: "GPT-4.1"),
        OpenAIModelInfo(id: "gpt-4o-mini", name: "GPT-4o Mini"),
        OpenAIModelInfo(id: "gpt-4o", name: "GPT-4o"),
        OpenAIModelInfo(id: "o4-mini", name: "o4-mini"),
        OpenAIModelInfo(id: "o3-mini", name: "o3-mini"),
        OpenAIModelInfo(id: "o3", name: "o3"),
    ]

    // MARK: - DeepSeek

    nonisolated static let defaultDeepSeekModels: [OpenAIModelInfo] = [
        OpenAIModelInfo(id: "deepseek-chat", name: "DeepSeek Chat (V3)"),
        OpenAIModelInfo(id: "deepseek-reasoner", name: "DeepSeek Reasoner (R1)"),
    ]

    // MARK: - Z.ai (ZhipuAI GLM)

    nonisolated static let defaultZAIModels: [OpenAIModelInfo] = [
        // Coding models (use /api/coding/paas/ endpoint)
        OpenAIModelInfo(id: "glm-5.1", name: "GLM-5.1"),
        OpenAIModelInfo(id: "glm-5", name: "GLM-5"),
        OpenAIModelInfo(id: "glm-5-turbo", name: "GLM-5 Turbo"),
        OpenAIModelInfo(id: "glm-4.7", name: "GLM-4.7"),
        OpenAIModelInfo(id: "glm-4.7-flash", name: "GLM-4.7 Flash"),
        OpenAIModelInfo(id: "glm-4.6", name: "GLM-4.6"),
        OpenAIModelInfo(id: "glm-4.5", name: "GLM-4.5"),
        OpenAIModelInfo(id: "glm-4.5-air", name: "GLM-4.5 Air"),
        OpenAIModelInfo(id: "glm-4.5-flash", name: "GLM-4.5 Flash"),
        OpenAIModelInfo(id: "glm-4-32b-0414-128k", name: "GLM-4-32B-128K"),
        // Non-coding / general models (use /api/paas/ endpoint)
        // Tagged with :v suffix — stripped before sending to API
        OpenAIModelInfo(id: "glm-5.1:v", name: "GLM-5.1"),
        OpenAIModelInfo(id: "glm-5:v", name: "GLM-5"),
        OpenAIModelInfo(id: "glm-5-turbo:v", name: "GLM-5 Turbo"),
        OpenAIModelInfo(id: "glm-4.7:v", name: "GLM-4.7"),
        OpenAIModelInfo(id: "glm-4.7-flash:v", name: "GLM-4.7 Flash"),
        OpenAIModelInfo(id: "glm-4.6:v", name: "GLM-4.6"),
        OpenAIModelInfo(id: "glm-4.5:v", name: "GLM-4.5"),
        OpenAIModelInfo(id: "glm-4.5-air:v", name: "GLM-4.5 Air"),
        OpenAIModelInfo(id: "glm-4.5-flash:v", name: "GLM-4.5 Flash"),
        // Vision models (use /api/paas/ endpoint, vision-capable)
        OpenAIModelInfo(id: "glm-5v-turbo:v", name: "GLM-5V-Turbo (Vision)"),
        OpenAIModelInfo(id: "glm-4.6v:v", name: "GLM-4.6V (Vision)"),
        OpenAIModelInfo(id: "glm-4.5v:v", name: "GLM-4.5V (Vision)"),
        OpenAIModelInfo(id: "glm-ocr:v", name: "GLM-OCR"),
        // Image/Video/Voice models
        OpenAIModelInfo(id: "glm-image:v", name: "GLM-Image"),
        OpenAIModelInfo(id: "cogvideox-3:v", name: "CogVideoX-3"),
        OpenAIModelInfo(id: "glm-asr-2512:v", name: "GLM-ASR-2512 (Voice)"),
    ]

    // MARK: - Qwen

    nonisolated static let defaultQwenModels: [OpenAIModelInfo] = [
        OpenAIModelInfo(id: "qwen-plus", name: "Qwen Plus"),
        OpenAIModelInfo(id: "qwen-max", name: "Qwen Max"),
        OpenAIModelInfo(id: "qwen-turbo", name: "Qwen Turbo"),
        OpenAIModelInfo(id: "qwen-long", name: "Qwen Long"),
        OpenAIModelInfo(id: "qwen-vl-plus", name: "Qwen VL Plus"),
        OpenAIModelInfo(id: "qwen-vl-max", name: "Qwen VL Max"),
        OpenAIModelInfo(id: "qwen-coder-plus", name: "Qwen Coder Plus"),
    ]

    // MARK: - Gemini

    nonisolated static let defaultGeminiModels: [OpenAIModelInfo] = [
        OpenAIModelInfo(id: "gemini-2.5-pro-preview-05-06", name: "Gemini 2.5 Pro"),
        OpenAIModelInfo(id: "gemini-2.5-flash-preview-05-20", name: "Gemini 2.5 Flash"),
        OpenAIModelInfo(id: "gemini-2.5-flash", name: "Gemini 2.5 Flash (Stable)"),
        OpenAIModelInfo(id: "gemini-2.0-flash", name: "Gemini 2.0 Flash"),
    ]

    // MARK: - Grok

    nonisolated static let defaultGrokModels: [OpenAIModelInfo] = [
        OpenAIModelInfo(id: "grok-3", name: "Grok 3"),
        OpenAIModelInfo(id: "grok-3-fast", name: "Grok 3 Fast"),
        OpenAIModelInfo(id: "grok-3-mini", name: "Grok 3 Mini"),
        OpenAIModelInfo(id: "grok-3-mini-fast", name: "Grok 3 Mini Fast"),
    ]

    // MARK: - Mistral

    nonisolated static let defaultMistralModels: [OpenAIModelInfo] = [
        OpenAIModelInfo(id: "mistral-large-latest", name: "Mistral Large"),
        OpenAIModelInfo(id: "mistral-small-latest", name: "Mistral Small"),
        OpenAIModelInfo(id: "codestral-latest", name: "Codestral"),
        OpenAIModelInfo(id: "mistral-medium-latest", name: "Mistral Medium"),
    ]

    // MARK: - Codestral

    nonisolated static let defaultCodestralModels: [OpenAIModelInfo] = [
        OpenAIModelInfo(id: "codestral-latest", name: "Codestral Latest"),
        OpenAIModelInfo(id: "codestral-2508", name: "Codestral 25.08"),
    ]

    // MARK: - Vibe (Devstral)

    nonisolated static let defaultVibeModels: [OpenAIModelInfo] = [
        OpenAIModelInfo(id: "devstral-latest", name: "Devstral Latest"),
        OpenAIModelInfo(id: "devstral-medium-latest", name: "Devstral Medium Latest"),
    ]

    // MARK: - Hugging Face

    nonisolated static let defaultHuggingFaceModels: [OpenAIModelInfo] = [
        OpenAIModelInfo(id: "deepseek-ai/DeepSeek-V3-0324", name: "DeepSeek V3"),
        OpenAIModelInfo(id: "deepseek-ai/DeepSeek-R1", name: "DeepSeek R1"),
        OpenAIModelInfo(id: "Qwen/Qwen2.5-Coder-32B-Instruct", name: "Qwen 2.5 Coder 32B"),
        OpenAIModelInfo(id: "meta-llama/Llama-3.3-70B-Instruct", name: "Llama 3.3 70B"),
        OpenAIModelInfo(id: "mistralai/Mistral-Small-24B-Instruct-2501", name: "Mistral Small 24B"),
    ]

    // MARK: - MiniMax

    nonisolated static let defaultMiniMaxModels: [OpenAIModelInfo] = [
        OpenAIModelInfo(id: "MiniMax-M2.7", name: "MiniMax-M2.7"),
        OpenAIModelInfo(id: "MiniMax-M2.7-highspeed", name: "MiniMax-M2.7-highspeed"),
    ]

    // MARK: - Ollama (Cloud)

    nonisolated static let defaultOllamaModels: [OllamaModelInfo] = [
        OllamaModelInfo(id: "nemotron-3-super", name: "nemotron-3-super", supportsVision: false),
        OllamaModelInfo(id: "qwen3.5:397b", name: "qwen3.5:397b", supportsVision: false),
        OllamaModelInfo(id: "minimax-m2.5", name: "minimax-m2.5", supportsVision: false),
        OllamaModelInfo(id: "glm-5", name: "glm-5", supportsVision: false),
        OllamaModelInfo(id: "kimi-k2.5", name: "kimi-k2.5", supportsVision: true),
        OllamaModelInfo(id: "glm-4.7", name: "glm-4.7", supportsVision: false),
        OllamaModelInfo(id: "minimax-m2.1", name: "minimax-m2.1", supportsVision: false),
        OllamaModelInfo(id: "gemini-3-flash-preview", name: "gemini-3-flash-preview", supportsVision: true),
        OllamaModelInfo(id: "nemotron-3-nano:30b", name: "nemotron-3-nano:30b", supportsVision: false),
        OllamaModelInfo(id: "devstral-small-2:24b", name: "devstral-small-2:24b", supportsVision: false),
        OllamaModelInfo(id: "devstral-2:123b", name: "devstral-2:123b", supportsVision: false),
        OllamaModelInfo(id: "ministral-3:8b", name: "ministral-3:8b", supportsVision: false),
        OllamaModelInfo(id: "ministral-3:14b", name: "ministral-3:14b", supportsVision: false),
        OllamaModelInfo(id: "deepseek-v3.2", name: "deepseek-v3.2", supportsVision: false),
        OllamaModelInfo(id: "mistral-large-3:675b", name: "mistral-large-3:675b", supportsVision: false),
        OllamaModelInfo(id: "deepseek-v3.1:671b", name: "deepseek-v3.1:671b", supportsVision: false),
        OllamaModelInfo(id: "cogito-2.1:671b", name: "cogito-2.1:671b", supportsVision: false),
        OllamaModelInfo(id: "minimax-m2", name: "minimax-m2", supportsVision: false),
        OllamaModelInfo(id: "glm-4.6", name: "glm-4.6", supportsVision: false),
        OllamaModelInfo(id: "qwen3-vl:235b-instruct", name: "qwen3-vl:235b-instruct", supportsVision: true),
        OllamaModelInfo(id: "qwen3-vl:235b", name: "qwen3-vl:235b", supportsVision: true),
        OllamaModelInfo(id: "qwen3-next:80b", name: "qwen3-next:80b", supportsVision: false),
        OllamaModelInfo(id: "kimi-k2:1t", name: "kimi-k2:1t", supportsVision: false),
        OllamaModelInfo(id: "gpt-oss:120b", name: "gpt-oss:120b", supportsVision: false),
        OllamaModelInfo(id: "qwen3-coder:480b", name: "qwen3-coder:480b", supportsVision: false),
        OllamaModelInfo(id: "gemma3:27b", name: "gemma3:27b", supportsVision: true),
        OllamaModelInfo(id: "gemma3:12b", name: "gemma3:12b", supportsVision: true),
        OllamaModelInfo(id: "gemma3:4b", name: "gemma3:4b", supportsVision: true),
        OllamaModelInfo(id: "qwen3-coder-next", name: "qwen3-coder-next", supportsVision: false),
        OllamaModelInfo(id: "gpt-oss:20b", name: "gpt-oss:20b", supportsVision: false)

    ]

    // MARK: - Claude

    nonisolated static let defaultClaudeModels: [ClaudeModelInfo] = [
        ClaudeModelInfo(
            id: "claude-sonnet-4-6",
            name: "claude-sonnet-4-6",
            displayName: "Claude Sonnet 4.6",
            createdAt: "2026-02-17",
            description: nil
        ),
        ClaudeModelInfo(
            id: "claude-opus-4-6",
            name: "claude-opus-4-6",
            displayName: "Claude Opus 4.6",
            createdAt: "2026-02-04",
            description: nil
        ),
        ClaudeModelInfo(
            id: "claude-opus-4-5-20251101",
            name: "claude-opus-4-5-20251101",
            displayName: "Claude Opus 4.5",
            createdAt: "2025-11-24",
            description: nil
        ),
        ClaudeModelInfo(
            id: "claude-haiku-4-5-20251001",
            name: "claude-haiku-4-5-20251001",
            displayName: "Claude Haiku 4.5",
            createdAt: "2025-10-15",
            description: nil
        ),
        ClaudeModelInfo(
            id: "claude-sonnet-4-5-20250929",
            name: "claude-sonnet-4-5-20250929",
            displayName: "Claude Sonnet 4.5",
            createdAt: "2025-09-29",
            description: nil
        ),
        ClaudeModelInfo(
            id: "claude-opus-4-1-20250805",
            name: "claude-opus-4-1-20250805",
            displayName: "Claude Opus 4.1",
            createdAt: "2025-08-05",
            description: nil
        ),
        ClaudeModelInfo(
            id: "claude-opus-4-20250514",
            name: "claude-opus-4-20250514",
            displayName: "Claude Opus 4",
            createdAt: "2025-05-22",
            description: nil
        ),
        ClaudeModelInfo(
            id: "claude-sonnet-4-20250514",
            name: "claude-sonnet-4-20250514",
            displayName: "Claude Sonnet 4",
            createdAt: "2025-05-22",
            description: nil
        ),
        ClaudeModelInfo(
            id: "claude-3-haiku-20240307",
            name: "claude-3-haiku-20240307",
            displayName: "Claude Haiku 3",
            createdAt: "2024-03-07",
            description: nil
        )
    ]
}
