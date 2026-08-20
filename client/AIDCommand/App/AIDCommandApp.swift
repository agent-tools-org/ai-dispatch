// App entry point for the AID Command desktop client (macOS + iPadOS).
// Exports: AIDCommandApp — the SwiftUI @main scene.
// See client/DESIGN.md for the screen inventory and the two-theme contract.

import SwiftUI

@main
struct AIDCommandApp: App {
    var body: some Scene {
        WindowGroup {
            RootView()
        }
        #if os(macOS)
        .defaultSize(width: 1440, height: 900)
        #endif
    }
}

struct RootView: View {
    var body: some View {
        Text("AID · FLEET COMMAND")
            .font(.system(size: 17, weight: .semibold, design: .monospaced))
            .tracking(3)
            .foregroundStyle(Color(red: 0.88, green: 0.69, blue: 0.37))
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(Color(red: 0.04, green: 0.04, blue: 0.04))
    }
}
