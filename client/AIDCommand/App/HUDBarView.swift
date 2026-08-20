// HUD bar — ship mark, rank/XP, condition lamp, tabs, clock.
// Exports: HUDBarView.

import SwiftUI

struct HUDBarView: View {
    @Environment(\.theme) private var theme
    @Environment(\.consoleLayout) private var layout
    let snapshot: FleetSnapshot
    let xpState: XPState
    let commanderName: String
    @Binding var selectedTab: CenterTab
    var onToggleRail: () -> Void

    @State private var clock = Date()

    var body: some View {
        HStack(spacing: theme.spacing.lg) {
            if layout == .compact {
                Button(action: onToggleRail) {
                    MonoLabel(text: "scan")
                }
                .buttonStyle(.plain)
            }
            shipMark
            titleBlock
            rankBlock
            conditionLamp
            Spacer()
            tabGroup
            clockView
            linkLamp
        }
        .padding(.horizontal, theme.spacing.lg)
        .frame(height: 62)
        .background(theme.bg)
        .onAppear { startClock() }
    }

    private var shipMark: some View {
        ZStack {
            Rectangle()
                .fill(theme.accent.opacity(0.2))
                .frame(width: 22, height: 22)
                .rotationEffect(.degrees(45))
            StatusLamp(color: theme.accent)
        }
    }

    private var titleBlock: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text("AID · FLEET COMMAND")
                .font(theme.font(.title))
                .foregroundStyle(theme.ink)
            Text("BUILD \(snapshot.serverVersion) · SECTORS \(snapshot.summary.sectorCount) · \(commanderName)")
                .font(theme.font(.caption))
                .foregroundStyle(theme.ink2)
        }
    }

    private var rankBlock: some View {
        VStack(alignment: .leading, spacing: 4) {
            MonoLabel(text: "rank \(xpState.rankLabel)")
            GeometryReader { geo in
                ZStack(alignment: .leading) {
                    Rectangle().fill(theme.panelEdge)
                    Rectangle().fill(theme.accent).frame(width: geo.size.width * xpState.barProgress)
                }
            }
            .frame(width: 80, height: 6)
        }
    }

    private var conditionLamp: some View {
        let failed = snapshot.summary.failed
        let color: Color = failed >= 4 ? theme.fail : (failed >= 1 ? theme.stop : theme.done)
        let label = failed >= 4 ? "RED" : (failed >= 1 ? "AMBER" : "GREEN")
        return HStack(spacing: 6) {
            StatusLamp(color: color)
            MonoLabel(text: "cond \(label)", color: color)
        }
    }

    private var tabGroup: some View {
        HStack(spacing: theme.spacing.xs) {
            ForEach(CenterTab.allCases, id: \.self) { tab in
                ThemedButton(title: tab.rawValue, selected: selectedTab == tab) {
                    selectedTab = tab
                }
            }
        }
    }

    private var clockView: some View {
        TimelineView(.periodic(from: .now, by: 1)) { context in
            Text(context.date.formatted(date: .omitted, time: .standard))
                .font(theme.font(.label))
                .foregroundStyle(theme.ink2)
                .monospacedDigit()
        }
    }

    private var linkLamp: some View {
        let color: Color
        let label: String
        switch snapshot.connection {
        case .live:
            color = theme.done; label = "LINK"
        case .connecting:
            color = theme.stop; label = "LINK…"
        case .degraded(let age):
            color = theme.stop; label = "STALE \(Int(age))s"
        case .error(let message):
            color = theme.fail; label = String(message.prefix(24))
        case .disconnected:
            color = theme.fail; label = "OFF"
        }
        return HStack(spacing: 4) {
            StatusLamp(color: color, active: snapshot.connection == .live)
            MonoLabel(text: label, color: color)
        }
    }

    private func startClock() {
        clock = Date()
    }
}
