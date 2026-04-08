import SwiftUI
import Charts

struct TokenBadge: View {
    let taskIn: Int
    let taskOut: Int
    let sessionIn: Int
    let sessionOut: Int
    var providerName: String = ""
    var modelName: String = ""
    /// Fraction of per-task token budget used (0.0–1.0). 0 = no budget set.
    var budgetUsedFraction: Double = 0

    @State private var showDetail: Bool = false

    var body: some View {
        Button {
            showDetail.toggle()
        } label: {
            let total = TokenUsageStore.shared.todayInput + TokenUsageStore.shared.todayOutput
            HStack(spacing: 4) {
                Text(formatTokens(total))
                    .font(.caption2.monospacedDigit())
                    .foregroundStyle(.secondary)
                if budgetUsedFraction > 0 {
                    Text("\(Int(budgetUsedFraction * 100))%")
                        .font(.caption2.monospacedDigit())
                        .foregroundStyle(budgetUsedFraction >= 0.9 ? .red : budgetUsedFraction >= 0.7 ? .orange : .secondary)
                }
            }
            .padding(.horizontal, 5)
            .padding(.vertical, 2)
            .background(Color.secondary.opacity(0.1))
            .clipShape(Capsule())
        }
        .buttonStyle(.plain)
        .popover(isPresented: $showDetail) {
            TokenDetailView(
                taskIn: taskIn, taskOut: taskOut,
                sessionIn: sessionIn, sessionOut: sessionOut,
                providerName: providerName, modelName: modelName
            )
        }
    }

    private func formatTokens(_ count: Int) -> String {
        if count >= 1_000_000 {
            return String(format: "%.1fM", Double(count) / 1_000_000)
        } else if count >= 1_000 {
            return String(format: "%.1fK", Double(count) / 1_000)
        }
        return "\(count)"
    }
}

// MARK: - Detail Popover

private struct TokenDetailView: View {
    let taskIn: Int
    let taskOut: Int
    let sessionIn: Int
    let sessionOut: Int
    let providerName: String
    let modelName: String

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            // Header
            VStack(alignment: .leading, spacing: 12) {
                Text("Token Usage")
                    .font(.headline)

                Text("Current session breakdown.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            .padding()
            .padding(.bottom, 4)

            Divider()

            // Provider & Model
            if !providerName.isEmpty {
                row {
                    Text("Provider").font(.subheadline)
                    Spacer()
                    Text(providerName)
                        .font(.subheadline.monospaced())
                        .foregroundStyle(.secondary)
                }
            }

            if !modelName.isEmpty {
                row {
                    Text("Model").font(.subheadline)
                    Spacer()
                    Text(shortModel(modelName))
                        .font(.subheadline.monospaced())
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                        .truncationMode(.middle)
                }
            }

            // Task tokens
            row {
                Text("Task").font(.subheadline)
                Spacer()
                VStack(alignment: .trailing, spacing: 4) {
                    tokenBar(label: "↑", value: taskIn, color: .blue, max: max(taskIn, taskOut))
                    tokenBar(label: "↓", value: taskOut, color: .green, max: max(taskIn, taskOut))
                }
            }

            // Session tokens
            row {
                Text("Session").font(.subheadline)
                Spacer()
                VStack(alignment: .trailing, spacing: 4) {
                    tokenBar(label: "↑", value: sessionIn, color: .blue, max: max(sessionIn, sessionOut))
                    tokenBar(label: "↓", value: sessionOut, color: .green, max: max(sessionIn, sessionOut))
                }
            }

            // Today
            let store = TokenUsageStore.shared
            row {
                Text("Today").font(.subheadline)
                Spacer()
                VStack(alignment: .trailing, spacing: 2) {
                    Text("↑ \(fmt(store.todayInput))")
                        .font(.caption.monospacedDigit())
                        .foregroundStyle(.blue)
                    Text("↓ \(fmt(store.todayOutput))")
                        .font(.caption.monospacedDigit())
                        .foregroundStyle(.green)
                    Text("Total: \(fmt(store.todayInput + store.todayOutput))")
                        .font(.caption.monospacedDigit())
                        .foregroundStyle(.secondary)
                }
            }

            // Daily chart
            let recent = store.recentDays(7)
            if !recent.isEmpty {
                VStack(alignment: .leading, spacing: 4) {
                    Divider()
                    Text("Daily Usage (7 days) — tokens are est.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .padding(.horizontal)
                        .padding(.top, 8)

                    Chart {
                        ForEach(recent, id: \.date) { day in
                            LineMark(
                                x: .value("Date", shortDate(day.date)),
                                y: .value("Tokens", day.inputTokens),
                                series: .value("Type", "Sent")
                            )
                            .foregroundStyle(.blue)
                            .interpolationMethod(.catmullRom)

                            LineMark(
                                x: .value("Date", shortDate(day.date)),
                                y: .value("Tokens", day.outputTokens),
                                series: .value("Type", "Received")
                            )
                            .foregroundStyle(.green)
                            .interpolationMethod(.catmullRom)

                            LineMark(
                                x: .value("Date", shortDate(day.date)),
                                y: .value("Tokens", day.cacheReadTokens),
                                series: .value("Type", "Cache")
                            )
                            .foregroundStyle(Color(red: 0.55, green: 0.78, blue: 1.0))
                            .interpolationMethod(.catmullRom)
                        }
                    }
                    .chartYAxis {
                        AxisMarks(position: .leading) { value in
                            AxisValueLabel {
                                if let v = value.as(Int.self) {
                                    Text(fmt(v)).font(.caption2)
                                }
                            }
                            AxisGridLine()
                        }
                    }
                    .chartXAxis {
                        AxisMarks(values: .automatic(desiredCount: 5)) { value in
                            AxisValueLabel {
                                if let s = value.as(String.self) {
                                    Text(s).font(.caption2)
                                }
                            }
                        }
                    }
                    .chartLegend(.hidden)
                    .frame(height: 120)
                    .padding(.horizontal)
                    .padding(.bottom, 8)

                    // Legend
                    HStack(spacing: 12) {
                        HStack(spacing: 4) {
                            Circle().fill(.blue).frame(width: 6, height: 6)
                            Text("↑ Sent").font(.caption2).foregroundStyle(.secondary)
                        }
                        HStack(spacing: 4) {
                            Circle().fill(.green).frame(width: 6, height: 6)
                            Text("↓ Received").font(.caption2).foregroundStyle(.secondary)
                        }
                        HStack(spacing: 4) {
                            Circle().fill(Color(red: 0.55, green: 0.78, blue: 1.0)).frame(width: 6, height: 6)
                            Text("⚡︎ Cache").font(.caption2).foregroundStyle(.secondary)
                        }
                    }
                    .padding(.horizontal)
                    .padding(.bottom, 8)
                }
            }
        }
        .padding(.bottom, 15)
        .frame(width: 320)
    }

    // MARK: - Helpers

    @ViewBuilder
    private func row<Content: View>(@ViewBuilder content: () -> Content) -> some View {
        VStack(spacing: 0) {
            Divider()
            HStack { content() }
                .padding(.vertical, 8)
                .padding(.horizontal)
        }
    }

    @ViewBuilder
    private func tokenBar(label: String, value: Int, color: Color, max: Int) -> some View {
        HStack(spacing: 6) {
            Text(label)
                .font(.caption2)
                .foregroundStyle(.tertiary)
                .frame(width: 24, alignment: .trailing)
            GeometryReader { geo in
                let fraction: CGFloat = max > 0 ? CGFloat(value) / CGFloat(max) : 0
                RoundedRectangle(cornerRadius: 3)
                    .fill(color.opacity(0.5))
                    .frame(width: geo.size.width * fraction)
            }
            .frame(width: 80, height: 8)
            Text(fmt(value))
                .font(.caption2.monospacedDigit())
                .foregroundStyle(.secondary)
                .frame(width: 50, alignment: .trailing)
        }
    }

    private func fmt(_ count: Int) -> String {
        if count >= 1_000_000 {
            return String(format: "%.1fM", Double(count) / 1_000_000)
        } else if count >= 1_000 {
            return String(format: "%.1fK", Double(count) / 1_000)
        }
        return "\(count)"
    }

    private func shortDate(_ dateStr: String) -> String {
        // "2026-03-29" → "Mar 29"
        let parts = dateStr.split(separator: "-")
        guard parts.count == 3, let day = Int(parts[2]) else { return dateStr }
        let months = ["", "Jan","Feb","Mar","Apr","May","Jun","Jul","Aug","Sep","Oct","Nov","Dec"]
        let month = Int(parts[1]) ?? 0
        return month > 0 && month <= 12 ? "\(months[month]) \(day)" : dateStr
    }

    private func shortModel(_ model: String) -> String {
        // Trim long model IDs like "claude-sonnet-4-20250514" to "claude-sonnet-4"
        let parts: [String] = model.components(separatedBy: "-")
        if parts.count > 3, let last = parts.last, last.count == 8, Int(last) != nil {
            return parts.dropLast().joined(separator: "-")
        }
        return model
    }
}
