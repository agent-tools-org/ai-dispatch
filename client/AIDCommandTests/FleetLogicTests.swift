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
        let _ = DemoSource()
        let snapshot = DemoDataset.initialSnapshot()
        let sectors = snapshot.sectors
        XCTAssertEqual(sectors.count, 3)
        for sector in sectors {
            XCTAssertFalse(sector.missions.isEmpty, "\(sector.tag) must have missions")
        }
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
}
