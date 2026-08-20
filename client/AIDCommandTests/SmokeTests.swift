// Smoke test proving the generated project builds and links the app target.
// Exports: SmokeTests.
import XCTest
@testable import AIDCommand

final class SmokeTests: XCTestCase {
    func testAppTargetLinks() { XCTAssertTrue(true) }
}
