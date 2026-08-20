// Screen corner frame — L-brackets (starship) or block sprites (pixel).
// Exports: ScreenFrame.

import SwiftUI

struct ScreenFrame: View {
    @Environment(\.theme) private var theme

    var body: some View {
        GeometryReader { geo in
            ZStack {
                if theme.motion.stepped {
                    blockCorners(in: geo.size)
                } else {
                    bracketCorners(in: geo.size)
                }
            }
        }
        .allowsHitTesting(false)
    }

    private func bracketCorners(in size: CGSize) -> some View {
        let len: CGFloat = 30
        let w = theme.hairline * 2
        return ZStack {
            cornerL(at: CGPoint(x: 0, y: 0), len: len, w: w, flipX: false, flipY: false)
            cornerL(at: CGPoint(x: size.width, y: 0), len: len, w: w, flipX: true, flipY: false)
            cornerL(at: CGPoint(x: 0, y: size.height), len: len, w: w, flipX: false, flipY: true)
            cornerL(at: CGPoint(x: size.width, y: size.height), len: len, w: w, flipX: true, flipY: true)
        }
    }

    private func cornerL(at origin: CGPoint, len: CGFloat, w: CGFloat, flipX: Bool, flipY: Bool) -> some View {
        Path { path in
            let dx: CGFloat = flipX ? -1 : 1
            let dy: CGFloat = flipY ? -1 : 1
            path.move(to: origin)
            path.addLine(to: CGPoint(x: origin.x + dx * len, y: origin.y))
            path.move(to: origin)
            path.addLine(to: CGPoint(x: origin.x, y: origin.y + dy * len))
        }
        .stroke(theme.accent, lineWidth: w)
    }

    private func blockCorners(in size: CGSize) -> some View {
        let s: CGFloat = 8
        return ZStack {
            block(at: CGPoint(x: 0, y: 0))
            block(at: CGPoint(x: size.width - s, y: 0))
            block(at: CGPoint(x: 0, y: size.height - s))
            block(at: CGPoint(x: size.width - s, y: size.height - s))
        }
    }

    private func block(at origin: CGPoint) -> some View {
        Rectangle()
            .fill(theme.accent)
            .frame(width: 8, height: 8)
            .position(x: origin.x + 4, y: origin.y + 4)
    }
}
