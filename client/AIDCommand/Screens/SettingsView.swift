// Settings sheet/window — theme picker, commander, and live server connect flow.
// Exports: SettingsView.

import SwiftUI

struct SettingsView: View {
    @Environment(\.theme) private var theme
    @Bindable var themeManager: ThemeManager
    @Binding var commanderName: String
    @Bindable var store: FleetStore
    @State private var host: String = ""
    @State private var port: String = "8080"
    @State private var token: String = ""
    @State private var source: DataSourceKind = .demo
    @State private var statusText: String?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: theme.spacing.xl) {
                MonoLabel(text: "settings")
                themeSection
                commanderSection
                sourceSection
            }
            .padding(theme.spacing.xl)
        }
        .background(theme.bgDeep)
        .frame(minWidth: 520, minHeight: 520)
        .onAppear(perform: loadFields)
    }

    private var themeSection: some View {
        VStack(alignment: .leading, spacing: theme.spacing.md) {
            MonoLabel(text: "theme")
            HStack(spacing: theme.spacing.lg) {
                ForEach(ThemeKind.allCases, id: \.self) { kind in
                    themePreview(kind)
                }
            }
        }
    }

    private func themePreview(_ kind: ThemeKind) -> some View {
        let tokens = themeFor(kind)
        let selected = themeManager.kind == kind
        return Button {
            withAnimation(.easeInOut(duration: tokens.motion.crossfade)) {
                themeManager.kind = kind
            }
        } label: {
            VStack(alignment: .leading, spacing: theme.spacing.sm) {
                ZStack {
                    tokens.bgDeep
                    HStack(spacing: 8) {
                        Circle().fill(tokens.run).frame(width: 8, height: 8)
                        RoundedRectangle(cornerRadius: tokens.motion.stepped ? 0 : 2)
                            .fill(tokens.accent)
                            .frame(width: 40, height: 8)
                        Rectangle().fill(tokens.panelEdge).frame(height: 2)
                    }
                    .padding(12)
                }
                .frame(width: 180, height: 100)
                .clipShape(PanelShape(style: tokens.panelStyle))
                .overlay(
                    PanelShape(style: tokens.panelStyle)
                        .stroke(selected ? tokens.accent : tokens.panelEdge, lineWidth: selected ? 2 : 1)
                )
                Text(kind.rawValue.uppercased())
                    .font(tokens.font(.label))
                    .foregroundStyle(selected ? tokens.accent : tokens.ink2)
            }
        }
        .buttonStyle(.plain)
    }

    private var commanderSection: some View {
        VStack(alignment: .leading, spacing: theme.spacing.sm) {
            MonoLabel(text: "commander name")
            TextField("CMDR", text: $commanderName)
                .textFieldStyle(.plain)
                .font(theme.font(.body))
                .padding(8)
                .background(theme.bg)
                .overlay(Rectangle().stroke(theme.panelEdge, lineWidth: theme.hairline))
        }
    }

    private var sourceSection: some View {
        VStack(alignment: .leading, spacing: theme.spacing.sm) {
            MonoLabel(text: "data source")
            Picker("source", selection: $source) {
                Text("Demo").tag(DataSourceKind.demo)
                Text("Live").tag(DataSourceKind.live)
            }
            .pickerStyle(.segmented)
            MonoLabel(text: "server host")
            TextField("127.0.0.1", text: $host)
                .textFieldStyle(.plain)
                .font(theme.font(.body))
                .padding(8)
                .background(theme.bg)
            MonoLabel(text: "port")
            TextField("8080", text: $port)
                .textFieldStyle(.plain)
                .font(theme.font(.body))
                .padding(8)
                .background(theme.bg)
            MonoLabel(text: "token (keychain)")
            SecureField("bearer token", text: $token)
                .textFieldStyle(.plain)
                .font(theme.font(.body))
                .padding(8)
                .background(theme.bg)
            HStack(spacing: theme.spacing.sm) {
                ThemedButton(title: "Save / Connect") { saveConnection() }
                ThemedButton(title: "Test") { Task { await testConnection() } }
            }
            if let statusText {
                Text(statusText)
                    .font(theme.font(.caption))
                    .foregroundStyle(theme.ink2)
                    .textSelection(.enabled)
            }
        }
    }

    private func loadFields() {
        let config = store.connectionConfig
        host = config.host
        port = String(config.port)
        source = config.source
        token = config.token ?? KeychainTokenStore.load() ?? ""
        statusText = store.probeResult
    }

    private func saveConnection() {
        let parsedPort = Int(port) ?? 8080
        var config = ConnectionConfig(
            host: host.trimmingCharacters(in: .whitespacesAndNewlines),
            port: parsedPort,
            source: source,
            token: token
        )
        do {
            try store.applyConnection(config)
            statusText = source == .live ? "connected settings saved" : "demo source selected"
        } catch {
            statusText = error.localizedDescription
        }
    }

    private func testConnection() async {
        let parsedPort = Int(port) ?? 8080
        let config = ConnectionConfig(
            host: host.trimmingCharacters(in: .whitespacesAndNewlines),
            port: parsedPort,
            source: .live,
            token: token
        )
        statusText = "testing…"
        statusText = await ConnectionProbe.test(config: config)
        store.probeResult = statusText
    }
}
