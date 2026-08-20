// Starship theme tokens — beveled panels, scanlines, eased motion.
// Exports: StarshipTheme.

import SwiftUI

struct StarshipTheme: ThemeTokens {
    let kind: ThemeKind = .starship

    var bgDeep: Color { Color(hex: 0x0A0B09) }
    var bg: Color { Color(hex: 0x1B1C19) }
    var panelEdge: Color { Color(hex: 0x2C2C29) }
    var ink: Color { Color(hex: 0xECEBE7) }
    var ink2: Color { Color(hex: 0xA7A6A0) }
    var ink3: Color { Color(hex: 0x7A7974) }
    var accent: Color { Color(hex: 0xE0B15E) }
    var run: Color { Color(hex: 0x9FD8CD) }
    var done: Color { Color(hex: 0x8FBE79) }
    var fail: Color { Color(hex: 0xD4826F) }
    var stop: Color { Color(hex: 0xE0B15E) }

    var panelStyle: PanelStyle { .beveled(cut: 18) }
    var panelCut: CGFloat { 18 }
    var hairline: CGFloat { 1 }
    var pipSize: CGSize { CGSize(width: 6, height: 6) }
    var spacing: SpacingScale {
        SpacingScale(xs: 4, sm: 8, md: 12, lg: 16, xl: 24)
    }
    var overlay: TextureStyle { .scanline }
    var motion: MotionProfile {
        MotionProfile(stepped: false, fps: 60, pulseMin: 0.6, pulseMax: 1.0, crossfade: 0.24)
    }

    func font(_ role: TypeRole) -> Font {
        switch role {
        case .body: return .system(size: 13)
        case .label: return .system(size: 11, weight: .medium, design: .monospaced)
        case .value: return .system(size: 28, weight: .semibold, design: .monospaced)
        case .title: return .system(size: 15, weight: .semibold)
        case .caption: return .system(size: 10, design: .monospaced)
        }
    }
}
