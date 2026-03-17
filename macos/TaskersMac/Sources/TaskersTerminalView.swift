import AppKit
import Darwin
import Foundation

#if DEBUG
func taskersMacDebugLog(_ message: @autoclosure () -> String) {
    NSLog("[taskers-macos] %@", message())
}
#else
func taskersMacDebugLog(_ message: @autoclosure () -> String) {
    _ = message()
}
#endif

final class TaskersGhosttySurfaceContext {
    private weak var host: TaskersGhosttyHost?
    private(set) var isClosing = false

    let workspaceID: String
    let paneID: String
    let surfaceID: String

    init(host: TaskersGhosttyHost, workspaceID: String, paneID: String, surfaceID: String) {
        self.host = host
        self.workspaceID = workspaceID
        self.paneID = paneID
        self.surfaceID = surfaceID
    }

    func retainForUserdata() -> UnsafeMutableRawPointer {
        Unmanaged.passRetained(self).toOpaque()
    }

    func beginTeardown() {
        isClosing = true
    }

    func handleChildExited(exitCode: UInt32) {
        _ = exitCode
        taskersMacDebugLog("surface \(surfaceID) child exited")
        closeSurfaceIfNeeded()
    }

    func handleSurfaceClosed() {
        taskersMacDebugLog("surface \(surfaceID) close callback")
        closeSurfaceIfNeeded()
    }

    private func closeSurfaceIfNeeded() {
        if !Thread.isMainThread {
            DispatchQueue.main.async { [self] in
                closeSurfaceIfNeeded()
            }
            return
        }

        guard !isClosing else {
            return
        }

        isClosing = true
        host?.surfaceDidClose(workspaceID: workspaceID, paneID: paneID, surfaceID: surfaceID)
    }

    static func from(surface: ghostty_surface_t) -> TaskersGhosttySurfaceContext? {
        from(userdata: ghostty_surface_userdata(surface))
    }

    static func from(userdata: UnsafeMutableRawPointer?) -> TaskersGhosttySurfaceContext? {
        guard let userdata else {
            return nil
        }

        return Unmanaged<TaskersGhosttySurfaceContext>.fromOpaque(userdata).takeUnretainedValue()
    }

    static func releaseUserdata(_ userdata: UnsafeMutableRawPointer) {
        Unmanaged<TaskersGhosttySurfaceContext>.fromOpaque(userdata).release()
    }
}

final class TaskersGhosttyLaunchStorage {
    private var workingDirectoryStorage: UnsafeMutablePointer<CChar>?
    private var commandStorage: UnsafeMutablePointer<CChar>?
    private var envKeyStorage: [UnsafeMutablePointer<CChar>] = []
    private var envValueStorage: [UnsafeMutablePointer<CChar>] = []
    private var envVars: [ghostty_env_var_s] = []

    init(cwd: String?, command: String, environment: [String: String]) {
        if let cwd {
            workingDirectoryStorage = strdup(cwd)
        }
        commandStorage = strdup(command)

        let entries = environment.sorted { $0.key < $1.key }
        envKeyStorage.reserveCapacity(entries.count)
        envValueStorage.reserveCapacity(entries.count)
        envVars.reserveCapacity(entries.count)

        for entry in entries {
            guard
                let key = strdup(entry.key),
                let value = strdup(entry.value)
            else {
                continue
            }

            envKeyStorage.append(key)
            envValueStorage.append(value)
            envVars.append(ghostty_env_var_s(key: UnsafePointer(key), value: UnsafePointer(value)))
        }
    }

    deinit {
        if let workingDirectoryStorage {
            free(workingDirectoryStorage)
        }
        if let commandStorage {
            free(commandStorage)
        }
        for key in envKeyStorage {
            free(key)
        }
        for value in envValueStorage {
            free(value)
        }
    }

    var workingDirectoryPointer: UnsafePointer<CChar>? {
        workingDirectoryStorage.map { UnsafePointer<CChar>($0) }
    }

    var commandPointer: UnsafePointer<CChar>? {
        commandStorage.map { UnsafePointer<CChar>($0) }
    }

    func withEnvironment<T>(
        _ body: (UnsafeMutablePointer<ghostty_env_var_s>?, Int) throws -> T
    ) rethrows -> T {
        try envVars.withUnsafeMutableBufferPointer { buffer in
            try body(buffer.baseAddress, buffer.count)
        }
    }
}

final class TaskersTerminalView: NSView, TaskersHostedSurface {
    let workspaceID: String
    let paneID: String
    let surfaceID: String

    private weak var host: TaskersGhosttyHost?
    private let callbackContext: TaskersGhosttySurfaceContext
    private var callbackContextHandle: UnsafeMutableRawPointer?
    private let launchStorage: TaskersGhosttyLaunchStorage
    private var isDisposed = false
    private var surface: ghostty_surface_t?
    private var commandString: String

    override var acceptsFirstResponder: Bool {
        true
    }

    var hostingView: NSView { self }

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
        self.callbackContext = TaskersGhosttySurfaceContext(
            host: host,
            workspaceID: workspaceID,
            paneID: paneID,
            surfaceID: surfaceID
        )
        self.callbackContextHandle = callbackContext.retainForUserdata()
        self.commandString = Self.commandString(for: descriptor.commandArgv)
        self.launchStorage = TaskersGhosttyLaunchStorage(
            cwd: descriptor.cwd,
            command: commandString,
            environment: descriptor.env
        )

        super.init(frame: NSRect(x: 0, y: 0, width: 640, height: 420))
        wantsLayer = true
        layer?.backgroundColor = NSColor.black.cgColor

        self.surface = try Self.createSurface(
            view: self,
            app: app,
            launchStorage: launchStorage,
            userdata: self.callbackContextHandle!
        )
        updateSurfaceMetrics()
    }

    required init?(coder: NSCoder) {
        return nil
    }

    deinit {
        dispose()
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
        if !sendKey(event, action: event.isARepeat ? GHOSTTY_ACTION_REPEAT : GHOSTTY_ACTION_PRESS) {
            super.keyDown(with: event)
        }
    }

    override func keyUp(with event: NSEvent) {
        if !sendKey(event, action: GHOSTTY_ACTION_RELEASE) {
            super.keyUp(with: event)
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

    func beginTeardown() {
        callbackContext.beginTeardown()
    }

    func dispose() {
        guard !isDisposed else {
            return
        }

        isDisposed = true
        callbackContext.beginTeardown()
        host?.unregisterSurface(self)
        taskersMacDebugLog("dispose start surface=\(surfaceID)")

        let surface = self.surface
        self.surface = nil
        let launchStorage = self.launchStorage

        guard let callbackContextHandle else {
            return
        }
        self.callbackContextHandle = nil

        let cleanup = {
            _ = launchStorage
            taskersMacDebugLog("dispose cleanup begin surface=\(self.surfaceID)")
            if let surface {
                ghostty_surface_free(surface)
            }
            TaskersGhosttySurfaceContext.releaseUserdata(callbackContextHandle)
            taskersMacDebugLog("dispose cleanup end surface=\(self.surfaceID)")
        }
        DispatchQueue.main.async(execute: cleanup)
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

    private func sendKey(_ event: NSEvent, action: ghostty_input_action_e) -> Bool {
        guard let surface else {
            return false
        }

        let translatedModifiers = Self.modifierFlags(
            from: ghostty_surface_key_translation_mods(
                surface,
                Self.modifiers(from: event.modifierFlags)
            )
        )

        let translationEvent: NSEvent
        if translatedModifiers == event.modifierFlags {
            translationEvent = event
        } else {
            translationEvent = NSEvent.keyEvent(
                with: event.type,
                location: event.locationInWindow,
                modifierFlags: translatedModifiers,
                timestamp: event.timestamp,
                windowNumber: event.windowNumber,
                context: nil,
                characters: event.characters(byApplyingModifiers: translatedModifiers) ?? "",
                charactersIgnoringModifiers: event.charactersIgnoringModifiers ?? "",
                isARepeat: event.isARepeat,
                keyCode: event.keyCode
            ) ?? event
        }

        var keyEvent = event.taskersGhosttyKeyEvent(
            action,
            translationMods: translationEvent.modifierFlags
        )

        let handled: Bool
        if let text = translationEvent.taskersGhosttyCharacters,
           !text.isEmpty,
           let codepoint = text.utf8.first,
           codepoint >= 0x20 {
            handled = text.withCString { pointer in
                keyEvent.text = pointer
                return ghostty_surface_key(surface, keyEvent)
            }
        } else {
            handled = ghostty_surface_key(surface, keyEvent)
        }

        if handled {
            refresh()
        }
        return handled
    }

    private static func createSurface(
        view: TaskersTerminalView,
        app: ghostty_app_t,
        launchStorage: TaskersGhosttyLaunchStorage,
        userdata: UnsafeMutableRawPointer
    ) throws -> ghostty_surface_t {
        let scale = NSScreen.main?.backingScaleFactor ?? 2.0
        var config = ghostty_surface_config_new()
        config.platform_tag = GHOSTTY_PLATFORM_MACOS
        config.platform = ghostty_platform_u(
            macos: ghostty_platform_macos_s(nsview: Unmanaged.passUnretained(view).toOpaque())
        )
        config.userdata = userdata
        config.scale_factor = scale
        config.context = GHOSTTY_SURFACE_CONTEXT_SPLIT
        config.working_directory = launchStorage.workingDirectoryPointer
        config.command = launchStorage.commandPointer

        return try launchStorage.withEnvironment { envVars, count in
            config.env_vars = envVars
            config.env_var_count = count
            guard let surface = ghostty_surface_new(app, &config) else {
                throw TaskersGhosttyHostError.appCreationFailed
            }
            return surface
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

    fileprivate static func modifiers(from flags: NSEvent.ModifierFlags) -> ghostty_input_mods_e {
        var mods = GHOSTTY_MODS_NONE.rawValue
        if flags.contains(.shift) {
            mods |= GHOSTTY_MODS_SHIFT.rawValue
        }
        if flags.contains(.control) {
            mods |= GHOSTTY_MODS_CTRL.rawValue
        }
        if flags.contains(.option) {
            mods |= GHOSTTY_MODS_ALT.rawValue
        }
        if flags.contains(.command) {
            mods |= GHOSTTY_MODS_SUPER.rawValue
        }
        return ghostty_input_mods_e(rawValue: mods)
    }

    private static func modifierFlags(from mods: ghostty_input_mods_e) -> NSEvent.ModifierFlags {
        let raw = mods.rawValue
        var flags: NSEvent.ModifierFlags = []
        if raw & GHOSTTY_MODS_SHIFT.rawValue != 0 {
            flags.insert(.shift)
        }
        if raw & GHOSTTY_MODS_CTRL.rawValue != 0 {
            flags.insert(.control)
        }
        if raw & GHOSTTY_MODS_ALT.rawValue != 0 {
            flags.insert(.option)
        }
        if raw & GHOSTTY_MODS_SUPER.rawValue != 0 {
            flags.insert(.command)
        }
        return flags
    }

}

private extension NSEvent {
    func taskersGhosttyKeyEvent(
        _ action: ghostty_input_action_e,
        translationMods: NSEvent.ModifierFlags? = nil
    ) -> ghostty_input_key_s {
        var keyEvent: ghostty_input_key_s = .init()
        keyEvent.action = action
        keyEvent.keycode = UInt32(keyCode)
        keyEvent.text = nil
        keyEvent.composing = false
        keyEvent.mods = TaskersTerminalView.modifiers(from: modifierFlags)
        keyEvent.consumed_mods = TaskersTerminalView.modifiers(
            from: (translationMods ?? modifierFlags).subtracting([.control, .command])
        )

        if type == .keyDown || type == .keyUp,
           let chars = characters(byApplyingModifiers: []),
           let codepoint = chars.unicodeScalars.first {
            keyEvent.unshifted_codepoint = codepoint.value
        } else {
            keyEvent.unshifted_codepoint = 0
        }

        return keyEvent
    }

    var taskersGhosttyCharacters: String? {
        guard let characters else {
            return nil
        }

        if characters.count == 1,
           let scalar = characters.unicodeScalars.first {
            if scalar.value < 0x20 {
                return self.characters(byApplyingModifiers: modifierFlags.subtracting(.control))
            }

            if scalar.value >= 0xF700 && scalar.value <= 0xF8FF {
                return nil
            }
        }

        return characters
    }
}
