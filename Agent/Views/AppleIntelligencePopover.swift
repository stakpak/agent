import SwiftUI

struct AppleIntelligencePopover: View {
    @ObservedObject private var aiMediator = AppleIntelligenceMediator.shared

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            mediatorSection
        }
        .padding(16)
        .frame(width: 380)
    }

    // MARK: - Mediator Section

    private var mediatorSection: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Apple Intelligence Mediator")
                .font(.headline)

            HStack(spacing: 6) {
                Circle()
                    .fill(AppleIntelligenceMediator.isAvailable ? Color.green : Color.red.opacity(0.6))
                    .frame(width: 8, height: 8)
                Text(AppleIntelligenceMediator.isAvailable ? "Available" : "Not Available")
                    .font(.caption)
                    .foregroundStyle(AppleIntelligenceMediator.isAvailable ? .green : .secondary)
            }

            if !AppleIntelligenceMediator.isAvailable {
                Text(AppleIntelligenceMediator.unavailabilityReason)
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }

            Grid(alignment: .leading, verticalSpacing: 8) {
                GridRow {
                    VStack(alignment: .leading) {
                        Text("Enable Mediator")
                            .font(.caption)
                        Text("Chat code completions using Apple AI")
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                    }
                    Toggle("", isOn: $aiMediator.isEnabled)
                        .toggleStyle(.switch)
                        .controlSize(.mini)
                        .labelsHidden()
                }

                if aiMediator.isEnabled {
                    GridRow {
                        VStack(alignment: .leading) {
                            Text("Show annotations to user")
                                .font(.caption)
                            Text("Display [\u{F8FF}AI → ...] flow tags in activity log")
                                .font(.caption2)
                                .foregroundStyle(.secondary)
                        }
                        Toggle("", isOn: $aiMediator.showAnnotationsToUser)
                            .toggleStyle(.switch)
                            .controlSize(.mini)
                            .labelsHidden()
                    }

                    GridRow {
                        VStack(alignment: .leading) {
                            Text("Inject context into LLM prompts")
                                .font(.caption)
                            Text("Adds rephrased context to LLM prompts")
                                .font(.caption2)
                                .foregroundStyle(.secondary)
                        }
                        Toggle("", isOn: $aiMediator.injectContextToLLM)
                            .toggleStyle(.switch)
                            .controlSize(.mini)
                            .labelsHidden()
                    }
                }
            }
        }
    }
}
