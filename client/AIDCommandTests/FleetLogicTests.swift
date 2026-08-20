// Unit tests for fleet client pure logic.
// Exports: FleetLogicTests, ThemeContractTests, ConsoleLayoutTests.

import XCTest
@testable import AIDCommand

final class StatusMapperTests: XCTestCase {
    func testRunningStatesMapToRun() {
        for status in ["running", "waiting", "pending", "awaiting_input", "stalled"] {
            XCTAssertEqual(StatusMapper.displayState(from: status), .run)
        }
    }

    func testDoneStates() {
        XCTAssertEqual(StatusMapper.displayState(from: "done"), .done)
        XCTAssertEqual(StatusMapper.displayState(from: "merged"), .done)
    }

    func testFailAndStop() {
        XCTAssertEqual(StatusMapper.displayState(from: "failed"), .fail)
        XCTAssertEqual(StatusMapper.displayState(from: "stopped"), .stop)
    }

    func testVerifyTagMapping() {
        XCTAssertNil(StatusMapper.verifyTag(from: "verified"))
        XCTAssertEqual(StatusMapper.verifyTag(from: "failed"), .vfail)
        XCTAssertEqual(StatusMapper.verifyTag(from: "timeout"), .vtimeout)
    }
}

final class XPStateTests: XCTestCase {
    func testDefaultXPAndRank() {
        let xp = XPState()
        XCTAssertEqual(xp.xp, 4280)
        XCTAssertEqual(xp.rankLabel, "V")
        XCTAssertEqual(xp.barProgress, 0.28, accuracy: 0.01)
    }

    func testAwardOnDoneAndFail() {
        var xp = XPState(xp: 1000)
        xp.award(for: .done, payloadXP: 320)
        XCTAssertEqual(xp.xp, 1320)
        xp.award(for: .fail)
        XCTAssertEqual(xp.xp, 1340)
    }
}

final class ProgressDeriverTests: XCTestCase {
    func testClampsBelowOne() {
        let sectors = DemoDataset.initialSnapshot().sectors
        let medians = ProgressDeriver.medianDurations(from: sectors)
        let p = ProgressDeriver.progress(elapsedSeconds: 99999, agent: "codex", completedDurations: medians)
        XCTAssertLessThanOrEqual(p, 0.97)
    }

    func testUsesMedianWhenAvailable() {
        let medians = ["codex": [600, 600, 600]]
        let p = ProgressDeriver.progress(elapsedSeconds: 300, agent: "codex", completedDurations: medians)
        XCTAssertEqual(p, 0.5, accuracy: 0.01)
    }
}

final class DemoTickEngineTests: XCTestCase {
    func testTickIncrementsCounter() {
        let start = DemoDataset.initialSnapshot()
        let result = DemoTickEngine.tick(start)
        XCTAssertEqual(result.snapshot.tick, 1)
    }

    func testCompleteEvery13Ticks() {
        var snap = DemoDataset.initialSnapshot()
        var completed = false
        for _ in 0..<13 {
            let result = DemoTickEngine.tick(snap)
            snap = result.snapshot
            if result.toasts.contains(where: { $0.state == .done }) {
                completed = true
            }
        }
        XCTAssertTrue(completed)
    }
}

final class ConsoleLayoutTests: XCTestCase {
    func testDesktopWidth() {
        XCTAssertEqual(ConsoleLayout.classify(width: 1440), .desktop)
        XCTAssertEqual(ConsoleLayout.classify(width: 1180).leftRailWidth, 290)
    }

    func testCompactWidth() {
        XCTAssertEqual(ConsoleLayout.classify(width: 800), .compact)
        XCTAssertFalse(ConsoleLayout.compact.showsBottomBrief)
    }
}

final class ThemeContractTests: XCTestCase {
    func testBothThemesSupplyTokens() {
        let themes: [any ThemeTokens] = [StarshipTheme(), PixelTheme()]
        for theme in themes {
            _ = theme.bgDeep
            _ = theme.bg
            _ = theme.panelEdge
            _ = theme.ink
            _ = theme.accent
            _ = theme.font(.body)
            _ = theme.panelCut
            _ = theme.motion
        }
    }

    func testThemesDifferStructurally() {
        let star = StarshipTheme()
        let pixel = PixelTheme()
        XCTAssertNotEqual(star.panelStyle, pixel.panelStyle)
        XCTAssertNotEqual(star.overlay, pixel.overlay)
        XCTAssertNotEqual(star.motion.stepped, pixel.motion.stepped)
        XCTAssertNotEqual(star.kind, pixel.kind)
    }
}

final class DemoDatasetCoverageTests: XCTestCase {
    func testAllSectorsAndMissionsPresent() {
        let snapshot = DemoDataset.initialSnapshot()
        XCTAssertEqual(snapshot.sectors.count, 3, "Expected 3 sectors")
        let sectorIDs = snapshot.sectors.map(\.id)
        XCTAssertTrue(sectorIDs.contains("SEC-01"))
        XCTAssertTrue(sectorIDs.contains("SEC-02"))
        XCTAssertTrue(sectorIDs.contains("SEC-03"))
        let totalMissions = snapshot.sectors.reduce(0) { $0 + $1.missions.count }
        XCTAssertEqual(totalMissions, 20, "Expected 20 missions across all sectors")
        XCTAssertEqual(snapshot.sectors[0].missions.count, 9, "SEC-01 should have 9 missions")
        XCTAssertEqual(snapshot.sectors[1].missions.count, 6, "SEC-02 should have 6 missions")
        XCTAssertEqual(snapshot.sectors[2].missions.count, 5, "SEC-03 should have 5 missions")
    }

    func testAllMissionIDsUnique() {
        let snapshot = DemoDataset.initialSnapshot()
        let allIDs = snapshot.sectors.flatMap(\.missions).map(\.id)
        XCTAssertEqual(allIDs.count, Set(allIDs).count, "Mission IDs must be unique")
    }

    func testFleetLogReceivesAllSectorsFromDemoSource() {
        let source = DemoSource()
        let snapshot = source.currentSnapshot()
        XCTAssertEqual(snapshot.sectors.count, 3)
        let tags = snapshot.sectors.map(\.tag).sorted()
        XCTAssertEqual(tags, ["SEC-01", "SEC-02", "SEC-03"])
        let totalMissions = snapshot.sectors.reduce(0) { $0 + $1.missions.count }
        XCTAssertEqual(totalMissions, 20, "Fleet log must receive all 20 demo missions")
        XCTAssertEqual(snapshot.sectors[0].missions.count, 9)
        XCTAssertEqual(snapshot.sectors[1].missions.count, 6)
        XCTAssertEqual(snapshot.sectors[2].missions.count, 5)
    }
}

final class FleetStoreSnapshotTests: XCTestCase {
    @MainActor
    func testStoreSnapshotMatchesDemoSourceMissionCount() {
        let store = FleetStore()
        let source = DemoSource()
        let demo = source.currentSnapshot()
        XCTAssertEqual(store.snapshot.sectors.count, demo.sectors.count)
        let storeTotal = store.snapshot.sectors.reduce(0) { $0 + $1.missions.count }
        let demoTotal = demo.sectors.reduce(0) { $0 + $1.missions.count }
        XCTAssertEqual(storeTotal, demoTotal)
        XCTAssertEqual(storeTotal, 20)
    }
}

final class PayloadDeriverTests: XCTestCase {
    func testKnownPayloadMappings() {
        let snapshot = DemoDataset.initialSnapshot()
        let missions = Dictionary(uniqueKeysWithValues: snapshot.sectors.flatMap(\.missions).map { ($0.id, $0) })

        let report = PayloadDeriver.derive(from: missions["t-85e75668"]!, sectorTag: "SEC-01")
        XCTAssertEqual(report?.kind, .report)
        XCTAssertEqual(report?.rarity, .legendary)

        let release = PayloadDeriver.derive(from: missions["t-9f10c3d2"]!, sectorTag: "SEC-03")
        XCTAssertEqual(release?.kind, .release)
        XCTAssertEqual(release?.rarity, .legendary)

        let ladder = PayloadDeriver.derive(from: missions["t-22c5346d"]!, sectorTag: "SEC-01")
        XCTAssertEqual(ladder?.kind, .bench)
        XCTAssertEqual(ladder?.rarity, .epic)
        XCTAssertEqual(ladder?.rarity.label, "HIGH")

        let dataset = PayloadDeriver.derive(from: missions["t-d5a7f26e"]!, sectorTag: "SEC-01")
        XCTAssertEqual(dataset?.kind, .dataset)

        let patch = PayloadDeriver.derive(from: missions["t-77b2e004"]!, sectorTag: "SEC-03")
        XCTAssertEqual(patch?.kind, .patch)

        let payloads = PayloadDeriver.payloads(from: snapshot.sectors)
        XCTAssertGreaterThan(payloads.filter { $0.kind == .dataset }.count, 0)
        XCTAssertGreaterThan(payloads.filter { $0.kind == .patch }.count, 0)
    }

    func testFailMissionYieldsScrap() {
        let snapshot = DemoDataset.initialSnapshot()
        let fail = snapshot.sectors[0].missions.first { $0.state == .fail }
        let payload = fail.flatMap { PayloadDeriver.derive(from: $0, sectorTag: "SEC-01") }
        XCTAssertEqual(payload?.kind, .scrap)
        XCTAssertEqual(payload?.rarity, .salvage)
    }

    func testRunningMissionHasNoPayload() {
        let snapshot = DemoDataset.initialSnapshot()
        let running = snapshot.sectors[0].missions.first { $0.state == .run }
        let payload = running.flatMap { PayloadDeriver.derive(from: $0, sectorTag: "SEC-01") }
        XCTAssertNil(payload)
    }

    func testAllDoneMissionsProducePayloads() {
        let snapshot = DemoDataset.initialSnapshot()
        let payloads = PayloadDeriver.payloads(from: snapshot.sectors)
        let doneCount = snapshot.sectors.flatMap(\.missions).filter { $0.state == .done || $0.state == .fail || $0.state == .stop }.count
        XCTAssertEqual(payloads.count, doneCount)
    }
}

final class FleetStoreLaunchTests: XCTestCase {
    @MainActor
    func testDefaultTabIsFleetLogWhenUnset() {
        let key = "aid.command.tab"
        let prior = UserDefaults.standard.string(forKey: key)
        UserDefaults.standard.removeObject(forKey: key)
        defer {
            if let prior { UserDefaults.standard.set(prior, forKey: key) }
            else { UserDefaults.standard.removeObject(forKey: key) }
        }
        let store = FleetStore()
        XCTAssertEqual(store.selectedTab, .fleetLog)
    }
}

final class DemoSourceActionTests: XCTestCase {
    func testAbortRunningMission() async throws {
        let source = DemoSource()
        let running = source.currentSnapshot().sectors.flatMap(\.missions).first { $0.state == .run }
        XCTAssertNotNil(running)
        let result = try await source.act(.abort, on: running!.id)
        XCTAssertTrue(result.ok)
        let updated = source.currentSnapshot().sectors.flatMap(\.missions).first { $0.id == running!.id }
        XCTAssertEqual(updated?.state, .stop)
    }

    func testDockRejectsRunningMission() async throws {
        let source = DemoSource()
        let running = source.currentSnapshot().sectors.flatMap(\.missions).first { $0.state == .run }
        let result = try await source.act(.dock, on: running!.id)
        XCTAssertFalse(result.ok)
    }
}

final class FleetFormattersTests: XCTestCase {
    func testUnknownCostAndModel() {
        XCTAssertEqual(FleetFormatters.cost(nil), "—")
        XCTAssertEqual(FleetFormatters.model(nil), "—")
    }

    func testElapsedFormatting() {
        XCTAssertEqual(FleetFormatters.elapsed(seconds: 0), "—")
        XCTAssertEqual(FleetFormatters.elapsed(seconds: 125), "2m 05s")
    }

    func testWorkgroupLabelOmitsPlaceholder() {
        XCTAssertEqual(FleetFormatters.workgroupLabel(""), "—")
        XCTAssertEqual(FleetFormatters.workgroupLabel("——"), "—")
        XCTAssertEqual(FleetFormatters.workgroupLabel("8937e74c"), "WG-8937E74C")
    }

    func testMeasuredCountTreatsZeroAsUnknown() {
        XCTAssertEqual(FleetFormatters.measuredCount(nil), "—")
        XCTAssertEqual(FleetFormatters.measuredCount(0), "—")
        XCTAssertEqual(FleetFormatters.measuredCount(4), "4")
    }
}
