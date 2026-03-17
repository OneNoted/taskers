import Foundation

struct OrderedMap<Value: Decodable>: Decodable {
    let elements: [(String, Value)]
    private let storage: [String: Value]

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: DynamicCodingKey.self)
        var elements: [(String, Value)] = []
        var storage: [String: Value] = [:]
        storage.reserveCapacity(container.allKeys.count)

        for key in container.allKeys {
            let value = try container.decode(Value.self, forKey: key)
            elements.append((key.stringValue, value))
            storage[key.stringValue] = value
        }

        self.elements = elements
        self.storage = storage
    }

    subscript(key: String) -> Value? {
        storage[key]
    }

    struct DynamicCodingKey: CodingKey {
        var stringValue: String
        var intValue: Int?

        init?(stringValue: String) {
            self.stringValue = stringValue
        }

        init?(intValue: Int) {
            self.stringValue = String(intValue)
            self.intValue = intValue
        }
    }
}

struct TaskersSnapshot: Decodable {
    let activeWindow: String
    let windows: OrderedMap<TaskersWindowRecord>
    let workspaces: OrderedMap<TaskersWorkspace>

    enum CodingKeys: String, CodingKey {
        case activeWindow = "active_window"
        case windows
        case workspaces
    }

    var activeWorkspace: TaskersWorkspace? {
        guard let window = windows[activeWindow] else {
            return nil
        }

        return workspaces[window.activeWorkspace]
    }

    var liveSurfaceIDs: Set<String> {
        var surfaceIDs: Set<String> = []
        for (_, workspace) in workspaces.elements {
            for (_, pane) in workspace.panes.elements {
                surfaceIDs.insert(pane.activeSurface)
            }
        }
        return surfaceIDs
    }
}

struct TaskersWindowRecord: Decodable {
    let id: String
    let workspaceOrder: [String]
    let activeWorkspace: String

    enum CodingKeys: String, CodingKey {
        case id
        case workspaceOrder = "workspace_order"
        case activeWorkspace = "active_workspace"
    }
}

struct TaskersWorkspace: Decodable {
    let id: String
    let label: String
    let columns: OrderedMap<TaskersWorkspaceColumn>
    let windows: OrderedMap<TaskersWorkspaceWindow>
    let activeWindow: String
    let panes: OrderedMap<TaskersPane>
    let activePane: String

    enum CodingKeys: String, CodingKey {
        case id
        case label
        case columns
        case windows
        case activeWindow = "active_window"
        case panes
        case activePane = "active_pane"
    }
}

struct TaskersWorkspaceColumn: Decodable {
    let id: String
    let width: Int
    let windowOrder: [String]
    let activeWindow: String

    enum CodingKeys: String, CodingKey {
        case id
        case width
        case windowOrder = "window_order"
        case activeWindow = "active_window"
    }
}

struct TaskersWorkspaceWindow: Decodable {
    let id: String
    let height: Int
    let layout: TaskersLayoutNode
    let activePane: String

    enum CodingKeys: String, CodingKey {
        case id
        case height
        case layout
        case activePane = "active_pane"
    }
}

struct TaskersPane: Decodable {
    let id: String
    let surfaces: OrderedMap<TaskersSurface>
    let activeSurface: String

    enum CodingKeys: String, CodingKey {
        case id
        case surfaces
        case activeSurface = "active_surface"
    }
}

struct TaskersSurface: Decodable {
    let id: String
    let metadata: TaskersSurfaceMetadata
}

struct TaskersSurfaceMetadata: Decodable {
    let title: String?
    let cwd: String?
}

enum TaskersSplitAxis: String, Decodable {
    case horizontal
    case vertical
}

indirect enum TaskersLayoutNode: Decodable {
    case leaf(paneID: String)
    case split(axis: TaskersSplitAxis, ratio: UInt16, first: TaskersLayoutNode, second: TaskersLayoutNode)

    enum CodingKeys: String, CodingKey {
        case kind
        case paneID = "pane_id"
        case axis
        case ratio
        case first
        case second
    }

    enum NodeKind: String, Decodable {
        case leaf
        case split
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        switch try container.decode(NodeKind.self, forKey: .kind) {
        case .leaf:
            self = .leaf(paneID: try container.decode(String.self, forKey: .paneID))
        case .split:
            self = .split(
                axis: try container.decode(TaskersSplitAxis.self, forKey: .axis),
                ratio: try container.decode(UInt16.self, forKey: .ratio),
                first: try container.decode(TaskersLayoutNode.self, forKey: .first),
                second: try container.decode(TaskersLayoutNode.self, forKey: .second)
            )
        }
    }
}
