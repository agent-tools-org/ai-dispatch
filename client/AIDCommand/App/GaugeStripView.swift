// Gauge strip — six summary cells plus window label and klaxon slot.
// Exports: GaugeStripView.

import SwiftUI

struct GaugeStripView: View {
    @Environment(\.theme) private var theme
    let summary: FleetSummary

    var body: some View {
        VStack(alignment: .leading, spacing: theme.spacing.xs) {
            MonoLabel(text: "window \(summary.window)")
            HStack(spacing: theme.spacing.sm) {
                GaugeCell(label: "drives", value: "\(summary.running)", color: theme.run, pipLevel: min(8, summary.running))
                GaugeCell(label: "cleared", value: "\(summary.done)", color: theme.done, pipLevel: min(8, summary.done))
                GaugeCell(label: "lost", value: "\(summary.failed)", color: theme.fail, pipLevel: min(8, summary.failed))
                GaugeCell(
                    label: "fuel spent",
                    value: summary.spendUSD.map { $0.hasPrefix("$") ? $0 : "$\($0)" } ?? "—",
                    color: theme.stop,
                    pipLevel: 5
                )
                GaugeCell(
                    label: "reactor",
                    value: FleetFormatters.reactorLoad(running: summary.running),
                    color: theme.ink,
                    pipLevel: min(8, summary.running + 2)
                )
                GaugeCell(
                    label: "memory",
                    value: "\(summary.memoryMB) MB",
                    color: theme.ink,
                    pipLevel: min(8, summary.memoryMB / 64)
                )
                klaxonSlot
            }
        }
        .padding(.horizontal, theme.spacing.lg)
        .padding(.vertical, theme.spacing.sm)
        .frame(height: 96)
        .background(theme.bgDeep)
    }

    private var klaxonSlot: some View {
        VStack {
            MonoLabel(text: "klaxon")
            StatusLamp(color: theme.accent, active: false)
        }
        .frame(width: 64)
    }
}
