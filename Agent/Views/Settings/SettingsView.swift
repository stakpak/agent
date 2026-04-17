import SwiftUI
import AgentTools

struct SettingsView: View {
    @Bindable var viewModel: AgentViewModel

    /// Per-provider temperature binding so the slider always edits the active model's temp.
    private var llmTemperatureBinding: Binding<Double> {
        switch viewModel.selectedProvider {
        case .claude: return $viewModel.claudeTemperature
        case .ollama: return $viewModel.ollamaTemperature
        case .openAI: return $viewModel.openAITemperature
        case .deepSeek: return $viewModel.deepSeekTemperature
        case .huggingFace: return $viewModel.huggingFaceTemperature
        case .localOllama: return $viewModel.localOllamaTemperature
        case .vLLM: return $viewModel.vLLMTemperature
        case .lmStudio: return $viewModel.lmStudioTemperature
        case .zAI: return $viewModel.zAITemperature
        case .bigModel: return $viewModel.zAITemperature
        case .miniMax: return $viewModel.miniMaxTemperature
        case .qwen: return $viewModel.openAITemperature
        case .gemini: return $viewModel.geminiTemperature
        case .grok: return $viewModel.grokTemperature
        case .mistral: return $viewModel.openAITemperature
        case .codestral: return $viewModel.openAITemperature
        case .vibe: return $viewModel.openAITemperature
        case .foundationModel: return $viewModel.claudeTemperature // unused
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            // Provider toggle
            VStack(alignment: .leading, spacing: 12) {
                Text("LLM Provider")
                    .font(.headline)

                Text("Configure your AI provider and API keys.")
                    .font(.caption)
                    .foregroundStyle(.secondary)

                Picker("AI", selection: $viewModel.selectedProvider) {
                    ForEach(APIProvider.selectableProviders, id: \.self) { provider in
                        Text(provider.displayName).tag(provider)
                    }
                }
                .labelsHidden()

                Text("Ollama Pro is preferred")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Divider()

            if viewModel.selectedProvider == .claude {
                // Claude settings
                VStack(alignment: .leading, spacing: 10) {
                    Text("Claude API")
                        .font(.headline)

                    VStack(alignment: .leading, spacing: 4) {
                        Text("API Key").font(.caption).foregroundStyle(.secondary)
                        LockedSecureField(text: $viewModel.apiKey, placeholder: "sk-ant-...", lockKey: "lock.claudeAPIKey")
                    }

                    VStack(alignment: .leading, spacing: 4) {
                        Text("Model").font(.caption).foregroundStyle(.secondary)
                        Picker("Model", selection: $viewModel.selectedModel) {
                            ForEach(viewModel.availableClaudeModels) { model in
                                Text(model.formattedDisplayName).tag(model.id)
                            }
                        }
                        .labelsHidden()
                    }
                }
            } else if viewModel.selectedProvider == .openAI {
                // OpenAI settings
                VStack(alignment: .leading, spacing: 10) {
                    Text("OpenAI API")
                        .font(.headline)

                    VStack(alignment: .leading, spacing: 4) {
                        Text("API Key").font(.caption).foregroundStyle(.secondary)
                        LockedSecureField(text: $viewModel.openAIAPIKey, placeholder: "sk-...", lockKey: "lock.openAIAPIKey")
                    }

                    VStack(alignment: .leading, spacing: 4) {
                        Text("Model").font(.caption).foregroundStyle(.secondary)
                        HStack {
                            if viewModel.openAIModels.isEmpty {
                                TextField("Model name", text: $viewModel.openAIModel)
                                    .textFieldStyle(.roundedBorder)
                            } else {
                                Picker("Model", selection: $viewModel.openAIModel) {
                                    ForEach(viewModel.openAIModels) { model in
                                        Text(model.name).tag(model.id)
                                    }
                                }
                                .labelsHidden()
                            }

                            Button {
                                viewModel.fetchModelsIfNeeded(for: .openAI, force: true)
                            } label: {
                                if viewModel.isFetchingOpenAIModels {
                                    ProgressView()
                                        .controlSize(.small)
                                } else {
                                    Image(systemName: "arrow.clockwise")
                                }
                            }
                            .buttonStyle(.bordered)
                            .controlSize(.small)
                            .disabled(viewModel.isFetchingOpenAIModels)
                            .help("Fetch available models")
                        }
                    }
                }
            } else if viewModel.selectedProvider == .deepSeek {
                // DeepSeek settings
                VStack(alignment: .leading, spacing: 10) {
                    Text("DeepSeek API")
                        .font(.headline)

                    VStack(alignment: .leading, spacing: 4) {
                        Text("API Key").font(.caption).foregroundStyle(.secondary)
                        LockedSecureField(text: $viewModel.deepSeekAPIKey, placeholder: "sk-...", lockKey: "lock.deepSeekAPIKey")
                    }

                    VStack(alignment: .leading, spacing: 4) {
                        Text("Model").font(.caption).foregroundStyle(.secondary)
                        HStack {
                            if viewModel.deepSeekModels.isEmpty {
                                TextField("Model name", text: $viewModel.deepSeekModel)
                                    .textFieldStyle(.roundedBorder)
                            } else {
                                Picker("Model", selection: $viewModel.deepSeekModel) {
                                    ForEach(viewModel.deepSeekModels) { model in
                                        Text(model.name).tag(model.id)
                                    }
                                }
                                .labelsHidden()
                            }

                            Button {
                                viewModel.fetchDeepSeekModels()
                            } label: {
                                if viewModel.isFetchingDeepSeekModels {
                                    ProgressView()
                                        .controlSize(.small)
                                } else {
                                    Image(systemName: "arrow.clockwise")
                                }
                            }
                            .buttonStyle(.bordered)
                            .controlSize(.small)
                            .disabled(viewModel.isFetchingDeepSeekModels)
                            .help("Fetch available models")
                        }
                    }
                }
            } else if viewModel.selectedProvider == .huggingFace {
                // Hugging Face settings
                VStack(alignment: .leading, spacing: 10) {
                    Text("Hugging Face Inference")
                        .font(.headline)

                    VStack(alignment: .leading, spacing: 4) {
                        Text("API Key").font(.caption).foregroundStyle(.secondary)
                        LockedSecureField(text: $viewModel.huggingFaceAPIKey, placeholder: "hf_...", lockKey: "lock.huggingFaceAPIKey")
                    }

                    VStack(alignment: .leading, spacing: 4) {
                        Text("Model").font(.caption).foregroundStyle(.secondary)
                        HStack {
                            if viewModel.huggingFaceModels.isEmpty {
                                TextField("Model name", text: $viewModel.huggingFaceModel)
                                    .textFieldStyle(.roundedBorder)
                            } else {
                                Picker("Model", selection: $viewModel.huggingFaceModel) {
                                    ForEach(viewModel.huggingFaceModels) { model in
                                        HStack(spacing: 4) {
                                            Text(model.name)
                                            if AgentViewModel.isVisionModel(model.id) {
                                                Image(systemName: "eye")
                                                    .foregroundStyle(.blue)
                                                    .font(.caption2)
                                            }
                                        }.tag(model.id)
                                    }
                                }
                                .labelsHidden()
                            }

                            Button {
                                viewModel.fetchHuggingFaceModels()
                            } label: {
                                if viewModel.isFetchingHuggingFaceModels {
                                    ProgressView()
                                        .controlSize(.small)
                                } else {
                                    Image(systemName: "arrow.clockwise")
                                }
                            }
                            .buttonStyle(.bordered)
                            .controlSize(.small)
                            .disabled(viewModel.isFetchingHuggingFaceModels)
                            .help("Fetch available models")
                        }
                    }
                }
            } else if viewModel.selectedProvider == .zAI {
                // Z.ai (ZhipuAI GLM) settings
                VStack(alignment: .leading, spacing: 10) {
                    Text("Z.ai API")
                        .font(.headline)

                    VStack(alignment: .leading, spacing: 4) {
                        Text("API Key").font(.caption).foregroundStyle(.secondary)
                        LockedSecureField(text: $viewModel.zAIAPIKey, placeholder: "Z.ai API key", lockKey: "lock.zAIAPIKey")
                    }

                    VStack(alignment: .leading, spacing: 4) {
                        Text("Model").font(.caption).foregroundStyle(.secondary)
                        HStack {
                            if viewModel.zAIModels.isEmpty {
                                TextField("Model name", text: $viewModel.zAIModel)
                                    .textFieldStyle(.roundedBorder)
                            } else {
                                Picker("Model", selection: $viewModel.zAIModel) {
                                    ForEach(viewModel.zAIModels) { model in
                                        HStack(spacing: 4) {
                                            Text(model.name)
                                            if model.id.hasSuffix(":v") {
                                                Image(systemName: "eye")
                                                    .foregroundStyle(.blue)
                                                    .font(.caption2)
                                            }
                                        }.tag(model.id)
                                    }
                                }
                                .labelsHidden()
                            }

                            Button {
                                viewModel.fetchModelsIfNeeded(for: .zAI, force: true)
                            } label: {
                                if viewModel.isFetchingZAIModels {
                                    ProgressView()
                                        .controlSize(.small)
                                } else {
                                    Image(systemName: "arrow.clockwise")
                                }
                            }
                            .buttonStyle(.bordered)
                            .controlSize(.small)
                            .disabled(viewModel.isFetchingZAIModels)
                            .help("Fetch available models")
                        }
                    }
                }
            } else if viewModel.selectedProvider == .bigModel {
                VStack(alignment: .leading, spacing: 10) {
                    Text("BigModel (China)")
                        .font(.headline)

                    VStack(alignment: .leading, spacing: 4) {
                        Text("API Key").font(.caption).foregroundStyle(.secondary)
                        LockedSecureField(text: $viewModel.bigModelAPIKey, placeholder: "BigModel API key", lockKey: "lock.bigModelAPIKey")
                    }

                    VStack(alignment: .leading, spacing: 4) {
                        Text("Model").font(.caption).foregroundStyle(.secondary)
                        TextField("Model name", text: $viewModel.bigModelModel)
                            .textFieldStyle(.roundedBorder)
                    }
                }
            } else if viewModel.selectedProvider == .miniMax {
                VStack(alignment: .leading, spacing: 10) {
                    Text("MiniMax API")
                        .font(.headline)

                    VStack(alignment: .leading, spacing: 4) {
                        Text("API Key").font(.caption).foregroundStyle(.secondary)
                        LockedSecureField(text: $viewModel.miniMaxAPIKey, placeholder: "MiniMax API key", lockKey: "lock.miniMaxAPIKey")
                    }

                    VStack(alignment: .leading, spacing: 4) {
                        Text("Model").font(.caption).foregroundStyle(.secondary)
                        HStack {
                            if viewModel.miniMaxModels.isEmpty {
                                TextField("Model name", text: $viewModel.miniMaxModel)
                                    .textFieldStyle(.roundedBorder)
                            } else {
                                Picker("Model", selection: $viewModel.miniMaxModel) {
                                    ForEach(viewModel.miniMaxModels) { model in
                                        Text(model.name).tag(model.id)
                                    }
                                }
                                .labelsHidden()
                            }

                            Button {
                                viewModel.fetchMiniMaxModels()
                            } label: {
                                if viewModel.isFetchingMiniMaxModels {
                                    ProgressView()
                                        .controlSize(.small)
                                } else {
                                    Image(systemName: "arrow.clockwise")
                                }
                            }
                            .buttonStyle(.bordered)
                            .controlSize(.small)
                            .disabled(viewModel.isFetchingMiniMaxModels)
                            .help("Fetch available models")
                        }
                    }
                }
            } else if viewModel.selectedProvider == .qwen {
                VStack(alignment: .leading, spacing: 10) {
                    Text("Qwen (DashScope)")
                        .font(.headline)

                    VStack(alignment: .leading, spacing: 4) {
                        Text("API Key").font(.caption).foregroundStyle(.secondary)
                        LockedSecureField(text: $viewModel.qwenAPIKey, placeholder: "DashScope API key", lockKey: "lock.qwenAPIKey")
                    }

                    VStack(alignment: .leading, spacing: 4) {
                        Text("Model").font(.caption).foregroundStyle(.secondary)
                        HStack {
                            if viewModel.qwenModels.isEmpty {
                                TextField("Model name", text: $viewModel.qwenModel)
                                    .textFieldStyle(.roundedBorder)
                            } else {
                                Picker("Model", selection: $viewModel.qwenModel) {
                                    ForEach(viewModel.qwenModels) { model in
                                        Text(model.name).tag(model.id)
                                    }
                                }
                                .labelsHidden()
                            }

                            Button {
                                viewModel.fetchQwenModels()
                            } label: {
                                if viewModel.isFetchingQwenModels {
                                    ProgressView().controlSize(.small)
                                } else {
                                    Image(systemName: "arrow.clockwise")
                                }
                            }
                            .buttonStyle(.borderless)
                            .disabled(viewModel.isFetchingQwenModels)
                        }
                    }
                }
            } else if viewModel.selectedProvider == .gemini {
                VStack(alignment: .leading, spacing: 10) {
                    Text("Google Gemini API")
                        .font(.headline)

                    VStack(alignment: .leading, spacing: 4) {
                        Text("API Key").font(.caption).foregroundStyle(.secondary)
                        LockedSecureField(text: $viewModel.geminiAPIKey, placeholder: "Gemini API key", lockKey: "lock.geminiAPIKey")
                    }

                    VStack(alignment: .leading, spacing: 4) {
                        Text("Model").font(.caption).foregroundStyle(.secondary)
                        HStack {
                            if viewModel.geminiModels.isEmpty {
                                TextField("Model name", text: $viewModel.geminiModel)
                                    .textFieldStyle(.roundedBorder)
                            } else {
                                Picker("Model", selection: $viewModel.geminiModel) {
                                    ForEach(viewModel.geminiModels) { model in
                                        HStack(spacing: 4) {
                                            Text(model.name)
                                            if model.id.contains("gemini-") {
                                                Image(systemName: "eye")
                                                    .foregroundStyle(.blue)
                                                    .font(.caption2)
                                            }
                                        }.tag(model.id)
                                    }
                                }
                                .labelsHidden()
                            }

                            Button {
                                viewModel.fetchGeminiModels()
                            } label: {
                                if viewModel.isFetchingGeminiModels {
                                    ProgressView().controlSize(.small)
                                } else {
                                    Image(systemName: "arrow.clockwise")
                                }
                            }
                            .buttonStyle(.bordered)
                            .controlSize(.small)
                            .disabled(viewModel.isFetchingGeminiModels)
                            .help("Fetch available models")
                        }
                    }
                }
            } else if viewModel.selectedProvider == .grok {
                VStack(alignment: .leading, spacing: 10) {
                    Text("Grok API (xAI)")
                        .font(.headline)

                    VStack(alignment: .leading, spacing: 4) {
                        Text("API Key").font(.caption).foregroundStyle(.secondary)
                        LockedSecureField(text: $viewModel.grokAPIKey, placeholder: "Grok API key", lockKey: "lock.grokAPIKey")
                    }

                    VStack(alignment: .leading, spacing: 4) {
                        Text("Model").font(.caption).foregroundStyle(.secondary)
                        HStack {
                            if viewModel.grokModels.isEmpty {
                                TextField("Model name", text: $viewModel.grokModel)
                                    .textFieldStyle(.roundedBorder)
                            } else {
                                Picker("Model", selection: $viewModel.grokModel) {
                                    ForEach(viewModel.grokModels) { model in
                                        Text(model.name).tag(model.id)
                                    }
                                }
                                .labelsHidden()
                            }

                            Button {
                                viewModel.fetchGrokModels()
                            } label: {
                                if viewModel.isFetchingGrokModels {
                                    ProgressView().controlSize(.small)
                                } else {
                                    Image(systemName: "arrow.clockwise")
                                }
                            }
                            .buttonStyle(.bordered)
                            .controlSize(.small)
                            .disabled(viewModel.isFetchingGrokModels)
                            .help("Fetch available models")
                        }
                    }
                }
            } else if viewModel.selectedProvider == .mistral {
                VStack(alignment: .leading, spacing: 10) {
                    Text("Mistral AI")
                        .font(.headline)

                    VStack(alignment: .leading, spacing: 4) {
                        Text("API Key").font(.caption).foregroundStyle(.secondary)
                        LockedSecureField(text: $viewModel.mistralAPIKey, placeholder: "Mistral API key", lockKey: "lock.mistralAPIKey")
                    }

                    VStack(alignment: .leading, spacing: 4) {
                        Text("Model").font(.caption).foregroundStyle(.secondary)
                        HStack {
                            if viewModel.mistralModels.isEmpty {
                                TextField("Model name", text: $viewModel.mistralModel)
                                    .textFieldStyle(.roundedBorder)
                            } else {
                                Picker("Model", selection: $viewModel.mistralModel) {
                                    ForEach(viewModel.mistralModels) { model in
                                        HStack(spacing: 4) {
                                            Text(model.name)
                                            if AgentViewModel.isVisionModel(model.id) {
                                                Image(systemName: "eye")
                                                    .foregroundStyle(.blue)
                                                    .font(.caption2)
                                            }
                                        }.tag(model.id)
                                    }
                                }
                                .labelsHidden()
                            }
                            Button {
                                viewModel.fetchMistralModels()
                            } label: {
                                if viewModel.isFetchingMistralModels {
                                    ProgressView().controlSize(.small)
                                } else {
                                    Image(systemName: "arrow.clockwise")
                                }
                            }
                            .buttonStyle(.bordered)
                            .controlSize(.small)
                            .disabled(viewModel.isFetchingMistralModels)
                            .help("Fetch available models")
                        }
                    }
                }
            } else if viewModel.selectedProvider == .codestral {
                VStack(alignment: .leading, spacing: 10) {
                    Text("Codestral")
                        .font(.headline)

                    VStack(alignment: .leading, spacing: 4) {
                        Text("API Key").font(.caption).foregroundStyle(.secondary)
                        LockedSecureField(
                            text: $viewModel.codestralAPIKey,
                            placeholder: "Codestral API key",
                            lockKey: "lock.codestralAPIKey"
                        )
                    }

                    VStack(alignment: .leading, spacing: 4) {
                        Text("Model").font(.caption).foregroundStyle(.secondary)
                        HStack {
                            Picker("", selection: $viewModel.codestralModel) {
                                ForEach(viewModel.codestralModels, id: \.id) { model in
                                    Text(model.name.isEmpty ? model.id : model.name).tag(model.id)
                                }
                            }
                            .labelsHidden()

                            Button {
                                viewModel.fetchCodestralModels()
                            } label: {
                                if viewModel.isFetchingCodestralModels {
                                    ProgressView().controlSize(.mini)
                                } else {
                                    Image(systemName: "arrow.clockwise")
                                }
                            }
                            .buttonStyle(.bordered)
                            .controlSize(.small)
                            .disabled(viewModel.isFetchingCodestralModels)
                            .help("Fetch available models")
                        }
                    }
                }
            } else if viewModel.selectedProvider == .vibe {
                VStack(alignment: .leading, spacing: 10) {
                    Text("Mistral Vibe")
                        .font(.headline)

                    VStack(alignment: .leading, spacing: 4) {
                        Text("API Key").font(.caption).foregroundStyle(.secondary)
                        LockedSecureField(text: $viewModel.vibeAPIKey, placeholder: "Vibe API key", lockKey: "lock.vibeAPIKey")
                    }

                    VStack(alignment: .leading, spacing: 4) {
                        Text("Model").font(.caption).foregroundStyle(.secondary)
                        HStack {
                            Picker("", selection: $viewModel.vibeModel) {
                                ForEach(viewModel.vibeModels, id: \.id) { model in
                                    Text(model.name.isEmpty ? model.id : model.name).tag(model.id)
                                }
                            }
                            .labelsHidden()

                            Button {
                                viewModel.fetchVibeModels()
                            } label: {
                                if viewModel.isFetchingVibeModels {
                                    ProgressView().controlSize(.mini)
                                } else {
                                    Image(systemName: "arrow.clockwise")
                                }
                            }
                            .buttonStyle(.bordered)
                            .controlSize(.small)
                            .disabled(viewModel.isFetchingVibeModels)
                            .help("Fetch available models")
                        }
                    }
                }
            } else if viewModel.selectedProvider == .ollama {
                // Cloud Ollama settings
                VStack(alignment: .leading, spacing: 10) {
                    Text("Ollama Cloud")
                        .font(.headline)

                    VStack(alignment: .leading, spacing: 4) {
                        Text("API Key").font(.caption).foregroundStyle(.secondary)
                        LockedSecureField(text: $viewModel.ollamaAPIKey, placeholder: "Required for cloud", lockKey: "lock.ollamaAPIKey")
                    }

                    VStack(alignment: .leading, spacing: 4) {
                        Text("Model").font(.caption).foregroundStyle(.secondary)
                        HStack {
                            if viewModel.ollamaModels.isEmpty {
                                TextField("Model name", text: $viewModel.ollamaModel)
                                    .textFieldStyle(.roundedBorder)
                            } else {
                                Picker("Model", selection: $viewModel.ollamaModel) {
                                    ForEach(viewModel.ollamaModels) { model in
                                        HStack(spacing: 4) {
                                            Text(model.name)
                                            if model.supportsVision {
                                                Image(systemName: "eye")
                                                    .foregroundStyle(.blue)
                                                    .font(.caption2)
                                            }
                                        }
                                        .tag(model.name)
                                    }
                                }
                                .labelsHidden()
                            }

                            Button {
                                viewModel.fetchModelsIfNeeded(for: .ollama, force: true)
                            } label: {
                                if viewModel.isFetchingModels {
                                    ProgressView()
                                        .controlSize(.small)
                                } else {
                                    Image(systemName: "arrow.clockwise")
                                }
                            }
                            .buttonStyle(.bordered)
                            .controlSize(.small)
                            .disabled(viewModel.isFetchingModels)
                            .help("Fetch available models")
                        }
                    }
                }
            } else if viewModel.selectedProvider == .lmStudio {
                // LM Studio settings
                VStack(alignment: .leading, spacing: 10) {
                    Text("LM Studio")
                        .font(.headline)

                    VStack(alignment: .leading, spacing: 4) {
                        Text("API Protocol").font(.caption).foregroundStyle(.secondary)
                        Picker("Protocol", selection: $viewModel.lmStudioProtocol) {
                            ForEach(LMStudioProtocol.allCases, id: \.self) { proto in
                                Text(proto.displayName).tag(proto)
                            }
                        }
                        .labelsHidden()
                    }

                    VStack(alignment: .leading, spacing: 4) {
                        Text("API Key (optional)").font(.caption).foregroundStyle(.secondary)
                        SecureField("Leave blank if not required", text: $viewModel.lmStudioAPIKey)
                            .textFieldStyle(.roundedBorder)
                    }

                    VStack(alignment: .leading, spacing: 4) {
                        Text("Endpoint").font(.caption).foregroundStyle(.secondary)
                        TextField(viewModel.lmStudioProtocol.defaultEndpoint, text: $viewModel.lmStudioEndpoint)
                            .textFieldStyle(.roundedBorder)
                    }

                    VStack(alignment: .leading, spacing: 4) {
                        Text("Model").font(.caption).foregroundStyle(.secondary)
                        HStack {
                            if viewModel.lmStudioModels.isEmpty {
                                TextField("Model name", text: $viewModel.lmStudioModel)
                                    .textFieldStyle(.roundedBorder)
                            } else {
                                Picker("Model", selection: $viewModel.lmStudioModel) {
                                    ForEach(viewModel.lmStudioModels) { model in
                                        Text(model.name).tag(model.id)
                                    }
                                }
                                .labelsHidden()
                            }

                            Button {
                                viewModel.fetchLMStudioModels()
                            } label: {
                                if viewModel.isFetchingLMStudioModels {
                                    ProgressView()
                                        .controlSize(.small)
                                } else {
                                    Image(systemName: "arrow.clockwise")
                                }
                            }
                            .buttonStyle(.bordered)
                            .controlSize(.small)
                            .disabled(viewModel.isFetchingLMStudioModels)
                            .help("Fetch available models")
                        }
                    }
                }
            } else if viewModel.selectedProvider == .vLLM {
                // vLLM settings
                VStack(alignment: .leading, spacing: 10) {
                    Text("vLLM")
                        .font(.headline)

                    VStack(alignment: .leading, spacing: 4) {
                        Text("Endpoint").font(.caption).foregroundStyle(.secondary)
                        TextField("http://localhost:8000/v1/chat/completions", text: $viewModel.vLLMEndpoint)
                            .textFieldStyle(.roundedBorder)
                    }

                    VStack(alignment: .leading, spacing: 4) {
                        Text("API Key (optional)").font(.caption).foregroundStyle(.secondary)
                        LockedSecureField(text: $viewModel.vLLMAPIKey, placeholder: "Optional", lockKey: "lock.vLLMAPIKey")
                    }

                    VStack(alignment: .leading, spacing: 4) {
                        Text("Model").font(.caption).foregroundStyle(.secondary)
                        HStack {
                            if viewModel.vLLMModels.isEmpty {
                                TextField("Model name", text: $viewModel.vLLMModel)
                                    .textFieldStyle(.roundedBorder)
                            } else {
                                Picker("Model", selection: $viewModel.vLLMModel) {
                                    ForEach(viewModel.vLLMModels) { model in
                                        Text(model.name).tag(model.id)
                                    }
                                }
                                .labelsHidden()
                            }

                            Button {
                                viewModel.fetchVLLMModels()
                            } label: {
                                if viewModel.isFetchingVLLMModels {
                                    ProgressView()
                                        .controlSize(.small)
                                } else {
                                    Image(systemName: "arrow.clockwise")
                                }
                            }
                            .buttonStyle(.bordered)
                            .controlSize(.small)
                            .disabled(viewModel.isFetchingVLLMModels)
                            .help("Fetch available models")
                        }
                    }
                }
            } else {
                // Local Ollama settings
                VStack(alignment: .leading, spacing: 10) {
                    Text("Local Ollama")
                        .font(.headline)

                    VStack(alignment: .leading, spacing: 4) {
                        Text("Endpoint").font(.caption).foregroundStyle(.secondary)
                        TextField("http://localhost:11434/api/chat", text: $viewModel.localOllamaEndpoint)
                            .textFieldStyle(.roundedBorder)
                    }

                    VStack(alignment: .leading, spacing: 4) {
                        Text("Model").font(.caption).foregroundStyle(.secondary)
                        HStack {
                            if viewModel.localOllamaModels.isEmpty {
                                TextField("Model name", text: $viewModel.localOllamaModel)
                                    .textFieldStyle(.roundedBorder)
                            } else {
                                Picker("Model", selection: $viewModel.localOllamaModel) {
                                    ForEach(viewModel.localOllamaModels) { model in
                                        HStack(spacing: 4) {
                                            Text(model.name)
                                            if model.supportsVision {
                                                Image(systemName: "eye")
                                                    .foregroundStyle(.blue)
                                                    .font(.caption2)
                                            }
                                        }
                                        .tag(model.name)
                                    }
                                }
                                .labelsHidden()
                            }

                            Button {
                                viewModel.fetchLocalOllamaModels()
                            } label: {
                                if viewModel.isFetchingLocalModels {
                                    ProgressView()
                                        .controlSize(.small)
                                } else {
                                    Image(systemName: "arrow.clockwise")
                                }
                            }
                            .buttonStyle(.bordered)
                            .controlSize(.small)
                            .disabled(viewModel.isFetchingLocalModels)
                            .help("Fetch available local models")
                        }
                    }

                    VStack(alignment: .leading, spacing: 4) {
                        Text("Context Window").font(.caption).foregroundStyle(.secondary)
                        HStack {
                            TextField("0 = auto", text: Binding(
                                get: { viewModel.localOllamaContextSize == 0 ? "" : "\(viewModel.localOllamaContextSize)" },
                                set: { viewModel.localOllamaContextSize = Int($0) ?? 0 }
                            ))
                            .textFieldStyle(.roundedBorder)
                            .frame(width: 100)

                            Text(
                                viewModel
                                    .localOllamaContextSize == 0 ? "Model default" : "\(viewModel.localOllamaContextSize / 1024)K tokens"
                            )
                            .font(.caption)
                            .foregroundStyle(.secondary)
                        }
                    }
                }
            }

            // Max Output Tokens — for providers that support it
            if viewModel.selectedProvider != .localOllama && viewModel.selectedProvider != .foundationModel {
                Divider()
                VStack(alignment: .leading, spacing: 4) {
                    Text("Max Output Tokens").font(.caption).foregroundStyle(.secondary)
                    HStack {
                        TextField(viewModel.selectedProvider == .claude ? "16384" : "0 = default", text: Binding(
                            get: { viewModel.maxTokens == 0 ? "" : "\(viewModel.maxTokens)" },
                            set: { viewModel.maxTokens = Int($0) ?? 0 }
                        ))
                        .textFieldStyle(.roundedBorder)
                        .frame(width: 100)

                        Text(
                            viewModel
                                .maxTokens == 0 ? (viewModel.selectedProvider == .claude ? "Defaults to 16384" : "Provider default") :
                                "\(viewModel.maxTokens) tokens"
                        )
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    }
                }
            }

            // Temperature — lives with the LLM since it's a per-provider setting
            Divider()
            VStack(alignment: .leading, spacing: 4) {
                HStack {
                    Text("Temperature").font(.caption).foregroundStyle(.secondary)
                    Spacer()
                    Text(viewModel.selectedProvider.displayName).font(.caption).foregroundStyle(.secondary)
                    Text(String(format: "%.1f", llmTemperatureBinding.wrappedValue))
                        .font(.caption.monospacedDigit())
                        .foregroundStyle(viewModel.temperatureColor(llmTemperatureBinding.wrappedValue))
                        .frame(width: 28, alignment: .trailing)
                }
                Slider(value: llmTemperatureBinding, in: 0...2, step: 0.1)
                    .id(viewModel.selectedProvider)
                    .tint(viewModel.temperatureColor(llmTemperatureBinding.wrappedValue))
                    .onAppear {
                        // Force the slider thumb + tint color to redraw on first appear.
                        let current = llmTemperatureBinding.wrappedValue
                        llmTemperatureBinding.wrappedValue = max(0, current - 0.1)
                        DispatchQueue.main.async {
                            llmTemperatureBinding.wrappedValue = min(2, current + 0.1)
                            DispatchQueue.main.async {
                                llmTemperatureBinding.wrappedValue = current
                            }
                        }
                    }
            }

            // Web Search (Tavily) — available for all providers
            VStack(alignment: .leading, spacing: 10) {
                Text("Web Search")
                    .font(.headline)
                Text("Tavily provides web search for all LLM providers.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                VStack(alignment: .leading, spacing: 4) {
                    Text("Tavily API Key").font(.caption).foregroundStyle(.secondary)
                    LockedSecureField(text: $viewModel.tavilyAPIKey, placeholder: "tvly-...", lockKey: "lock.tavilyAPIKey")
                }
            }

            // System Prompts Editor
            Button("Edit System Prompts...") {
                SystemPromptWindow.shared.show()
            }

            Divider()

            HStack {
                Text("Force Vision").font(.caption)
                Spacer()
                Toggle("", isOn: $viewModel.forceVision)
                    .toggleStyle(.switch)
                    .controlSize(.mini)
                    .help("Always send images to LLM, even for non-vision models")
            }

        }
        .padding(16)
        .padding(.bottom, 15)
        .frame(width: 360)
        .onAppear {
            refreshModelsForCurrentProvider()
        }
        .onChange(of: viewModel.selectedProvider) { _, _ in
            refreshModelsForCurrentProvider()
        }
    }

    private func refreshModelsForCurrentProvider() {
        viewModel.fetchModelsIfNeeded(for: viewModel.selectedProvider, force: true)
    }
}

// MARK: - Locked Secure Field

/// A SecureField with a lock/unlock button. When locked, the field is disabled.
/// Lock state persists in UserDefaults via the lockKey.
struct LockedSecureField: View {
    @Binding var text: String
    let placeholder: String
    let lockKey: String
    @State private var isLocked: Bool

    init(text: Binding<String>, placeholder: String, lockKey: String) {
        self._text = text
        self.placeholder = placeholder
        self.lockKey = lockKey
        _isLocked = State(initialValue: UserDefaults.standard.bool(forKey: lockKey))
    }

    var body: some View {
        HStack(spacing: 4) {
            SecureField(placeholder, text: $text)
                .textFieldStyle(.roundedBorder)
                .disabled(isLocked)
                .opacity(isLocked ? 0.6 : 1)

            Button {
                isLocked.toggle()
                UserDefaults.standard.set(isLocked, forKey: lockKey)
            } label: {
                Image(systemName: isLocked ? "lock.fill" : "lock.open")
                    .foregroundStyle(isLocked ? .orange : .secondary)
            }
            .buttonStyle(.bordered)
            .controlSize(.small)
            .help(isLocked ? "Unlock to edit" : "Lock to protect")
        }
    }
}
