import AppKit
import Foundation

enum TaskersGhosttyHostError: LocalizedError {
    case configurationFailed
    case appCreationFailed

    var errorDescription: String? {
        switch self {
        case .configurationFailed:
            return "failed to initialize Ghostty configuration"
        case .appCreationFailed:
            return "failed to initialize the embedded Ghostty runtime"
        }
    }
}

final class TaskersGhosttyHost: NSObject {
    private var config: ghostty_config_t?
    private var app: ghostty_app_t?
    private let surfaces = NSHashTable<TaskersTerminalView>.weakObjects()

    var onSurfaceClosed: ((String, String, String) -> Void)?

    override init() {
        super.init()
    }

    deinit {
        if let app {
            ghostty_app_free(app)
        }
        if let config {
            ghostty_config_free(config)
        }
    }

    func bootstrap() throws {
        guard config == nil, app == nil else {
            return
        }

        guard let config = ghostty_config_new() else {
            throw TaskersGhosttyHostError.configurationFailed
        }

        ghostty_config_load_default_files(config)
        ghostty_config_load_recursive_files(config)
        ghostty_config_finalize(config)

        var runtime = ghostty_runtime_config_s(
            userdata: Unmanaged.passUnretained(self).toOpaque(),
            supports_selection_clipboard: false,
            wakeup_cb: { userdata in
                TaskersGhosttyHost.from(userdata)?.scheduleTick()
            },
            action_cb: { _, target, action in
                TaskersGhosttyHost.handleAction(target: target, action: action)
            },
            read_clipboard_cb: { _, _, _ in },
            confirm_read_clipboard_cb: { _, _, _, _ in },
            write_clipboard_cb: { _, _, _, _, _ in },
            close_surface_cb: { userdata, _ in
                TaskersGhosttySurfaceContext.from(userdata: userdata)?.handleSurfaceClosed()
            }
        )

        guard let app = ghostty_app_new(&runtime, config) else {
            ghostty_config_free(config)
            throw TaskersGhosttyHostError.appCreationFailed
        }

        self.config = config
        self.app = app
        ghostty_app_set_focus(app, NSApp.isActive)
    }

    func makeSurface(
        workspaceID: String,
        paneID: String,
        surfaceID: String,
        descriptor: TaskersSurfaceDescriptor
    ) throws -> TaskersTerminalView {
        try bootstrap()
        guard let app else {
            throw TaskersGhosttyHostError.appCreationFailed
        }

        let view = try TaskersTerminalView(
            host: self,
            app: app,
            workspaceID: workspaceID,
            paneID: paneID,
            surfaceID: surfaceID,
            descriptor: descriptor
        )
        registerSurface(view)
        return view
    }

    func registerSurface(_ surface: TaskersTerminalView) {
        surfaces.add(surface)
    }

    func unregisterSurface(_ surface: TaskersTerminalView) {
        surfaces.remove(surface)
    }

    func setFocused(_ focused: Bool) {
        guard let app else {
            return
        }

        ghostty_app_set_focus(app, focused)
    }

    private func scheduleTick() {
        DispatchQueue.main.async { [weak self] in
            self?.tick()
        }
    }

    private func tick() {
        guard let app else {
            return
        }

        ghostty_app_tick(app)
        for view in surfaces.allObjects {
            view.refresh()
        }
    }

    func surfaceDidClose(workspaceID: String, paneID: String, surfaceID: String) {
        onSurfaceClosed?(workspaceID, paneID, surfaceID)
    }

    private static func handleAction(target: ghostty_target_s, action: ghostty_action_s) -> Bool {
        guard target.tag == GHOSTTY_TARGET_SURFACE else {
            return false
        }

        guard let surface = target.target.surface else {
            return false
        }

        guard let context = TaskersGhosttySurfaceContext.from(surface: surface) else {
            return false
        }

        switch action.tag {
        case GHOSTTY_ACTION_SHOW_CHILD_EXITED:
            context.handleChildExited(exitCode: action.action.child_exited.exit_code)
            return true
        default:
            return false
        }
    }

    private static func from(_ userdata: UnsafeMutableRawPointer?) -> TaskersGhosttyHost? {
        guard let userdata else {
            return nil
        }

        return Unmanaged<TaskersGhosttyHost>.fromOpaque(userdata).takeUnretainedValue()
    }
}
