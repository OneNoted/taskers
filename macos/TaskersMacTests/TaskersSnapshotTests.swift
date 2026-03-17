import XCTest
@testable import TaskersMac

final class TaskersSnapshotTests: XCTestCase {
    func testOrderedMapsPreserveJSONKeyOrder() throws {
        let json = """
        {
          "active_window": "window-a",
          "windows": [
            {
              "id": "window-a",
              "workspace_order": ["workspace-a"],
              "active_workspace": "workspace-a"
            }
          ],
          "workspaces": [
            {
              "id": "workspace-a",
              "label": "Main",
              "columns": [
                {
                  "id": "column-a",
                  "width": 520,
                  "window_order": ["window-1"],
                  "active_window": "window-1"
                },
                {
                  "id": "column-b",
                  "width": 360,
                  "window_order": ["window-2"],
                  "active_window": "window-2"
                }
              ],
              "windows": [
                {
                  "id": "window-1",
                  "height": 400,
                  "layout": { "kind": "leaf", "pane_id": "pane-1" },
                  "active_pane": "pane-1"
                },
                {
                  "id": "window-2",
                  "height": 400,
                  "layout": { "kind": "leaf", "pane_id": "pane-2" },
                  "active_pane": "pane-2"
                }
              ],
              "active_window": "window-1",
              "panes": [
                {
                  "id": "pane-1",
                  "surfaces": [
                    {
                      "id": "surface-1",
                      "metadata": { "title": "One", "cwd": null }
                    }
                  ],
                  "active_surface": "surface-1"
                },
                {
                  "id": "pane-2",
                  "surfaces": [
                    {
                      "id": "surface-2",
                      "metadata": { "title": "Two", "cwd": null }
                    }
                  ],
                  "active_surface": "surface-2"
                }
              ],
              "active_pane": "pane-1"
            }
          ]
        }
        """

        let snapshot = try TaskersSnapshot.parse(data: Data(json.utf8))
        let columns = snapshot.activeWorkspace?.columns.elements.map(\.0)
        XCTAssertEqual(columns, ["column-a", "column-b"])
        XCTAssertEqual(snapshot.liveSurfaceIDs, Set(["surface-1", "surface-2"]))
    }
}
