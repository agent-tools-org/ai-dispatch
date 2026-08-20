// Priority chevrons for cargo payloads.
// Exports: RarityChevrons, PayloadRarity.

import SwiftUI

enum PayloadRarity: String, Sendable, CaseIterable {
    case legendary, epic, rare, uncommon, common, salvage

    var label: String {
        switch self {
        case .legendary: return "CRITICAL"
        case .epic: return "HIGH"
        case .rare: return "STANDARD"
        case .uncommon: return "LOW"
        case .common: return "ROUTINE"
        case .salvage: return "SALVAGE"
        }
    }

    var filledCount: Int {
        switch self {
        case .legendary: return 5
        case .epic: return 4
        case .rare: return 3
        case .uncommon: return 2
        case .common, .salvage: return 1
        }
    }

    var xp: Int {
        switch self {
        case .legendary: return 320
        case .epic: return 220
        case .rare: return 140
        case .uncommon: return 90
        case .common: return 50
        case .salvage: return 20
        }
    }
}

struct RarityChevrons: View {
    @Environment(\.theme) private var theme
    let rarity: PayloadRarity

    var body: some View {
        HStack(spacing: 1) {
            ForEach(0..<5, id: \.self) { index in
                Text(index < rarity.filledCount ? "▰" : "▱")
                    .font(theme.font(.caption))
                    .foregroundStyle(index < rarity.filledCount ? color : theme.ink3)
            }
            Text(rarity.label)
                .font(theme.font(.caption))
                .foregroundStyle(color)
                .padding(.leading, 4)
        }
    }

    private var color: Color {
        switch rarity {
        case .legendary: return theme.done
        case .epic: return theme.stop
        case .rare, .uncommon: return theme.ink2
        case .common: return theme.ink3
        case .salvage: return theme.fail
        }
    }
}
