// Panel shapes — beveled starship corners vs pixel staircase.
// Exports: PanelShape, BeveledPanelShape, SteppedPanelShape.

import SwiftUI

struct PanelShape: Shape {
    let style: PanelStyle

    func path(in rect: CGRect) -> Path {
        switch style {
        case .beveled(let cut):
            return BeveledPanelShape(cut: cut).path(in: rect)
        case .stepped(let step):
            return SteppedPanelShape(step: step).path(in: rect)
        }
    }
}

struct BeveledPanelShape: Shape {
    let cut: CGFloat

    func path(in rect: CGRect) -> Path {
        let w = rect.width
        let h = rect.height
        let c = min(cut, min(w, h) / 2)
        var path = Path()
        path.move(to: CGPoint(x: 0, y: 0))
        path.addLine(to: CGPoint(x: w - c, y: 0))
        path.addLine(to: CGPoint(x: w, y: c))
        path.addLine(to: CGPoint(x: w, y: h))
        path.addLine(to: CGPoint(x: c, y: h))
        path.addLine(to: CGPoint(x: 0, y: h - c))
        path.closeSubpath()
        return path
    }
}

struct SteppedPanelShape: Shape {
    let step: CGFloat

    func path(in rect: CGRect) -> Path {
        let s = step
        let w = rect.width
        let h = rect.height
        var path = Path()
        path.move(to: CGPoint(x: 0, y: 0))
        path.addLine(to: CGPoint(x: w - s * 3, y: 0))
        path.addLine(to: CGPoint(x: w - s * 2, y: s))
        path.addLine(to: CGPoint(x: w - s, y: s))
        path.addLine(to: CGPoint(x: w, y: s * 2))
        path.addLine(to: CGPoint(x: w, y: h))
        path.addLine(to: CGPoint(x: s * 2, y: h))
        path.addLine(to: CGPoint(x: s, y: h - s))
        path.addLine(to: CGPoint(x: 0, y: h - s * 2))
        path.closeSubpath()
        return path
    }
}
