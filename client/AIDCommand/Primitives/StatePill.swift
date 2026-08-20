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
            Text(StatusMapper.label(for: state))
                .font(theme.font(.label))
                .tracking(theme.kind == .pixel ? 0.8 : 1.2)
            if let verifyTag {
                Text(verifyTag.rawValue)
                    .font(theme.font(.caption))
                    .foregroundStyle(theme.stop)
                    .padding(.horizontal, 4)
                    .background(theme.stop.opacity(0.15))
            }
        }
        .foregroundStyle(stateColor)
        .padding(.horizontal, 8)
        .padding(.vertical, 4)
        .background(stateColor.opacity(0.12))
        .overlay(
            RoundedRectangle(cornerRadius: theme.motion.stepped ? 0 : 4)
                .stroke(stateColor.opacity(0.5), lineWidth: theme.hairline)
        )
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
