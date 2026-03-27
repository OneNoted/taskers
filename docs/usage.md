# Daily Usage

This guide is the quickest way to get oriented in the active Taskers app.

## Mental Model

Taskers has four layers of layout:

- A workspace contains top-level workspace windows.
- A workspace window contains window tabs.
- A window tab contains panes.
- A pane contains surface tabs.

That distinction matters:

- Workspace navigation and panning operate on top-level windows.
- Window tabs stay local to the current workspace window until you merge or extract them.
- Pane splits stay local to the current window tab.
- Surface tabs stay local to the current pane until you move them.

If something feels like “Niri behavior,” it should usually happen at the workspace-window layer, not at the pane or tab layer.

## Core Surfaces

Taskers currently ships two live surface kinds:

- Terminal surfaces backed by embedded Ghostty
- Browser surfaces backed by embedded WebKit

Each pane can hold one or more surface tabs of either kind. The active surface tab supplies the live content for that pane.

## Embedded Ghostty Config

On the GTK/Linux shell, embedded terminal panes start from the user's normal Ghostty config file and inherit most visual and terminal behavior settings from there.

Taskers still pins a small set of embedded-pane invariants:

- the launch command stays Taskers-owned
- Ghostty shell integration stays disabled because Taskers provides its own shell wrapper
- Ghostty Linux cgroup settings stay disabled for embedded panes

That means Ghostty settings such as fonts, theme, colors, cursor behavior, scrollback, and window padding should carry over, while shell-launch behavior remains controlled by Taskers.

## A Typical Session

Start Taskers:

```bash
taskers
```

Build out the workspace:

- Create or switch to a workspace from the sidebar.
- Use the pane controls to add a new terminal tab or browser tab.
- Use the window top bar to add or rearrange whole window tabs when you want another local pane layout in the same top-level window.
- Split the active pane when you want another local work area inside the current workspace window.
- Create or move top-level workspace windows when you want side-by-side tiled regions.

The important boundary is:

- New split inside the current window: pane operation
- New tab inside the current top-level window: window-tab operation
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

If you are testing repo-local changes from your desktop environment, install the app into Cargo's bin directory and repoint the launcher there:

```bash
bash scripts/install-dev-desktop-entry.sh
```

That reinstalls the local app into Cargo's install root and writes a desktop entry that launches that installed binary directly.
