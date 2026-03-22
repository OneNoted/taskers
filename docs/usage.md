# Daily Usage

This guide is the quickest way to get oriented in the active Taskers app.

## Mental Model

Taskers has three layers of layout:

- A workspace contains top-level workspace windows.
- A workspace window contains panes.
- A pane contains tabs.

That distinction matters:

- Workspace navigation and panning operate on top-level windows.
- Pane splits stay local to the current workspace window.
- Tabs stay local to the current pane until you move them.

If something feels like “Niri behavior,” it should usually happen at the workspace-window layer, not at the pane or tab layer.

## Core Surfaces

Taskers currently ships two live surface kinds:

- Terminal surfaces backed by embedded Ghostty
- Browser surfaces backed by embedded WebKit

Each pane can hold one or more tabs of either kind. The active tab supplies the live content for that pane.

## A Typical Session

Start Taskers:

```bash
taskers
```

Build out the workspace:

- Create or switch to a workspace from the sidebar.
- Use the pane controls to add a new terminal tab or browser tab.
- Split the active pane when you want another local work area inside the current workspace window.
- Create or move top-level workspace windows when you want side-by-side tiled regions.

The important boundary is:

- New split inside the current window: pane operation
- New top-level tile in the scrolling workspace: workspace-window operation

## Working Inside A Taskers Terminal

Taskers terminals export runtime context for the current pane and surface. That means `taskersctl` can usually infer the current target without extra flags when you run it inside an embedded terminal.

For example:

```bash
taskersctl identify
taskersctl agent status set --text "Running sync"
taskersctl browser snapshot
```

When you run the same commands outside Taskers, pass explicit ids such as `--workspace`, `--pane`, or `--surface`.

## Attention And Status

Taskers keeps active agent state visible in three places:

- the workspace row in the sidebar
- the attention rail on the right
- the current pane or surface when a flash or unread item targets it

Workspace rows and pane/window chrome also carry runtime-aware icons, so you can tell at a glance whether a region is currently acting as a Codex, Claude, OpenCode, Aider, browser, or plain terminal surface.

Use that split intentionally:

- Status text is for “what this workspace is doing right now”
- Progress is for bounded ongoing work
- Notifications are for unread events that need follow-up
- Log entries are for recent history that should not become unread noise

For the full notification lifecycle, see [Notifications and attention](notifications.md).

## Browser And Terminal Introspection

The built-in CLI can inspect both live browser and terminal surfaces.

Browser examples:

```bash
taskersctl browser snapshot
taskersctl browser get title
taskersctl browser wait --text DuckDuckGo
```

Terminal examples:

```bash
taskersctl debug terminal is-focused
taskersctl debug terminal read-text --tail-lines 40
taskersctl debug terminal render-stats
```

## Desktop Launcher For Development

If you are testing repo-local changes from your desktop environment, repoint the launcher to the local checkout:

```bash
bash scripts/install-dev-desktop-entry.sh
```

That writes a dev desktop entry that launches `cargo run` against the repo root instead of a previously installed release bundle.
