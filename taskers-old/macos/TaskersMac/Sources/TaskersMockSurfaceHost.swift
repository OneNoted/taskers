import AppKit
import Foundation

final class TaskersMockSurfaceView: NSView, TaskersHostedSurface {
    var hostingView: NSView { self }

    init(title: String?) {
        super.init(frame: NSRect(x: 0, y: 0, width: 640, height: 420))
        wantsLayer = true
        layer?.backgroundColor = NSColor.windowBackgroundColor.cgColor

        let label = NSTextField(labelWithString: title ?? "Taskers Mock Surface")
        label.font = .monospacedSystemFont(ofSize: 14, weight: .medium)
        label.textColor = .labelColor
        label.translatesAutoresizingMaskIntoConstraints = false
        addSubview(label)

        NSLayoutConstraint.activate([
            label.centerXAnchor.constraint(equalTo: centerXAnchor),
            label.centerYAnchor.constraint(equalTo: centerYAnchor)
        ])
    }

    required init?(coder: NSCoder) {
        return nil
    }

    func dispose() {}
}

final class TaskersMockSurfaceHost: TaskersSurfaceHosting {
    var onSurfaceClosed: ((String, String, String) -> Void)?

    func makeSurface(
        workspaceID: String,
        paneID: String,
        surfaceID: String,
        descriptor: TaskersSurfaceDescriptor
    ) throws -> any TaskersHostedSurface {
        _ = workspaceID
        _ = paneID
        _ = surfaceID
        let title = switch descriptor.kind {
        case .terminal:
            descriptor.title
        case .browser:
            descriptor.title ?? descriptor.url ?? "Taskers Mock Browser"
        }
        return TaskersMockSurfaceView(title: title)
    }

    func setFocused(_ focused: Bool) {
        _ = focused
    }
}
