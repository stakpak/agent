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
                        Text("Triage greetings, summarize tasks, explain errors via on-device Apple AI")
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
                            Text("Display task summaries and error explanations in the activity log")
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
                            Text("Token compression")
                                .font(.caption)
                            Text("Tier 1 of context compaction — Apple AI summarizes old messages on-device when context exceeds 30K tokens. Free, private, no API tokens consumed")
                                .font(.caption2)
                                .foregroundStyle(.secondary)
                        }
                        Toggle("", isOn: $aiMediator.tokenCompressionEnabled)
                            .toggleStyle(.switch)
                            .controlSize(.mini)
                            .labelsHidden()
                    }

                    GridRow {
                        VStack(alignment: .leading) {
                            Text("Accessibility intent parsing")
                                .font(.caption)
                            Text("Parse \"click the Save button in TextEdit\" locally and dispatch directly to the accessibility tool — skips the cloud LLM round-trip")
                                .font(.caption2)
                                .foregroundStyle(.secondary)
                        }
                        Toggle("", isOn: $aiMediator.accessibilityIntentEnabled)
                            .toggleStyle(.switch)
                            .controlSize(.mini)
                            .labelsHidden()
                    }
                }
            }
        }
    }
}
