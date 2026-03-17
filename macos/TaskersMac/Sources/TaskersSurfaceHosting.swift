import AppKit
import Foundation

protocol TaskersHostedSurface: AnyObject {
    var hostingView: NSView { get }
    func dispose()
}

protocol TaskersSurfaceHosting: AnyObject {
    var onSurfaceClosed: ((String, String, String) -> Void)? { get set }

    func makeSurface(
        workspaceID: String,
        paneID: String,
        surfaceID: String,
        descriptor: TaskersSurfaceDescriptor
    ) throws -> any TaskersHostedSurface

    func setFocused(_ focused: Bool)
}
