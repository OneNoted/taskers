import AppKit
import Foundation

final class TaskersSurfaceHostView: NSView {
    let surface: any TaskersHostedSurface

    init(surface: any TaskersHostedSurface) {
        self.surface = surface
        super.init(frame: .zero)
        translatesAutoresizingMaskIntoConstraints = false

        let hostedView = surface.hostingView
        hostedView.translatesAutoresizingMaskIntoConstraints = false
        addSubview(hostedView)
        NSLayoutConstraint.activate([
            hostedView.leadingAnchor.constraint(equalTo: leadingAnchor),
            hostedView.trailingAnchor.constraint(equalTo: trailingAnchor),
            hostedView.topAnchor.constraint(equalTo: topAnchor),
            hostedView.bottomAnchor.constraint(equalTo: bottomAnchor)
        ])
    }

    required init?(coder: NSCoder) {
        return nil
    }

    func dispose() {
        surface.dispose()
    }
}

final class WeightedSplitView: NSSplitView {
    private let weights: [CGFloat]
    private var hasPendingWeightApplication = false
    private var lastAppliedSignature: WeightApplicationSignature?

    private struct WeightApplicationSignature: Equatable {
        let size: CGSize
        let arrangedSubviewCount: Int
    }

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
        scheduleWeightApplication()
    }

    override func didAddSubview(_ subview: NSView) {
        super.didAddSubview(subview)
        scheduleWeightApplication()
    }

    private func scheduleWeightApplication() {
        guard arrangedSubviews.count > 1 else {
            return
        }

        let signature = WeightApplicationSignature(
            size: bounds.size,
            arrangedSubviewCount: arrangedSubviews.count
        )
        guard signature != lastAppliedSignature || !hasPendingWeightApplication else {
            return
        }

        hasPendingWeightApplication = true
        DispatchQueue.main.async { [weak self] in
            self?.applyWeightsIfNeeded(expectedSignature: signature)
        }
    }

    private func applyWeightsIfNeeded(expectedSignature: WeightApplicationSignature) {
        hasPendingWeightApplication = false
        guard arrangedSubviews.count > 1 else {
            lastAppliedSignature = expectedSignature
            return
        }

        let currentSignature = WeightApplicationSignature(
            size: bounds.size,
            arrangedSubviewCount: arrangedSubviews.count
        )
        guard currentSignature == expectedSignature else {
            scheduleWeightApplication()
            return
        }

        let totalWeight = max(weights.reduce(0, +), 1)
        let dividerCount = CGFloat(arrangedSubviews.count - 1)
        let available = max((isVertical ? bounds.width : bounds.height) - (dividerThickness * dividerCount), 0)

        var consumed: CGFloat = 0
        for index in 0..<(arrangedSubviews.count - 1) {
            consumed += available * (weights[index] / totalWeight)
            setPosition(consumed + dividerThickness * CGFloat(index), ofDividerAt: index)
        }
        lastAppliedSignature = currentSignature
    }
}

enum TaskersWorkspaceControllerError: LocalizedError {
    case missingActiveWorkspace
    case missingWorkspace(String)
    case missingPane(String)

    var errorDescription: String? {
        switch self {
        case .missingActiveWorkspace:
            return "no active workspace is available"
        case .missingWorkspace(let workspaceID):
            return "workspace \(workspaceID) is not present"
        case .missingPane(let paneID):
            return "pane \(paneID) is not present"
        }
    }
}

final class TaskersWorkspaceController: NSWindowController {
    private let core: TaskersCoreBridge
    private let surfaceHost: any TaskersSurfaceHosting
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

    init(core: TaskersCoreBridge, surfaceHost: any TaskersSurfaceHosting) {
        self.core = core
        self.surfaceHost = surfaceHost
        let window = NSWindow(
            contentRect: NSRect(x: 80, y: 80, width: 1380, height: 900),
            styleMask: [.titled, .closable, .miniaturizable, .resizable],
            backing: .buffered,
            defer: false
        )
        window.title = "Taskers"
        window.isReleasedWhenClosed = false
        super.init(window: window)
        surfaceHost.onSurfaceClosed = { [weak self] workspaceID, paneID, surfaceID in
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

    func openBrowserSplit(url: String? = nil) throws {
        let snapshot = try core.snapshot()
        guard let workspace = snapshot.activeWorkspace else {
            throw TaskersWorkspaceControllerError.missingActiveWorkspace
        }

        let newPaneID = try core.splitPane(
            workspaceId: workspace.id,
            paneId: workspace.activePane,
            axis: "horizontal"
        )
        let placeholderSurfaceID = try activeSurfaceID(
            workspaceID: workspace.id,
            paneID: newPaneID
        )
        let browserSurfaceID = try core.createSurface(
            workspaceId: workspace.id,
            paneId: newPaneID,
            kind: .browser
        )
        if let url = url?.trimmingCharacters(in: .whitespacesAndNewlines),
           !url.isEmpty {
            try core.updateSurfaceMetadata(surfaceId: browserSurfaceID, url: url)
        }
        try core.closeSurface(
            workspaceID: workspace.id,
            paneID: newPaneID,
            surfaceID: placeholderSurfaceID
        )
        try refresh(force: true)
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
        let surface = try surfaceHost.makeSurface(
            workspaceID: workspace.id,
            paneID: paneID,
            surfaceID: activeSurfaceID,
            descriptor: descriptor
        )
        let hostView = TaskersSurfaceHostView(surface: surface)
        surfaceRegistry[activeSurfaceID] = hostView
        return hostView
    }

    private func pruneSurfaceRegistry(keeping liveSurfaceIDs: Set<String>) {
        for surfaceID in Array(surfaceRegistry.keys) where !liveSurfaceIDs.contains(surfaceID) {
            taskersMacDebugLog("prune surface=\(surfaceID)")
            guard let surface = surfaceRegistry.removeValue(forKey: surfaceID) else {
                continue
            }
            surface.dispose()
            surface.removeFromSuperview()
        }
    }

    private func closeSurface(workspaceID: String, paneID: String, surfaceID: String) {
        taskersMacDebugLog("close surface command workspace=\(workspaceID) pane=\(paneID) surface=\(surfaceID)")
        try? core.closeSurface(workspaceID: workspaceID, paneID: paneID, surfaceID: surfaceID)
    }

    private func activeSurfaceID(workspaceID: String, paneID: String) throws -> String {
        let snapshot = try core.snapshot()
        guard let workspace = snapshot.workspaces[workspaceID] else {
            throw TaskersWorkspaceControllerError.missingWorkspace(workspaceID)
        }
        guard let pane = workspace.panes[paneID] else {
            throw TaskersWorkspaceControllerError.missingPane(paneID)
        }
        return pane.activeSurface
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
        surfaceHost.onSurfaceClosed = nil
        for surface in surfaceRegistry.values {
            surface.dispose()
            surface.removeFromSuperview()
        }
        surfaceRegistry.removeAll()
        window?.contentView = nil
    }
}
