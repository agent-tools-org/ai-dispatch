// Uppercase mono label primitive.
// Exports: MonoLabel.

import SwiftUI

struct MonoLabel: View {
    @Environment(\.theme) private var theme
    let text: String
    var color: Color?

    var body: some View {
        Text(text.uppercased())
            .font(theme.font(.label))
            .tracking(theme.kind == .pixel ? 1.0 : 1.4)
            .foregroundStyle(color ?? theme.ink2)
    }
}
