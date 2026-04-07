import AgentLLM

/// All Agent! LLM provider configurations — defined in the app, not the package.
/// To add a new LLM: add a static config here and register it in registerAllProviders().
@MainActor
enum LLMProviderSetup {

    static func registerAllProviders() {
        LLMRegistry.shared.registerAll([
            claude, openAI, gemini, grok, mistral, codestral, vibe, deepSeek, huggingFace, zAI, bigModel, qwen,
            ollama, localOllama, vLLM, lmStudio, appleIntelligence
        ])
    }

    // MARK: - Cloud API Providers

    static let claude = LLMProviderConfig(
        id: "claude", displayName: "Claude",
        kind: .cloudAPI, apiProtocol: .anthropic,
        endpoint: LLMEndpoint(
            chatURL: "https://api.anthropic.com/v1/messages",
            modelsURL: "https://api.anthropic.com/v1/models",
            authHeader: "x-api-key",
            authPrefix: "",
            extraHeaders: ["anthropic-version": "2023-06-01"]
        ),
        capabilities: [.streaming, .tools, .vision, .systemPrompt, .caching, .thinking, .webSearch]
    )

    static let openAI = LLMProviderConfig(
        id: "openAI", displayName: "OpenAI",
        kind: .cloudAPI, apiProtocol: .openAI,
        endpoint: LLMEndpoint(
            chatURL: "https://api.openai.com/v1/chat/completions",
            modelsURL: "https://api.openai.com/v1/models"
        ),
        capabilities: [.streaming, .tools, .vision, .systemPrompt]
    )

    static let deepSeek = LLMProviderConfig(
        id: "deepSeek", displayName: "DeepSeek",
        kind: .cloudAPI, apiProtocol: .openAI,
        endpoint: LLMEndpoint(
            chatURL: "https://api.deepseek.com/chat/completions",
            modelsURL: "https://api.deepseek.com/v1/models"
        ),
        capabilities: [.streaming, .tools, .systemPrompt]
    )

    static let huggingFace = LLMProviderConfig(
        id: "huggingFace", displayName: "Hugging Face",
        kind: .cloudAPI, apiProtocol: .openAI,
        endpoint: LLMEndpoint(
            chatURL: "https://router.huggingface.co/v1/chat/completions",
            modelsURL: "https://router.huggingface.co/v1/models"
        ),
        capabilities: [.streaming, .tools, .systemPrompt]
    )

    // Z.ai — general endpoint supports vision, coding endpoint for text-only models
    // URL is swapped at runtime based on model name (V suffix = general, else = coding)
    static let zAI = LLMProviderConfig(
        id: "zAI", displayName: "Z.ai",
        kind: .cloudAPI, apiProtocol: .openAI,
        endpoint: LLMEndpoint(
            chatURL: "https://api.z.ai/api/paas/v4/chat/completions",
            modelsURL: "https://api.z.ai/api/paas/v4/models"
        ),
        capabilities: [.streaming, .tools, .systemPrompt, .vision],
        temperature: 0.7
    )

    // BigModel.cn — China mainland mirror of Z.ai, same model lineup
    static let bigModel = LLMProviderConfig(
        id: "bigModel", displayName: "BigModel",
        kind: .cloudAPI, apiProtocol: .openAI,
        endpoint: LLMEndpoint(
            chatURL: "https://open.bigmodel.cn/api/paas/v4/chat/completions",
            modelsURL: "https://open.bigmodel.cn/api/paas/v4/models"
        ),
        capabilities: [.streaming, .tools, .systemPrompt, .vision],
        temperature: 0.7
    )

    // Qwen (Alibaba DashScope) — URL based on user locale
    static let qwen: LLMProviderConfig = {
        let region = Locale.current.region?.identifier ?? ""
        let baseURL: String
        switch region {
        case "CN": baseURL = "https://dashscope.aliyuncs.com/compatible-mode/v1"
        case "HK": baseURL = "https://cn-hongkong.aliyuncs.com/compatible-mode/v1"
        default:   baseURL = "https://dashscope-intl.aliyuncs.com/compatible-mode/v1"
        }
        return LLMProviderConfig(
            id: "qwen", displayName: "Qwen",
            kind: .cloudAPI, apiProtocol: .openAI,
            endpoint: LLMEndpoint(
                chatURL: "\(baseURL)/chat/completions",
                modelsURL: "\(baseURL)/models"
            ),
            capabilities: [.streaming, .tools, .systemPrompt, .vision]
        )
    }()

    static let gemini = LLMProviderConfig(
        id: "gemini", displayName: "Google Gemini",
        kind: .cloudAPI, apiProtocol: .openAI,
        endpoint: LLMEndpoint(
            chatURL: "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions",
            modelsURL: "https://generativelanguage.googleapis.com/v1beta/openai/models"
        ),
        capabilities: [.streaming, .tools, .vision, .systemPrompt]
    )

    static let grok = LLMProviderConfig(
        id: "grok", displayName: "Grok",
        kind: .cloudAPI, apiProtocol: .openAI,
        endpoint: LLMEndpoint(
            chatURL: "https://api.x.ai/v1/chat/completions",
            modelsURL: "https://api.x.ai/v1/models"
        ),
        capabilities: [.streaming, .tools, .vision, .systemPrompt]
    )

    static let mistral = LLMProviderConfig(
        id: "mistral", displayName: "Mistral",
        kind: .cloudAPI, apiProtocol: .openAI,
        endpoint: LLMEndpoint(
            chatURL: "https://api.mistral.ai/v1/chat/completions",
            modelsURL: "https://api.mistral.ai/v1/models"
        ),
        capabilities: [.streaming, .tools, .vision, .systemPrompt]
    )

    static let codestral = LLMProviderConfig(
        id: "codestral", displayName: "Codestral",
        kind: .cloudAPI, apiProtocol: .openAI,
        endpoint: LLMEndpoint(
            chatURL: "https://codestral.mistral.ai/v1/chat/completions",
            modelsURL: "https://codestral.mistral.ai/v1/models"
        ),
        capabilities: [.streaming, .tools, .systemPrompt]
    )

    static let vibe = LLMProviderConfig(
        id: "vibe", displayName: "Mistral Vibe",
        kind: .cloudAPI, apiProtocol: .openAI,
        endpoint: LLMEndpoint(
            chatURL: "https://api.mistral.ai/v1/chat/completions",
            modelsURL: "https://api.mistral.ai/v1/models"
        ),
        capabilities: [.streaming, .tools, .systemPrompt]
    )

    // MARK: - Ollama

    static let ollama = LLMProviderConfig(
        id: "ollama", displayName: "Ollama",
        kind: .remoteServer, apiProtocol: .ollama,
        endpoint: LLMEndpoint(
            chatURL: "https://ollama.com/api/chat",
            modelsURL: "https://ollama.com/api/tags",
            defaultPort: LLMEndpoint.ollamaPort
        ),
        capabilities: [.streaming, .tools, .systemPrompt, .vision]
    )

    static let localOllama = LLMProviderConfig(
        id: "localOllama", displayName: "Local Ollama",
        kind: .localServer, apiProtocol: .ollama,
        endpoint: LLMEndpoint(
            chatURL: "http://localhost:\(LLMEndpoint.ollamaPort)/api/chat",
            modelsURL: "http://localhost:\(LLMEndpoint.ollamaPort)/api/tags",
            authHeader: "", authPrefix: "",
            defaultPort: LLMEndpoint.ollamaPort
        ),
        capabilities: [.streaming, .tools, .systemPrompt, .vision],
        apiKeyOptional: true
    )

    // MARK: - Self-Hosted

    static let vLLM = LLMProviderConfig(
        id: "vLLM", displayName: "vLLM",
        kind: .remoteServer, apiProtocol: .openAI,
        endpoint: LLMEndpoint(
            chatURL: "http://localhost:\(LLMEndpoint.vLLMPort)/v1/chat/completions",
            modelsURL: "http://localhost:\(LLMEndpoint.vLLMPort)/v1/models",
            authHeader: "", authPrefix: "",
            defaultPort: LLMEndpoint.vLLMPort
        ),
        capabilities: [.streaming, .tools, .systemPrompt],
        apiKeyOptional: true
    )

    // MARK: - LM Studio (3 protocol variants)

    static let lmStudio = LLMProviderConfig(
        id: "lmStudio", displayName: "LM Studio",
        kind: .localServer, apiProtocol: .openAI,
        endpoint: LLMEndpoint(
            chatURL: "http://localhost:\(LLMEndpoint.lmStudioPort)/v1/chat/completions",
            modelsURL: "http://localhost:\(LLMEndpoint.lmStudioPort)/v1/models",
            authHeader: "", authPrefix: "",
            defaultPort: LLMEndpoint.lmStudioPort
        ),
        capabilities: [.streaming, .tools, .systemPrompt],
        apiKeyOptional: true,
        supportedProtocols: [.openAI, .anthropic, .custom]
    )

    /// LM Studio endpoint for Anthropic Compatible mode
    static let lmStudioAnthropicEndpoint = LLMEndpoint(
        chatURL: "http://localhost:\(LLMEndpoint.lmStudioPort)/v1/messages",
        modelsURL: "http://localhost:\(LLMEndpoint.lmStudioPort)/v1/models",
        authHeader: "", authPrefix: "",
        defaultPort: LLMEndpoint.lmStudioPort
    )

    /// LM Studio endpoint for Native mode
    static let lmStudioNativeEndpoint = LLMEndpoint(
        chatURL: "http://localhost:\(LLMEndpoint.lmStudioPort)/api/v1/chat",
        modelsURL: "http://localhost:\(LLMEndpoint.lmStudioPort)/api/v1/models",
        authHeader: "", authPrefix: "",
        defaultPort: LLMEndpoint.lmStudioPort
    )

    // MARK: - On-Device

    static let appleIntelligence = LLMProviderConfig(
        id: "foundationModel", displayName: "Apple Intelligence",
        kind: .embedded, apiProtocol: .foundationModel,
        endpoint: LLMEndpoint(chatURL: ""),
        capabilities: [.streaming, .tools, .systemPrompt],
        apiKeyOptional: true
    )
}
