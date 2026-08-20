// State pill showing mission display state with optional verify tag.
// Exports: StatePill.

import SwiftUI

struct StatePill: View {
    @Environment(\.theme) private var theme
    let state: MissionDisplayState
    var verifyTag: VerifyTag?

    var body: some View {
        HStack(spacing: 4) {
            StateMark(state: state, size: 12)
            MonoLabel(text: StatusMapper.label(for: state), color: stateColor)
            if let verifyTag {
                MonoLabel(text: verifyTag.rawValue, color: theme.stop)
                    .padding(.horizontal, 4)
                    .background(theme.stop.opacity(0.15))
            }
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 4)
        .background(stateColor.opacity(0.12))
        .overlay(
            RoundedRectangle(cornerRadius: theme.motion.stepped ? 0 : 4)
                .stroke(stateColor.opacity(0.5), lineWidth: theme.hairline)
        )
        .fixedSize(horizontal: true, vertical: false)
    }

    private var stateColor: Color {
        switch state {
        case .run: return theme.run
        case .done: return theme.done
        case .fail: return theme.fail
        case .stop: return theme.stop
        }
    }
}
