// Mono text document sheet for DIFF / EXPORT payloads.
// Exports: TextDocumentSheet, DocumentPayload.

import SwiftUI
#if os(macOS)
import AppKit
#endif

struct DocumentPayload: Identifiable {
    let id: UUID
    let title: String
    let body: String
    let allowsSave: Bool

    init(title: String, body: String, allowsSave: Bool) {
        self.id = UUID()
        self.title = title
        self.body = body
        self.allowsSave = allowsSave
    }
}

struct TextDocumentSheet: View {
    @Environment(\.theme) private var theme
    @Environment(\.dismiss) private var dismiss
    let payload: DocumentPayload

    var body: some View {
        VStack(alignment: .leading, spacing: theme.spacing.md) {
            HStack {
                MonoLabel(text: payload.title, color: theme.accent)
                Spacer()
                if payload.allowsSave {
                    ThemedButton(title: "Save", compact: true) { save() }
                }
                ThemedButton(title: "Close", compact: true) { dismiss() }
            }
            ScrollView {
                Text(payload.body)
                    .font(theme.font(.caption))
                    .foregroundStyle(theme.ink)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .textSelection(.enabled)
            }
            .padding(theme.spacing.sm)
            .background(theme.bg)
            .clipShape(PanelShape(style: theme.panelStyle))
        }
        .padding(theme.spacing.lg)
        .background(theme.bgDeep)
        .frame(minWidth: 520, minHeight: 360)
    }

    private func save() {
        #if os(macOS)
        let panel = NSSavePanel()
        panel.nameFieldStringValue = payload.title.lowercased().contains("diff")
            ? "mission.diff"
            : "result.md"
        panel.canCreateDirectories = true
        if panel.runModal() == .OK, let url = panel.url {
            try? payload.body.write(to: url, atomically: true, encoding: .utf8)
        }
        #endif
    }
}
