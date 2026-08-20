// Main fleet console store — snapshot stream, selection, XP, toasts.
// Exports: FleetStore.

import Foundation
import SwiftUI

@MainActor
@Observable
final class FleetStore {
    var snapshot: FleetSnapshot
    var selectedTab: CenterTab = .fleetLog
    var selectedSectorID: String?
    var selectedMissionID: MissionID?
    var xpState: XPState
    var toasts: [ToastEvent] = []
    var commanderName: String
    var showSettings = false
    var showLeftRail = false

    private var streamTask: Task<Void, Never>?
    private let demoSource = DemoSource()

    init() {
        snapshot = DemoDataset.initialSnapshot()
        xpState = XPState(xp: UserDefaults.standard.integer(forKey: Self.xpKey).nonZeroOr(4280))
        commanderName = UserDefaults.standard.string(forKey: Self.commanderKey) ?? "CMDR"
        selectedSectorID = snapshot.sectors.first?.id
        startDemoStream()
    }

    func startDemoStream() {
        streamTask?.cancel()
        var previous = snapshot
        streamTask = Task {
            for await update in demoSource.snapshots() {
                guard !Task.isCancelled else { break }
                handleUpdate(update, previous: &previous)
            }
        }
    }

    func selectMission(_ id: MissionID?) {
        selectedMissionID = id
    }

    func selectSector(_ id: String?) {
        selectedSectorID = id
    }

    func dismissToast(_ id: UUID) {
        toasts.removeAll { $0.id == id }
    }

    func persistCommanderName(_ name: String) {
        commanderName = name
        UserDefaults.standard.set(name, forKey: Self.commanderKey)
    }

    private func handleUpdate(_ update: FleetSnapshot, previous: inout FleetSnapshot) {
        detectTransitions(from: previous, to: update)
        snapshot = update
        previous = update
    }

    private func detectTransitions(from old: FleetSnapshot, to new: FleetSnapshot) {
        let oldMap = missionMap(old)
        for sector in new.sectors {
            for mission in sector.missions {
                guard let prior = oldMap[mission.id], prior.state == .run else { continue }
                if mission.state == .done || mission.state == .fail {
                    let xp = mission.state == .done ? PayloadRarity.rare.xp : 20
                    xpState.award(for: mission.state, payloadXP: xp)
                    UserDefaults.standard.set(xpState.xp, forKey: Self.xpKey)
                    let toast = ToastEvent(
                        id: UUID(), missionID: mission.id, title: mission.title,
                        state: mission.state, xpAward: xp
                    )
                    toasts.append(toast)
                    scheduleDismiss(toast.id)
                }
            }
        }
    }

    private func scheduleDismiss(_ id: UUID) {
        Task {
            try? await Task.sleep(for: .seconds(5))
            dismissToast(id)
        }
    }

    private func missionMap(_ snapshot: FleetSnapshot) -> [MissionID: Mission] {
        var map: [MissionID: Mission] = [:]
        for sector in snapshot.sectors {
            for mission in sector.missions {
                map[mission.id] = mission
            }
        }
        return map
    }

    private static let xpKey = "aid.command.xp"
    private static let commanderKey = "aid.command.commander"
}

private extension Int {
    func nonZeroOr(_ fallback: Int) -> Int {
        self == 0 ? fallback : self
    }
}
