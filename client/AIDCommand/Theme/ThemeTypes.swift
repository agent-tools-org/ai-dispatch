// Shared theme type aliases: roles, spacing, texture, motion.
// Exports: TypeRole, SpacingScale, TextureStyle, MotionProfile, PanelStyle.

import SwiftUI

enum TypeRole: Sendable {
    case body, label, value, title, caption
}

struct SpacingScale: Sendable {
    let xs: CGFloat
    let sm: CGFloat
    let md: CGFloat
    let lg: CGFloat
    let xl: CGFloat
}

enum TextureStyle: Sendable, Equatable {
    case scanline, dither, none
}

struct MotionProfile: Sendable {
    let stepped: Bool
    let fps: Double
    let pulseMin: Double
    let pulseMax: Double
    let crossfade: Double
}

enum PanelStyle: Sendable, Equatable {
    case beveled(cut: CGFloat)
    case stepped(step: CGFloat)
}
