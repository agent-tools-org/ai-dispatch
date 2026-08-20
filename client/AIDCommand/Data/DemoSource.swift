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

    func currentSnapshot() -> FleetSnapshot { snapshot }

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

    func detail(_ id: MissionID) async throws -> MissionDetail {
        guard let mission = findMission(id) else { throw DemoSourceError.notFound }
        return MissionDetail(
            mission: mission,
            prompt: DemoMissionContent.prompt(for: mission),
            events: DemoMissionContent.events(for: mission)
        )
    }

    func diff(_ id: MissionID) async throws -> String {
        guard let mission = findMission(id) else { throw DemoSourceError.notFound }
        guard mission.state == .done else {
            throw DemoSourceError.rejected("no diff for \(mission.state) mission")
        }
        return "diff --git a/src/\(mission.id).rs\n+ // demo diff for \(mission.title)"
    }

    func result(_ id: MissionID) async throws -> String {
        guard let mission = findMission(id) else { throw DemoSourceError.notFound }
        guard mission.state == .done else {
            throw DemoSourceError.rejected("no result for \(mission.state) mission")
        }
        return "# \(mission.title)\n\nDemo report for \(mission.id)."
    }

    func act(_ action: MissionAction, on id: MissionID) async throws -> MissionActionResult {
        guard let loc = findLocation(id) else { throw DemoSourceError.notFound }
        var mission = snapshot.sectors[loc.s].missions[loc.m]
        switch action {
        case .abort:
            guard mission.state == .run else {
                return MissionActionResult(ok: false, message: "mission is not running")
            }
            mission = copy(mission, state: .stop, progress: mission.progress)
        case .relaunch:
            guard mission.state == .fail || mission.state == .stop else {
                return MissionActionResult(ok: false, message: "mission is not relaunchable")
            }
            mission = copy(mission, state: .run, progress: 0.05, elapsed: 0)
        case .steer(let message):
            guard mission.state == .run else {
                return MissionActionResult(ok: false, message: "cannot steer a non-running mission")
            }
            guard !message.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
                return MissionActionResult(ok: false, message: "steer message required")
            }
            return MissionActionResult(ok: true, message: "steer queued: \(message.prefix(40))")
        case .diff:
            _ = try await diff(id)
            return MissionActionResult(ok: true, message: "diff ready")
        case .export:
            _ = try await result(id)
            return MissionActionResult(ok: true, message: "export ready")
        case .dock:
            guard mission.state == .done else {
                return MissionActionResult(ok: false, message: "only complete missions can dock")
            }
            return MissionActionResult(ok: true, message: "merge queued for \(mission.id)")
        }
        snapshot.sectors[loc.s].missions[loc.m] = mission
        refreshSummary()
        return MissionActionResult(ok: true, message: StatusMapper.label(for: mission.state))
    }

    func tickOnce() -> DemoTickEngine.Result {
        let result = DemoTickEngine.tick(snapshot)
        snapshot = result.snapshot
        return result
    }

    private func findMission(_ id: MissionID) -> Mission? {
        for sector in snapshot.sectors {
            if let mission = sector.missions.first(where: { $0.id == id }) { return mission }
        }
        return nil
    }

    private struct Loc { let s: Int; let m: Int }

    private func findLocation(_ id: MissionID) -> Loc? {
        for (s, sector) in snapshot.sectors.enumerated() {
            if let m = sector.missions.firstIndex(where: { $0.id == id }) { return Loc(s: s, m: m) }
        }
        return nil
    }

    private func copy(
        _ mission: Mission,
        state: MissionDisplayState,
        progress: Double,
        elapsed: Int? = nil
    ) -> Mission {
        Mission(
            id: mission.id,
            title: mission.title,
            agent: mission.agent,
            model: mission.model,
            state: state,
            threat: mission.threat,
            progress: progress,
            elapsedSeconds: elapsed ?? mission.elapsedSeconds,
            startedAt: state == .run
                ? (mission.startedAt ?? Date().addingTimeInterval(TimeInterval(-(elapsed ?? mission.elapsedSeconds))))
                : nil,
            tokens: mission.tokens,
            cost: mission.cost,
            memoryMB: mission.memoryMB,
            verifyTag: mission.verifyTag,
            awaitingReason: mission.awaitingReason
        )
    }

    private func refreshSummary() {
        snapshot.summary.running = snapshot.sectors.flatMap(\.missions).filter { $0.state == .run }.count
        snapshot.summary.done = snapshot.sectors.flatMap(\.missions).filter { $0.state == .done }.count
        snapshot.summary.failed = snapshot.sectors.flatMap(\.missions).filter { $0.state == .fail }.count
        snapshot.agents = DemoAgents.roster(from: snapshot.sectors)
    }
}

enum DemoSourceError: Error {
    case notFound
    case notSupported
    case rejected(String)
}
