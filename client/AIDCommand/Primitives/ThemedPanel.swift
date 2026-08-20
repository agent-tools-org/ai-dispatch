// Themed panel container with edge stroke and optional fill.
// Exports: ThemedPanel.

import SwiftUI

struct ThemedPanel<Content: View>: View {
    @Environment(\.theme) private var theme
    let fill: Color?
    @ViewBuilder let content: () -> Content

    init(fill: Color? = nil, @ViewBuilder content: @escaping () -> Content) {
        self.fill = fill
        self.content = content
    }

    var body: some View {
        content()
            .background(panelBackground)
            .clipShape(PanelShape(style: theme.panelStyle))
            .overlay(
                PanelShape(style: theme.panelStyle)
                    .stroke(theme.panelEdge, lineWidth: theme.hairline)
            )
    }

    @ViewBuilder
    private var panelBackground: some View {
        if let fill {
            fill
        } else {
            theme.bg
        }
    }
}
