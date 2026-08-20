// Persisted connection preferences and launch env override for LiveSource.
// Exports: ConnectionConfig, DataSourceKind.

import Foundation

enum DataSourceKind: String, Sendable, CaseIterable {
    case demo
    case live
}

struct ConnectionConfig: Sendable, Equatable {
    var host: String
    var port: Int
    var source: DataSourceKind
    var token: String?

    var baseURL: URL? {
        var components = URLComponents()
        components.scheme = "http"
        components.host = host
        components.port = port
        return components.url
    }

    static func load() -> ConnectionConfig {
        if let override = fromEnvironment() { return override }
        let defaults = UserDefaults.standard
        let host = defaults.string(forKey: Keys.host) ?? "127.0.0.1"
        let port = defaults.object(forKey: Keys.port) as? Int ?? 8080
        let sourceRaw = defaults.string(forKey: Keys.source) ?? DataSourceKind.demo.rawValue
        let source = DataSourceKind(rawValue: sourceRaw) ?? .demo
        return ConnectionConfig(
            host: host,
            port: port,
            source: source,
            token: KeychainTokenStore.load()
        )
    }

    func persist(token: String?) throws {
        let defaults = UserDefaults.standard
        defaults.set(host, forKey: Keys.host)
        defaults.set(port, forKey: Keys.port)
        defaults.set(source.rawValue, forKey: Keys.source)
        if let token, !token.isEmpty {
            try KeychainTokenStore.save(token)
        } else if token != nil {
            KeychainTokenStore.clear()
        }
    }

    static func fromEnvironment() -> ConnectionConfig? {
        let env = ProcessInfo.processInfo.environment
        guard let base = env["AID_BASE_URL"], !base.isEmpty,
              let token = env["AID_TOKEN"], !token.isEmpty,
              let url = URL(string: base),
              let host = url.host else {
            return nil
        }
        let port = url.port ?? 8080
        return ConnectionConfig(host: host, port: port, source: .live, token: token)
    }

    private enum Keys {
        static let host = "aid.command.host"
        static let port = "aid.command.port"
        static let source = "aid.command.source"
    }
}
