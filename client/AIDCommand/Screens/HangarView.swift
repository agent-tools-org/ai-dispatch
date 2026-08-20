// HANGAR center tab — bay grid for the selected sector.
// Exports: HangarView.

import SwiftUI

struct HangarView: View {
    @Environment(\.theme) private var theme
    @Environment(\.consoleLayout) private var layout
    let sector: Sector
    @Binding var selectedMissionID: MissionID?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: theme.spacing.md) {
                header
                LazyVGrid(columns: columns, spacing: theme.spacing.md) {
                    ForEach(Array(bays.enumerated()), id: \.offset) { index, bay in
                        bayCard(index: index + 1, bay: bay)
                    }
                }
            }
            .padding(theme.spacing.md)
        }
    }

    private var columns: [GridItem] {
        Array(repeating: GridItem(.flexible(), spacing: theme.spacing.md), count: columnCount)
    }

    private var columnCount: Int {
        switch layout {
        case .desktop: return 4
        case .tabletLandscape: return 3
        case .compact: return 2
        }
    }

    private var bays: [BaySlot] {
        var slots = sector.missions.map { BaySlot.mission($0) }
        let remainder = slots.count % columnCount
        if remainder != 0 {
            for _ in 0..<(columnCount - remainder) {
                slots.append(.empty)
            }
        }
        return slots
    }

    private var header: some View {
        let done = sector.missions.filter { $0.state == .done }.count
        return VStack(alignment: .leading, spacing: theme.spacing.xs) {
            HStack {
                MonoLabel(text: sector.tag, color: theme.accent)
                Text(sector.name)
                    .font(theme.font(.body))
                    .foregroundStyle(theme.ink)
            }
            MonoLabel(
                text: "\(FleetFormatters.workgroupLabel(sector.workgroupID)) · WORKTREE ISOLATION ON",
                color: theme.ink2
            )
            MonoLabel(text: "\(done)/\(sector.missions.count) bays cleared")
        }
        .padding(theme.spacing.sm)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(theme.bg)
    }

    @ViewBuilder
    private func bayCard(index: Int, bay: BaySlot) -> some View {
        switch bay {
        case .empty:
            emptyBay(index: index)
        case .mission(let mission):
            missionBay(index: index, mission: mission)
        }
    }

    private func emptyBay(index: Int) -> some View {
        ThemedPanel {
            ZStack {
                HazardStripes()
                VStack(spacing: theme.spacing.sm) {
                    MonoLabel(text: "bay \(index)", color: theme.ink3)
                    Text("EMPTY")
                        .font(theme.font(.label))
                        .foregroundStyle(theme.ink3)
                }
            }
            .frame(minHeight: 140)
        }
    }

    private func missionBay(index: Int, mission: Mission) -> some View {
        let selected = selectedMissionID == mission.id
        return Button {
            selectedMissionID = mission.id
        } label: {
            ThemedPanel {
                VStack(alignment: .leading, spacing: theme.spacing.sm) {
                    HStack {
                        MonoLabel(text: "bay \(index)")
                        Spacer()
                        StatePill(state: mission.state, verifyTag: mission.verifyTag)
                    }
                    percentFrame(mission)
                    Text(mission.title)
                        .font(theme.font(.body))
                        .foregroundStyle(theme.ink)
                        .lineLimit(2)
                    HStack(spacing: 6) {
                        UnitGlyph(agent: mission.agent, size: 16)
                        Text(mission.agent)
                            .font(theme.font(.caption))
                            .foregroundStyle(theme.ink2)
                        DriveGlyph(model: mission.model, size: 12)
                        Spacer()
                        ElapsedLabel(mission: mission)
                            .font(theme.font(.caption))
                            .foregroundStyle(theme.ink3)
                        Text(FleetFormatters.cost(mission.cost))
                            .font(theme.font(.caption))
                            .foregroundStyle(theme.ink3)
                    }
                }
                .padding(theme.spacing.sm)
            }
            .overlay {
                if selected {
                    PanelShape(style: theme.panelStyle)
                        .stroke(theme.accent, lineWidth: theme.hairline * 2)
                }
            }
        }
        .buttonStyle(.plain)
    }

    private func percentFrame(_ mission: Mission) -> some View {
        ThemedPanel {
            Text(FleetFormatters.percent(mission.progress))
                .font(theme.font(.value))
                .foregroundStyle(color(for: mission.state))
                .frame(maxWidth: .infinity)
                .padding(.vertical, theme.spacing.sm)
        }
    }

    private func color(for state: MissionDisplayState) -> Color {
        switch state {
        case .run: return theme.run
        case .done: return theme.done
        case .fail: return theme.fail
        case .stop: return theme.stop
        }
    }
}

private enum BaySlot {
    case mission(Mission)
    case empty
}
