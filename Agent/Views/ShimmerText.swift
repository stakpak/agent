import SwiftUI

/// Text with a pulse animation matching the LLM icon throb effect.
struct ShimmerText: View {
    let text: String
    let color: Color
    @State private var dimmed = false

    init(_ text: String, color: Color = .blue) {
        self.text = text
        self.color = color
    }

    var body: some View {
        Text(text)
            .font(.caption)
            .foregroundStyle(color)
            .opacity(dimmed ? 0.65 : 1.0)
            .onAppear {
                withAnimation(.easeInOut(duration: 1.0).repeatForever(autoreverses: true)) {
                    dimmed = true
                }
            }
    }
}
