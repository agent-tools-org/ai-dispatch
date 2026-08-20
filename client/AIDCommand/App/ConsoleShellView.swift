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
                        selectedTab: $store.selectedTab,
                        onToggleRail: { store.showLeftRail.toggle() }
                    )
                    ThemedPanel {
                        GaugeStripView(summary: store.snapshot.summary)
                    }
                    .padding(.horizontal, theme.spacing.md)
                    HStack(spacing: 0) {
                        if layout != .compact {
                            LeftRailView(
                                sectors: store.snapshot.sectors,
                                selectedSectorID: $store.selectedSectorID
                            )
                            .frame(width: layout.leftRailWidth)
                        }
                        centerContent
                            .frame(maxWidth: .infinity, maxHeight: .infinity)
                    }
                    .frame(maxHeight: .infinity)
                    if layout.showsBottomBrief {
                        MissionBriefPlaceholder()
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
                            selectedSectorID: $store.selectedSectorID
                        )
                        .navigationTitle("Tactical Scan")
                    }
                }
            }
        }
    }

    @ViewBuilder
    private var centerContent: some View {
        switch store.selectedTab {
        case .fleetLog:
            FleetLogView(
                sectors: store.snapshot.sectors,
                selectedMissionID: $store.selectedMissionID
            )
        case .hangar:
            HangarPlaceholder()
        case .cargo:
            CargoPlaceholder()
        }
    }

}
