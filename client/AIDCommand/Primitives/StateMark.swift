// Canvas-drawn mission state marks — star, cross, play, pause.
// Exports: StateMark.

import SwiftUI

struct StateMark: View {
    @Environment(\.theme) private var theme
    let state: MissionDisplayState
    var size: CGFloat = 13

    var body: some View {
        markPath
            .stroke(color, style: StrokeStyle(lineWidth: strokeWidth, lineCap: .round, lineJoin: .round))
            .frame(width: size, height: size)
            .modifier(GlyphQuantizer(enabled: theme.motion.stepped))
    }

    private var color: Color {
        switch state {
        case .run: return theme.run
        case .done: return theme.done
        case .fail: return theme.fail
        case .stop: return theme.stop
        }
    }

    private var strokeWidth: CGFloat {
        theme.motion.stepped ? 2 : 1.5
    }

    private var markPath: Path {
        let box = CGRect(x: 0, y: 0, width: size, height: size)
        switch state {
        case .run:
            return play(in: box)
        case .done:
            return star(in: box)
        case .fail:
            return cross(in: box)
        case .stop:
            return pause(in: box)
        }
    }

    private func play(in box: CGRect) -> Path {
        Path { path in
            let inset = box.width * 0.18
            path.move(to: CGPoint(x: box.minX + inset, y: box.minY + inset))
            path.addLine(to: CGPoint(x: box.maxX - inset, y: box.midY))
            path.addLine(to: CGPoint(x: box.minX + inset, y: box.maxY - inset))
            path.closeSubpath()
        }
    }

    private func star(in box: CGRect) -> Path {
        Path { path in
            let cx = box.midX
            let cy = box.midY
            let outer = box.width * 0.46
            let inner = box.width * 0.14
            path.move(to: CGPoint(x: cx, y: cy - outer))
            path.addLine(to: CGPoint(x: cx + inner, y: cy - inner))
            path.addLine(to: CGPoint(x: cx + outer, y: cy))
            path.addLine(to: CGPoint(x: cx + inner, y: cy + inner))
            path.addLine(to: CGPoint(x: cx, y: cy + outer))
            path.addLine(to: CGPoint(x: cx - inner, y: cy + inner))
            path.addLine(to: CGPoint(x: cx - outer, y: cy))
            path.addLine(to: CGPoint(x: cx - inner, y: cy - inner))
            path.closeSubpath()
        }
    }

    private func cross(in box: CGRect) -> Path {
        Path { path in
            let inset = box.width * 0.22
            path.move(to: CGPoint(x: box.minX + inset, y: box.minY + inset))
            path.addLine(to: CGPoint(x: box.maxX - inset, y: box.maxY - inset))
            path.move(to: CGPoint(x: box.maxX - inset, y: box.minY + inset))
            path.addLine(to: CGPoint(x: box.minX + inset, y: box.maxY - inset))
        }
    }

    private func pause(in box: CGRect) -> Path {
        Path { path in
            let barW = box.width * 0.22
            let inset = box.width * 0.18
            let top = box.minY + inset
            let bottom = box.maxY - inset
            path.addRect(CGRect(x: box.minX + inset, y: top, width: barW, height: bottom - top))
            path.addRect(CGRect(x: box.maxX - inset - barW, y: top, width: barW, height: bottom - top))
        }
    }
}
