// Settings sheet/window — theme picker with live previews.
// Exports: SettingsView.

import SwiftUI

struct SettingsView: View {
    @Environment(\.theme) private var theme
    @Bindable var themeManager: ThemeManager
    @Binding var commanderName: String
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: theme.spacing.xl) {
                MonoLabel(text: "settings")
                themeSection
                commanderSection
                sourceSection
            }
            .padding(theme.spacing.xl)
        }
        .background(theme.bgDeep)
        .frame(minWidth: 480, minHeight: 420)
    }

    private var themeSection: some View {
        VStack(alignment: .leading, spacing: theme.spacing.md) {
            MonoLabel(text: "theme")
            HStack(spacing: theme.spacing.lg) {
                ForEach(ThemeKind.allCases, id: \.self) { kind in
                    themePreview(kind)
                }
            }
        }
    }

    private func themePreview(_ kind: ThemeKind) -> some View {
        let tokens = themeFor(kind)
        let selected = themeManager.kind == kind
        return Button {
            withAnimation(.easeInOut(duration: tokens.motion.crossfade)) {
                themeManager.kind = kind
            }
        } label: {
            VStack(alignment: .leading, spacing: theme.spacing.sm) {
                ZStack {
                    tokens.bgDeep
                    HStack(spacing: 8) {
                        Circle().fill(tokens.run).frame(width: 8, height: 8)
                        RoundedRectangle(cornerRadius: tokens.motion.stepped ? 0 : 2)
                            .fill(tokens.accent)
                            .frame(width: 40, height: 8)
                        Rectangle().fill(tokens.panelEdge).frame(height: 2)
                    }
                    .padding(12)
                }
                .frame(width: 180, height: 100)
                .clipShape(PanelShape(style: tokens.panelStyle))
                .overlay(
                    PanelShape(style: tokens.panelStyle)
                        .stroke(selected ? tokens.accent : tokens.panelEdge, lineWidth: selected ? 2 : 1)
                )
                Text(kind.rawValue.uppercased())
                    .font(tokens.font(.label))
                    .foregroundStyle(selected ? tokens.accent : tokens.ink2)
            }
        }
        .buttonStyle(.plain)
    }

    private var commanderSection: some View {
        VStack(alignment: .leading, spacing: theme.spacing.sm) {
            MonoLabel(text: "commander name")
            TextField("CMDR", text: $commanderName)
                .textFieldStyle(.plain)
                .font(theme.font(.body))
                .padding(8)
                .background(theme.bg)
                .overlay(Rectangle().stroke(theme.panelEdge, lineWidth: theme.hairline))
        }
    }

    private var sourceSection: some View {
        VStack(alignment: .leading, spacing: theme.spacing.sm) {
            MonoLabel(text: "data source")
            MonoLabel(text: "demo — live source placeholder", color: theme.ink3)
        }
    }
}
