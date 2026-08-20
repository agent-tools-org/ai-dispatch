// Diagonal hazard stripe fill for empty hangar bays.
// Exports: HazardStripes.

import SwiftUI

struct HazardStripes: View {
    @Environment(\.theme) private var theme

    var body: some View {
        Canvas { context, size in
            let stripeWidth: CGFloat = theme.motion.stepped ? 6 : 8
            let gap: CGFloat = theme.motion.stepped ? 6 : 8
            var x: CGFloat = -size.height
            while x < size.width + size.height {
                var path = Path()
                path.move(to: CGPoint(x: x, y: size.height))
                path.addLine(to: CGPoint(x: x + size.height, y: 0))
                path.addLine(to: CGPoint(x: x + size.height + stripeWidth, y: 0))
                path.addLine(to: CGPoint(x: x + stripeWidth, y: size.height))
                path.closeSubpath()
                context.fill(path, with: .color(theme.ink3.opacity(0.18)))
                x += stripeWidth + gap
            }
        }
        .allowsHitTesting(false)
    }
}
