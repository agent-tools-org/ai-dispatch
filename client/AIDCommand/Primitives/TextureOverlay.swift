// Theme texture overlays — scanlines, dither, accent grid, sweep.
// Exports: TextureOverlay.

import SwiftUI

struct TextureOverlay: View {
    @Environment(\.theme) private var theme
    @State private var sweepOffset: CGFloat = 0
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        GeometryReader { geo in
            ZStack {
                switch theme.overlay {
                case .scanline:
                    scanlines(in: geo.size)
                    accentGrid(in: geo.size)
                    if !reduceMotion { sweep(in: geo.size) }
                case .dither:
                    dither(in: geo.size)
                case .none:
                    EmptyView()
                }
            }
        }
        .allowsHitTesting(false)
        .onAppear { startSweep() }
    }

    private func scanlines(in size: CGSize) -> some View {
        Canvas { context, canvasSize in
            var y: CGFloat = 0
            while y < canvasSize.height {
                let rect = CGRect(x: 0, y: y, width: canvasSize.width, height: 1)
                context.fill(Path(rect), with: .color(.black.opacity(0.22)))
                y += 4
            }
        }
    }

    private func accentGrid(in size: CGSize) -> some View {
        Canvas { context, canvasSize in
            let step: CGFloat = 72
            var x: CGFloat = 0
            while x < canvasSize.width {
                var y: CGFloat = 0
                while y < canvasSize.height {
                    let rect = CGRect(x: x, y: y, width: 1, height: 1)
                    context.fill(Path(rect), with: .color(theme.accent.opacity(0.06)))
                    y += step
                }
                x += step
            }
        }
    }

    private func sweep(in size: CGSize) -> some View {
        LinearGradient(
            colors: [.clear, theme.accent.opacity(0.06), .clear],
            startPoint: .top,
            endPoint: .bottom
        )
        .frame(height: 80)
        .offset(y: sweepOffset)
        .frame(height: size.height, alignment: .top)
        .clipped()
    }

    private func dither(in size: CGSize) -> some View {
        Canvas { context, canvasSize in
            var y: CGFloat = 0
            while y < canvasSize.height {
                var x: CGFloat = (Int(y) / 2) % 2 == 0 ? 0 : 2
                while x < canvasSize.width {
                    let rect = CGRect(x: x, y: y, width: 2, height: 2)
                    context.fill(Path(rect), with: .color(.white.opacity(0.06)))
                    x += 4
                }
                y += 2
            }
        }
    }

    private func startSweep() {
        guard theme.overlay == .scanline, !reduceMotion else { return }
        withAnimation(.linear(duration: 7).repeatForever(autoreverses: false)) {
            sweepOffset = 900
        }
    }
}
