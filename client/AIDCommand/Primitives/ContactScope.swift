// Tactical scan scope — circular radar (starship) or blocky minimap (pixel).
// Exports: ContactScope.

import SwiftUI

struct ContactScope: View {
    @Environment(\.theme) private var theme
    let missions: [Mission]

    var body: some View {
        VStack(alignment: .leading, spacing: theme.spacing.sm) {
            MonoLabel(text: "\(missions.count) contacts")
            ZStack {
                if theme.motion.stepped {
                    blockScope
                } else {
                    radarScope
                }
            }
            .frame(width: 128, height: 128)
        }
    }

    private var radarScope: some View {
        ZStack {
            Circle()
                .stroke(theme.panelEdge, lineWidth: theme.hairline)
            Circle()
                .stroke(theme.panelEdge.opacity(0.5), lineWidth: theme.hairline)
                .scaleEffect(0.66)
            Circle()
                .stroke(theme.panelEdge.opacity(0.3), lineWidth: theme.hairline)
                .scaleEffect(0.33)
            ForEach(missions) { mission in
                blip(for: mission)
            }
        }
    }

    private var blockScope: some View {
        ZStack {
            Rectangle()
                .stroke(theme.panelEdge, lineWidth: theme.hairline)
            Path { path in
                path.move(to: CGPoint(x: 0, y: 64))
                path.addLine(to: CGPoint(x: 128, y: 64))
                path.move(to: CGPoint(x: 64, y: 0))
                path.addLine(to: CGPoint(x: 64, y: 128))
            }
            .stroke(theme.panelEdge.opacity(0.5), lineWidth: theme.hairline)
            ForEach(missions) { mission in
                blip(for: mission)
            }
        }
    }

    @ViewBuilder
    private func blip(for mission: Mission) -> some View {
        let placement = blipPlacement(for: mission.id, progress: mission.progress)
        let color = blipColor(for: mission.state)
        if theme.motion.stepped {
            Rectangle()
                .fill(color)
                .frame(width: 6, height: 6)
                .offset(x: placement.x - 64, y: placement.y - 64)
        } else {
            Circle()
                .fill(color)
                .frame(width: mission.state == .run ? 8 : 6, height: mission.state == .run ? 8 : 6)
                .shadow(color: color.opacity(0.6), radius: mission.state == .run ? 4 : 0)
                .offset(x: placement.x - 64, y: placement.y - 64)
                .opacity(mission.state == .run ? 1 : 0.85)
        }
    }

    private func blipColor(for state: MissionDisplayState) -> Color {
        switch state {
        case .run: return theme.run
        case .done: return theme.done
        case .fail: return theme.fail
        case .stop: return theme.stop
        }
    }

    private func blipPlacement(for id: MissionID, progress: Double) -> CGPoint {
        let hash = abs(id.hashValue)
        let angle = Double(hash % 360) * .pi / 180
        let radius = 20 + (1 - progress) * 40
        let x = 64 + cos(angle) * radius
        let y = 64 + sin(angle) * radius
        return CGPoint(x: x, y: y)
    }
}
