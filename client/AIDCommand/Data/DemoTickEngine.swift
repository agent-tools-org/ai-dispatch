// Pure demo tick logic — advances running missions, completes/loses on schedule.
// Exports: DemoTickEngine.

import Foundation

enum DemoTickEngine {
    static let completeInterval = 13
    static let failInterval = 29

    struct Result: Sendable {
        var snapshot: FleetSnapshot
        var toasts: [ToastEvent]
        var xpAwards: [(MissionDisplayState, Int)]
    }

    static func tick(_ snapshot: FleetSnapshot, xpPayload: Int = 140) -> Result {
        var sectors = snapshot.sectors
        var toasts: [ToastEvent] = []
        var xpAwards: [(MissionDisplayState, Int)] = []
        let tick = snapshot.tick + 1
        let medians = ProgressDeriver.medianDurations(from: sectors)

        for sIdx in sectors.indices {
            for mIdx in sectors[sIdx].missions.indices {
                var mission = sectors[sIdx].missions[mIdx]
                guard mission.state == .run else { continue }
                mission = advanceRunning(mission, medians: medians)
                sectors[sIdx].missions[mIdx] = mission
            }
        }

        if tick % completeInterval == 0, let loc = firstRunning(in: sectors) {
            sectors[loc.s].missions[loc.m] = finish(
                sectors[loc.s].missions[loc.m], state: .done
            )
            let mission = sectors[loc.s].missions[loc.m]
            toasts.append(toast(for: mission, xp: xpPayload))
            xpAwards.append((.done, xpPayload))
        }

        if tick % failInterval == 0, let loc = lastRunning(in: sectors) {
            sectors[loc.s].missions[loc.m] = finish(
                sectors[loc.s].missions[loc.m], state: .fail
            )
            let mission = sectors[loc.s].missions[loc.m]
            toasts.append(toast(for: mission, xp: 20))
            xpAwards.append((.fail, 20))
        }

        var summary = snapshot.summary
        summary.running = countState(.run, in: sectors)
        summary.done = countState(.done, in: sectors)
        summary.failed = countState(.fail, in: sectors)

        let updated = FleetSnapshot(
            sectors: sectors,
            summary: summary,
            serverVersion: snapshot.serverVersion,
            connection: snapshot.connection,
            agents: DemoAgents.roster(from: sectors),
            tick: tick
        )
        return Result(snapshot: updated, toasts: toasts, xpAwards: xpAwards)
    }

    private struct Loc: Equatable { let s: Int; let m: Int }

    private static func advanceRunning(_ mission: Mission, medians: [String: [Int]]) -> Mission {
        let elapsed = mission.elapsedSeconds + 1
        let progress = ProgressDeriver.progress(
            elapsedSeconds: elapsed,
            agent: mission.agent,
            completedDurations: medians
        )
        return Mission(
            id: mission.id,
            title: mission.title,
            agent: mission.agent,
            model: mission.model,
            state: mission.state,
            threat: mission.threat,
            progress: progress,
            elapsedSeconds: elapsed,
            tokens: mission.tokens,
            cost: mission.cost,
            memoryMB: mission.memoryMB,
            verifyTag: mission.verifyTag,
            awaitingReason: mission.awaitingReason
        )
    }

    private static func finish(_ mission: Mission, state: MissionDisplayState) -> Mission {
        Mission(
            id: mission.id,
            title: mission.title,
            agent: mission.agent,
            model: mission.model,
            state: state,
            threat: mission.threat,
            progress: state == .done ? 1 : mission.progress,
            elapsedSeconds: mission.elapsedSeconds,
            tokens: mission.tokens,
            cost: mission.cost,
            memoryMB: mission.memoryMB,
            verifyTag: mission.verifyTag,
            awaitingReason: mission.awaitingReason
        )
    }

    private static func firstRunning(in sectors: [Sector]) -> Loc? {
        for (s, sector) in sectors.enumerated() {
            if let m = sector.missions.firstIndex(where: { $0.state == .run }) {
                return Loc(s: s, m: m)
            }
        }
        return nil
    }

    private static func lastRunning(in sectors: [Sector]) -> Loc? {
        for (s, sector) in sectors.enumerated().reversed() {
            if let m = sector.missions.lastIndex(where: { $0.state == .run }) {
                return Loc(s: s, m: m)
            }
        }
        return nil
    }

    private static func toast(for mission: Mission, xp: Int) -> ToastEvent {
        ToastEvent(id: UUID(), missionID: mission.id, title: mission.title, state: mission.state, xpAward: xp)
    }

    private static func countState(_ state: MissionDisplayState, in sectors: [Sector]) -> Int {
        sectors.flatMap(\.missions).filter { $0.state == state }.count
    }
}
