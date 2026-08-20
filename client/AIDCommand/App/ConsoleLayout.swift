// Console layout size class — one decision point for Mac/iPad adaptation.
// Exports: ConsoleLayout, ConsoleLayoutReader.

import SwiftUI

enum ConsoleLayout: Sendable, Equatable {
    case desktop
    case tabletLandscape
    case compact

    var leftRailWidth: CGFloat {
        switch self {
        case .desktop: return 290
        case .tabletLandscape: return 244
        case .compact: return 0
        }
    }

    var showsBottomBrief: Bool {
        self != .compact
    }

    static func classify(width: CGFloat) -> ConsoleLayout {
        if width >= 1180 { return .desktop }
        if width >= 1000 { return .tabletLandscape }
        return .compact
    }
}

private struct ConsoleLayoutKey: EnvironmentKey {
    static let defaultValue: ConsoleLayout = .desktop
}

extension EnvironmentValues {
    var consoleLayout: ConsoleLayout {
        get { self[ConsoleLayoutKey.self] }
        set { self[ConsoleLayoutKey.self] = newValue }
    }
}

struct ConsoleLayoutReader<Content: View>: View {
    @ViewBuilder let content: (ConsoleLayout) -> Content

    var body: some View {
        GeometryReader { geo in
            content(ConsoleLayout.classify(width: geo.size.width))
                .environment(\.consoleLayout, ConsoleLayout.classify(width: geo.size.width))
        }
    }
}
