// Fixture JSON and decode tests for GET /api/fleet → FleetSnapshot.
// Exports: FleetAPIDecoderTests.

import XCTest
@testable import AIDCommand

final class FleetAPIDecoderTests: XCTestCase {
    func testDecodesFleetSnapshot() throws {
        let data = Self.fleetFixture.data(using: .utf8)!
        let snapshot = try FleetAPIDecoder.decodeSnapshot(from: data, connection: .live)
        XCTAssertEqual(snapshot.serverVersion, "10.37.0")
        XCTAssertEqual(snapshot.summary.window, "today")
        XCTAssertEqual(snapshot.summary.running, 1)
        XCTAssertEqual(snapshot.sectors.count, 1)
        XCTAssertEqual(snapshot.sectors[0].missions.count, 2)
        XCTAssertEqual(snapshot.sectors[0].tag, "SEC-01")
        let running = snapshot.sectors[0].missions.first { $0.id == "t-abc" }
        XCTAssertEqual(running?.state, .run)
        XCTAssertEqual(running?.model, "gpt-5.6")
        XCTAssertNil(running?.cost)
        XCTAssertEqual(FleetFormatters.cost(running?.cost), "—")
        let done = snapshot.sectors[0].missions.first { $0.id == "t-def" }
        XCTAssertEqual(done?.state, .done)
        XCTAssertEqual(done?.cost, "$0.31")
        XCTAssertEqual(snapshot.agents.count, 1)
        XCTAssertEqual(snapshot.agents[0].id, "codex")
        XCTAssertTrue(snapshot.agents[0].busy)
        XCTAssertEqual(snapshot.agents[0].taskCount, 41)
        XCTAssertEqual(snapshot.connection, .live)
        XCTAssertEqual(FleetFormatters.workgroupLabel(snapshot.sectors[0].workgroupID), "WG-41BA0DD2")
    }

    func testZeroTaskCountIsUnmeasured() throws {
        let json = Self.fleetFixture.replacingOccurrences(of: "\"task_count\": 41", with: "\"task_count\": 0")
        let data = json.data(using: .utf8)!
        let snapshot = try FleetAPIDecoder.decodeSnapshot(from: data, connection: .live)
        XCTAssertNil(snapshot.agents[0].taskCount)
        XCTAssertEqual(FleetFormatters.measuredCount(snapshot.agents[0].taskCount), "—")
    }

    func testMissingWorkgroupRendersDash() throws {
        let json = Self.fleetFixture.replacingOccurrences(
            of: "\"workgroup_id\": \"wg-41ba0dd2\",",
            with: "\"workgroup_id\": null,"
        )
        let data = json.data(using: .utf8)!
        let snapshot = try FleetAPIDecoder.decodeSnapshot(from: data, connection: .live)
        XCTAssertEqual(FleetFormatters.workgroupLabel(snapshot.sectors[0].workgroupID), "—")
    }

    func testNullCostStaysUnknown() throws {
        let data = Self.fleetFixture.data(using: .utf8)!
        let snapshot = try FleetAPIDecoder.decodeSnapshot(from: data, connection: .live)
        let mission = snapshot.sectors[0].missions.first { $0.id == "t-abc" }
        XCTAssertNil(mission?.cost)
        XCTAssertNil(mission?.tokens)
    }

    func testRunningElapsedFromStartedAtWhenDurationMissing() throws {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime]
        let startedAt = Date().addingTimeInterval(-125)
        let started = formatter.string(from: startedAt)
        let json = """
        {
          "server": {
            "version": "10.37.0",
            "host": "127.0.0.1",
            "port": 8080,
            "started_at": "2026-08-20T07:00:00Z",
            "aid_home": "/tmp/.aid"
          },
          "summary": {
            "running": 1, "done": 0, "failed": 0, "spend_usd": null,
            "tokens": null, "memory_mb": null, "window": "today"
          },
          "sectors": [{
            "id": "aid-core",
            "name": "aid-core",
            "repo_path": "/tmp/aid-core",
            "workgroup_id": "wg-41ba0dd2",
            "tasks": [{
              "id": "t-run",
              "agent": "codex",
              "status": "running",
              "outcome": "in_progress",
              "prompt_excerpt": "Live elapsed tick",
              "requested_model": "gpt-5.6",
              "observed_model": "gpt-5.6",
              "tokens": null,
              "cost_usd": null,
              "duration_ms": null,
              "started_at": "\(started)",
              "difficulty": "simple",
              "memory_mb": null,
              "latest_events": []
            }]
          }],
          "agents": [{ "name": "codex", "busy": true, "task_count": 1, "quota": { "state": "ok" } }]
        }
        """
        let data = json.data(using: .utf8)!
        let snapshot = try FleetAPIDecoder.decodeSnapshot(from: data, connection: .live)
        let running = try XCTUnwrap(snapshot.sectors[0].missions.first)
        XCTAssertEqual(running.state, .run)
        XCTAssertNotNil(running.startedAt)
        XCTAssertGreaterThanOrEqual(running.elapsedSeconds, 120)
        XCTAssertLessThanOrEqual(running.elapsedSeconds, 135)
        XCTAssertNotEqual(FleetFormatters.elapsed(seconds: running.elapsedSeconds), "—")
    }

    func testUnauthorizedProbeMessage() async {
        let body = #"{"error":"unauthorized"}"#.data(using: .utf8)!
        let dto = try? JSONDecoder().decode(ProbeError.self, from: body)
        XCTAssertEqual(dto?.error, "unauthorized")
    }

    func testConnectionConfigEnvironmentOverride() {
        let config = ConnectionConfig(
            host: "127.0.0.1", port: 8080, source: .live, token: "secret"
        )
        XCTAssertEqual(config.baseURL?.absoluteString, "http://127.0.0.1:8080")
    }

    func testDecodesCapturedLiveFleetPayload() throws {
        let path = "/tmp/fleet-live.json"
        guard FileManager.default.fileExists(atPath: path) else {
            throw XCTSkip("no live fleet fixture at \(path)")
        }
        let data = try Data(contentsOf: URL(fileURLWithPath: path))
        do {
            let snapshot = try FleetAPIDecoder.decodeSnapshot(from: data, connection: .live)
            XCTAssertFalse(snapshot.sectors.isEmpty)
            XCTAssertEqual(snapshot.connection, .live)
            XCTAssertNotEqual(snapshot.serverVersion, "0.1.0-demo")
            let total = snapshot.sectors.reduce(0) { $0 + $1.missions.count }
            XCTAssertGreaterThan(total, 0)
        } catch {
            XCTFail("live fleet decode failed: \(error)")
        }
    }

    private struct ProbeError: Decodable { let error: String? }

    private static let fleetFixture = """
    {
      "server": {
        "version": "10.37.0",
        "host": "127.0.0.1",
        "port": 8080,
        "started_at": "2026-08-20T07:00:00Z",
        "aid_home": "/tmp/.aid"
      },
      "summary": {
        "running": 1,
        "done": 1,
        "failed": 0,
        "stopped": 0,
        "spend_usd": 0.31,
        "tokens": 1900000,
        "memory_mb": 98,
        "window": "today"
      },
      "sectors": [
        {
          "id": "aid-core",
          "name": "aid-core",
          "repo_path": "/tmp/aid-core",
          "workgroup_id": "wg-41ba0dd2",
          "tasks": [
            {
              "id": "t-abc",
              "agent": "codex",
              "status": "running",
              "outcome": "in_progress",
              "prompt_excerpt": "Batch progress plumbing",
              "requested_model": "gpt-5.6",
              "observed_model": "gpt-5.6",
              "tokens": null,
              "cost_usd": null,
              "duration_ms": 372000,
              "difficulty": "simple",
              "memory_mb": 98,
              "latest_events": []
            },
            {
              "id": "t-def",
              "agent": "codex",
              "status": "done",
              "outcome": "verified",
              "prompt_excerpt": "Cut release v8.78.0",
              "requested_model": "gpt-5.6",
              "observed_model": "gpt-5.6",
              "tokens": 1900000,
              "cost_usd": 0.31,
              "duration_ms": 747000,
              "difficulty": "complex",
              "memory_mb": null,
              "latest_events": []
            }
          ]
        }
      ],
      "agents": [
        {
          "name": "codex",
          "busy": true,
          "task_count": 41,
          "quota": { "state": "ok" }
        }
      ]
    }
    """
}
