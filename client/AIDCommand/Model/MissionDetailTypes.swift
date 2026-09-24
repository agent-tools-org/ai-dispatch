// Mission detail and agent roster types for the brief panel.
// Exports: MissionEvent, QuotaState, AgentInfo, MissionDetail, MissionActionResult.

import Foundation

struct MissionEvent: Sendable, Equatable, Identifiable {
    let id: String
    let message: String
}

/// Quota state from `/api/fleet`. `unknown` means no probe observed the route:
/// absent evidence, never a failure.
enum QuotaState: String, Sendable, Equatable {
    case ok, unknown, degraded, partial, limited

    /// Absent or unrecognised wire values carry no evidence, so they read as `unknown`.
    init(wire: String?) {
        self = wire.flatMap(QuotaState.init(rawValue:)) ?? .unknown
    }
}

struct AgentInfo: Identifiable, Sendable, Equatable {
    let id: String
    let busy: Bool
    let quota: QuotaState
    /// nil when the server did not measure a trustworthy count.
    let taskCount: Int?
}

struct MissionDetail: Sendable, Equatable {
    let mission: Mission
    let prompt: String
    let events: [MissionEvent]
}

struct MissionActionResult: Sendable, Equatable {
    let ok: Bool
    let message: String
}
