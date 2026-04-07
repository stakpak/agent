import SwiftUI
import AgentTools

/// Editor for the model fallback chain — users pick ordered provider/model pairs.
struct FallbackChainView: View {
    @Bindable var viewModel: AgentViewModel
    @State private var selectedProvider: APIProvider
    @State private var selectedModel: String = ""

    init(viewModel: AgentViewModel) {
        self.viewModel = viewModel
        // Default the picker to the user's currently-active provider — never hard-code.
        _selectedProvider = State(initialValue: viewModel.selectedProvider)
    }

    private var service: FallbackChainService { FallbackChainService.shared }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            // Header
            VStack(alignment: .leading, spacing: 8) {
                HStack {
                    Text("Fallback Chain")
                        .font(.headline)
                    Spacer()
                    Toggle("", isOn: Binding(
                        get: { service.enabled },
                        set: { service.enabled = $0 }
                    ))
                    .toggleStyle(.switch)
                    .controlSize(.mini)
                    .labelsHidden()
                }
                Text("When the primary LLM fails 3 times, auto-switch to the next provider. Drag to reorder.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            .padding()

            Divider()

            // Chain entries
            if service.chain.isEmpty {
                VStack(spacing: 8) {
                    Text("No fallback providers configured.")
                        .font(.caption)
                        .foregroundStyle(.tertiary)
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 20)
                }
            } else {
                ForEach(Array(service.chain.enumerated()), id: \.element.id) { index, entry in
                    VStack(spacing: 0) {
                        Divider()
                        HStack(spacing: 8) {
                            Text("\(index + 1).")
                                .font(.caption.monospacedDigit())
                                .foregroundStyle(.secondary)
                                .frame(width: 20)

                            VStack(alignment: .leading, spacing: 1) {
                                Text(APIProvider(rawValue: entry.provider)?.displayName ?? entry.provider)
                                    .font(.subheadline.weight(.medium))
                                HStack(spacing: 4) {
                                    Text(shortModel(entry.model))
                                        .font(.caption)
                                        .foregroundStyle(.secondary)
                                        .lineLimit(1)
                                        .truncationMode(.middle)
                                    if entry.model.hasSuffix(":v") {
                                        Image(systemName: "eye")
                                            .foregroundStyle(.blue)
                                            .font(.caption2)
                                    }
                                }
                            }

                            Spacer()

                            // Active indicator
                            if service.currentIndex == index {
                                Text("active")
                                    .font(.caption2)
                                    .foregroundStyle(.green)
                                    .padding(.horizontal, 6)
                                    .padding(.vertical, 2)
                                    .background(Color.green.opacity(0.15))
                                    .clipShape(Capsule())
                            }

                            Toggle("", isOn: Binding(
                                get: { entry.enabled },
                                set: { _ in service.toggle(id: entry.id) }
                            ))
                            .toggleStyle(.switch)
                            .controlSize(.mini)
                            .labelsHidden()

                            Button {
                                service.remove(id: entry.id)
                            } label: {
                                Image(systemName: "minus.circle.fill")
                                    .foregroundStyle(.red.opacity(0.7))
                            }
                            .buttonStyle(.plain)
                        }
                        .padding(.vertical, 6)
                        .padding(.horizontal)
                    }
                }
            }

            // Add new entry
            VStack(spacing: 0) {
                Divider()
                HStack(spacing: 8) {
                    Picker("", selection: $selectedProvider) {
                        ForEach(APIProvider.selectableProviders, id: \.self) { p in
                            Text(p.displayName).tag(p)
                        }
                    }
                    .labelsHidden()
                    .frame(width: 110)
                    .onAppear { viewModel.fetchModelsIfNeeded(for: selectedProvider) }
                    .onChange(of: selectedProvider) { _, newP in
                        viewModel.fetchModelsIfNeeded(for: newP)
                        selectedModel = modelsForProvider(newP).first ?? defaultModel(for: newP)
                    }

                    Picker("", selection: $selectedModel) {
                        let models = modelsForProvider(selectedProvider)
                        if models.isEmpty {
                            // Show default as only option until models load
                            let def = defaultModel(for: selectedProvider)
                            Text(shortModel(def)).tag(def)
                        } else {
                            ForEach(models, id: \.self) { model in
                                HStack(spacing: 4) {
                                    Text(shortModel(model))
                                    if model.hasSuffix(":v") {
                                        Image(systemName: "eye")
                                            .foregroundStyle(.blue)
                                            .font(.caption2)
                                    }
                                }.tag(model)
                            }
                        }
                    }
                    .labelsHidden()
                    .frame(width: 160)
                    .onAppear {
                        if selectedModel
                            .isEmpty { selectedModel = modelsForProvider(selectedProvider).first ?? defaultModel(for: selectedProvider) } }

                    Button {
                        guard !selectedModel.isEmpty else { return }
                        service.add(provider: selectedProvider.rawValue, model: selectedModel)
                        selectedModel = ""
                    } label: {
                        Image(systemName: "plus.circle.fill")
                            .foregroundStyle(.green)
                    }
                    .buttonStyle(.plain)
                }
                .padding(.vertical, 8)
                .padding(.horizontal)
            }

            // Footer
            if !service.chain.isEmpty {
                VStack(spacing: 0) {
                    Divider()
                    HStack {
                        Text("\(service.chain.filter(\.enabled).count) of \(service.chain.count) enabled")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                        Spacer()
                        Button("Clear All") {
                            service.clear()
                        }
                        .font(.caption)
                        .buttonStyle(.plain)
                        .foregroundStyle(.red.opacity(0.7))
                    }
                    .padding(.vertical, 6)
                    .padding(.horizontal)
                }
            }
        }
        .padding(.bottom, 15)
        .frame(width: 380)
    }

    /// Get available models for a provider from the ViewModel's fetched lists.
    private func modelsForProvider(_ provider: APIProvider) -> [String] {
        switch provider {
        case .claude: return viewModel.availableClaudeModels.map(\.id)
        case .openAI: return viewModel.openAIModels.map(\.id)
        case .ollama: return viewModel.ollamaModels.map(\.name)
        case .localOllama: return viewModel.localOllamaModels.map(\.name)
        case .deepSeek: return viewModel.deepSeekModels.map(\.id)
        case .huggingFace: return viewModel.huggingFaceModels.map(\.id)
        case .vLLM: return viewModel.vLLMModels.map(\.id)
        case .lmStudio: return viewModel.lmStudioModels.map(\.id)
        case .zAI: return viewModel.zAIModels.map(\.id)
        case .qwen: return viewModel.qwenModels.map(\.id)
        case .gemini: return viewModel.geminiModels.map(\.id)
        case .grok: return viewModel.grokModels.map(\.id)
        case .mistral: return viewModel.mistralModels.map(\.id)
        default: return []
        }
    }

    private func shortModel(_ model: String) -> String {
        let clean = model.replacingOccurrences(of: ":v", with: "")
        let parts = clean.components(separatedBy: "-")
        if parts.count > 3, let last = parts.last, last.count == 8, Int(last) != nil {
            return parts.dropLast().joined(separator: "-")
        }
        return clean
    }

    /// Default model for a provider — uses the user's currently-selected model for that
    /// provider (read from the ViewModel), falling back to the first dynamically-fetched
    /// model. Never hardcoded — model strings change frequently across provider updates.
    private func defaultModel(for provider: APIProvider) -> String {
        // Prefer the model the user is actively using for that provider
        switch provider {
        case .claude: if !viewModel.selectedModel.isEmpty { return viewModel.selectedModel }
        case .openAI: if !viewModel.openAIModel.isEmpty { return viewModel.openAIModel }
        case .ollama: if !viewModel.ollamaModel.isEmpty { return viewModel.ollamaModel }
        case .localOllama: if !viewModel.localOllamaModel.isEmpty { return viewModel.localOllamaModel }
        case .deepSeek: if !viewModel.deepSeekModel.isEmpty { return viewModel.deepSeekModel }
        case .huggingFace: if !viewModel.huggingFaceModel.isEmpty { return viewModel.huggingFaceModel }
        case .vLLM: if !viewModel.vLLMModel.isEmpty { return viewModel.vLLMModel }
        case .lmStudio: if !viewModel.lmStudioModel.isEmpty { return viewModel.lmStudioModel }
        case .zAI: if !viewModel.zAIModel.isEmpty { return viewModel.zAIModel }
        case .qwen: if !viewModel.qwenModel.isEmpty { return viewModel.qwenModel }
        case .gemini: if !viewModel.geminiModel.isEmpty { return viewModel.geminiModel }
        case .grok: if !viewModel.grokModel.isEmpty { return viewModel.grokModel }
        case .mistral: if !viewModel.mistralModel.isEmpty { return viewModel.mistralModel }
        case .codestral: if !viewModel.codestralModel.isEmpty { return viewModel.codestralModel }
        case .vibe: if !viewModel.vibeModel.isEmpty { return viewModel.vibeModel }
        case .bigModel: if !viewModel.bigModelModel.isEmpty { return viewModel.bigModelModel }
        case .foundationModel: return "Apple Intelligence"
        }
        // Fall back to the first dynamically-fetched model for this provider
        return modelsForProvider(provider).first ?? ""
    }
}
