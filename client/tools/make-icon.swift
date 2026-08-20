// Deterministic AID Command app icon renderer for macOS and iPadOS.
// Exports: PNG assets for every AppIcon slot, written to the requested directory.
// Deps: CoreGraphics, ImageIO, Foundation; no network or package dependencies.

import CoreGraphics
import Foundation
import ImageIO

private enum IconPlatform {
    case mac
    case ipad
}

private struct IconSpec {
    let filename: String
    let pixels: Int
    let platform: IconPlatform
}

private enum IconGeometry {
    static let macInset: CGFloat = 0.095
    static let macCornerRadius: CGFloat = 0.17
    static let macBadgeRadius: CGFloat = 0.285
    static let ipadBadgeRadius: CGFloat = 0.355
    static let badgeStrokeRatio: CGFloat = 0.075
    static let minimumStroke: CGFloat = 1.75
    static let badgeFillAlpha: CGFloat = 0.27
    static let lampRadiusRatio: CGFloat = 0.043
    static let minimumLampRadius: CGFloat = 1.65
    static let lampHaloRatio: CGFloat = 0.105
    static let lampHaloAlpha: CGFloat = 0.18
    static let macCaseEdgeAlpha: CGFloat = 0.35
}

private enum IconColor {
    static let ground = CGColor(red: 0.0392, green: 0.0431, blue: 0.0353, alpha: 1)
    static let macCase = CGColor(red: 0.0706, green: 0.0824, blue: 0.0627, alpha: 1)
    static let macCaseEdge = CGColor(red: 0.1725, green: 0.1725, blue: 0.1608, alpha: 1)
    static let amber = CGColor(red: 0.8784, green: 0.6941, blue: 0.3686, alpha: 1)
}

private let iconSpecs: [IconSpec] = [
    IconSpec(filename: "mac-16@1x.png", pixels: 16, platform: .mac),
    IconSpec(filename: "mac-16@2x.png", pixels: 32, platform: .mac),
    IconSpec(filename: "mac-32@1x.png", pixels: 32, platform: .mac),
    IconSpec(filename: "mac-32@2x.png", pixels: 64, platform: .mac),
    IconSpec(filename: "mac-128@1x.png", pixels: 128, platform: .mac),
    IconSpec(filename: "mac-128@2x.png", pixels: 256, platform: .mac),
    IconSpec(filename: "mac-256@1x.png", pixels: 256, platform: .mac),
    IconSpec(filename: "mac-256@2x.png", pixels: 512, platform: .mac),
    IconSpec(filename: "mac-512@1x.png", pixels: 512, platform: .mac),
    IconSpec(filename: "mac-512@2x.png", pixels: 1024, platform: .mac),
    IconSpec(filename: "ipad-20@1x.png", pixels: 20, platform: .ipad),
    IconSpec(filename: "ipad-20@2x.png", pixels: 40, platform: .ipad),
    IconSpec(filename: "ipad-29@1x.png", pixels: 29, platform: .ipad),
    IconSpec(filename: "ipad-29@2x.png", pixels: 58, platform: .ipad),
    IconSpec(filename: "ipad-40@1x.png", pixels: 40, platform: .ipad),
    IconSpec(filename: "ipad-40@2x.png", pixels: 80, platform: .ipad),
    IconSpec(filename: "ipad-76@1x.png", pixels: 76, platform: .ipad),
    IconSpec(filename: "ipad-76@2x.png", pixels: 152, platform: .ipad),
    IconSpec(filename: "ipad-83.5@2x.png", pixels: 167, platform: .ipad),
    IconSpec(filename: "ipad-marketing@1x.png", pixels: 1024, platform: .ipad)
]

private func makeDiamond(center: CGPoint, radius: CGFloat) -> CGPath {
    let path = CGMutablePath()
    path.move(to: CGPoint(x: center.x, y: center.y - radius))
    path.addLine(to: CGPoint(x: center.x + radius, y: center.y))
    path.addLine(to: CGPoint(x: center.x, y: center.y + radius))
    path.addLine(to: CGPoint(x: center.x - radius, y: center.y))
    path.closeSubpath()
    return path
}

private func fillPath(_ context: CGContext, _ path: CGPath, color: CGColor) {
    context.setFillColor(color)
    context.addPath(path)
    context.fillPath()
}

private func drawCase(in context: CGContext, size: CGFloat) {
    let inset = size * IconGeometry.macInset
    let rect = CGRect(x: inset, y: inset, width: size - inset * 2, height: size - inset * 2)
    let radius = size * IconGeometry.macCornerRadius
    let path = CGPath(roundedRect: rect, cornerWidth: radius, cornerHeight: radius, transform: nil)
    fillPath(context, path, color: IconColor.macCase)
    context.setStrokeColor(IconColor.macCaseEdge.copy(alpha: IconGeometry.macCaseEdgeAlpha) ?? IconColor.macCaseEdge)
    context.setLineWidth(max(size * 0.008, 0.5))
    context.addPath(path)
    context.strokePath()
}

private func drawBadge(in context: CGContext, size: CGFloat, platform: IconPlatform) {
    let center = CGPoint(x: size / 2, y: size / 2)
    let radiusRatio = platform == .mac ? IconGeometry.macBadgeRadius : IconGeometry.ipadBadgeRadius
    let radius = size * radiusRatio
    let badge = makeDiamond(center: center, radius: radius)
    let fill = IconColor.amber.copy(alpha: IconGeometry.badgeFillAlpha) ?? IconColor.amber
    fillPath(context, badge, color: fill)
    context.setStrokeColor(IconColor.amber)
    context.setLineWidth(max(size * IconGeometry.badgeStrokeRatio, IconGeometry.minimumStroke))
    context.setLineCap(.round)
    context.setLineJoin(.round)
    context.addPath(badge)
    context.strokePath()

    let haloRadius = max(size * IconGeometry.lampHaloRatio, IconGeometry.minimumLampRadius * 1.75)
    context.setFillColor(IconColor.amber.copy(alpha: IconGeometry.lampHaloAlpha) ?? IconColor.amber)
    context.fillEllipse(in: CGRect(x: center.x - haloRadius, y: center.y - haloRadius,
                                   width: haloRadius * 2, height: haloRadius * 2))
    let lampRadius = max(size * IconGeometry.lampRadiusRatio, IconGeometry.minimumLampRadius)
    context.setFillColor(IconColor.amber)
    context.fillEllipse(in: CGRect(x: center.x - lampRadius, y: center.y - lampRadius,
                                   width: lampRadius * 2, height: lampRadius * 2))
}

private func makeImage(size: Int, platform: IconPlatform) throws -> CGImage {
    guard let context = CGContext(data: nil, width: size, height: size, bitsPerComponent: 8,
                                  bytesPerRow: 0, space: CGColorSpaceCreateDeviceRGB(),
                                  bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else {
        throw NSError(domain: "AIDIcon", code: 1, userInfo: [NSLocalizedDescriptionKey: "Unable to create bitmap context"])
    }
    context.setAllowsAntialiasing(true)
    context.setShouldAntialias(true)
    context.setFillColor(IconColor.ground)
    context.fill(CGRect(x: 0, y: 0, width: size, height: size))
    if platform == .mac {
        drawCase(in: context, size: CGFloat(size))
    }
    drawBadge(in: context, size: CGFloat(size), platform: platform)
    guard let image = context.makeImage() else {
        throw NSError(domain: "AIDIcon", code: 2, userInfo: [NSLocalizedDescriptionKey: "Unable to create icon image"])
    }
    return image
}

private func writePNG(_ image: CGImage, to url: URL) throws {
    guard let destination = CGImageDestinationCreateWithURL(url as CFURL, "public.png" as CFString, 1, nil) else {
        throw NSError(domain: "AIDIcon", code: 3, userInfo: [NSLocalizedDescriptionKey: "Unable to create PNG destination"])
    }
    CGImageDestinationAddImage(destination, image, nil)
    guard CGImageDestinationFinalize(destination) else {
        throw NSError(domain: "AIDIcon", code: 4, userInfo: [NSLocalizedDescriptionKey: "Unable to finalize PNG"])
    }
}

private func renderIcons(to outputDirectory: URL) throws {
    try FileManager.default.createDirectory(at: outputDirectory, withIntermediateDirectories: true)
    for spec in iconSpecs {
        let url = outputDirectory.appendingPathComponent(spec.filename)
        let image = try makeImage(size: spec.pixels, platform: spec.platform)
        try writePNG(image, to: url)
    }
}

guard CommandLine.arguments.count == 2 else {
    throw NSError(domain: "AIDIcon", code: 5,
                  userInfo: [NSLocalizedDescriptionKey: "Usage: swift client/tools/make-icon.swift <outdir>"])
}

do {
    try renderIcons(to: URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true))
} catch {
    let message = "AID icon generation failed: \(error.localizedDescription)\n"
    FileHandle.standardError.write(Data(message.utf8))
    Foundation.exit(1)
}
