// Segment bar — thin glowing segments (starship) or chunky blocks (pixel).
// Exports: SegmentBar.

import SwiftUI

struct SegmentBar: View {
    @Environment(\.theme) private var theme
    let segments: [Color]
    var count: Int = 14
    var progress: Double?

    @State private var blink = false
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        HStack(spacing: theme.motion.stepped ? 2 : 3) {
            ForEach(0..<count, id: \.self) { index in
                segment(at: index)
            }
        }
        .fixedSize(horizontal: true, vertical: false)
        .onAppear { startBlink() }
    }

    @ViewBuilder
    private func segment(at index: Int) -> some View {
        let color = index < segments.count ? segments[index] : theme.ink3.opacity(0.25)
        let isLeading = index == leadingIndex
        if theme.motion.stepped {
            Rectangle()
                .fill(isLeading && blink ? color : color.opacity(isLeading ? 0.6 : 1))
                .frame(width: 8, height: 8)
        } else {
            RoundedRectangle(cornerRadius: 1)
                .fill(color)
                .frame(width: 6, height: 4)
                .shadow(color: isLeading ? color.opacity(0.8) : .clear, radius: 3)
        }
    }

    private var leadingIndex: Int? {
        guard let progress else { return nil }
        return min(count - 1, Int(progress * Double(count)))
    }

    private func startBlink() {
        guard theme.motion.stepped, !reduceMotion else { return }
        withAnimation(.linear(duration: 0.25).repeatForever(autoreverses: true)) {
            blink.toggle()
        }
    }
}
