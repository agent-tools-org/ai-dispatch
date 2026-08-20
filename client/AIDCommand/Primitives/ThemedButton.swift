// Themed button — clipped tab style with accent fill variant.
// Exports: ThemedButton.

import SwiftUI

struct ThemedButton: View {
    @Environment(\.theme) private var theme
    let title: String
    var filled: Bool = false
    var selected: Bool = false
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Text(title.uppercased())
                .font(theme.font(.label))
                .tracking(theme.kind == .pixel ? 0.8 : 1.2)
                .foregroundStyle(foreground)
                .padding(.horizontal, 12)
                .padding(.vertical, 8)
                .background(background)
                .clipShape(PanelShape(style: theme.panelStyle))
                .overlay(
                    PanelShape(style: theme.panelStyle)
                        .stroke(border, lineWidth: theme.hairline)
                )
        }
        .buttonStyle(.plain)
    }

    private var foreground: Color {
        if filled || selected { return theme.bgDeep }
        return theme.ink
    }

    private var background: Color {
        if filled { return theme.accent }
        if selected { return theme.accent.opacity(0.85) }
        return theme.bg
    }

    private var border: Color {
        selected || filled ? theme.accent : theme.panelEdge
    }
}
