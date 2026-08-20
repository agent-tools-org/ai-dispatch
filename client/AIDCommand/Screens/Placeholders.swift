// Placeholder screens for tabs and bands not in this task's scope.
// Exports: HangarPlaceholder, CargoPlaceholder, MissionBriefPlaceholder.

import SwiftUI

struct HangarPlaceholder: View {
    @Environment(\.theme) private var theme

    var body: some View {
        placeholder(title: "HANGAR", detail: "Bay grid — coming in next task")
    }
}

struct CargoPlaceholder: View {
    @Environment(\.theme) private var theme

    var body: some View {
        placeholder(title: "CARGO", detail: "Payload hold — coming in next task")
    }
}

struct MissionBriefPlaceholder: View {
    @Environment(\.theme) private var theme

    var body: some View {
        ThemedPanel {
            VStack(alignment: .leading, spacing: theme.spacing.sm) {
                MonoLabel(text: "mission brief")
                Text("Unit card, crew, actions — coming in next task")
                    .font(theme.font(.body))
                    .foregroundStyle(theme.ink2)
            }
            .padding(theme.spacing.lg)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .padding(theme.spacing.md)
    }
}

@ViewBuilder
private func placeholder(title: String, detail: String) -> some View {
    VStack(spacing: 12) {
        MonoLabel(text: title)
        Text(detail)
            .font(.system(.body, design: .monospaced))
            .foregroundStyle(.secondary)
    }
    .frame(maxWidth: .infinity, maxHeight: .infinity)
}
