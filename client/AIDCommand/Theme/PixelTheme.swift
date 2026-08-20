// Pixel theme tokens — stepped corners, dither, 8fps stepped motion.
// Exports: PixelTheme.

import SwiftUI

struct PixelTheme: ThemeTokens {
    let kind: ThemeKind = .pixel

    var bgDeep: Color { Color(hex: 0x0F0F1B) }
    var bg: Color { Color(hex: 0x242438) }
    var panelEdge: Color { Color(hex: 0x5A5A8C) }
    var ink: Color { Color(hex: 0xF4F4F8) }
    var ink2: Color { Color(hex: 0xB8B8D0) }
    var ink3: Color { Color(hex: 0x7A7A9E) }
    var accent: Color { Color(hex: 0xF8D048) }
    var run: Color { Color(hex: 0x3CE0D0) }
    var done: Color { Color(hex: 0x5CD44C) }
    var fail: Color { Color(hex: 0xF04848) }
    var stop: Color { Color(hex: 0xF8D048) }

    var panelStyle: PanelStyle { .stepped(step: 4) }
    var panelCut: CGFloat { 4 }
    var hairline: CGFloat { 2 }
    var pipSize: CGSize { CGSize(width: 6, height: 6) }
    var spacing: SpacingScale {
        SpacingScale(xs: 4, sm: 8, md: 12, lg: 16, xl: 24)
    }
    var overlay: TextureStyle { .dither }
    var motion: MotionProfile {
        MotionProfile(stepped: true, fps: 8, pulseMin: 0, pulseMax: 1, crossfade: 0.24)
    }

    func font(_ role: TypeRole) -> Font {
        switch role {
        case .body: return .system(size: 13, weight: .bold, design: .monospaced)
        case .label: return .system(size: 10, weight: .bold, design: .monospaced)
        case .value: return .system(size: 26, weight: .bold, design: .monospaced)
        case .title: return .system(size: 14, weight: .bold, design: .monospaced)
        case .caption: return .system(size: 9, weight: .bold, design: .monospaced)
        }
    }
}
