//
//  ScreenshotPreviewView.swift
//  Agent
//
//  Extracted from ContentView.swift
//

import SwiftUI

struct ScreenshotPreviewView: View {
    let images: [NSImage]
    let onRemove: (Int) -> Void
    let onRemoveAll: () -> Void
    
    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                ForEach(Array(images.enumerated()), id: \.offset) { index, image in
                    ZStack(alignment: .topTrailing) {
                        Image(nsImage: image)
                            .resizable()
                            .aspectRatio(contentMode: .fit)
                            .frame(maxHeight: 70)
                            .clipShape(RoundedRectangle(cornerRadius: 6))
                            .overlay(
                                RoundedRectangle(cornerRadius: 6)
                                    .stroke(.secondary.opacity(0.3))
                            )
                        Button {
                            onRemove(index)
                        } label: {
                            Image(systemName: "xmark.circle.fill")
                                .font(.caption)
                                .foregroundStyle(.white, .red)
                        }
                        .buttonStyle(.plain)
                        .offset(x: 4, y: -4)
                    }
                }
                Text("\(images.count) image(s)")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Button("Clear All") { onRemoveAll() }
                    .buttonStyle(.bordered)
                    .controlSize(.mini)
            }
            .padding(.horizontal)
            .padding(.vertical, 6)
        }
    }
}