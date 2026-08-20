// Demo fleet source — canvas dataset with 1s tick simulation.
// Exports: DemoSource.

import Foundation

final class DemoSource: FleetDataSource, @unchecked Sendable {
    private let interval: Duration
    private var snapshot: FleetSnapshot

    init(interval: Duration = .seconds(1)) {
        self.interval = interval
        self.snapshot = DemoDataset.initialSnapshot()
    }

    func snapshots() -> AsyncStream<FleetSnapshot> {
        AsyncStream { continuation in
            let task = Task {
                continuation.yield(self.snapshot)
                while !Task.isCancelled {
                    try? await Task.sleep(for: self.interval)
                    let result = DemoTickEngine.tick(self.snapshot)
                    self.snapshot = result.snapshot
                    continuation.yield(result.snapshot)
                }
                continuation.finish()
            }
            continuation.onTermination = { _ in task.cancel() }
        }
    }

    func detail(_ id: MissionID) async throws -> Mission {
        guard let mission = findMission(id) else {
            throw DemoSourceError.notFound
        }
        return mission
    }

    func diff(_ id: MissionID) async throws -> String { "—" }
    func result(_ id: MissionID) async throws -> String { "—" }

    func act(_ action: MissionAction, on id: MissionID) async throws {
        throw DemoSourceError.notSupported
    }

    func tickOnce() -> DemoTickEngine.Result {
        let result = DemoTickEngine.tick(snapshot)
        snapshot = result.snapshot
        return result
    }

    private func findMission(_ id: MissionID) -> Mission? {
        for sector in snapshot.sectors {
            if let mission = sector.missions.first(where: { $0.id == id }) {
                return mission
            }
        }
        return nil
    }
}

enum DemoSourceError: Error {
    case notFound
    case notSupported
}
