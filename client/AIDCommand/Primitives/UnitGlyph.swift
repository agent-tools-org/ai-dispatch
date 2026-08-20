// Agent and drive glyphs — SVG paths ported to SwiftUI Path.
// Exports: UnitGlyph, DriveGlyph.

import SwiftUI

struct UnitGlyph: View {
    @Environment(\.theme) private var theme
    let agent: String
    var size: CGFloat = 24

    var body: some View {
        agentPath
            .stroke(theme.ink, style: StrokeStyle(lineWidth: 1.7, lineCap: .round, lineJoin: .round))
            .frame(width: size, height: size)
            .modifier(GlyphQuantizer(enabled: theme.motion.stepped))
    }

    private var agentPath: Path {
        switch agent.lowercased() {
        case "codex":
            return path("M9 4 L3 12 L9 20 M15 4 L21 12 L15 20 M12 7 L12 17")
        case "cursor":
            return path("M5 3 L19 11 L12 12.5 L9.5 20 Z")
        case "grok":
            return path("M3 12 C7 7 17 7 21 12 C17 17 7 17 3 12 Z M12 9.6 A2.4 2.4 0 1 0 12 14.4 A2.4 2.4 0 1 0 12 9.6")
        case "gemini":
            return path("M12 2 L14 10 L22 12 L14 14 L12 22 L10 14 L2 12 L10 10 Z")
        case "opencode":
            return path("M12 3 L20 7.5 L20 16.5 L12 21 L4 16.5 L4 7.5 Z M12 8.5 L15.5 10.5 L15.5 14 L12 16 L8.5 14 L8.5 10.5 Z")
        case "kilo":
            return path("M4 19 L12 5 L20 19 M8.5 19 L12 12 L15.5 19")
        default:
            return path("M12 3 L20 7.5 L20 16.5 L12 21 L4 16.5 L4 7.5 Z")
        }
    }

    private func path(_ d: String) -> Path {
        SVGPathParser.parse(d, in: CGRect(x: 0, y: 0, width: 24, height: 24))
    }
}

struct DriveGlyph: View {
    @Environment(\.theme) private var theme
    let model: String?
    var size: CGFloat = 14

    var body: some View {
        Group {
            if let model {
                drivePath(for: model)
            } else {
                Text("—")
                    .font(theme.font(.caption))
                    .foregroundStyle(theme.ink3)
            }
        }
        .frame(width: size, height: size)
        .modifier(GlyphQuantizer(enabled: theme.motion.stepped))
    }

    @ViewBuilder
    private func drivePath(for model: String) -> some View {
        let lower = model.lowercased()
        if lower.contains("gpt") {
            hexagon.stroke(theme.ink2, lineWidth: 1.2)
        } else if lower.contains("grok") {
            arrow.stroke(theme.ink2, lineWidth: 1.2)
        } else if lower.contains("gemini") {
            sparkle.stroke(theme.ink2, lineWidth: 1.2)
        } else if lower.contains("glm") {
            bars.stroke(theme.ink2, lineWidth: 1.2)
        } else if lower.contains("composer") {
            ringedDot
        } else if lower == "auto" {
            Circle().stroke(theme.ink2, lineWidth: 1.2)
        } else {
            Rectangle().stroke(theme.ink2, lineWidth: 1.2)
        }
    }

    private var hexagon: Path {
        path("M12 2 L20 7 L20 17 L12 22 L4 17 L4 7 Z")
    }

    private var arrow: Path {
        path("M4 12 L20 4 L16 20 Z")
    }

    private var sparkle: Path {
        path("M12 2 L13 9 L20 12 L13 15 L12 22 L11 15 L4 12 L11 9 Z")
    }

    private var bars: Path {
        path("M4 6 L4 18 M9 4 L9 20 M14 8 L14 16")
    }

    private var ringedDot: some View {
        ZStack {
            Circle().stroke(theme.ink2, lineWidth: 1)
            Circle().fill(theme.ink2).scaleEffect(0.35)
        }
    }

    private func path(_ d: String) -> Path {
        SVGPathParser.parse(d, in: CGRect(x: 0, y: 0, width: 24, height: 24))
    }
}

private struct GlyphQuantizer: ViewModifier {
    let enabled: Bool

    func body(content: Content) -> some View {
        if enabled {
            content.drawingGroup()
        } else {
            content
        }
    }
}
