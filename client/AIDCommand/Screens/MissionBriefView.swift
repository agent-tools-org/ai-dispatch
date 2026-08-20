// MISSION BRIEF bottom band — unit card, crew, brief, stats, actions.
// Exports: MissionBriefView.

import SwiftUI

struct MissionBriefView: View {
    @Environment(\.theme) private var theme
    let snapshot: FleetSnapshot
    let selectedMissionID: MissionID?
    let detail: MissionDetail?
    let actionMessage: String?
    let onAction: (MissionAction) -> Void

    @State private var showSteerSheet = false
    @State private var steerText = ""
    @State private var confirmAction: MissionAction?

    var body: some View {
        ThemedPanel {
            VStack(alignment: .leading, spacing: theme.spacing.sm) {
                if let mission {
                    HStack(alignment: .top, spacing: theme.spacing.md) {
                        unitCard(for: mission)
                        crewList
                        briefColumn(mission)
                        sideColumn(mission)
                    }
                    actionBar
                } else {
                    emptyState
                }
                if let actionMessage {
                    Text(actionMessage)
                        .font(theme.font(.caption))
                        .foregroundStyle(theme.fail)
                }
            }
            .padding(theme.spacing.md)
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        }
        .padding(.horizontal, theme.spacing.md)
        .padding(.bottom, theme.spacing.sm)
        .sheet(isPresented: $showSteerSheet) { steerSheet }
        .alert("Confirm action", isPresented: confirmBinding) {
            Button("Cancel", role: .cancel) { confirmAction = nil }
            Button("Proceed", role: .destructive) {
                if let action = confirmAction { onAction(action) }
                confirmAction = nil
            }
        } message: {
            Text(confirmMessage)
        }
    }

    /// Prefer hydrated detail when it matches the selection; else the snapshot row.
    private var mission: Mission? {
        if let id = selectedMissionID {
            if let detail, detail.mission.id == id { return detail.mission }
            return snapshot.sectors.flatMap(\.missions).first { $0.id == id }
        }
        return nil
    }

    private var emptyState: some View {
        VStack(alignment: .leading, spacing: theme.spacing.sm) {
            MonoLabel(text: "mission brief", color: theme.ink3)
            Text("Select a mission from the fleet log, hangar, or cargo hold.")
                .font(theme.font(.body))
                .foregroundStyle(theme.ink2)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
    }

    private func unitCard(for mission: Mission) -> some View {
        let agent = mission.agent
        let profile = UnitCatalog.profile(for: agent)
        let flown = snapshot.agents.first { $0.id == agent }?.taskCount
        return VStack(spacing: theme.spacing.sm) {
            UnitGlyph(agent: agent, size: 48)
            Text(agent)
                .font(theme.font(.title))
                .foregroundStyle(theme.ink)
            MonoLabel(
                text: profile.map { "\($0.role) · LV \($0.level)" } ?? "— · LV —",
                color: theme.ink2
            )
            if let model = mission.model {
                HStack(spacing: 4) {
                    DriveGlyph(model: model, size: 14)
                    Text(FleetFormatters.model(model))
                        .font(theme.font(.caption))
                        .foregroundStyle(theme.ink2)
                }
            }
            starRow(level: profile?.level)
            MonoLabel(
                text: "missions flown \(FleetFormatters.measuredCount(flown))",
                color: theme.ink3
            )
        }
        .frame(width: 140)
    }

    private func starRow(level: Int?) -> some View {
        HStack(spacing: 2) {
            ForEach(0..<5, id: \.self) { index in
                Text(index < (level ?? 0) / 2 ? "★" : "☆")
                    .font(.system(size: 10))
                    .foregroundStyle(theme.accent)
            }
        }
    }

    private var crewList: some View {
        VStack(alignment: .leading, spacing: theme.spacing.xs) {
            MonoLabel(text: "crew")
            ScrollView {
                LazyVGrid(
                    columns: [GridItem(.flexible()), GridItem(.flexible())],
                    alignment: .leading,
                    spacing: theme.spacing.xs
                ) {
                    ForEach(snapshot.agents) { agent in
                        HStack(spacing: 4) {
                            StatusLamp(color: agent.busy ? theme.run : theme.done, active: agent.busy)
                            Text(agent.id)
                                .font(theme.font(.caption))
                                .foregroundStyle(theme.ink)
                                .lineLimit(1)
                            MonoLabel(
                                text: agent.busy ? "ENG" : "RDY",
                                color: agent.busy ? theme.run : theme.ink2
                            )
                            StatusLamp(color: agent.quotaOK ? theme.done : theme.fail, active: agent.quotaOK)
                        }
                    }
                }
            }
            .frame(maxHeight: .infinity)
        }
        .frame(width: 220)
        .frame(maxHeight: .infinity, alignment: .topLeading)
    }

    private func briefColumn(_ mission: Mission) -> some View {
        VStack(alignment: .leading, spacing: theme.spacing.sm) {
            HStack(spacing: theme.spacing.sm) {
                StatePill(state: mission.state, verifyTag: mission.verifyTag)
                MonoLabel(text: "threat \(mission.threat.map(String.init) ?? "—")")
            }
            Text("\(mission.id) · \(mission.agent) · \(FleetFormatters.model(mission.model))")
                .font(theme.font(.caption))
                .foregroundStyle(theme.ink3)
            Text(mission.title)
                .font(theme.font(.body))
                .foregroundStyle(theme.ink)
            if let detail {
                Text(detail.prompt)
                    .font(theme.font(.caption))
                    .foregroundStyle(theme.ink2)
                    .padding(theme.spacing.sm)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(theme.bgDeep.opacity(0.5))
                    .lineLimit(4)
                ForEach(detail.events.prefix(3)) { event in
                    Text("› \(event.message)")
                        .font(theme.font(.caption))
                        .foregroundStyle(theme.ink3)
                        .lineLimit(1)
                }
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func sideColumn(_ mission: Mission) -> some View {
        VStack(alignment: .leading, spacing: theme.spacing.sm) {
            HStack(spacing: theme.spacing.md) {
                stat("elapsed", FleetFormatters.elapsed(seconds: mission.elapsedSeconds))
                stat("memory", mission.memoryMB ?? "—")
                stat("tokens", FleetFormatters.tokens(mission.tokens))
                stat("cost", FleetFormatters.cost(mission.cost))
            }
            if let payload = PayloadDeriver.derive(from: mission, sectorTag: "—") {
                HStack(spacing: theme.spacing.sm) {
                    MonoLabel(text: payload.kind.rawValue, color: theme.accent)
                    Text(payload.name)
                        .font(theme.font(.caption))
                        .foregroundStyle(theme.ink)
                        .lineLimit(1)
                    RarityChevrons(rarity: payload.rarity)
                }
            }
            lampRow
        }
        .frame(width: 280, alignment: .leading)
    }

    private func stat(_ label: String, _ value: String) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            MonoLabel(text: label, color: theme.ink3)
            Text(value)
                .font(theme.font(.caption))
                .foregroundStyle(theme.ink)
        }
    }

    private var lampRow: some View {
        HStack(spacing: theme.spacing.sm) {
            ForEach(["LINK", "DRIVE", "SHIELD", "QUOTA", "DOCK"], id: \.self) { label in
                HStack(spacing: 4) {
                    StatusLamp(color: lampColor(label), active: true)
                    MonoLabel(text: label, color: theme.ink3)
                }
            }
        }
    }

    private func lampColor(_ label: String) -> Color {
        switch label {
        case "LINK":
            switch snapshot.connection {
            case .live: return theme.done
            case .degraded: return theme.stop
            case .connecting: return theme.ink2
            case .disconnected, .error: return theme.fail
            }
        case "QUOTA": return theme.stop
        default: return theme.ink2
        }
    }

    private var actionBar: some View {
        HStack(spacing: theme.spacing.xs) {
            ThemedButton(title: "ABORT", compact: true) { onAction(.abort) }
            ThemedButton(title: "RELAUNCH", compact: true) { confirmAction = .relaunch }
            ThemedButton(title: "STEER", compact: true) { showSteerSheet = true }
            ThemedButton(title: "DIFF", compact: true) { onAction(.diff) }
            ThemedButton(title: "EXPORT", compact: true) { onAction(.export) }
            ThemedButton(title: "DOCK", filled: true, compact: true) { confirmAction = .dock }
            Spacer(minLength: 0)
        }
    }

    private var steerSheet: some View {
        VStack(alignment: .leading, spacing: theme.spacing.md) {
            MonoLabel(text: "steer message")
            TextField("Course correction…", text: $steerText, axis: .vertical)
                .textFieldStyle(.plain)
                .font(theme.font(.body))
                .padding(8)
                .background(theme.bg)
            HStack {
                Spacer()
                ThemedButton(title: "Cancel") { showSteerSheet = false }
                ThemedButton(title: "Send", filled: true) {
                    onAction(.steer(steerText))
                    showSteerSheet = false
                    steerText = ""
                }
            }
        }
        .padding(theme.spacing.lg)
        .background(theme.bgDeep)
        .frame(minWidth: 360, minHeight: 180)
    }

    private var confirmBinding: Binding<Bool> {
        Binding(
            get: { confirmAction != nil },
            set: { if !$0 { confirmAction = nil } }
        )
    }

    private var confirmMessage: String {
        switch confirmAction {
        case .relaunch: return "Relaunch this mission?"
        case .dock: return "Dock (merge) this mission?"
        default: return "Proceed?"
        }
    }
}
