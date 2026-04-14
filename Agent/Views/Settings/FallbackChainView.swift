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
                                    if AgentViewModel.isVisionModel(entry.model) {
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
                        let models = modelOptionsForProvider(selectedProvider)
                        if models.isEmpty {
                            let def = defaultModel(for: selectedProvider)
                            Text(shortModel(def)).tag(def)
                        } else {
                            ForEach(models, id: \.id) { model in
                                HStack(spacing: 4) {
                                    Text(shortModel(model.display))
                                    if AgentViewModel.isVisionModel(model.id) {
                                        Image(systemName: "eye")
                                            .foregroundStyle(.blue)
                                            .font(.caption2)
                                    }
                                }.tag(model.id)
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

    /// (id, display) pairs for the model picker. `id` is stored/sent to API,
    /// `display` is shown in the UI (e.g. Z.ai coding models show `-Code` suffix).
    private func modelOptionsForProvider(_ provider: APIProvider) -> [(id: String, display: String)] {
        func oai(_ list: [AgentViewModel.OpenAIModelInfo]) -> [(id: String, display: String)] {
            list.map { ($0.id, $0.name) }
        }
        switch provider {
        case .claude: return viewModel.availableClaudeModels.map { ($0.id, $0.formattedDisplayName) }
        case .openAI: return oai(viewModel.openAIModels)
        case .ollama: return viewModel.ollamaModels.map { ($0.name, $0.name) }
        case .localOllama: return viewModel.localOllamaModels.map { ($0.name, $0.name) }
        case .deepSeek: return oai(viewModel.deepSeekModels)
        case .huggingFace: return oai(viewModel.huggingFaceModels)
        case .vLLM: return oai(viewModel.vLLMModels)
        case .lmStudio: return oai(viewModel.lmStudioModels)
        case .zAI: return oai(viewModel.zAIModels)
        case .qwen: return oai(viewModel.qwenModels)
        case .gemini: return oai(viewModel.geminiModels)
        case .grok: return oai(viewModel.grokModels)
        case .mistral: return oai(viewModel.mistralModels)
        default: return []
        }
    }

    private func modelsForProvider(_ provider: APIProvider) -> [String] {
        modelOptionsForProvider(provider).map(\.id)
    }

    private func shortModel(_ model: String) -> String {
        let clean = model.replacingOccurrences(of: ":v", with: "")
        let parts = clean.components(separatedBy: "-")
        if parts.count > 3, let last = parts.last, last.count == 8, Int(last) != nil {
            return parts.dropLast().joined(separator: "-")
        }
        return clean
    }

    /// / Default model for a provider — uses the user's currently-selected model for that / provider (read from the
    /// ViewModel), falling back to the first dynamically-fetched / model. Never hardcoded — model strings change frequently across provider updates.
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
