import XCTest
@testable import TaskersMac

final class TaskersCoreBridgeTests: XCTestCase {
    func testMockBackendBootstrapsAndRevisionsIncrement() throws {
        let tempDir = URL(fileURLWithPath: NSTemporaryDirectory(), isDirectory: true)
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: tempDir, withIntermediateDirectories: true)

        let options = TaskersCoreOptions(
            sessionPath: tempDir.appendingPathComponent("session.json").path,
            socketPath: tempDir.appendingPathComponent("taskers.sock").path,
            configuredShell: "/bin/sh",
            demo: false,
            backend: "mock"
        )
        let bridge = try TaskersCoreBridge(options: options)

        XCTAssertEqual(bridge.revision, 0)
        let snapshot = try bridge.snapshot()
        XCTAssertNotNil(snapshot.activeWorkspace)

        _ = try bridge.dispatch(command: [
            "command": "create_workspace",
            "label": "Docs"
        ])
        XCTAssertEqual(bridge.revision, 1)
    }
}
