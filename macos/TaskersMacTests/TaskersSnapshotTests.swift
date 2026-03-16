import XCTest
@testable import TaskersMac

final class TaskersSnapshotTests: XCTestCase {
    func testOrderedMapsPreserveJSONKeyOrder() throws {
        let json = """
        {
          "active_window": "window-a",
          "windows": {
            "window-a": {
              "id": "window-a",
              "workspace_order": ["workspace-a"],
              "active_workspace": "workspace-a"
            }
          },
          "workspaces": {
            "workspace-a": {
              "id": "workspace-a",
              "label": "Main",
              "columns": {
                "column-a": {
                  "id": "column-a",
                  "width": 520,
                  "window_order": ["window-1"],
                  "active_window": "window-1"
                },
                "column-b": {
                  "id": "column-b",
                  "width": 360,
                  "window_order": ["window-2"],
                  "active_window": "window-2"
                }
              },
              "windows": {
                "window-1": {
                  "id": "window-1",
                  "height": 400,
                  "layout": { "kind": "leaf", "pane_id": "pane-1" },
                  "active_pane": "pane-1"
                },
                "window-2": {
                  "id": "window-2",
                  "height": 400,
                  "layout": { "kind": "leaf", "pane_id": "pane-2" },
                  "active_pane": "pane-2"
                }
              },
              "active_window": "window-1",
              "panes": {
                "pane-1": {
                  "id": "pane-1",
                  "surfaces": {
                    "surface-1": {
                      "id": "surface-1",
                      "metadata": { "title": "One", "cwd": null }
                    }
                  },
                  "active_surface": "surface-1"
                },
                "pane-2": {
                  "id": "pane-2",
                  "surfaces": {
                    "surface-2": {
                      "id": "surface-2",
                      "metadata": { "title": "Two", "cwd": null }
                    }
                  },
                  "active_surface": "surface-2"
                }
              },
              "active_pane": "pane-1"
            }
          }
        }
        """

        let snapshot = try JSONDecoder().decode(TaskersSnapshot.self, from: Data(json.utf8))
        let columns = snapshot.activeWorkspace?.columns.elements.map(\.0)
        XCTAssertEqual(columns, ["column-a", "column-b"])
        XCTAssertEqual(snapshot.liveSurfaceIDs, Set(["surface-1", "surface-2"]))
    }
}
