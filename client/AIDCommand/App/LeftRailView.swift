// Left rail — tactical scan scope and sector list.
// Exports: LeftRailView.

import SwiftUI

struct LeftRailView: View {
    @Environment(\.theme) private var theme
    let sectors: [Sector]
    @Binding var selectedSectorID: String?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: theme.spacing.lg) {
                ThemedPanel {
                    ContactScope(missions: sectors.flatMap(\.missions))
                        .padding(theme.spacing.md)
                }
                MonoLabel(text: "sectors")
                ForEach(sectors) { sector in
                    sectorRow(sector)
                }
            }
            .padding(theme.spacing.md)
        }
        .background(theme.bgDeep)
    }

    private func sectorRow(_ sector: Sector) -> some View {
        let selected = selectedSectorID == sector.id
        let segments = sector.missions.map { missionColor($0.state) }
        let done = sector.missions.filter { $0.state == .done }.count
        let run = sector.missions.filter { $0.state == .run }.count
        let fail = sector.missions.filter { $0.state == .fail }.count

        return Button {
            selectedSectorID = sector.id
        } label: {
            VStack(alignment: .leading, spacing: theme.spacing.xs) {
                HStack {
                    MonoLabel(text: sector.tag, color: selected ? theme.accent : theme.ink2)
                    Text(sector.name)
                        .font(theme.font(.body))
                        .foregroundStyle(theme.ink)
                        .lineLimit(1)
                }
                Text("\(done)✦ \(run)▶ \(fail)✕")
                    .font(theme.font(.caption))
                    .foregroundStyle(theme.ink2)
                SegmentBar(segments: segments, count: max(segments.count, 1))
            }
            .padding(theme.spacing.sm)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(selected ? theme.accent.opacity(0.1) : Color.clear)
            .overlay(alignment: .leading) {
                if selected {
                    Rectangle().fill(theme.accent).frame(width: 2)
                }
            }
        }
        .buttonStyle(.plain)
    }

    private func missionColor(_ state: MissionDisplayState) -> Color {
        switch state {
        case .run: return theme.run
        case .done: return theme.done
        case .fail: return theme.fail
        case .stop: return theme.stop
        }
    }
}
