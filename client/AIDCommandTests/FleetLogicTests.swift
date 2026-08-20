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
