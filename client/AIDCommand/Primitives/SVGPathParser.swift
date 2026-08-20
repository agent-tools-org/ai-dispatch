// Minimal SVG path parser for unit glyph paths.
// Exports: SVGPathParser.

import SwiftUI

enum SVGPathParser {
    static func parse(_ d: String, in rect: CGRect) -> Path {
        var path = Path()
        let scaleX = rect.width / 24
        let scaleY = rect.height / 24
        let tokens = tokenize(d)
        var index = 0
        var current = CGPoint.zero

        while index < tokens.count {
            let cmd = tokens[index]
            index += 1
            switch cmd {
            case "M":
                let p = point(tokens, &index, scaleX, scaleY)
                current = p
                path.move(to: p)
            case "L":
                let p = point(tokens, &index, scaleX, scaleY)
                current = p
                path.addLine(to: p)
            case "Z":
                path.closeSubpath()
            case "C":
                let c1 = point(tokens, &index, scaleX, scaleY)
                let c2 = point(tokens, &index, scaleX, scaleY)
                let p = point(tokens, &index, scaleX, scaleY)
                path.addCurve(to: p, control1: c1, control2: c2)
                current = p
            case "A":
                index += 5
                let p = point(tokens, &index, scaleX, scaleY)
                path.addLine(to: p)
                current = p
            default:
                break
            }
        }
        return path
    }

    private static func tokenize(_ d: String) -> [String] {
        var result: [String] = []
        var current = ""
        for ch in d {
            if ch.isLetter {
                if !current.isEmpty { result.append(current); current = "" }
                result.append(String(ch))
            } else if ch == "," || ch == " " {
                if !current.isEmpty { result.append(current); current = "" }
            } else {
                current.append(ch)
            }
        }
        if !current.isEmpty { result.append(current) }
        return result
    }

    private static func point(
        _ tokens: [String], _ index: inout Int, _ sx: CGFloat, _ sy: CGFloat
    ) -> CGPoint {
        let x = CGFloat(Double(tokens[index]) ?? 0) * sx
        index += 1
        let y = CGFloat(Double(tokens[index]) ?? 0) * sy
        index += 1
        return CGPoint(x: x, y: y)
    }
}
