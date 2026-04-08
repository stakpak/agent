import SwiftUI
import UniformTypeIdentifiers

struct InputSectionView: View {
    @Bindable var viewModel: AgentViewModel
    @FocusState.Binding var isTaskFieldFocused: Bool
    var selectedTab: ScriptTab?
    @State private var showSuggestions = false
    @State private var selectedSuggestionIndex = 0
    @State private var hoveredSuggestionIndex = -1

    var body: some View {
        if let tab = selectedTab {
            // Tab input
            HStack {
                inputButtons

                VStack(spacing: 4) {
                    if tab.isBusy {
                        Button {
                            if tab.isLLMRunning {
                                viewModel.stopTabTask(tab: tab)
                            } else if tab.isRunning {
                                viewModel.cancelScriptTab(id: tab.id)
                            }
                        } label: {
                            Image(systemName: "xmark.circle.fill")
                                .foregroundStyle(.red)
                                .frame(width: 36)
                        }
                        .buttonStyle(.bordered)
                        .clipShape(Capsule())
                        .controlSize(.small)
                        .help("Cancel tab task")
                        .accessibilityLabel("Cancel task")
                    } else {
                        Button { tab.taskInput = "" } label: {
                            Image(systemName: "xmark.circle.fill")
                                .foregroundStyle(.secondary)
                                .frame(width: 36)
                        }
                        .buttonStyle(.bordered)
                        .clipShape(Capsule())
                        .controlSize(.small)
                        .help("Clear task input")
                        .accessibilityLabel("Clear input")
                        .disabled(tab.taskInput.isEmpty)
                    }

                    Button { viewModel.runTabTask(tab: tab) } label: {
                        Image(systemName: "play.fill")
                            .foregroundStyle(.white)
                            .frame(width: 36)
                    }
                    .buttonStyle(.borderedProminent)
                    .clipShape(Capsule())
                    .controlSize(.small)
                    .accessibilityLabel("Run task")
                    .disabled(tab.taskInput.isEmpty || {
                        let provider = tab.llmConfig?.provider ?? viewModel.selectedProvider
                        return provider == .claude && viewModel.apiKey.isEmpty
                    }())
                }

                TextField(
                    tab.isMainTab ? "Enter task..." : tab.isMessagesTab ? "Messages task..." : "Ask about \(tab.scriptName)...",
                    text: Binding(
                        get: { tab.taskInput },
                        set: { tab.taskInput = $0 }
                    ),
                    axis: .vertical
                )
                .textFieldStyle(.plain)
                // Match Terminal Neo's 16.5pt — same visual size as the LLM output buffer
                .font(.system(size: 16.5))
                .padding(.vertical, 5)
                .padding(.horizontal, 7)
                .background(Color(nsColor: .controlBackgroundColor))
                .clipShape(RoundedRectangle(cornerRadius: 12))
                .overlay(RoundedRectangle(cornerRadius: 12).stroke(Color.gray.opacity(0.4), lineWidth: 1))
                .lineLimit(2...16)
                .background(GeometryReader { geo in
                    Color.clear.onChange(of: geo.size.width, initial: true) { _, w in
                        viewModel.inputFieldWidth = w - 14 // minus horizontal padding
                    }
                })
                .onKeyPress(.tab) {
                    if showSuggestions && !suggestions.isEmpty {
                        let idx = min(selectedSuggestionIndex, suggestions.count - 1)
                        tab.taskInput = suggestions[idx]
                        showSuggestions = false
                        return .handled
                    }
                    return .ignored
                }
                .onKeyPress(.escape) {
                    if showSuggestions {
                        showSuggestions = false
                        return .handled
                    }
                    return .ignored
                }
                .onChange(of: tab.taskInput) { _, newValue in
                    selectedSuggestionIndex = 0
                    withAnimation(.easeInOut(duration: 0.15)) {
                        showSuggestions = viewModel.taskAutoComplete && !newValue.isEmpty && !suggestions.isEmpty
                    }
                }
                .onSubmit {
                    showSuggestions = false
                    if !tab.taskInput.isEmpty {
                        viewModel.runTabTask(tab: tab)
                    }
                }
            }
            .padding(.vertical, 8)
            .padding(.horizontal, 14)
            .background(Color(nsColor: .windowBackgroundColor))
            .overlay(alignment: .bottom) {
                suggestionsDropdown
                    .offset(y: -55)
            }
            .onDrop(of: [.fileURL, .text], isTargeted: nil) { providers in
                handleDrop(providers, tab: tab)
            }
        } else {
            // Main tab input
            HStack {
                inputButtons

                VStack(spacing: 4) {
                    if viewModel.isRunning || viewModel.isThinking {
                        Button { viewModel.stop() } label: {
                            Image(systemName: "xmark.circle.fill")
                                .foregroundStyle(.red)
                                .frame(width: 36)
                        }
                        .buttonStyle(.bordered)
                        .clipShape(Capsule())
                        .controlSize(.small)
                        .help("Cancel running task")
                        .accessibilityLabel("Cancel task")
                    } else {
                        Button { viewModel.taskInput = "" } label: {
                            Image(systemName: "xmark.circle.fill")
                                .foregroundStyle(.secondary)
                                .frame(width: 36)
                        }
                        .buttonStyle(.bordered)
                        .clipShape(Capsule())
                        .controlSize(.small)
                        .help("Clear task input")
                        .accessibilityLabel("Clear input")
                        .disabled(viewModel.taskInput.isEmpty)
                    }

                    Button { viewModel.run() } label: {
                        Image(systemName: "play.fill")
                            .foregroundStyle(.white)
                            .frame(width: 36)
                    }
                    .buttonStyle(.borderedProminent)
                    .clipShape(Capsule())
                    .controlSize(.small)
                    .accessibilityLabel("Run task")
                    .disabled(viewModel.taskInput.isEmpty || (viewModel.selectedProvider == .claude && viewModel.apiKey.isEmpty))
                }

                TextField("Enter task...", text: $viewModel.taskInput, axis: .vertical)
                    .focused($isTaskFieldFocused)
                    .textFieldStyle(.plain)
                    // Match Terminal Neo's 16.5pt — same visual size as the LLM output buffer
                    .font(.system(size: 16.5))
                    .padding(.vertical, 5)
                    .padding(.horizontal, 7)
                    .background(Color(nsColor: .controlBackgroundColor))
                    .clipShape(RoundedRectangle(cornerRadius: 12))
                    .overlay(RoundedRectangle(cornerRadius: 12).stroke(Color.gray.opacity(0.4), lineWidth: 1))
                    .lineLimit(2...16)
                    .background(GeometryReader { geo in
                        Color.clear.onChange(of: geo.size.width, initial: true) { _, w in
                            viewModel.inputFieldWidth = w - 14
                        }
                    })
                    .onKeyPress(.tab) {
                        if showSuggestions && !suggestions.isEmpty {
                            let idx = min(selectedSuggestionIndex, suggestions.count - 1)
                            viewModel.taskInput = suggestions[idx]
                            showSuggestions = false
                            return .handled
                        }
                        return .ignored
                    }
                    .onKeyPress(.escape) {
                        if showSuggestions {
                            showSuggestions = false
                            return .handled
                        }
                        return .ignored
                    }
                    .onChange(of: viewModel.taskInput) { _, newValue in
                        selectedSuggestionIndex = 0
                        withAnimation(.easeInOut(duration: 0.15)) {
                            showSuggestions = viewModel.taskAutoComplete && !newValue.isEmpty && !suggestions.isEmpty
                        }
                    }
                    .onSubmit {
                        showSuggestions = false
                        if !viewModel.taskInput.isEmpty {
                            viewModel.run()
                        }
                    }
            }
            .padding(.vertical, 8)
            .padding(.horizontal, 14)
            .background(Color(nsColor: .windowBackgroundColor))
            .overlay(alignment: .bottom) {
                suggestionsDropdown
                    .offset(y: -55)
            }
            .onDrop(of: [.fileURL, .text], isTargeted: nil) { providers in
                handleDrop(providers, tab: nil)
            }
        }
    }

    // MARK: - Current Input Binding

    private var currentInput: Binding<String> {
        if let tab = selectedTab {
            return Binding(get: { tab.taskInput }, set: { tab.taskInput = $0 })
        }
        return $viewModel.taskInput
    }

    // MARK: - Suggestions

    private var suggestions: [String] {
        let query = currentInput.wrappedValue.lowercased()
        guard !query.isEmpty else { return [] }
        let history = viewModel.currentTabPromptHistory
        let currentValue = currentInput.wrappedValue
        let matches = history.reversed().filter {
            $0.lowercased().contains(query) && $0.lowercased() != currentValue.lowercased()
        }
        // Deduplicate preserving order
        var seen = Set<String>()
        return matches.filter { seen.insert($0).inserted }.prefix(6).map { $0 }
    }

    @ViewBuilder
    private var suggestionsDropdown: some View {
        let items = suggestions
        if showSuggestions && !items.isEmpty {
            VStack(alignment: .leading, spacing: 0) {
                // Dismiss bar — tap anywhere on it to close
                Button {
                    showSuggestions = false
                } label: {
                    HStack {
                        Image(systemName: "xmark.circle.fill")
                            .font(.caption)
                        Text("Dismiss")
                            .font(.caption2)
                        Spacer()
                    }
                    .foregroundStyle(.red.opacity(0.6))
                    .padding(.horizontal, 8)
                    .padding(.vertical, 4)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                ForEach(Array(items.enumerated()), id: \.offset) { idx, suggestion in
                    Button {
                        currentInput.wrappedValue = suggestion
                        showSuggestions = false
                    } label: {
                        HStack(spacing: 6) {
                            Image(systemName: "clock.arrow.circlepath")
                                .font(.caption2)
                                .foregroundStyle(.secondary)
                                .frame(width: 14)
                            Text(suggestion)
                                .font(.system(size: 11))
                                .lineLimit(1)
                                .truncationMode(.tail)
                            Spacer()
                            if idx == selectedSuggestionIndex {
                                Text("Tab")
                                    .font(.caption2)
                                    .foregroundStyle(.tertiary)
                                    .padding(.horizontal, 4)
                                    .padding(.vertical, 1)
                                    .background(Color.secondary.opacity(0.15))
                                    .cornerRadius(3)
                            }
                        }
                        .padding(.horizontal, 8)
                        .padding(.vertical, 5)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .background(
                        hoveredSuggestionIndex == idx ? Color.blue.opacity(0.2) :
                            idx == selectedSuggestionIndex ? Color.accentColor.opacity(0.15) : Color.clear
                    )
                    .onHover { hovering in
                        hoveredSuggestionIndex = hovering ? idx : -1
                    }
                }
            }
            .background(Color(nsColor: .controlBackgroundColor))
            .cornerRadius(8)
            .overlay(RoundedRectangle(cornerRadius: 8).stroke(Color.gray.opacity(0.3), lineWidth: 1))
            .shadow(radius: 4)
            .padding(.horizontal, 50)
            .transition(.opacity.combined(with: .move(edge: .bottom)))
        }
    }

    /// Handle drag-and-drop of text files into the input area.
    /// Works regardless of whether the text field has focus.
    private func handleDrop(_ providers: [NSItemProvider], tab: ScriptTab? = nil) -> Bool {
        for provider in providers {
            // File URLs — read text content
            if provider.hasItemConformingToTypeIdentifier(UTType.fileURL.identifier) {
                provider.loadItem(forTypeIdentifier: UTType.fileURL.identifier, options: nil) { data, _ in
                    guard let urlData = data as? Data,
                          let url = URL(dataRepresentation: urlData, relativeTo: nil) else { return }
                    // Read text-based files
                    guard let content = try? String(contentsOfFile: url.path, encoding: .utf8) else { return }
                    let filename = url.lastPathComponent
                    let dropped = "[\(filename)]\n\(content)"
                    DispatchQueue.main.async {
                        if let tab {
                            tab.taskInput += (tab.taskInput.isEmpty ? "" : " ") + dropped
                        } else {
                            viewModel.taskInput += (viewModel.taskInput.isEmpty ? "" : " ") + dropped
                            isTaskFieldFocused = true
                        }
                    }
                }
                return true
            }
            // Plain text
            if provider.hasItemConformingToTypeIdentifier(UTType.text.identifier) {
                provider.loadItem(forTypeIdentifier: UTType.text.identifier, options: nil) { data, _ in
                    guard let text = data as? String, !text.isEmpty else { return }
                    DispatchQueue.main.async {
                        if let tab {
                            tab.taskInput += (tab.taskInput.isEmpty ? "" : " ") + text
                        } else {
                            viewModel.taskInput += (viewModel.taskInput.isEmpty ? "" : " ") + text
                            isTaskFieldFocused = true
                        }
                    }
                }
                return true
            }
        }
        return false
    }

    private var inputButtons: some View {
        let buttonWidth: CGFloat = 36
        return VStack(spacing: 4) {
            HStack(spacing: 4) {
                Button { viewModel.captureScreenshot() } label: {
                    Image(systemName: "camera")
                        .frame(width: buttonWidth)
                }
                .buttonStyle(.bordered)
                .clipShape(Capsule())
                .controlSize(.small)
                .help("Take a screenshot to attach")
                .accessibilityLabel("Screenshot")

                Button { viewModel.pasteImageFromClipboard() } label: {
                    Image(systemName: "photo.on.rectangle.angled")
                        .frame(width: buttonWidth)
                }
                .buttonStyle(.bordered)
                .clipShape(Capsule())
                .controlSize(.small)
                .help("Paste image from clipboard")
                .accessibilityLabel("Paste image")
            }
            HStack(spacing: 4) {
                Button { viewModel.toggleDictation() } label: {
                    Image(systemName: viewModel.isListening ? "mic.fill" : "mic")
                        .foregroundStyle(viewModel.isListening ? Color.blue : .primary)
                        .frame(width: buttonWidth)
                }
                .buttonStyle(.bordered)
                .clipShape(Capsule())
                .controlSize(.small)
                .help(viewModel.isListening ? "Stop dictation" : "Start dictation")
                .accessibilityLabel("Dictation")
                .accessibilityValue(viewModel.isListening ? "Recording" : "Off")

                Button { viewModel.toggleHotwordListening() } label: {
                    Image(systemName: viewModel.isHotwordListening ? "waveform.circle.fill" : "waveform.circle")
                        .foregroundStyle(
                            viewModel.isHotwordListening
                                ? (viewModel.isHotwordCapturing ? Color.green : Color.orange)
                                : .primary
                        )
                        .frame(width: buttonWidth)
                }
                .buttonStyle(.bordered)
                .clipShape(Capsule())
                .controlSize(.small)
                .help(
                    viewModel.isHotwordListening
                        ? (viewModel.isHotwordCapturing ? "Capturing command..." : "Listening for \"Agent!\" — click to stop")
                        : "Say \"Agent!\" to send a voice command"
                )
                .accessibilityLabel("Hotword")
                .accessibilityValue(viewModel.isHotwordListening ? (viewModel.isHotwordCapturing ? "Capturing" : "Listening") : "Off")
            }
        }
    }

    private static let tabColors: [Color] = [
        .orange, .purple, .pink, .cyan, .mint, .indigo, .teal, .yellow
    ]

    static func tabColor(for tabId: UUID, in tabs: [ScriptTab]) -> Color {
        guard let index = tabs.firstIndex(where: { $0.id == tabId }) else { return .red }
        return tabColors[index % tabColors.count]
    }
}
