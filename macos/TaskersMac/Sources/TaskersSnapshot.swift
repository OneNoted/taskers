import Foundation

protocol TaskersIdentifiedRecord {
    var id: String { get }
}

struct OrderedMap<Value: TaskersIdentifiedRecord> {
    let elements: [(String, Value)]
    private let storage: [String: Value]

    init(values: [Value]) {
        self.elements = values.map { ($0.id, $0) }
        self.storage = Dictionary(uniqueKeysWithValues: elements)
    }

    subscript(key: String) -> Value? {
        storage[key]
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

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        activeWindow = try container.decode(String.self, forKey: .activeWindow)
        windows = OrderedMap(values: try container.decode([TaskersWindowRecord].self, forKey: .windows))
        workspaces = OrderedMap(values: try container.decode([TaskersWorkspace].self, forKey: .workspaces))
    }

    static func parse(data: Data) throws -> TaskersSnapshot {
        try JSONDecoder().decode(Self.self, from: data)
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

struct TaskersWindowRecord: Decodable, TaskersIdentifiedRecord {
    let id: String
    let workspaceOrder: [String]
    let activeWorkspace: String

    enum CodingKeys: String, CodingKey {
        case id
        case workspaceOrder = "workspace_order"
        case activeWorkspace = "active_workspace"
    }
}

struct TaskersWorkspace: Decodable, TaskersIdentifiedRecord {
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

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        id = try container.decode(String.self, forKey: .id)
        label = try container.decode(String.self, forKey: .label)
        columns = OrderedMap(values: try container.decode([TaskersWorkspaceColumn].self, forKey: .columns))
        windows = OrderedMap(values: try container.decode([TaskersWorkspaceWindow].self, forKey: .windows))
        activeWindow = try container.decode(String.self, forKey: .activeWindow)
        panes = OrderedMap(values: try container.decode([TaskersPane].self, forKey: .panes))
        activePane = try container.decode(String.self, forKey: .activePane)
    }
}

struct TaskersWorkspaceColumn: Decodable, TaskersIdentifiedRecord {
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

struct TaskersWorkspaceWindow: Decodable, TaskersIdentifiedRecord {
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

struct TaskersPane: Decodable, TaskersIdentifiedRecord {
    let id: String
    let surfaces: OrderedMap<TaskersSurface>
    let activeSurface: String

    enum CodingKeys: String, CodingKey {
        case id
        case surfaces
        case activeSurface = "active_surface"
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        id = try container.decode(String.self, forKey: .id)
        surfaces = OrderedMap(values: try container.decode([TaskersSurface].self, forKey: .surfaces))
        activeSurface = try container.decode(String.self, forKey: .activeSurface)
    }
}

struct TaskersSurface: Decodable, TaskersIdentifiedRecord {
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

    enum Kind: String, Decodable {
        case leaf
        case split
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        switch try container.decode(Kind.self, forKey: .kind) {
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
