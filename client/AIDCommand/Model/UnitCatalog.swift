// Known unit metadata — role and level for listed agents, unknown otherwise.
// Exports: UnitCatalog.

import Foundation

enum UnitCatalog {
    struct Profile: Sendable, Equatable {
        let role: String
        let level: Int
    }

    static func profile(for agent: String) -> Profile? {
        switch agent.lowercased() {
        case "codex": return Profile(role: "BUILDER", level: 7)
        case "cursor": return Profile(role: "PILOT", level: 5)
        case "grok": return Profile(role: "AUDITOR", level: 9)
        case "gemini": return Profile(role: "SCOUT", level: 4)
        case "opencode": return Profile(role: "ENGINEER", level: 6)
        case "kilo": return Profile(role: "SAPPER", level: 3)
        default: return nil
        }
    }
}
