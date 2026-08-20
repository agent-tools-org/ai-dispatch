// Fleet data source protocol — demo and live implementations share this contract.
// Exports: FleetDataSource, MissionAction.

import Foundation

enum MissionAction: Sendable {
    case abort, relaunch, steer, dock
}

protocol FleetDataSource: Sendable {
    func snapshots() -> AsyncStream<FleetSnapshot>
    func detail(_ id: MissionID) async throws -> Mission
    func diff(_ id: MissionID) async throws -> String
    func result(_ id: MissionID) async throws -> String
    func act(_ action: MissionAction, on id: MissionID) async throws
}
