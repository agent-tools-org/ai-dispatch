// Uppercase mono label primitive.
// Exports: MonoLabel.

import SwiftUI

struct MonoLabel: View {
    @Environment(\.theme) private var theme
    let text: String
    var color: Color?

    var body: some View {
        // Join with word-joiners so a tight parent never wraps mid-glyph.
        Text(Self.unbreakable(text.uppercased()))
            .font(theme.font(.label))
            .tracking(theme.kind == .pixel ? 1.0 : 1.4)
            .foregroundStyle(color ?? theme.ink2)
            .lineLimit(1)
            .fixedSize(horizontal: true, vertical: true)
            .layoutPriority(1)
    }

    private static func unbreakable(_ text: String) -> String {
        String(text.flatMap { [$0, "\u{2060}"] }.dropLast())
    }
}
