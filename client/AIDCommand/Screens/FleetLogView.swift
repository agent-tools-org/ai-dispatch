// FLEET LOG center tab — collapsible sector groups and mission rows.
// Exports: FleetLogView.

import SwiftUI

struct FleetLogView: View {
    @Environment(\.theme) private var theme
    let sectors: [Sector]
    @Binding var selectedMissionID: MissionID?
    @State private var collapsed: Set<String> = []

    var body: some View {
        ScrollView(.vertical, showsIndicators: true) {
            VStack(alignment: .leading, spacing: theme.spacing.md) {
                ForEach(sectors) { sector in
                    sectorGroup(sector)
                }
            }
            .padding(theme.spacing.md)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private func sectorGroup(_ sector: Sector) -> some View {
        let expanded = !collapsed.contains(sector.id)
        let done = sector.missions.filter { $0.state == .done }.count
        return VStack(alignment: .leading, spacing: theme.spacing.sm) {
            Button {
                toggle(sector.id)
            } label: {
                header(sector, done: done, expanded: expanded)
            }
            .buttonStyle(.plain)
            if expanded {
                ForEach(sector.missions) { mission in
                    missionRow(mission)
                }
            }
        }
    }

    private func header(_ sector: Sector, done: Int, expanded: Bool) -> some View {
        VStack(alignment: .leading, spacing: theme.spacing.xs) {
            HStack {
                Text(expanded ? "▼" : "▶")
                    .font(theme.font(.caption))
                    .foregroundStyle(theme.ink3)
                MonoLabel(text: sector.tag, color: theme.accent)
                Text(sector.name)
                    .font(theme.font(.body))
                    .foregroundStyle(theme.ink)
                Spacer()
                MonoLabel(text: FleetFormatters.workgroupLabel(sector.workgroupID))
            }
            MonoLabel(text: "\(done)/\(sector.missions.count) cleared")
            SegmentBar(segments: sector.missions.map { color(for: $0.state) })
        }
        .padding(theme.spacing.sm)
        .background(theme.bg)
    }

    private func missionRow(_ mission: Mission) -> some View {
        let selected = selectedMissionID == mission.id
        return Button {
            selectedMissionID = mission.id
        } label: {
            HStack(spacing: theme.spacing.sm) {
                StateMark(state: mission.state, size: 13)
                    .frame(width: 16)
                missionColumn(mission)
                unitColumn(mission)
                StatePill(state: mission.state, verifyTag: mission.verifyTag)
                    .frame(width: 110, alignment: .leading)
                progressColumn(mission)
                Text(FleetFormatters.elapsed(seconds: mission.elapsedSeconds))
                    .font(theme.font(.caption))
                    .foregroundStyle(theme.ink2)
                    .frame(width: 72, alignment: .trailing)
                Text(FleetFormatters.tokens(mission.tokens))
                    .font(theme.font(.caption))
                    .foregroundStyle(theme.ink2)
                    .frame(width: 56, alignment: .trailing)
                Text(FleetFormatters.cost(mission.cost))
                    .font(theme.font(.caption))
                    .foregroundStyle(theme.ink2)
                    .frame(width: 56, alignment: .trailing)
            }
            .padding(.vertical, theme.spacing.xs)
            .padding(.horizontal, theme.spacing.sm)
            .background(selected ? theme.accent.opacity(0.08) : Color.clear)
            .overlay(alignment: .leading) {
                if selected { Rectangle().fill(theme.accent).frame(width: 2) }
            }
            .overlay(alignment: .trailing) {
                if selected { Rectangle().fill(theme.accent).frame(width: 2) }
            }
        }
        .buttonStyle(.plain)
    }

    private func missionColumn(_ mission: Mission) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(mission.title)
                .font(theme.font(.body))
                .foregroundStyle(theme.ink)
                .lineLimit(1)
            Text("\(mission.id) · THREAT \(mission.threat.map(String.init) ?? "—")")
                .font(theme.font(.caption))
                .foregroundStyle(theme.ink3)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func unitColumn(_ mission: Mission) -> some View {
        HStack(spacing: 6) {
            UnitGlyph(agent: mission.agent, size: 18)
            VStack(alignment: .leading, spacing: 0) {
                Text(mission.agent)
                    .font(theme.font(.caption))
                    .foregroundStyle(theme.ink2)
                HStack(spacing: 4) {
                    DriveGlyph(model: mission.model, size: 12)
                    Text(FleetFormatters.model(mission.model))
                        .font(theme.font(.caption))
                        .foregroundStyle(theme.ink3)
                        .lineLimit(1)
                }
            }
        }
        .frame(width: 140, alignment: .leading)
    }

    private func progressColumn(_ mission: Mission) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            SegmentBar(
                segments: Array(repeating: color(for: mission.state), count: 14),
                progress: mission.state == .run ? mission.progress : nil
            )
            .frame(width: 120)
            Text(FleetFormatters.percent(mission.progress))
                .font(theme.font(.caption))
                .foregroundStyle(theme.ink3)
        }
        .frame(width: 130, alignment: .leading)
    }

    private func toggle(_ id: String) {
        if collapsed.contains(id) {
            collapsed.remove(id)
        } else {
            collapsed.insert(id)
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
