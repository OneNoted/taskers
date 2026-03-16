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

struct TaskersSurfaceDescriptor: Decodable {
    let cols: UInt16
    let rows: UInt16
    let cwd: String?
    let title: String?
    let commandArgv: [String]
    let env: [String: String]

    enum CodingKeys: String, CodingKey {
        case cols
        case rows
        case cwd
        case title
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
        return try decode(TaskersSnapshot.self, from: json)
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

    private func decode<T: Decodable>(_ type: T.Type, from string: String) throws -> T {
        do {
            return try decoder.decode(T.self, from: Data(string.utf8))
        } catch {
            throw TaskersCoreBridgeError.invalidResponse(error.localizedDescription)
        }
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
