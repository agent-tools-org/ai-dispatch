// Status lamp — round pulse (starship) or square blink (pixel).
// Exports: StatusLamp.

import SwiftUI

struct StatusLamp: View {
    @Environment(\.theme) private var theme
    let color: Color
    var active: Bool = true

    @State private var phase: Bool = false
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        Group {
            if theme.motion.stepped {
                Rectangle()
                    .fill(active && phase ? color : color.opacity(0.25))
                    .frame(width: 6, height: 6)
            } else {
                Circle()
                    .fill(color.opacity(active ? (phase ? 1 : 0.55) : 0.3))
                    .frame(width: 8, height: 8)
                    .shadow(color: color.opacity(0.5), radius: active ? 4 : 0)
            }
        }
        .onAppear { startPulse() }
        .onChange(of: active) { _, _ in startPulse() }
    }

    private func startPulse() {
        guard active, !reduceMotion else {
            phase = true
            return
        }
        let duration = theme.motion.stepped ? 0.25 : 1.4
        withAnimation(
            theme.motion.stepped
                ? .linear(duration: duration).repeatForever(autoreverses: true)
                : .easeInOut(duration: duration).repeatForever(autoreverses: true)
        ) {
            phase.toggle()
        }
    }
}
