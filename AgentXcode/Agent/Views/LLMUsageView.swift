import SwiftUI

/// Per-model token usage popover with cost estimates.
struct LLMUsageView: View {
    @Bindable var viewModel: AgentViewModel

    private var store: TokenUsageStore { TokenUsageStore.shared }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            // Header
            VStack(alignment: .leading, spacing: 8) {
                HStack {
                    Text("LLM Usage")
                        .font(.headline)
                    Spacer()
                    if !store.modelUsage.isEmpty {
                        Button("Reset") {
                            store.resetModelUsage()
                            store.resetCacheMetrics()
                        }
                        .font(.caption)
                        .buttonStyle(.plain)
                        .foregroundStyle(.secondary)
                    }
                }
                Text("Token usage per model this session.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            .padding()
            .padding(.bottom, 4)

            if store.modelUsage.isEmpty {
                VStack(spacing: 8) {
                    Divider()
                    Text("No LLM calls yet.")
                        .font(.caption)
                        .foregroundStyle(.tertiary)
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 20)
                }
            } else {
                let sorted = store.modelUsage.sorted { $0.value.totalTokens > $1.value.totalTokens }
                let maxTokens = sorted.first?.value.totalTokens ?? 1

                ForEach(sorted, id: \.key) { model, usage in
                    VStack(spacing: 0) {
                        Divider()
                        HStack(spacing: 8) {
                            VStack(alignment: .leading, spacing: 2) {
                                Text(shortModel(model))
                                    .font(.subheadline.weight(.medium))
                                    .lineLimit(1)
                                    .truncationMode(.middle)
                                Text("\(usage.callCount) call\(usage.callCount == 1 ? "" : "s")")
                                    .font(.caption2)
                                    .foregroundStyle(.tertiary)
                            }
                            .frame(width: 120, alignment: .leading)

                            VStack(alignment: .leading, spacing: 4) {
                                // Input bar
                                HStack(spacing: 4) {
                                    Text("↑")
                                        .font(.caption2)
                                        .foregroundStyle(.blue)
                                        .frame(width: 12)
                                    GeometryReader { geo in
                                        let frac = CGFloat(usage.inputTokens) / CGFloat(max(maxTokens, 1))
                                        RoundedRectangle(cornerRadius: 3)
                                            .fill(Color.blue.opacity(0.6))
                                            .frame(width: geo.size.width * frac)
                                    }
                                    .frame(height: 8)
                                    Text(fmt(usage.inputTokens))
                                        .font(.caption2.monospacedDigit())
                                        .foregroundStyle(.secondary)
                                        .frame(width: 45, alignment: .trailing)
                                }
                                // Output bar
                                HStack(spacing: 4) {
                                    Text("↓")
                                        .font(.caption2)
                                        .foregroundStyle(.green)
                                        .frame(width: 12)
                                    GeometryReader { geo in
                                        let frac = CGFloat(usage.outputTokens) / CGFloat(max(maxTokens, 1))
                                        RoundedRectangle(cornerRadius: 3)
                                            .fill(Color.green.opacity(0.6))
                                            .frame(width: geo.size.width * frac)
                                    }
                                    .frame(height: 8)
                                    Text(fmt(usage.outputTokens))
                                        .font(.caption2.monospacedDigit())
                                        .foregroundStyle(.secondary)
                                        .frame(width: 45, alignment: .trailing)
                                }
                            }

                            // Cost
                            let cost = store.estimatedCost(model: model, inputTokens: usage.inputTokens, outputTokens: usage.outputTokens)
                            if cost > 0 {
                                Text(String(format: "$%.3f", cost))
                                    .font(.caption.monospacedDigit())
                                    .foregroundStyle(.orange)
                                    .frame(width: 50, alignment: .trailing)
                            } else {
                                Text("free")
                                    .font(.caption)
                                    .foregroundStyle(.tertiary)
                                    .frame(width: 50, alignment: .trailing)
                            }
                        }
                        .padding(.vertical, 8)
                        .padding(.horizontal)
                    }
                }

                // Totals
                let totalCost = store.sessionEstimatedCost
                VStack(spacing: 0) {
                    Divider()
                    HStack {
                        Text("Total")
                            .font(.subheadline.weight(.semibold))
                        Spacer()
                        let totalIn = store.modelUsage.values.reduce(0) { $0 + $1.inputTokens }
                        let totalOut = store.modelUsage.values.reduce(0) { $0 + $1.outputTokens }
                        HStack(spacing: 8) {
                            Text("↑ \(fmt(totalIn))")
                                .font(.caption.monospacedDigit())
                                .foregroundStyle(.blue)
                            Text("↓ \(fmt(totalOut))")
                                .font(.caption.monospacedDigit())
                                .foregroundStyle(.green)
                            if totalCost > 0 {
                                Text(String(format: "$%.3f", totalCost))
                                    .font(.caption.monospacedDigit().weight(.semibold))
                                    .foregroundStyle(.orange)
                            }
                        }
                    }
                    .padding(.vertical, 8)
                    .padding(.horizontal)
                }

                // Cache metrics
                if store.sessionCacheReadTokens > 0 || store.sessionCacheCreationTokens > 0 {
                    VStack(spacing: 0) {
                        Divider()
                        HStack {
                            Text("Cache")
                                .font(.subheadline)
                            Spacer()
                            HStack(spacing: 8) {
                                Text("Hit: \(fmt(store.sessionCacheReadTokens))")
                                    .font(.caption.monospacedDigit())
                                    .foregroundStyle(.cyan)
                                Text("Miss: \(fmt(store.sessionCacheCreationTokens))")
                                    .font(.caption.monospacedDigit())
                                    .foregroundStyle(.secondary)
                                Text("\(store.cacheHitRate)%")
                                    .font(.caption.monospacedDigit().weight(.medium))
                                    .foregroundStyle(store.cacheHitRate > 70 ? .green : store.cacheHitRate > 30 ? .yellow : .red)
                            }
                        }
                        .padding(.vertical, 8)
                        .padding(.horizontal)
                    }
                }
            }
        }
        .padding(.bottom, 15)
        .frame(width: 400)
    }

    private func fmt(_ count: Int) -> String {
        if count >= 1_000_000 { return String(format: "%.1fM", Double(count) / 1_000_000) }
        if count >= 1_000 { return String(format: "%.1fK", Double(count) / 1_000) }
        return "\(count)"
    }

    private func shortModel(_ model: String) -> String {
        let parts = model.components(separatedBy: "-")
        if parts.count > 3, let last = parts.last, last.count == 8, Int(last) != nil {
            return parts.dropLast().joined(separator: "-")
        }
        return model
    }
}
