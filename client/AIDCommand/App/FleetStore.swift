// Main fleet console store — snapshot stream, selection, XP, toasts, source switch.
// Exports: FleetStore.

import Foundation
import SwiftUI

@MainActor
@Observable
final class FleetStore {
    var snapshot: FleetSnapshot
    var selectedTab: CenterTab
    var selectedSectorID: String?
    var selectedMissionID: MissionID?
    var missionDetail: MissionDetail?
    var actionMessage: String?
    var xpState: XPState
    var toasts: [ToastEvent] = []
    var commanderName: String
    var showSettings = false
    var showLeftRail = false
    var connectionConfig: ConnectionConfig
    var probeResult: String?

    private var streamTask: Task<Void, Never>?
    private var detailTask: Task<Void, Never>?
    private let demoSource = DemoSource()
    private var liveSource: LiveSource?

    init() {
        connectionConfig = ConnectionConfig.load()
        snapshot = DemoDataset.initialSnapshot()
        xpState = XPState(xp: UserDefaults.standard.integer(forKey: Self.xpKey).nonZeroOr(4280))
        commanderName = UserDefaults.standard.string(forKey: Self.commanderKey) ?? "CMDR"
        selectedTab = Self.loadTab()
        selectedSectorID = Self.loadSectorID(fallback: snapshot.sectors.first?.id)
        startSource()
    }

    func startSource() {
        streamTask?.cancel()
        liveSource?.updateConfig(connectionConfig)
        if connectionConfig.source == .live {
            let source = liveSource ?? LiveSource(config: connectionConfig)
            source.updateConfig(connectionConfig)
            liveSource = source
            var previous = snapshot
            streamTask = Task {
                for await update in source.snapshots() {
                    guard !Task.isCancelled else { break }
                    handleUpdate(update, previous: &previous)
                }
            }
        } else {
            startDemoStream()
        }
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

    func applyConnection(_ config: ConnectionConfig) throws {
        try config.persist(token: config.token)
        connectionConfig = ConnectionConfig.load()
        if ProcessInfo.processInfo.environment["AID_BASE_URL"] != nil {
            connectionConfig = config
        }
        startSource()
    }

    func refresh() {
        startSource()
    }

    func runProbe() async {
        var config = connectionConfig
        config.token = config.token ?? KeychainTokenStore.load()
        probeResult = await ConnectionProbe.test(config: config)
    }

    func selectMission(_ id: MissionID?) {
        selectedMissionID = id
        loadDetail(for: id)
    }

    func selectSector(_ id: String?) {
        selectedSectorID = id
        persistSectorID(id)
    }

    func persistTab(_ tab: CenterTab) {
        selectedTab = tab
        UserDefaults.standard.set(tab.rawValue, forKey: Self.tabKey)
    }

    func applyLaunchState() {
        selectedTab = Self.loadTab()
        selectedSectorID = Self.loadSectorID(fallback: snapshot.sectors.first?.id)
    }

    func dismissToast(_ id: UUID) {
        toasts.removeAll { $0.id == id }
    }

    func persistCommanderName(_ name: String) {
        commanderName = name
        UserDefaults.standard.set(name, forKey: Self.commanderKey)
    }

    func performAction(_ action: MissionAction) async {
        guard let id = selectedMissionID else {
            actionMessage = "no mission selected"
            return
        }
        do {
            let result = try await activeSource().act(action, on: id)
            snapshot = activeSource().currentSnapshot()
            actionMessage = result.message
            if result.ok {
                loadDetail(for: id)
            }
        } catch {
            actionMessage = LiveSource.message(for: error)
        }
    }

    var selectedSector: Sector? {
        guard let selectedSectorID else { return snapshot.sectors.first }
        return snapshot.sectors.first { $0.id == selectedSectorID } ?? snapshot.sectors.first
    }

    private func activeSource() -> any FleetDataSource {
        if connectionConfig.source == .live, let liveSource {
            return liveSource
        }
        return demoSource
    }

    private func loadDetail(for id: MissionID?) {
        detailTask?.cancel()
        guard let id else {
            missionDetail = nil
            return
        }
        detailTask = Task {
            missionDetail = try? await activeSource().detail(id)
        }
    }

    private func handleUpdate(_ update: FleetSnapshot, previous: inout FleetSnapshot) {
        detectTransitions(from: previous, to: update)
        snapshot = update
        if selectedSectorID == nil {
            selectedSectorID = update.sectors.first?.id
        }
        if let id = selectedMissionID {
            loadDetail(for: id)
        }
        previous = update
    }

    private func detectTransitions(from old: FleetSnapshot, to new: FleetSnapshot) {
        let oldMap = missionMap(old)
        for sector in new.sectors {
            for mission in sector.missions {
                guard let prior = oldMap[mission.id], prior.state == .run else { continue }
                if mission.state == .done || mission.state == .fail {
                    let payload = PayloadDeriver.derive(from: mission, sectorTag: sector.tag)
                    let xp = mission.state == .done ? (payload?.rarity.xp ?? PayloadRarity.rare.xp) : 20
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

    private static func loadTab() -> CenterTab {
        guard let raw = UserDefaults.standard.string(forKey: tabKey),
              let tab = CenterTab(rawValue: raw) else {
            return .fleetLog
        }
        return tab
    }

    private static func loadSectorID(fallback: String?) -> String? {
        UserDefaults.standard.string(forKey: sectorKey) ?? fallback
    }

    private func persistSectorID(_ id: String?) {
        if let id {
            UserDefaults.standard.set(id, forKey: Self.sectorKey)
        }
    }

    private static let xpKey = "aid.command.xp"
    private static let commanderKey = "aid.command.commander"
    private static let tabKey = "aid.command.tab"
    private static let sectorKey = "aid.command.sector"
}

private extension Int {
    func nonZeroOr(_ fallback: Int) -> Int {
        self == 0 ? fallback : self
    }
}
