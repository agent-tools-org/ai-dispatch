// Mission detail and agent roster types for the brief panel.
// Exports: MissionEvent, AgentInfo, MissionDetail, MissionActionResult.

import Foundation

struct MissionEvent: Sendable, Equatable, Identifiable {
    let id: String
    let message: String
}

struct AgentInfo: Identifiable, Sendable, Equatable {
    let id: String
    let busy: Bool
    let quotaOK: Bool
    let taskCount: Int
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
