// Core fleet model types — missions, sectors, snapshots, connection state.
// Exports: MissionID, Mission, Sector, FleetSummary, FleetSnapshot, etc.

import Foundation

typealias MissionID = String

enum MissionDisplayState: String, Sendable, CaseIterable {
    case run, done, fail, stop
}

enum VerifyTag: String, Sendable {
    case vfail = "VFAIL"
    case vtimeout = "VTIMEOUT"
    case vinfra = "VINFRA"
    case vnoresult = "VNORESULT"
    case broken = "BROKEN"
}

enum ConnectionState: Sendable, Equatable {
    case disconnected
    case connecting
    case live
    case degraded(age: TimeInterval)
    case error(String)
}

enum CenterTab: String, CaseIterable, Sendable {
    case fleetLog = "FLEET LOG"
    case hangar = "HANGAR"
    case cargo = "CARGO"
}

struct Mission: Identifiable, Sendable, Equatable {
    let id: MissionID
    let title: String
    let agent: String
    let model: String?
    let state: MissionDisplayState
    let threat: Int?
    let progress: Double
    let elapsedSeconds: Int
    /// When set on a RUN mission, UI derives a live elapsed tick from this instant.
    let startedAt: Date?
    let tokens: String?
    let cost: String?
    let memoryMB: String?
    let verifyTag: VerifyTag?
    let awaitingReason: String?
}

struct Sector: Identifiable, Sendable, Equatable {
    let id: String
    let tag: String
    let name: String
    let workgroupID: String
    var missions: [Mission]
}

struct FleetSummary: Sendable, Equatable {
    let window: String
    var running: Int
    var done: Int
    var failed: Int
    let spendUSD: String?
    let memoryMB: Int
    let sectorCount: Int
}

struct FleetSnapshot: Sendable, Equatable {
    var sectors: [Sector]
    var summary: FleetSummary
    var serverVersion: String
    var connection: ConnectionState
    var agents: [AgentInfo]
    var tick: Int
}

struct ToastEvent: Identifiable, Sendable, Equatable {
    let id: UUID
    let missionID: MissionID
    let title: String
    let state: MissionDisplayState
    let xpAward: Int
}

struct AppPreferences: Sendable {
    var commanderName: String
    var reduceMotion: Bool
    var useDemoSource: Bool
}
