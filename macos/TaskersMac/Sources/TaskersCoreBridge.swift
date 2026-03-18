import Foundation

struct TaskersCoreOptions: Encodable {
    let sessionPath: String?
    let socketPath: String?
    let configuredShell: String?
    let demo: Bool
    let backend: String

    enum CodingKeys: String, CodingKey {
        case sessionPath = "session_path"
        case socketPath = "socket_path"
        case configuredShell = "configured_shell"
        case demo
        case backend
    }
}

enum TaskersSurfaceKind: String, Codable {
    case terminal
    case browser
}

struct TaskersSurfaceDescriptor: Decodable {
    let cols: UInt16
    let rows: UInt16
    let kind: TaskersSurfaceKind
    let cwd: String?
    let title: String?
    let url: String?
    let commandArgv: [String]
    let env: [String: String]

    enum CodingKeys: String, CodingKey {
        case cols
        case rows
        case kind
        case cwd
        case title
        case url
        case commandArgv = "command_argv"
        case env
    }
}

enum TaskersCoreBridgeError: LocalizedError {
    case createFailed(String)
    case callFailed(String)
    case invalidResponse(String)

    var errorDescription: String? {
        switch self {
        case .createFailed(let message):
            return message
        case .callFailed(let message):
            return message
        case .invalidResponse(let message):
            return message
        }
    }
}

final class TaskersCoreBridge {
    private let handle: OpaquePointer
    private let decoder = JSONDecoder()
    private let encoder = JSONEncoder()

    init(options: TaskersCoreOptions) throws {
        let encoded = try encoder.encode(options)
        guard
            let json = String(data: encoded, encoding: .utf8),
            let created = json.withCString({ taskers_macos_core_new_with_options_json($0) })
        else {
            throw TaskersCoreBridgeError.createFailed(Self.lastError())
        }

        handle = created
    }

    deinit {
        taskers_macos_core_free(handle)
    }

    var revision: UInt64 {
        taskers_macos_core_revision(handle)
    }

    func snapshot() throws -> TaskersSnapshot {
        let json = try callString {
            taskers_macos_core_snapshot_json(handle)
        }
        return try TaskersSnapshot.parse(data: Data(json.utf8))
    }

    @discardableResult
    func dispatch(command: [String: Any]) throws -> [String: Any] {
        let data = try JSONSerialization.data(withJSONObject: command, options: [.sortedKeys])
        guard let json = String(data: data, encoding: .utf8) else {
            throw TaskersCoreBridgeError.invalidResponse("failed to encode command JSON")
        }

        let response = try callString {
            json.withCString { taskers_macos_core_dispatch_json(handle, $0) }
        }

        guard
            let value = try JSONSerialization.jsonObject(with: Data(response.utf8)) as? [String: Any]
        else {
            throw TaskersCoreBridgeError.invalidResponse("failed to decode dispatch response")
        }

        return value
    }

    func surfaceDescriptor(workspaceId: String, paneId: String) throws -> TaskersSurfaceDescriptor {
        let json = try callString {
            workspaceId.withCString { workspacePtr in
                paneId.withCString { panePtr in
                    taskers_macos_core_surface_descriptor_json(handle, workspacePtr, panePtr)
                }
            }
        }

        return try decode(TaskersSurfaceDescriptor.self, from: json)
    }

    func splitPane(workspaceId: String, paneId: String, axis: String) throws -> String {
        let response = try dispatch(command: [
            "command": "split_pane",
            "workspace_id": workspaceId,
            "pane_id": paneId,
            "axis": axis
        ])
        return try requiredString("pane_id", in: response, context: "split pane")
    }

    func createSurface(
        workspaceId: String,
        paneId: String,
        kind: TaskersSurfaceKind
    ) throws -> String {
        let response = try dispatch(command: [
            "command": "create_surface",
            "workspace_id": workspaceId,
            "pane_id": paneId,
            "kind": kind.rawValue
        ])
        return try requiredString("surface_id", in: response, context: "create surface")
    }

    func closeSurface(workspaceID: String, paneID: String, surfaceID: String) throws {
        _ = try dispatch(command: [
            "command": "close_surface",
            "workspace_id": workspaceID,
            "pane_id": paneID,
            "surface_id": surfaceID
        ])
    }

    func updateSurfaceMetadata(surfaceId: String, title: String? = nil, url: String? = nil) throws {
        var patch: [String: Any] = [:]
        if let title {
            patch["title"] = title
        }
        if let url {
            patch["url"] = url
        }
        guard !patch.isEmpty else {
            return
        }

        _ = try dispatch(command: [
            "command": "update_surface_metadata",
            "surface_id": surfaceId,
            "patch": patch
        ])
    }

    private func decode<T: Decodable>(_ type: T.Type, from string: String) throws -> T {
        do {
            return try decoder.decode(T.self, from: Data(string.utf8))
        } catch {
            throw TaskersCoreBridgeError.invalidResponse(error.localizedDescription)
        }
    }

    private func requiredString(
        _ key: String,
        in payload: [String: Any],
        context: String
    ) throws -> String {
        guard let value = payload[key] as? String else {
            throw TaskersCoreBridgeError.invalidResponse("\(context) response missing \(key)")
        }
        return value
    }

    private func callString(_ body: () -> UnsafeMutablePointer<CChar>?) throws -> String {
        guard let pointer = body() else {
            throw TaskersCoreBridgeError.callFailed(Self.lastError())
        }

        defer {
            taskers_macos_string_free(pointer)
        }

        return String(cString: pointer)
    }

    private static func lastError() -> String {
        guard let pointer = taskers_macos_last_error_message() else {
            return "unknown error"
        }

        defer {
            taskers_macos_string_free(pointer)
        }

        let message = String(cString: pointer)
        return message.isEmpty ? "unknown error" : message
    }
}
