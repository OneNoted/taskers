import Foundation

final class TaskersDefaultSurfaceHost: TaskersSurfaceHosting {
    private let core: TaskersCoreBridge
    private let ghosttyHost: TaskersGhosttyHost

    var onSurfaceClosed: ((String, String, String) -> Void)? {
        didSet {
            ghosttyHost.onSurfaceClosed = onSurfaceClosed
        }
    }

    init(core: TaskersCoreBridge, ghosttyHost: TaskersGhosttyHost = TaskersGhosttyHost()) {
        self.core = core
        self.ghosttyHost = ghosttyHost
        self.ghosttyHost.onSurfaceClosed = onSurfaceClosed
    }

    func makeSurface(
        workspaceID: String,
        paneID: String,
        surfaceID: String,
        descriptor: TaskersSurfaceDescriptor
    ) throws -> any TaskersHostedSurface {
        switch descriptor.kind {
        case .terminal:
            return try ghosttyHost.makeSurface(
                workspaceID: workspaceID,
                paneID: paneID,
                surfaceID: surfaceID,
                descriptor: descriptor
            )
        case .browser:
            return TaskersBrowserView(
                core: core,
                workspaceID: workspaceID,
                paneID: paneID,
                surfaceID: surfaceID,
                descriptor: descriptor
            )
        }
    }

    func setFocused(_ focused: Bool) {
        ghosttyHost.setFocused(focused)
    }
}
