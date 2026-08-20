// Toast notification sliding in from trailing edge.
// Exports: ToastView, ToastStack.

import SwiftUI

struct ToastView: View {
    @Environment(\.theme) private var theme
    let toast: ToastEvent

    var body: some View {
        HStack(spacing: theme.spacing.sm) {
            Text(StatusMapper.mark(for: toast.state))
                .font(theme.font(.value))
                .foregroundStyle(edgeColor)
            VStack(alignment: .leading, spacing: 2) {
                Text(toast.title)
                    .font(theme.font(.body))
                    .foregroundStyle(theme.ink)
                    .lineLimit(1)
                Text("\(toast.missionID) · +\(toast.xpAward) XP")
                    .font(theme.font(.caption))
                    .foregroundStyle(theme.ink2)
                    .lineLimit(1)
            }
        }
        .padding(theme.spacing.md)
        .background(theme.bg)
        .overlay(
            Rectangle()
                .fill(edgeColor)
                .frame(width: 3),
            alignment: .leading
        )
        .clipShape(PanelShape(style: theme.panelStyle))
        .shadow(color: edgeColor.opacity(0.3), radius: 8)
        .frame(maxWidth: 320, alignment: .leading)
        .fixedSize(horizontal: true, vertical: true)
    }

    private var edgeColor: Color {
        toast.state == .done ? theme.done : theme.fail
    }
}

struct ToastStack: View {
    @Environment(\.theme) private var theme
    let toasts: [ToastEvent]

    var body: some View {
        VStack(alignment: .trailing, spacing: theme.spacing.sm) {
            ForEach(toasts) { toast in
                ToastView(toast: toast)
                    .transition(.move(edge: .trailing).combined(with: .opacity))
            }
        }
        .padding(theme.spacing.lg)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topTrailing)
        .allowsHitTesting(false)
        .animation(.easeOut(duration: theme.motion.crossfade), value: toasts)
    }
}
