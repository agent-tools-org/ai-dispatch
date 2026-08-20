// Client-side XP and rank math — persisted locally, not from server.
// Exports: XPState, RankCalculator.

import Foundation

struct XPState: Sendable, Equatable {
    var xp: Int

    static let defaultXP = 4280

    init(xp: Int = defaultXP) {
        self.xp = xp
    }

    mutating func award(for state: MissionDisplayState, payloadXP: Int = 140) {
        switch state {
        case .done: xp += payloadXP
        case .fail: xp += 20
        default: break
        }
    }

    var rankIndex: Int { min(7, xp / 1000) }
    var rankLabel: String { RankCalculator.label(for: rankIndex) }
    var barProgress: Double { Double(xp % 1000) / 1000 }
}

enum RankCalculator {
    static let ranks = ["I", "II", "III", "IV", "V", "VI", "VII", "VIII"]

    static func label(for index: Int) -> String {
        let clamped = min(max(index, 0), ranks.count - 1)
        return ranks[clamped]
    }
}
