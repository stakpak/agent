import SwiftUI

/// Per-model token usage popover with cost estimates.
/// Scope picker: All tabs (aggregate), Current tab, or any individual tab that produced usage this session.
struct LLMUsageView: View {
    @Bindable var viewModel: AgentViewModel

    private var store: TokenUsageStore { TokenUsageStore.shared }

    enum Scope: Hashable {
        case all
        case current
        case tab(UUID)
    }

    @State private var scope: Scope = .current

    /// Usage dictionary selected by the current scope.
    private var scopedUsage: [String: TokenUsageStore.ModelUsage] {
        switch scope {
        case .all:
            return store.modelUsage
        case .current:
            let key = viewModel.selectedTabId ?? TokenUsageStore.mainTabKey
            return store.tabModelUsage[key] ?? [:]
        case .tab(let id):
            return store.tabModelUsage[id] ?? [:]
        }
    }

    /// Tabs that have recorded usage this session, sorted by label. Main first.
    private var tabsWithUsage: [(id: UUID, label: String)] {
        let main = TokenUsageStore.mainTabKey
        return store.tabModelUsage.keys.compactMap { id -> (UUID, String)? in
            guard !(store.tabModelUsage[id]?.isEmpty ?? true) else { return nil }
            let label = store.tabLabel[id] ?? (id == main ? "Main" : id.uuidString.prefix(6).description)
            return (id, label)
        }
        .sorted { lhs, rhs in
            if lhs.0 == main { return true }
            if rhs.0 == main { return false }
            return lhs.1.localizedCaseInsensitiveCompare(rhs.1) == .orderedAscending
        }
        .map { (id: $0.0, label: $0.1) }
    }

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

                scopePicker
            }
            .padding()
            .padding(.bottom, 4)

            let usage = scopedUsage
            if usage.isEmpty {
                VStack(spacing: 8) {
                    Divider()
                    Text(emptyMessage)
                        .font(.caption)
                        .foregroundStyle(.tertiary)
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 20)
                }
            } else {
                let sorted = usage.sorted { $0.value.totalTokens > $1.value.totalTokens }
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
                            .help(store.modelProvider[model].map { "Provider: \($0)\nModel: \(model)" } ?? model)

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
                VStack(spacing: 0) {
                    Divider()
                    HStack {
                        Text("Total")
                            .font(.subheadline.weight(.semibold))
                        Spacer()
                        let totalIn = usage.values.reduce(0) { $0 + $1.inputTokens }
                        let totalOut = usage.values.reduce(0) { $0 + $1.outputTokens }
                        let totalCost = usage.reduce(0.0) { acc, entry in
                            acc + store.estimatedCost(model: entry.key, inputTokens: entry.value.inputTokens, outputTokens: entry.value.outputTokens)
                        }
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

                // Cache metrics (session-wide, not scoped per tab yet)
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
        .frame(width: 420)
    }

    @ViewBuilder
    private var scopePicker: some View {
        let tabs = tabsWithUsage
        if tabs.count > 1 || (tabs.count == 1 && scope == .all) {
            Picker("Scope", selection: $scope) {
                Text("Current tab").tag(Scope.current)
                Text("All tabs").tag(Scope.all)
                Divider()
                ForEach(tabs, id: \.id) { tab in
                    Text(tab.label).tag(Scope.tab(tab.id))
                }
            }
            .labelsHidden()
            .pickerStyle(.menu)
        }
    }

    private var emptyMessage: String {
        switch scope {
        case .all: return "No LLM calls yet."
        case .current: return "No LLM calls on this tab yet."
        case .tab: return "No LLM calls recorded for this tab."
        }
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
