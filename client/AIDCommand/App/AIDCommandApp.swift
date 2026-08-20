// App entry point for the AID Command desktop client (macOS + iPadOS).
// Exports: AIDCommandApp — the SwiftUI @main scene.

import SwiftUI

@main
struct AIDCommandApp: App {
    @State private var themeManager = ThemeManager()
    @State private var store = FleetStore()

    var body: some Scene {
        WindowGroup {
            RootView(store: store, themeManager: themeManager)
                .environment(\.theme, themeManager.tokens)
                .animation(.easeInOut(duration: themeManager.tokens.motion.crossfade), value: themeManager.kind)
        }
        #if os(macOS)
        .defaultSize(width: 1440, height: 900)
        .commands {
            CommandGroup(replacing: .appSettings) {
                Button("Settings…") { store.showSettings = true }
                    .keyboardShortcut(",", modifiers: .command)
            }
            CommandMenu("View") {
                Button("Toggle Theme") { themeManager.toggle() }
                    .keyboardShortcut("t", modifiers: .command)
                ForEach(CenterTab.allCases, id: \.self) { tab in
                    Button(tab.rawValue) { store.selectedTab = tab }
                }
            }
        }
        #endif

        #if os(macOS)
        Settings {
            SettingsView(
                themeManager: themeManager,
                commanderName: Binding(
                    get: { store.commanderName },
                    set: { store.persistCommanderName($0) }
                )
            )
            .environment(\.theme, themeManager.tokens)
        }
        #endif
    }
}

struct RootView: View {
    @Bindable var store: FleetStore
    @Bindable var themeManager: ThemeManager

    var body: some View {
        ConsoleShellView(store: store, themeManager: themeManager)
            .environment(\.theme, themeManager.tokens)
            #if os(iOS)
            .sheet(isPresented: $store.showSettings) {
                SettingsView(
                    themeManager: themeManager,
                    commanderName: Binding(
                        get: { store.commanderName },
                        set: { store.persistCommanderName($0) }
                    )
                )
                .environment(\.theme, themeManager.tokens)
            }
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button { store.showSettings = true } label: {
                        Image(systemName: "gearshape")
                    }
                }
            }
            #endif
    }
}
