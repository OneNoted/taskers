import AppKit
import Foundation

final class TaskersTerminalView: NSView {
    let workspaceID: String
    let paneID: String
    let surfaceID: String

    private weak var host: TaskersGhosttyHost?
    private var surface: ghostty_surface_t?
    private var commandString: String

    override var acceptsFirstResponder: Bool {
        true
    }

    init(
        host: TaskersGhosttyHost,
        app: ghostty_app_t,
        workspaceID: String,
        paneID: String,
        surfaceID: String,
        descriptor: TaskersSurfaceDescriptor
    ) throws {
        self.host = host
        self.workspaceID = workspaceID
        self.paneID = paneID
        self.surfaceID = surfaceID
        self.commandString = Self.commandString(for: descriptor.commandArgv)

        super.init(frame: NSRect(x: 0, y: 0, width: 640, height: 420))
        wantsLayer = true
        layer?.backgroundColor = NSColor.black.cgColor

        self.surface = try Self.createSurface(
            view: self,
            app: app,
            descriptor: descriptor,
            commandString: commandString
        )
        updateSurfaceMetrics()
    }

    required init?(coder: NSCoder) {
        return nil
    }

    deinit {
        if let surface {
            ghostty_surface_free(surface)
        }
        host?.unregisterSurface(self)
    }

    override func becomeFirstResponder() -> Bool {
        setFocused(true)
        return true
    }

    override func resignFirstResponder() -> Bool {
        setFocused(false)
        return true
    }

    override func draw(_ dirtyRect: NSRect) {
        super.draw(dirtyRect)

        guard let surface else {
            return
        }

        ghostty_surface_draw(surface)
    }

    override func layout() {
        super.layout()
        updateSurfaceMetrics()
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        updateSurfaceMetrics()
    }

    override func viewDidChangeBackingProperties() {
        super.viewDidChangeBackingProperties()
        updateSurfaceMetrics()
    }

    override func keyDown(with event: NSEvent) {
        if let characters = event.characters, !characters.isEmpty {
            sendText(characters)
        } else {
            super.keyDown(with: event)
        }
    }

    override func mouseDown(with event: NSEvent) {
        window?.makeFirstResponder(self)
        sendMouseButton(event, action: GHOSTTY_MOUSE_PRESS)
    }

    override func mouseUp(with event: NSEvent) {
        sendMouseButton(event, action: GHOSTTY_MOUSE_RELEASE)
    }

    override func mouseDragged(with event: NSEvent) {
        sendMousePosition(event)
    }

    override func mouseMoved(with event: NSEvent) {
        sendMousePosition(event)
    }

    override func scrollWheel(with event: NSEvent) {
        guard let surface else {
            return
        }

        let mods = Int32(Self.modifiers(from: event.modifierFlags).rawValue)
        ghostty_surface_mouse_scroll(
            surface,
            event.scrollingDeltaX,
            event.scrollingDeltaY,
            mods
        )
        refresh()
    }

    func refresh() {
        needsDisplay = true
    }

    func handleChildExited(exitCode: UInt32) {
        _ = exitCode
        host?.surfaceDidClose(workspaceID: workspaceID, paneID: paneID, surfaceID: surfaceID)
    }

    func handleSurfaceClosed() {
        host?.surfaceDidClose(workspaceID: workspaceID, paneID: paneID, surfaceID: surfaceID)
    }

    private func setFocused(_ focused: Bool) {
        guard let surface else {
            return
        }

        ghostty_surface_set_focus(surface, focused)
        refresh()
    }

    private func updateSurfaceMetrics() {
        guard let surface else {
            return
        }

        let backingRect = convertToBacking(bounds)
        let width = UInt32(max(1, Int(backingRect.width)))
        let height = UInt32(max(1, Int(backingRect.height)))
        ghostty_surface_set_size(surface, width, height)

        let scale = window?.backingScaleFactor ?? window?.screen?.backingScaleFactor ?? NSScreen.main?.backingScaleFactor ?? 2.0
        ghostty_surface_set_content_scale(surface, scale, scale)
    }

    private func sendText(_ text: String) {
        guard let surface else {
            return
        }

        let length = text.utf8.count
        text.withCString { pointer in
            ghostty_surface_text(surface, pointer, UInt(length))
        }
        refresh()
    }

    private func sendMouseButton(_ event: NSEvent, action: ghostty_input_mouse_state_e) {
        guard let surface else {
            return
        }

        ghostty_surface_mouse_button(
            surface,
            action,
            Self.mouseButton(from: event.buttonNumber),
            Self.modifiers(from: event.modifierFlags)
        )
        sendMousePosition(event)
    }

    private func sendMousePosition(_ event: NSEvent) {
        guard let surface else {
            return
        }

        let point = convert(event.locationInWindow, from: nil)
        ghostty_surface_mouse_pos(
            surface,
            Double(point.x),
            Double(point.y),
            Self.modifiers(from: event.modifierFlags)
        )
        refresh()
    }

    private static func createSurface(
        view: TaskersTerminalView,
        app: ghostty_app_t,
        descriptor: TaskersSurfaceDescriptor,
        commandString: String
    ) throws -> ghostty_surface_t {
        let scale = NSScreen.main?.backingScaleFactor ?? 2.0
        var config = ghostty_surface_config_new()
        config.platform_tag = GHOSTTY_PLATFORM_MACOS
        config.platform = ghostty_platform_u(
            macos: ghostty_platform_macos_s(nsview: Unmanaged.passUnretained(view).toOpaque())
        )
        config.userdata = Unmanaged.passUnretained(view).toOpaque()
        config.scale_factor = scale
        config.context = GHOSTTY_SURFACE_CONTEXT_SPLIT

        return try withCString(descriptor.cwd) { workingDirectory in
            config.working_directory = workingDirectory
            return try commandString.withCString { command in
                config.command = command
                return try withEnvironment(descriptor.env) { envVars, count in
                    config.env_vars = envVars
                    config.env_var_count = count
                    guard let surface = ghostty_surface_new(app, &config) else {
                        throw TaskersGhosttyHostError.appCreationFailed
                    }
                    return surface
                }
            }
        }
    }

    private static func commandString(for argv: [String]) -> String {
        argv.map(shellQuote).joined(separator: " ")
    }

    private static func shellQuote(_ value: String) -> String {
        if value.isEmpty {
            return "''"
        }

        let allowed = CharacterSet(charactersIn: "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-._/:")
        if value.unicodeScalars.allSatisfy({ allowed.contains($0) }) {
            return value
        }

        return "'" + value.replacingOccurrences(of: "'", with: "'\"'\"'") + "'"
    }

    private static func withCString<T>(_ value: String?, body: (UnsafePointer<CChar>?) throws -> T) rethrows -> T {
        guard let value else {
            return try body(nil)
        }

        return try value.withCString(body)
    }

    private static func withEnvironment<T>(
        _ environment: [String: String],
        body: (UnsafeMutablePointer<ghostty_env_var_s>?, Int) throws -> T
    ) rethrows -> T {
        let entries = Array(environment)
        return try withCStringPairs(entries) { envVars in
            try envVars.withUnsafeMutableBufferPointer { buffer in
                try body(buffer.baseAddress, buffer.count)
            }
        }
    }

    private static func withCStringPairs<T>(
        _ entries: [(key: String, value: String)],
        body: ([ghostty_env_var_s]) throws -> T
    ) rethrows -> T {
        func recurse(
            _ index: Int,
            _ envVars: inout [ghostty_env_var_s],
            _ body: ([ghostty_env_var_s]) throws -> T
        ) rethrows -> T {
            if index == entries.count {
                return try body(envVars)
            }

            let entry = entries[index]
            return try entry.key.withCString { keyPointer in
                try entry.value.withCString { valuePointer in
                    envVars.append(ghostty_env_var_s(key: keyPointer, value: valuePointer))
                    defer { envVars.removeLast() }
                    return try recurse(index + 1, &envVars, body)
                }
            }
        }

        var envVars: [ghostty_env_var_s] = []
        envVars.reserveCapacity(entries.count)
        return try recurse(0, &envVars, body)
    }

    private static func mouseButton(from buttonNumber: Int) -> ghostty_input_mouse_button_e {
        switch buttonNumber {
        case 1:
            return GHOSTTY_MOUSE_RIGHT
        case 2:
            return GHOSTTY_MOUSE_MIDDLE
        default:
            return GHOSTTY_MOUSE_LEFT
        }
    }

    private static func modifiers(from flags: NSEvent.ModifierFlags) -> ghostty_input_mods_e {
        var mods = Int32(GHOSTTY_MODS_NONE.rawValue)
        if flags.contains(.shift) {
            mods |= Int32(GHOSTTY_MODS_SHIFT.rawValue)
        }
        if flags.contains(.control) {
            mods |= Int32(GHOSTTY_MODS_CTRL.rawValue)
        }
        if flags.contains(.option) {
            mods |= Int32(GHOSTTY_MODS_ALT.rawValue)
        }
        if flags.contains(.command) {
            mods |= Int32(GHOSTTY_MODS_SUPER.rawValue)
        }
        return ghostty_input_mods_e(mods)
    }

    static func from(surface: ghostty_surface_t) -> TaskersTerminalView? {
        from(userdata: ghostty_surface_userdata(surface))
    }

    static func from(userdata: UnsafeMutableRawPointer?) -> TaskersTerminalView? {
        guard let userdata else {
            return nil
        }

        return Unmanaged<TaskersTerminalView>.fromOpaque(userdata).takeUnretainedValue()
    }
}
