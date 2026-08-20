// Live fleet source placeholder — wired in a later task.
// Exports: LiveSource.

import Foundation

final class LiveSource: FleetDataSource, @unchecked Sendable {
    func currentSnapshot() -> FleetSnapshot { DemoDataset.initialSnapshot() }

    func snapshots() -> AsyncStream<FleetSnapshot> {
        AsyncStream { $0.finish() }
    }

    func detail(_ id: MissionID) async throws -> MissionDetail {
        throw LiveSourceError.notImplemented
    }

    func diff(_ id: MissionID) async throws -> String {
        throw LiveSourceError.notImplemented
    }

    func result(_ id: MissionID) async throws -> String {
        throw LiveSourceError.notImplemented
    }

    func act(_ action: MissionAction, on id: MissionID) async throws -> MissionActionResult {
        throw LiveSourceError.notImplemented
    }
}

enum LiveSourceError: Error {
    case notImplemented
}
