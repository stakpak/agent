//  SearchBarView.swift Agent  Extracted from ContentView.swift 

import SwiftUI

struct SearchBarView: View {
    @Binding var searchText: String
    @Binding var caseSensitive: Bool
    let totalMatches: Int
    let currentMatchIndex: Int
    let previousMatch: () -> Void
    let nextMatch: () -> Void
    let onClose: () -> Void

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 4) {
                Image(systemName: "magnifyingglass")
                    .foregroundStyle(.secondary)

                Button { caseSensitive.toggle() } label: {
                    Text("Aa")
                        .font(.system(size: 12, weight: caseSensitive ? .bold : .regular))
                        .foregroundStyle(caseSensitive ? .blue : .secondary)
                        .frame(width: 24, height: 14)
                }
                .buttonStyle(.bordered)
                .clipShape(Capsule())
                .controlSize(.small)
                .disabled(searchText.isEmpty)
                .help(caseSensitive ? "Case Sensitive: ON" : "Case Sensitive: OFF")

                Button { previousMatch() } label: {
                    Image(systemName: "chevron.up")
                        .frame(height: 14)
                }
                .buttonStyle(.bordered)
                .clipShape(Capsule())
                .controlSize(.small)
                .disabled(searchText.isEmpty || totalMatches == 0)

                Button { nextMatch() } label: {
                    Image(systemName: "chevron.down")
                        .frame(height: 14)
                }
                .buttonStyle(.bordered)
                .clipShape(Capsule())
                .controlSize(.small)
                .disabled(searchText.isEmpty || totalMatches == 0)

                Button { onClose() } label: {
                    Image(systemName: "xmark.circle.fill")
                        .foregroundStyle(.secondary)
                        .frame(height: 14)
                }
                .buttonStyle(.bordered)
                .clipShape(Capsule())
                .controlSize(.small)

                Text(searchText.isEmpty ? "" : (totalMatches > 0 ? "\(currentMatchIndex + 1)/\(totalMatches)" : "0 results"))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .frame(minWidth: 50)

                TextField("Find in log...", text: $searchText)
                    .textFieldStyle(.roundedBorder)
                    .controlSize(.small)
                    .onSubmit { nextMatch() }
            }
            .padding(.horizontal)
            .padding(.vertical, 6)
            Divider()
        }
    }
}
