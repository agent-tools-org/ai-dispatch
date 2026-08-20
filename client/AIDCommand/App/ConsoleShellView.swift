// Console shell — assembles HUD, gauges, rail, center tabs, brief band.
// Exports: ConsoleShellView.

import SwiftUI

struct ConsoleShellView: View {
    @Environment(\.theme) private var theme
    @Bindable var store: FleetStore
    @Bindable var themeManager: ThemeManager

    var body: some View {
        ConsoleLayoutReader { layout in
            ZStack {
                VStack(spacing: 0) {
                    HUDBarView(
                        snapshot: store.snapshot,
                        xpState: store.xpState,
                        commanderName: store.commanderName,
                        selectedTab: tabBinding,
                        onToggleRail: { store.showLeftRail.toggle() }
                    )
                    GaugeStripView(summary: store.snapshot.summary)
                        .padding(.horizontal, theme.spacing.md)
                    HStack(spacing: 0) {
                        if layout != .compact {
                            LeftRailView(
                                sectors: store.snapshot.sectors,
                                selectedSectorID: sectorBinding
                            )
                            .frame(width: layout.leftRailWidth)
                            .frame(maxHeight: .infinity, alignment: .top)
                            .layoutPriority(1)
                        }
                        centerContent
                            .frame(maxWidth: .infinity, maxHeight: .infinity)
                    }
                    .frame(maxHeight: .infinity)
                    .frame(minHeight: 0)
                    if layout.showsBottomBrief {
                        MissionBriefView(
                            snapshot: store.snapshot,
                            detail: store.missionDetail,
                            actionMessage: store.actionMessage,
                            onAction: { action in
                                Task { await store.performAction(action) }
                            }
                        )
                        .frame(height: 268)
                    }
                }
                ScreenFrame()
                ToastStack(toasts: store.toasts)
            }
            .background { ConsoleBackground() }
            .sheet(isPresented: $store.showLeftRail) {
                if layout == .compact {
                    NavigationStack {
                        LeftRailView(
                            sectors: store.snapshot.sectors,
                            selectedSectorID: sectorBinding
                        )
                        .navigationTitle("Tactical Scan")
                    }
                }
            }
        }
    }

    private var tabBinding: Binding<CenterTab> {
        Binding(
            get: { store.selectedTab },
            set: { store.persistTab($0) }
        )
    }

    private var sectorBinding: Binding<String?> {
        Binding(
            get: { store.selectedSectorID },
            set: { store.selectSector($0) }
        )
    }

    @ViewBuilder
    private var centerContent: some View {
        switch store.selectedTab {
        case .fleetLog:
            FleetLogView(
                sectors: store.snapshot.sectors,
                selectedMissionID: missionBinding
            )
        case .hangar:
            if let sector = store.selectedSector {
                HangarView(sector: sector, selectedMissionID: missionBinding)
            } else {
                HangarView(
                    sector: store.snapshot.sectors[0],
                    selectedMissionID: missionBinding
                )
            }
        case .cargo:
            CargoView(
                sectors: store.snapshot.sectors,
                selectedMissionID: missionBinding
            )
        }
    }

    private var missionBinding: Binding<MissionID?> {
        Binding(
            get: { store.selectedMissionID },
            set: { store.selectMission($0) }
        )
    }
}
