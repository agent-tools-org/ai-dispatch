// Live fleet source — GET /api/fleet, SSE /api/events, and task action endpoints.
// Exports: LiveSource, LiveSourceError, ConnectionProbe.

import Foundation

final class LiveSource: FleetDataSource, @unchecked Sendable {
    private let session: URLSession
    private var config: ConnectionConfig
    private var snapshot: FleetSnapshot
    private var sse: SSEClient?
    private var lastHeartbeat = Date()
    private var lastSnapshotAt = Date()
    private let lock = NSLock()
    private var continuation: AsyncStream<FleetSnapshot>.Continuation?

    init(config: ConnectionConfig, session: URLSession = .shared) {
        self.config = config
        self.session = session
        self.snapshot = FleetSnapshot(
            sectors: [],
            summary: FleetSummary(
                window: "—", running: 0, done: 0, failed: 0,
                spendUSD: nil, memoryMB: 0, sectorCount: 0
            ),
            serverVersion: "—",
            connection: .disconnected,
            agents: [],
            tick: 0
        )
    }

    func updateConfig(_ config: ConnectionConfig) {
        lock.lock(); self.config = config; lock.unlock()
    }

    func currentSnapshot() -> FleetSnapshot {
        lock.lock(); defer { lock.unlock() }
        return snapshot
    }

    func snapshots() -> AsyncStream<FleetSnapshot> {
        AsyncStream { continuation in
            self.continuation = continuation
            let task = Task { await self.run(continuation: continuation) }
            continuation.onTermination = { [weak self] _ in
                task.cancel()
                self?.sse?.stop()
            }
        }
    }

    func detail(_ id: MissionID) async throws -> MissionDetail {
        let data = try await get("api/tasks/\(id)")
        let decoded = try FleetAPIDecoder.decodeTaskDetail(from: data)
        return MissionDetail(mission: decoded.mission, prompt: decoded.prompt, events: decoded.events)
    }

    func diff(_ id: MissionID) async throws -> String {
        try FleetAPIDecoder.decodeDiff(from: try await get("api/tasks/\(id)/diff"))
    }

    func result(_ id: MissionID) async throws -> String {
        try FleetAPIDecoder.decodeResult(from: try await get("api/tasks/\(id)/result"))
    }

    func act(_ action: MissionAction, on id: MissionID) async throws -> MissionActionResult {
        switch action {
        case .abort:
            return try await postAction("api/tasks/\(id)/stop", body: nil)
        case .relaunch:
            return try await postAction(
                "api/tasks/\(id)/retry",
                body: try JSONEncoder().encode(RetryBody(feedback: nil))
            )
        case .steer(let message):
            return try await postAction(
                "api/tasks/\(id)/steer",
                body: try JSONEncoder().encode(MessageBody(message: message))
            )
        case .diff:
            _ = try await diff(id)
            return MissionActionResult(ok: true, message: "diff ready")
        case .export:
            _ = try await result(id)
            return MissionActionResult(ok: true, message: "export ready")
        case .dock:
            return try await postAction("api/tasks/\(id)/merge", body: nil)
        }
    }

    private func run(continuation: AsyncStream<FleetSnapshot>.Continuation) async {
        publish(connection: .connecting)
        do {
            let data = try await get("api/fleet?window=today")
            let mapped = try FleetAPIDecoder.decodeSnapshot(from: data, connection: .live)
            replace(mapped)
            continuation.yield(currentSnapshot())
            startSSE()
            await heartbeatWatch(continuation: continuation)
        } catch {
            publish(connection: .error(Self.message(for: error)))
            continuation.yield(currentSnapshot())
            continuation.finish()
        }
    }

    private func startSSE() {
        guard let request = try? makeRequest(path: "api/events", method: "GET", body: nil) else { return }
        let client = SSEClient(
            onMessage: { [weak self] message in self?.handleSSE(message) },
            onFailure: { [weak self] error in
                self?.publish(connection: .error(Self.message(for: error)))
                self?.continuation?.yield(self?.currentSnapshot() ?? DemoDataset.initialSnapshot())
            }
        )
        sse = client
        client.start(request: request)
    }

    private func handleSSE(_ message: SSEMessage) {
        lastHeartbeat = Date()
        switch message.event {
        case "heartbeat":
            publish(connection: .live)
        case "fleet_summary", "task_update", "agent_update":
            Task { await self.refreshFleet() }
        default:
            break
        }
        continuation?.yield(currentSnapshot())
    }

    private func refreshFleet() async {
        do {
            let data = try await get("api/fleet?window=today")
            let mapped = try FleetAPIDecoder.decodeSnapshot(from: data, connection: .live)
            replace(mapped)
            continuation?.yield(currentSnapshot())
        } catch {
            publish(connection: .error(Self.message(for: error)))
            continuation?.yield(currentSnapshot())
        }
    }

    private func heartbeatWatch(continuation: AsyncStream<FleetSnapshot>.Continuation) async {
        while !Task.isCancelled {
            try? await Task.sleep(for: .seconds(5))
            let age = Date().timeIntervalSince(lastHeartbeat)
            if age > 30 {
                publish(connection: .degraded(age: age))
                continuation.yield(currentSnapshot())
            }
        }
    }

    private func get(_ path: String) async throws -> Data {
        let request = try makeRequest(path: path, method: "GET", body: nil)
        let (data, response) = try await session.data(for: request)
        try Self.throwIfNeeded(response: response, data: data)
        return data
    }

    private func postAction(_ path: String, body: Data?) async throws -> MissionActionResult {
        let request = try makeRequest(path: path, method: "POST", body: body)
        let (data, response) = try await session.data(for: request)
        if let http = response as? HTTPURLResponse, http.statusCode == 409 {
            return try FleetAPIDecoder.decodeAction(from: data)
        }
        try Self.throwIfNeeded(response: response, data: data)
        return try FleetAPIDecoder.decodeAction(from: data)
    }

    private func makeRequest(path: String, method: String, body: Data?) throws -> URLRequest {
        lock.lock(); let cfg = config; lock.unlock()
        guard let base = cfg.baseURL else { throw LiveSourceError.badURL }
        guard let url = URL(string: path, relativeTo: base)?.absoluteURL else {
            throw LiveSourceError.badURL
        }
        var request = URLRequest(url: url)
        request.httpMethod = method
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        if let token = cfg.token, !token.isEmpty {
            request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        }
        if let body {
            request.httpBody = body
            request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        }
        return request
    }

    private func replace(_ next: FleetSnapshot) {
        lock.lock()
        snapshot = next
        lastSnapshotAt = Date()
        lastHeartbeat = Date()
        lock.unlock()
    }

    private func publish(connection: ConnectionState) {
        lock.lock()
        snapshot.connection = connection
        lock.unlock()
    }

    static func message(for error: Error) -> String {
        if let live = error as? LiveSourceError {
            switch live {
            case .httpStatus(let code, let body): return "HTTP \(code): \(body)"
            case .badURL: return "invalid server URL"
            case .unauthorized: return "unauthorized"
            }
        }
        return error.localizedDescription
    }

    static func throwIfNeeded(response: URLResponse, data: Data) throws {
        guard let http = response as? HTTPURLResponse else { return }
        if http.statusCode == 401 { throw LiveSourceError.unauthorized }
        if http.statusCode >= 400 {
            let body = String(data: data, encoding: .utf8) ?? ""
            throw LiveSourceError.httpStatus(http.statusCode, body)
        }
    }
}

enum LiveSourceError: Error {
    case badURL
    case unauthorized
    case httpStatus(Int, String)
}

enum ConnectionProbe {
    static func test(config: ConnectionConfig, session: URLSession = .shared) async -> String {
        guard let base = config.baseURL,
              let url = URL(string: "api/fleet?window=today", relativeTo: base)?.absoluteURL else {
            return "invalid URL"
        }
        var request = URLRequest(url: url)
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        if let token = config.token, !token.isEmpty {
            request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        }
        do {
            let (data, response) = try await session.data(for: request)
            let code = (response as? HTTPURLResponse)?.statusCode ?? 0
            let body = String(data: data, encoding: .utf8) ?? ""
            if let err = try? JSONDecoder().decode(ErrorDTO.self, from: data), let message = err.error {
                return "\(code) \(message)"
            }
            let snippet = body.prefix(120).replacingOccurrences(of: "\n", with: " ")
            return "\(code) \(snippet)"
        } catch {
            return error.localizedDescription
        }
    }
}

private struct ErrorDTO: Decodable {
    let error: String?
}

private struct MessageBody: Encodable {
    let message: String
}

private struct RetryBody: Encodable {
    let feedback: String?
}
