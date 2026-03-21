import AppKit
import Foundation

final class TaskersMacApplication: NSObject, NSApplicationDelegate {
    private var core: TaskersCoreBridge?
    private var surfaceHost: TaskersDefaultSurfaceHost?
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
            configureMenus()
            TaskersEnvironment.emitSmokeLog("creating core bridge")
            let core = try TaskersCoreBridge(options: TaskersEnvironment.defaultCoreOptions())
            TaskersEnvironment.emitSmokeLog("creating surface host")
            let surfaceHost = TaskersDefaultSurfaceHost(core: core)
            TaskersEnvironment.emitSmokeLog("creating workspace controller")
            let controller = TaskersWorkspaceController(core: core, surfaceHost: surfaceHost)

            self.core = core
            self.surfaceHost = surfaceHost
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
        surfaceHost?.setFocused(true)
    }

    func applicationDidResignActive(_ notification: Notification) {
        _ = notification
        surfaceHost?.setFocused(false)
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        _ = sender
        return !TaskersEnvironment.isRunningUnderXCTest
    }

    @objc private func openBrowserInSplit(_ sender: Any?) {
        _ = sender
        do {
            try workspaceController?.openBrowserSplit()
        } catch {
            NSApp.presentError(error)
        }
    }

    private func configureMenus() {
        let mainMenu = NSMenu()

        let appMenuItem = NSMenuItem()
        let appMenu = NSMenu(title: "Taskers")
        appMenu.addItem(
            withTitle: "Quit Taskers",
            action: #selector(NSApplication.terminate(_:)),
            keyEquivalent: "q"
        )
        appMenuItem.submenu = appMenu
        mainMenu.addItem(appMenuItem)

        let surfaceMenuItem = NSMenuItem()
        let surfaceMenu = NSMenu(title: "Surface")
        let browserItem = NSMenuItem(
            title: "Open Browser in Split",
            action: #selector(openBrowserInSplit(_:)),
            keyEquivalent: "L"
        )
        browserItem.keyEquivalentModifierMask = [.command, .shift]
        browserItem.target = self
        surfaceMenu.addItem(browserItem)
        surfaceMenuItem.title = "Surface"
        surfaceMenuItem.submenu = surfaceMenu
        mainMenu.addItem(surfaceMenuItem)

        NSApp.mainMenu = mainMenu
    }
}

let app = NSApplication.shared
let delegate = TaskersMacApplication()
app.delegate = delegate
app.setActivationPolicy(.regular)
app.run()
