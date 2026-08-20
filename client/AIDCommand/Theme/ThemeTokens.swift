// Theme token contract — every primitive reads colors and geometry through this.
// Exports: ThemeTokens protocol.

import SwiftUI

protocol ThemeTokens: Sendable {
    var kind: ThemeKind { get }
    var bgDeep: Color { get }
    var bg: Color { get }
    var panelEdge: Color { get }
    var ink: Color { get }
    var ink2: Color { get }
    var ink3: Color { get }
    var accent: Color { get }
    var run: Color { get }
    var done: Color { get }
    var fail: Color { get }
    var stop: Color { get }
    var panelStyle: PanelStyle { get }
    var panelCut: CGFloat { get }
    var hairline: CGFloat { get }
    var pipSize: CGSize { get }
    var spacing: SpacingScale { get }
    var overlay: TextureStyle { get }
    var motion: MotionProfile { get }
    func font(_ role: TypeRole) -> Font
}

enum ThemeKind: String, CaseIterable, Codable, Sendable {
    case starship
    case pixel
}
