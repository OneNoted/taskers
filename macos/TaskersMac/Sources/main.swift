import AppKit
import Foundation

final class TaskersMacApplication: NSObject, NSApplicationDelegate {
    private var core: TaskersCoreBridge?
    private var ghosttyHost: TaskersGhosttyHost?
    private var workspaceController: TaskersWorkspaceController?

    func applicationDidFinishLaunching(_ notification: Notification) {
        _ = notification
        TaskersEnvironment.emitSmokeLog("applicationDidFinishLaunching")
        TaskersEnvironment.scrubInheritedTerminalEnvironment()
        TaskersEnvironment.configureBundledPaths()
        if TaskersEnvironment.isRunningUnderXCTest {
            return
        }

        do {
            TaskersEnvironment.emitSmokeLog("creating core bridge")
            let core = try TaskersCoreBridge(options: TaskersEnvironment.defaultCoreOptions())
            TaskersEnvironment.emitSmokeLog("creating Ghostty host")
            let ghosttyHost = TaskersGhosttyHost()
            TaskersEnvironment.emitSmokeLog("creating workspace controller")
            let controller = TaskersWorkspaceController(core: core, ghosttyHost: ghosttyHost)

            self.core = core
            self.ghosttyHost = ghosttyHost
            self.workspaceController = controller

            TaskersEnvironment.emitSmokeLog("starting workspace controller")
            try controller.start()
            TaskersEnvironment.emitSmokeLog("workspace controller started")

            if TaskersEnvironment.isSmokeTestEnabled {
                DispatchQueue.main.asyncAfter(deadline: .now() + 2) {
                    guard let controller = self.workspaceController, controller.surfaceCount > 0 else {
                        fputs("Taskers macOS smoke failed: no surfaces rendered\n", stderr)
                        fflush(stderr)
                        exit(1)
                    }
                    NSApp.terminate(nil)
                }
            }
        } catch {
            fputs("Taskers macOS bootstrap failed: \(error.localizedDescription)\n", stderr)
            NSApp.terminate(nil)
            exit(1)
        }
    }

    func applicationDidBecomeActive(_ notification: Notification) {
        _ = notification
        ghosttyHost?.setFocused(true)
    }

    func applicationDidResignActive(_ notification: Notification) {
        _ = notification
        ghosttyHost?.setFocused(false)
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        _ = sender
        return !TaskersEnvironment.isRunningUnderXCTest
    }
}

let app = NSApplication.shared
let delegate = TaskersMacApplication()
app.delegate = delegate
app.setActivationPolicy(.regular)
app.run()
