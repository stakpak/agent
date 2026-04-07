import SwiftUI

struct AgentOptionsView: View {
    @Bindable var viewModel: AgentViewModel

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            // Header
            VStack(alignment: .leading, spacing: 12) {
                Text("Options")
                    .font(.headline)

                Text("Configure agent behavior and limits.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            .padding(.bottom, 8)

            row {
                Text("Iterations").font(.subheadline)
                Spacer()
                VStack(alignment: .trailing, spacing: 2) {
                    Text("Max per task").font(.caption).foregroundStyle(.secondary)
                    Stepper(
                        "\(viewModel.maxIterations)",
                        onIncrement: {
                            let opts = AgentViewModel.iterationOptions
                            if let i = opts.firstIndex(of: viewModel.maxIterations), i > 0 {
                                viewModel.maxIterations = opts[i - 1]
                            }
                        },
                        onDecrement: {
                            let opts = AgentViewModel.iterationOptions
                            if let i = opts.firstIndex(of: viewModel.maxIterations), i + 1 < opts.count {
                                viewModel.maxIterations = opts[i + 1]
                            }
                        }
                    )
                }
            }

            row {
                Text("Retries").font(.subheadline)
                Spacer()
                VStack(alignment: .trailing, spacing: 2) {
                    Text("On server/timeout errors").font(.caption).foregroundStyle(.secondary)
                    Stepper(
                        "\(viewModel.maxRetries)",
                        onIncrement: {
                            let opts = AgentViewModel.retryOptions
                            if let i = opts.firstIndex(of: viewModel.maxRetries), i + 1 < opts.count {
                                viewModel.maxRetries = opts[i + 1]
                            }
                        },
                        onDecrement: {
                            let opts = AgentViewModel.retryOptions
                            if let i = opts.firstIndex(of: viewModel.maxRetries), i > 0 {
                                viewModel.maxRetries = opts[i - 1]
                            }
                        }
                    )
                }
            }

            row {
                Text("Network Retry").font(.subheadline)
                Spacer()
                VStack(alignment: .trailing, spacing: 2) {
                    Text("Seconds between retries").font(.caption).foregroundStyle(.secondary)
                    Picker("", selection: $viewModel.networkRetryDelay) {
                        ForEach([10, 20, 30, 40, 50, 60], id: \.self) { sec in
                            Text("\(sec)s").tag(sec)
                        }
                    }
                    .labelsHidden()
                    .frame(width: 80)
                }
            }

            row {
                Text("Output").font(.subheadline)
                Spacer()
                VStack(alignment: .trailing, spacing: 2) {
                    Text("Lines before truncated").font(.caption).foregroundStyle(.secondary)
                    Stepper(
                        "\(viewModel.maxOutputLines)",
                        onIncrement: {
                            let opts = AgentViewModel.outputLineOptions
                            if let i = opts.firstIndex(of: viewModel.maxOutputLines), i > 0 {
                                viewModel.maxOutputLines = opts[i - 1]
                            }
                        },
                        onDecrement: {
                            let opts = AgentViewModel.outputLineOptions
                            if let i = opts.firstIndex(of: viewModel.maxOutputLines), i + 1 < opts.count {
                                viewModel.maxOutputLines = opts[i + 1]
                            }
                        }
                    )
                }
            }

            row {
                Text("Read File").font(.subheadline)
                Spacer()
                VStack(alignment: .trailing, spacing: 2) {
                    Text("Preview lines").font(.caption).foregroundStyle(.secondary)
                    Stepper(
                        "\(viewModel.readFilePreviewLines)",
                        onIncrement: {
                            let opts = AgentViewModel.readPreviewOptions
                            if let i = opts.firstIndex(of: viewModel.readFilePreviewLines), i > 0 {
                                viewModel.readFilePreviewLines = opts[i - 1]
                            }
                        },
                        onDecrement: {
                            let opts = AgentViewModel.readPreviewOptions
                            if let i = opts.firstIndex(of: viewModel.readFilePreviewLines), i + 1 < opts.count {
                                viewModel.readFilePreviewLines = opts[i + 1]
                            }
                        }
                    )
                }
            }


            row {
                Text("AgentScript").font(.subheadline)
                Spacer()
                Toggle("Capture stderr", isOn: $viewModel.scriptCaptureStderr)
                    .toggleStyle(.switch)
                    .controlSize(.mini)
            }

            row {
                Text("Task Input").font(.subheadline)
                Spacer()
                Toggle("Autocomplete", isOn: $viewModel.taskAutoComplete)
                    .toggleStyle(.switch)
                    .controlSize(.mini)
            }

            row {
                Text("History").font(.subheadline)
                Spacer()
                VStack(alignment: .trailing, spacing: 2) {
                    Text("Summarize after").font(.caption).foregroundStyle(.secondary)
                    Stepper(
                        "\(viewModel.maxHistoryBeforeSummary) tasks",
                        onIncrement: { if viewModel.maxHistoryBeforeSummary > 5 { viewModel.maxHistoryBeforeSummary -= 5 } },
                        onDecrement: { if viewModel.maxHistoryBeforeSummary < 50 { viewModel.maxHistoryBeforeSummary += 5 } }
                    )
                }
                Spacer().frame(width: 12)
                VStack(alignment: .trailing, spacing: 2) {
                    Text("Visible in chat").font(.caption).foregroundStyle(.secondary)
                    Stepper(
                        "\(viewModel.visibleTaskCount)",
                        onIncrement: { if viewModel.visibleTaskCount > 1 { viewModel.visibleTaskCount -= 1 } },
                        onDecrement: { if viewModel.visibleTaskCount < 5 { viewModel.visibleTaskCount += 1 } }
                    )
                }
            }

            row {
                Text("Shell").font(.subheadline)
                Spacer()
                Picker("", selection: Binding(
                    get: { AppConstants.shellPath },
                    set: { UserDefaults.standard.set($0, forKey: "agentShellPath") }
                )) {
                    Text("zsh").tag("/bin/zsh")
                    Text("bash").tag("/bin/bash")
                }
                .pickerStyle(.segmented)
                .frame(width: 120)
            }

            row {
                Text("Token Budget").font(.subheadline)
                Spacer()
                HStack(spacing: 8) {
                    Toggle("", isOn: Binding(
                        get: { viewModel.tokenBudgetCeiling > 0 },
                        set: { viewModel.tokenBudgetCeiling = $0 ? 100_000 : 0 }
                    ))
                    .toggleStyle(.switch)
                    .controlSize(.mini)
                    .labelsHidden()

                    if viewModel.tokenBudgetCeiling > 0 {
                        Stepper(
                            Self.formatBudget(viewModel.tokenBudgetCeiling),
                            onIncrement: {
                                let opts = Self.budgetOptions
                                if let i = opts.firstIndex(of: viewModel.tokenBudgetCeiling), i > 0 {
                                    viewModel.tokenBudgetCeiling = opts[i - 1]
                                } else if let prev = opts.last(where: { $0 < viewModel.tokenBudgetCeiling }) {
                                    viewModel.tokenBudgetCeiling = prev
                                }
                            },
                            onDecrement: {
                                let opts = Self.budgetOptions
                                if let i = opts.firstIndex(of: viewModel.tokenBudgetCeiling), i + 1 < opts.count {
                                    viewModel.tokenBudgetCeiling = opts[i + 1]
                                } else if let next = opts.first(where: { $0 > viewModel.tokenBudgetCeiling }) {
                                    viewModel.tokenBudgetCeiling = next
                                }
                            }
                        )
                        Button {
                            viewModel.budgetUsedFraction = 0
                        } label: {
                            Image(systemName: "arrow.counterclockwise")
                                .font(.caption2)
                        }
                        .buttonStyle(.plain)
                        .help("Reset token usage for current task")
                    } else {
                        Text("Unlimited")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }
            }
        }
        .padding(16)
        .padding(.bottom, 15)
        .frame(width: 360)
    }

    private static let budgetOptions = [
        10_000, 25_000, 50_000, 100_000, 250_000, 500_000,
        1_000_000, 2_000_000, 3_000_000, 4_000_000, 5_000_000,
        6_000_000, 7_000_000, 8_000_000, 9_000_000, 10_000_000
    ]

    private static func formatBudget(_ value: Int) -> String {
        if value >= 1_000_000 {
            let m = Double(value) / 1_000_000
            return m == Double(Int(m)) ? "\(Int(m))M" : String(format: "%.1fM", m)
        }
        return "\(value / 1000)K"
    }

    @ViewBuilder
    private func row<Content: View>(@ViewBuilder content: () -> Content) -> some View {
        VStack(spacing: 0) {
            Divider()
            HStack { content() }
                .padding(.vertical, 10)
        }
    }
}
