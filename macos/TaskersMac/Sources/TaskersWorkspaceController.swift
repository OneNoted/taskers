import AppKit
import Foundation

final class TaskersSurfaceHostView: NSView {
    let terminalView: TaskersTerminalView

    init(terminalView: TaskersTerminalView) {
        self.terminalView = terminalView
        super.init(frame: .zero)
        translatesAutoresizingMaskIntoConstraints = false

        terminalView.translatesAutoresizingMaskIntoConstraints = false
        addSubview(terminalView)
        NSLayoutConstraint.activate([
            terminalView.leadingAnchor.constraint(equalTo: leadingAnchor),
            terminalView.trailingAnchor.constraint(equalTo: trailingAnchor),
            terminalView.topAnchor.constraint(equalTo: topAnchor),
            terminalView.bottomAnchor.constraint(equalTo: bottomAnchor)
        ])
    }

    required init?(coder: NSCoder) {
        return nil
    }

    func dispose() {
        terminalView.dispose()
    }
}

final class WeightedSplitView: NSSplitView {
    private let weights: [CGFloat]

    init(isVertical: Bool, weights: [CGFloat]) {
        self.weights = weights
        super.init(frame: .zero)
        self.isVertical = isVertical
        dividerStyle = .thin
        translatesAutoresizingMaskIntoConstraints = false
    }

    required init?(coder: NSCoder) {
        return nil
    }

    override func layout() {
        super.layout()
        guard arrangedSubviews.count > 1 else {
            return
        }

        let totalWeight = max(weights.reduce(0, +), 1)
        let dividerCount = CGFloat(arrangedSubviews.count - 1)
        let available = (isVertical ? bounds.width : bounds.height) - (dividerThickness * dividerCount)

        var consumed: CGFloat = 0
        for index in 0..<(arrangedSubviews.count - 1) {
            consumed += available * (weights[index] / totalWeight)
            setPosition(consumed + dividerThickness * CGFloat(index), ofDividerAt: index)
        }
    }
}

final class TaskersWorkspaceController: NSWindowController {
    private let core: TaskersCoreBridge
    private let ghosttyHost: TaskersGhosttyHost
    private var surfaceRegistry: [String: TaskersSurfaceHostView] = [:]
    private var pollTimer: Timer?
    private var lastRevision: UInt64?
    private var didShutdown = false

    var surfaceCount: Int {
        surfaceRegistry.count
    }

    var lastRenderedSurfaceIDs: Set<String> {
        Set(surfaceRegistry.keys)
    }

    init(core: TaskersCoreBridge, ghosttyHost: TaskersGhosttyHost) {
        self.core = core
        self.ghosttyHost = ghosttyHost
        let window = NSWindow(
            contentRect: NSRect(x: 80, y: 80, width: 1380, height: 900),
            styleMask: [.titled, .closable, .miniaturizable, .resizable],
            backing: .buffered,
            defer: false
        )
        window.title = "Taskers"
        window.isReleasedWhenClosed = false
        super.init(window: window)
        ghosttyHost.onSurfaceClosed = { [weak self] workspaceID, paneID, surfaceID in
            self?.closeSurface(workspaceID: workspaceID, paneID: paneID, surfaceID: surfaceID)
        }
    }

    required init?(coder: NSCoder) {
        return nil
    }

    deinit {
        shutdown()
    }

    override func close() {
        shutdown()
        super.close()
    }

    func start() throws {
        try refresh(force: true)
        showWindow(nil)

        pollTimer = Timer.scheduledTimer(withTimeInterval: 0.1, repeats: true) { [weak self] _ in
            try? self?.refresh(force: false)
        }
    }

    func refresh(force: Bool) throws {
        let currentRevision = core.revision
        if !force, lastRevision == currentRevision {
            return
        }

        let snapshot = try core.snapshot()
        lastRevision = currentRevision
        pruneSurfaceRegistry(keeping: snapshot.liveSurfaceIDs)
        try render(snapshot: snapshot)
    }

    private func render(snapshot: TaskersSnapshot) throws {
        guard let workspace = snapshot.activeWorkspace else {
            window?.contentView = makeMessageView("No active workspace.")
            return
        }

        window?.title = workspace.label

        let rootView = try buildWorkspaceView(workspace)
        let container = NSView(frame: rootView.frame)
        container.translatesAutoresizingMaskIntoConstraints = false
        container.addSubview(rootView)

        NSLayoutConstraint.activate([
            rootView.leadingAnchor.constraint(equalTo: container.leadingAnchor),
            rootView.trailingAnchor.constraint(equalTo: container.trailingAnchor),
            rootView.topAnchor.constraint(equalTo: container.topAnchor),
            rootView.bottomAnchor.constraint(equalTo: container.bottomAnchor)
        ])

        window?.contentView = container
    }

    private func buildWorkspaceView(_ workspace: TaskersWorkspace) throws -> NSView {
        let columns = workspace.columns.elements.compactMap { _, column -> (TaskersWorkspaceColumn, [TaskersWorkspaceWindow])? in
            let windows = column.windowOrder.compactMap { workspace.windows[$0] }
            guard !windows.isEmpty else {
                return nil
            }
            return (column, windows)
        }

        guard !columns.isEmpty else {
            return makeMessageView("Workspace has no columns.")
        }

        if columns.count == 1 {
            return try buildColumnView(workspace: workspace, column: columns[0].0, windows: columns[0].1)
        }

        let split = WeightedSplitView(
            isVertical: true,
            weights: columns.map { CGFloat(max($0.0.width, 1)) }
        )
        for column in columns {
            split.addArrangedSubview(try buildColumnView(workspace: workspace, column: column.0, windows: column.1))
        }
        return split
    }

    private func buildColumnView(
        workspace: TaskersWorkspace,
        column: TaskersWorkspaceColumn,
        windows: [TaskersWorkspaceWindow]
    ) throws -> NSView {
        _ = column
        if windows.count == 1 {
            return try buildWindowView(workspace: workspace, window: windows[0])
        }

        let split = WeightedSplitView(
            isVertical: false,
            weights: windows.map { CGFloat(max($0.height, 1)) }
        )
        for window in windows {
            split.addArrangedSubview(try buildWindowView(workspace: workspace, window: window))
        }
        return split
    }

    private func buildWindowView(workspace: TaskersWorkspace, window: TaskersWorkspaceWindow) throws -> NSView {
        try buildLayoutView(workspace: workspace, node: window.layout)
    }

    private func buildLayoutView(workspace: TaskersWorkspace, node: TaskersLayoutNode) throws -> NSView {
        switch node {
        case .leaf(let paneID):
            return try surfaceView(workspace: workspace, paneID: paneID)
        case .split(let axis, let ratio, let first, let second):
            let split = WeightedSplitView(
                isVertical: axis == .horizontal,
                weights: [CGFloat(ratio), CGFloat(max(1000 - Int(ratio), 1))]
            )
            split.addArrangedSubview(try buildLayoutView(workspace: workspace, node: first))
            split.addArrangedSubview(try buildLayoutView(workspace: workspace, node: second))
            return split
        }
    }

    private func surfaceView(workspace: TaskersWorkspace, paneID: String) throws -> NSView {
        guard let pane = workspace.panes[paneID] else {
            return makeMessageView("Missing pane \(paneID)")
        }

        let activeSurfaceID = pane.activeSurface
        if let existing = surfaceRegistry[activeSurfaceID] {
            return existing
        }

        let descriptor = try core.surfaceDescriptor(workspaceId: workspace.id, paneId: paneID)
        let terminalView = try ghosttyHost.makeSurface(
            workspaceID: workspace.id,
            paneID: paneID,
            surfaceID: activeSurfaceID,
            descriptor: descriptor
        )
        let hostView = TaskersSurfaceHostView(terminalView: terminalView)
        surfaceRegistry[activeSurfaceID] = hostView
        return hostView
    }

    private func pruneSurfaceRegistry(keeping liveSurfaceIDs: Set<String>) {
        for surfaceID in Array(surfaceRegistry.keys) where !liveSurfaceIDs.contains(surfaceID) {
            guard let surface = surfaceRegistry.removeValue(forKey: surfaceID) else {
                continue
            }
            surface.dispose()
            surface.removeFromSuperview()
        }
    }

    private func closeSurface(workspaceID: String, paneID: String, surfaceID: String) {
        _ = try? core.dispatch(command: [
            "command": "close_surface",
            "workspace_id": workspaceID,
            "pane_id": paneID,
            "surface_id": surfaceID
        ])
    }

    private func makeMessageView(_ message: String) -> NSView {
        let label = NSTextField(labelWithString: message)
        label.font = .systemFont(ofSize: 16, weight: .medium)
        label.textColor = .secondaryLabelColor
        label.alignment = .center
        label.translatesAutoresizingMaskIntoConstraints = false

        let view = NSView(frame: .zero)
        view.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(label)
        NSLayoutConstraint.activate([
            label.centerXAnchor.constraint(equalTo: view.centerXAnchor),
            label.centerYAnchor.constraint(equalTo: view.centerYAnchor)
        ])
        return view
    }

    private func shutdown() {
        guard !didShutdown else {
            return
        }

        didShutdown = true
        pollTimer?.invalidate()
        pollTimer = nil
        ghosttyHost.onSurfaceClosed = nil
        for surface in surfaceRegistry.values {
            surface.dispose()
            surface.removeFromSuperview()
        }
        surfaceRegistry.removeAll()
        window?.contentView = nil
    }
}
