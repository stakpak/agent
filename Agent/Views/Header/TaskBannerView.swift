//  TaskBannerView.swift Agent  Extracted from ContentView.swift 

import SwiftUI

/// Green banner showing current task with cancel button and optional Apple AI prompt
struct TaskBannerView: View {
    let prompt: String
    let appleAIPrompt: String?
    @Binding var showAppleAIBanner: Bool
    let onCancel: () -> Void

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 6) {
                Button {
                    if appleAIPrompt != nil {
                        withAnimation(.easeInOut(duration: 0.2)) {
                            showAppleAIBanner.toggle()
                        }
                    }
                } label: {
                    Image(systemName: "person.fill")
                        .font(.caption2)
                        .frame(width: 14)
                        .foregroundStyle(.white)
                }
                .buttonStyle(.plain)
                .help("User prompt")

                Text(prompt)
                    .font(.caption)
                    .lineLimit(1)
                    .truncationMode(.tail)
                    .foregroundStyle(.white)

                Spacer()

                Button(action: onCancel) {
                    Label("Cancel", systemImage: "xmark.circle.fill")
                        .font(.caption)
                        .foregroundStyle(.white)
                }
                .buttonStyle(.plain)
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 5)
            .background(Color.green.opacity(0.7))

            // Apple AI prompt row (toggled by tapping person icon)
            if showAppleAIBanner, let aiPrompt = appleAIPrompt {
                HStack(spacing: 6) {
                    Text("\u{F8FF}")
                        .font(.caption2)
                        .frame(width: 14)
                        .foregroundStyle(.white.opacity(0.8))
                    Text(aiPrompt)
                        .font(.caption)
                        .lineLimit(4)
                        .foregroundStyle(.white.opacity(0.9))
                    Spacer()
                }
                .padding(.horizontal, 12)
                .padding(.vertical, 4)
                .background(Color.blue.opacity(0.6))
                .transition(.move(edge: .top).combined(with: .opacity))
            }
        }
    }
}
