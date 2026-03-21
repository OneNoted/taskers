import XCTest
@testable import TaskersMac

final class TaskersBrowserViewTests: XCTestCase {
    func testResolvedURLPreservesExplicitSchemes() {
        XCTAssertEqual(
            TaskersBrowserView.resolvedURL(from: "https://example.com/docs")?.absoluteString,
            "https://example.com/docs"
        )
        XCTAssertEqual(
            TaskersBrowserView.resolvedURL(from: "about:blank")?.absoluteString,
            "about:blank"
        )
    }

    func testResolvedURLUsesDuckDuckGoForQueries() {
        let url = TaskersBrowserView.resolvedURL(from: "taskers browser integration")
        let components = url.flatMap { URLComponents(url: $0, resolvingAgainstBaseURL: false) }

        XCTAssertEqual(components?.host, "duckduckgo.com")
        XCTAssertEqual(components?.queryItems?.first(where: { $0.name == "q" })?.value, "taskers browser integration")
    }

    func testResolvedURLUsesHttpForLocalTargets() {
        XCTAssertEqual(
            TaskersBrowserView.resolvedURL(from: "localhost:3777")?.absoluteString,
            "http://localhost:3777"
        )
        XCTAssertEqual(
            TaskersBrowserView.resolvedURL(from: "devbox:8080")?.absoluteString,
            "http://devbox:8080"
        )
    }

    func testResolvedURLDefaultsToHttpsForHosts() {
        XCTAssertEqual(
            TaskersBrowserView.resolvedURL(from: "taskers.dev")?.absoluteString,
            "https://taskers.dev"
        )
    }
}
