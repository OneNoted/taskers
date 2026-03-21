# Notifications And Attention

Taskers supports both in-app attention and desktop notifications. This page describes what creates notifications, how unread state works, and which integration paths are available to agents and shell scripts.

## How Notification State Works

A Taskers notification moves through three distinct states:

- Unread: the item exists and has not been opened yet.
- Read: you opened or jumped to the target, but the item is still kept in history.
- Cleared: you explicitly dismissed it, so it no longer appears in active attention.

Opening a notification does not clear it. It only marks it read and focuses the target.

Desktop delivery is separate:

- Each notification is delivered to the desktop at most once.
- If desktop suppression is enabled and the target is already visible, Taskers marks the notification as processed without showing a banner.

## Where Notifications Appear

Notifications surface in three places:

- The sidebar shows unread counts and a short preview for each workspace.
- The attention rail shows active unread items, workspace status, progress, and recent log entries.
- Desktop banners appear through the GTK app when notification delivery is enabled.

## Notification Settings

Taskers exposes notification settings in the in-app Settings page.

Current toggles:

- Alert on waiting
- Alert on errors
- Alert on completion
- Suppress when visible

Default behavior:

- Waiting, error, and completion alerts are enabled
- Desktop banners are suppressed when the current app view is already showing the exact target

## Fastest Manual Path

From a Taskers terminal:

```bash
taskersctl notify --title "Build Complete" --subtitle "Project X" --body "All tests passed"
```

That creates a notification targeted at the current surface. Because the terminal already carries `TASKERS_*` ids, no extra target flags are usually needed.

Outside Taskers, pass explicit ids:

```bash
taskersctl notify \
  --workspace <workspace-id> \
  --pane <pane-id> \
  --surface <surface-id> \
  --title "Build Complete" \
  --body "All tests passed"
```

## Agent-Facing Paths

For richer workspace and agent state, use the `agent` command family:

```bash
taskersctl agent status set --text "Running sync"
taskersctl agent progress set --value 650 --label "65%"
taskersctl agent log append --source codex --message "Applied patch"
taskersctl agent notify create --title "Codex" --message "Need review"
taskersctl agent focus-unread
```

Practical split:

- `status` is the live one-line workspace summary
- `progress` is the live progress indicator
- `log append` is non-unread history
- `notify create` is an attention item
- `focus-unread` jumps to the newest unread notification

## Shell Helpers

The shipped shell integration exposes three helpers in supported shells:

```bash
taskers_waiting "Need review"
taskers_done "Finished"
taskers_error "Build failed"
```

Those helpers emit Taskers signals from the current terminal. They are the easiest path for shell scripts and ad hoc workflows.

## Agent Hook Integration

For tool or wrapper integrations, Taskers also supports explicit hook-style commands:

```bash
taskersctl agent-hook waiting --message "Need review"
taskersctl agent-hook notification --title "Codex" --message "Turn complete"
taskersctl agent-hook stop --message "Finished"
```

Use this path when you already have structured lifecycle hooks and want to translate them into Taskers attention items directly.

## CMUX-Compatible Terminal Escapes

Taskers understands both the Taskers-native signal frames and the CMUX-compatible notification sequences.

Simple `OSC 777` notification:

```bash
printf '\e]777;notify;OSC777 Title;OSC777 Body\a'
```

Rich `OSC 99` notification with an id, subtitle, and final body chunk:

```bash
printf '\e]99;i=build:d=0:p=title;Build Complete\e\\'
printf '\e]99;i=build:d=0:p=subtitle;Project X\e\\'
printf '\e]99;i=build:d=1:p=body;All tests passed\e\\'
```

Simple `OSC 99` payload:

```bash
printf '\e]99;;Hello World\e\\'
```

Use `OSC 777` when you only need a title and body. Use `OSC 99` when you need a notification id or subtitle.

## Verifying The Flow

From a Taskers terminal, this is a good end-to-end check:

```bash
taskersctl agent status set --text "Running sync"
taskersctl notify --title "Taskers" --body "Hello from the current pane"
taskersctl query notifications
taskersctl agent focus-unread
```

Expected behavior:

- the workspace row shows an unread badge
- the attention rail shows the notification
- a desktop banner appears unless the target is already visible and suppression is enabled
- focusing the unread item marks it read but leaves it in history until you clear it
