// SwiftUI environment plumbing and theme manager with persistence.
// Exports: theme environment key, ThemeManager, themeFor(_:).

import SwiftUI

private struct ThemeKey: EnvironmentKey {
    static let defaultValue: any ThemeTokens = StarshipTheme()
}

extension EnvironmentValues {
    var theme: any ThemeTokens {
        get { self[ThemeKey.self] }
        set { self[ThemeKey.self] = newValue }
    }
}

func themeFor(_ kind: ThemeKind) -> any ThemeTokens {
    switch kind {
    case .starship: return StarshipTheme()
    case .pixel: return PixelTheme()
    }
}

@MainActor
@Observable
final class ThemeManager {
    var kind: ThemeKind {
        didSet { UserDefaults.standard.set(kind.rawValue, forKey: Self.storageKey) }
    }

    var tokens: any ThemeTokens { themeFor(kind) }

    init() {
        let raw = UserDefaults.standard.string(forKey: Self.storageKey) ?? ThemeKind.starship.rawValue
        kind = ThemeKind(rawValue: raw) ?? .starship
    }

    func toggle() {
        kind = kind == .starship ? .pixel : .starship
    }

    private static let storageKey = "aid.command.theme"
}
