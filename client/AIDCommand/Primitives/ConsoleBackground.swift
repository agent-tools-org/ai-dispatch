// Console background — deep floor with optional radial gradient or flat fill.
// Exports: ConsoleBackground.

import SwiftUI

struct ConsoleBackground: View {
    @Environment(\.theme) private var theme

    var body: some View {
        ZStack {
            if let stops = gradientStops {
                RadialGradient(
                    colors: stops,
                    center: .top,
                    startRadius: 40,
                    endRadius: 800
                )
            } else {
                theme.bgDeep
            }
            TextureOverlay()
        }
        .ignoresSafeArea()
    }

    private var gradientStops: [Color]? {
        switch theme.kind {
        case .starship:
            return [Color(hex: 0x22251F), Color(hex: 0x101210), theme.bgDeep]
        case .pixel:
            return nil
        }
    }
}
