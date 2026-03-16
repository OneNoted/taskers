import AppKit
import XCTest
@testable import TaskersMac

@MainActor
final class TaskersSmokeTests: XCTestCase {
    private var tempDirectory: URL!

    override func setUpWithError() throws {
        _ = NSApplication.shared
        try bootstrapTestEnvironment()
        tempDirectory = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(
            at: tempDirectory,
            withIntermediateDirectories: true
        )
    }

    override func tearDownWithError() throws {
        if let tempDirectory {
            try? FileManager.default.removeItem(at: tempDirectory)
        }
    }

    func testWorkspaceControllerRendersAndTracksSurfaceLifecycle() throws {
        let core = try TaskersCoreBridge(options: TaskersCoreOptions(
            sessionPath: tempDirectory.appendingPathComponent("session.json").path,
            socketPath: tempDirectory.appendingPathComponent("taskers.sock").path,
            configuredShell: "/bin/sh",
            demo: false,
            backend: "ghostty_embedded"
        ))
        let host = TaskersGhosttyHost()
        let controller = TaskersWorkspaceController(core: core, ghosttyHost: host)

        controller.showWindow(nil)
        controller.window?.makeKeyAndOrderFront(nil)
        drainMainRunLoop()

        try controller.refresh(force: true)
        XCTAssertEqual(controller.surfaceCount, 1)
        let initialSurfaceIDs = controller.lastRenderedSurfaceIDs

        let initialWorkspace = try XCTUnwrap(try core.snapshot().activeWorkspace)
        _ = try core.dispatch(command: [
            "command": "split_pane",
            "workspace_id": initialWorkspace.id,
            "pane_id": initialWorkspace.activePane,
            "axis": "vertical"
        ])
        drainMainRunLoop()
        try controller.refresh(force: true)
        XCTAssertEqual(controller.surfaceCount, 2)
        XCTAssertTrue(initialSurfaceIDs.isSubset(of: controller.lastRenderedSurfaceIDs))

        let updatedWorkspace = try XCTUnwrap(try core.snapshot().activeWorkspace)
        _ = try core.dispatch(command: [
            "command": "close_pane",
            "workspace_id": updatedWorkspace.id,
            "pane_id": updatedWorkspace.activePane
        ])
        drainMainRunLoop()
        try controller.refresh(force: true)
        XCTAssertEqual(controller.surfaceCount, 1)
        XCTAssertEqual(controller.lastRenderedSurfaceIDs.count, 1)

        controller.close()
        drainMainRunLoop()
    }

    private func bootstrapTestEnvironment() throws {
        let repoRoot = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
        let resourcesRoot = repoRoot.appendingPathComponent("build/macos/resources", isDirectory: true)
        let ghosttyResources = resourcesRoot.appendingPathComponent("ghostty", isDirectory: true)
        let terminfo = resourcesRoot.appendingPathComponent("terminfo", isDirectory: true)
        let helper = repoRoot.appendingPathComponent("build/macos/bin/taskersctl")

        for requiredPath in [ghosttyResources, terminfo, helper] {
            guard FileManager.default.fileExists(atPath: requiredPath.path) else {
                throw XCTSkip("missing preview dependency at \(requiredPath.path)")
            }
        }

        setenv("GHOSTTY_RESOURCES_DIR", ghosttyResources.path, 1)
        setenv("TERMINFO", terminfo.path, 1)
        setenv("TASKERS_CTL_PATH", helper.path, 1)
        setenv("TASKERS_DISABLE_SHELL_INTEGRATION", "1", 1)
    }

    private func drainMainRunLoop() {
        controllerWindow?.contentView?.layoutSubtreeIfNeeded()
        RunLoop.current.run(until: Date().addingTimeInterval(0.1))
    }

    private var controllerWindow: NSWindow? {
        NSApplication.shared.windows.first
    }
}
