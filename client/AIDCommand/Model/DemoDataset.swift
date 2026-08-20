// Canvas demo dataset transcribed from DESIGN.md §7.
// Exports: DemoDataset.initialSnapshot().

import Foundation

enum DemoDataset {
    static func initialSnapshot() -> FleetSnapshot {
        let sectors = [sector01(), sector02(), sector03()]
        let summary = FleetSummary(
            window: "24h",
            running: countState(.run, in: sectors),
            done: countState(.done, in: sectors),
            failed: countState(.fail, in: sectors),
            spendUSD: "$3.23",
            memoryMB: 512,
            sectorCount: sectors.count
        )
        return FleetSnapshot(
            sectors: sectors,
            summary: summary,
            serverVersion: "0.1.0-demo",
            connection: .live,
            agents: DemoAgents.roster(from: sectors),
            tick: 0
        )
    }

    private static func sector01() -> Sector {
        Sector(
            id: "SEC-01",
            tag: "SEC-01",
            name: "uniswapx-filler",
            workgroupID: "8937e74c",
            missions: [
                m("t-561886a0", "Advisory sweep of the filler quote path", "codex", "gpt-5.6-sol", .run, 3, 0.42, 1084, "1.4M", nil, "145M"),
                m("t-1462b444", "Blind-lane candidate cut", "cursor", "auto", .run, 2, 0.66, 1491, nil, "subscr", "136M"),
                m("t-85e75668", "Fill-rate regression pass", "grok", "grok-4.6-build", .done, 4, 1, 2759, "8.8M", "$1.37", nil),
                m("t-576eeb2d", "Venue and pool discovery", "gemini", "gemini-3.1-pro", .fail, 3, 0.31, 966, nil, nil, nil),
                m("t-22c5346d", "Ladder-gas study", "cursor", "Auto (asked)", .done, 2, 1, 3248, nil, "subscr", nil),
                m("t-a05e22ce", "Blind lane locator", "opencode", "auto", .fail, 1, 0.08, 12, nil, nil, nil),
                m("t-d5a7f26e", "Call-site rescue", "gemini", "gemini-3.1-pro", .done, 2, 1, 1439, nil, nil, nil),
                m("t-23c61dfd", "Blind-lane cost measurement", "cursor", "Auto (asked)", .fail, 4, 0.55, 1836, nil, "subscr", nil),
                m("t-9cda19fa", "Measurement retry", "cursor", "auto", .stop, 2, 0.22, 0, nil, "subscr", nil),
            ]
        )
    }

    private static func sector02() -> Sector {
        Sector(
            id: "SEC-02",
            tag: "SEC-02",
            name: "poolstra-decompounder",
            workgroupID: "a279c7ca",
            missions: [
                m("t-6690cdb4", "Joint audit of two accounting changes", "grok", "grok-4.6", .run, 3, 0.78, 44, "32M", nil, nil),
                m("t-d9e2333b", "Rounding correctness review", "grok", "grok-4.6", .done, 2, 1, 1088, "2.3M", "$0.36", nil),
                m("t-59ff4757", "Re-audit after rounding fix", "grok", "grok-4.6", .done, 2, 1, 1140, "2.2M", "$0.40", nil),
                m("t-fcafc202", "Re-audit after withdrawal fix", "grok", "grok-4.6", .done, 2, 1, 1115, "2.6M", "$0.42", nil),
                m("t-f886029c", "Fee-split correctness review", "grok", "grok-4.6", .done, 3, 1, 1324, "2.8M", "$0.50", nil),
                m("t-2bc437e2", "Compounding interval measurement", "grok", "grok-4.6", .done, 4, 1, 1036, "3.6M", "$0.46", nil),
            ]
        )
    }

    private static func sector03() -> Sector {
        Sector(
            id: "SEC-03",
            tag: "SEC-03",
            name: "aid-core",
            workgroupID: "41ba0dd2",
            missions: [
                m("t-0c41aa19", "Batch progress plumbing", "codex", "gpt-5.6", .run, 2, 0.34, 372, "0.6M", nil, "98M"),
                m("t-77b2e004", "Rename watch engine symbols", "opencode", "glm-4.7", .done, 1, 1, 221, "0.2M", "$0.01", nil),
                m("t-b1930f5c", "Type fixes in the store layer", "kilo", "default", .done, 1, 1, 118, "free", nil, nil),
                m("t-9f10c3d2", "Cut release v8.78.0", "codex", "gpt-5.6", .done, 5, 1, 747, "1.9M", "$0.31", nil),
                m("t-e5567a2b", "Dashboard table restyle", "cursor", "composer-1.5", .stop, 2, 0.44, 0, nil, "subscr", nil),
            ]
        )
    }

    private static func m(
        _ id: MissionID,
        _ title: String,
        _ agent: String,
        _ model: String?,
        _ state: MissionDisplayState,
        _ threat: Int,
        _ progress: Double,
        _ elapsed: Int,
        _ tokens: String?,
        _ cost: String?,
        _ memory: String?
    ) -> Mission {
        Mission(
            id: id,
            title: title,
            agent: agent,
            model: model,
            state: state,
            threat: threat,
            progress: progress,
            elapsedSeconds: elapsed,
            tokens: tokens,
            cost: cost,
            memoryMB: memory,
            verifyTag: nil,
            awaitingReason: nil
        )
    }

    private static func countState(_ state: MissionDisplayState, in sectors: [Sector]) -> Int {
        sectors.flatMap(\.missions).filter { $0.state == state }.count
    }
}
