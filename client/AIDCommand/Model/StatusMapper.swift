// Maps aid status strings to the four display states and verify tags.
// Exports: StatusMapper.

import Foundation

enum StatusMapper {
    static func displayState(from aidStatus: String) -> MissionDisplayState {
        switch aidStatus.lowercased() {
        case "running", "waiting", "pending", "awaiting_input", "stalled":
            return .run
        case "done", "merged":
            return .done
        case "failed":
            return .fail
        case "stopped", "skipped":
            return .stop
        default:
            return .run
        }
    }

    static func label(for state: MissionDisplayState) -> String {
        switch state {
        case .run: return "RUNNING"
        case .done: return "COMPLETE"
        case .fail: return "LOST"
        case .stop: return "HELD"
        }
    }

    static func mark(for state: MissionDisplayState) -> String {
        switch state {
        case .run: return "▶"
        case .done: return "✦"
        case .fail: return "✕"
        case .stop: return "⏸"
        }
    }

    static func verifyTag(from outcome: String?) -> VerifyTag? {
        guard let outcome else { return nil }
        switch outcome.lowercased() {
        case "verified", "delivered", "in_progress":
            return nil
        case "failed":
            return .vfail
        case "timeout":
            return .vtimeout
        case "infra":
            return .vinfra
        case "no_result":
            return .vnoresult
        default:
            return .broken
        }
    }
}
