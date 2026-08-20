// Gauge strip cell — lamp, label, value, and 8 pips.
// Exports: GaugeCell.

import SwiftUI

struct GaugeCell: View {
    @Environment(\.theme) private var theme
    let label: String
    let value: String
    let color: Color
    var pipLevel: Int = 4

    var body: some View {
        VStack(alignment: .leading, spacing: theme.spacing.xs) {
            HStack(spacing: 6) {
                StatusLamp(color: color)
                MonoLabel(text: label)
            }
            Text(value)
                .font(theme.font(.value))
                .foregroundStyle(color)
                .lineLimit(1)
                .minimumScaleFactor(0.7)
            pipRow
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(theme.spacing.sm)
    }

    private var pipRow: some View {
        HStack(spacing: 3) {
            ForEach(0..<8, id: \.self) { index in
                pip(at: index)
            }
        }
    }

    @ViewBuilder
    private func pip(at index: Int) -> some View {
        let filled = index < pipLevel
        if theme.motion.stepped {
            Rectangle()
                .fill(filled ? color : theme.ink3.opacity(0.3))
                .frame(width: theme.pipSize.width, height: theme.pipSize.height)
        } else {
            RoundedRectangle(cornerRadius: 1)
                .fill(filled ? color : theme.ink3.opacity(0.3))
                .frame(width: theme.pipSize.width, height: theme.pipSize.height)
        }
    }
}
