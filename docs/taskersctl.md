# Taskersctl Operator Guide

`taskersctl` is the local control CLI for Taskers. It is the shortest path for:

- inspecting the current workspace state
- creating notifications and agent status
- driving embedded browsers
- identifying the current runtime target
- reading terminal debug state

## Target Resolution

When you run `taskersctl` inside a Taskers terminal, it usually infers the current target from the exported `TASKERS_*` environment.

That means commands like these usually work without extra flags from an embedded terminal:

```bash
taskersctl identify
taskersctl notify --title "Hello" --body "Current pane"
taskersctl browser snapshot
taskersctl debug terminal read-text --tail-lines 40
```

When you run `taskersctl` outside Taskers, pass explicit ids such as `--workspace`, `--pane`, or `--surface`.

To discover ids:

```bash
taskersctl query tree
taskersctl query status
```

## Command Families

Top-level command groups:

- `query`: inspect workspaces, agents, notifications, and the full tree
- `notify`: create a notification directly
- `agent`: manage workspace status, progress, logs, notifications, flash, and unread navigation
- `agent-hook`: hook-oriented lifecycle commands for external tools
- `browser`: inspect and automate embedded browser surfaces
- `completion`: generate shell completion scripts for bash, zsh, or fish
- `identify`: print the current resolved workspace, pane, and surface context
- `debug`: inspect terminal focus, text, and render stats
- `workspace`, `pane`, `surface`: create and manipulate Taskers structure directly

## Common Recipes

### Inspect The Current Context

```bash
taskersctl identify
taskersctl query status
taskersctl query notifications
```

Use this first when you want to understand where a script or agent is currently operating.

### Create User-Facing Notifications

```bash
taskersctl notify --title "Build Complete" --subtitle "Project X" --body "All tests passed"
taskersctl agent focus-unread
```

For more lifecycle detail, see [Notifications and attention](notifications.md).

### Publish Agent State

```bash
taskersctl agent status set --text "Running sync"
taskersctl agent progress set --value 650 --label "65%"
taskersctl agent log append --source codex --message "Applied patch"
taskersctl agent notify create --title "Codex" --message "Need review"
```

Clear paths:

```bash
taskersctl agent status clear
taskersctl agent progress clear
taskersctl agent log clear
taskersctl agent notify clear
```

### Drive A Browser Surface

Open a browser in a known pane:

```bash
taskersctl surface new --workspace <workspace-id> --pane <pane-id> --kind browser --url https://duckduckgo.com/
taskersctl browser open --workspace <workspace-id> --pane <pane-id> --url https://duckduckgo.com/ --ephemeral
```

Inspect the active browser from a Taskers terminal:

```bash
taskersctl browser snapshot
taskersctl browser get title
taskersctl browser wait --text DuckDuckGo
```

Interact with DOM targets:

```bash
taskersctl browser click --selector 'a[href]'
taskersctl browser click --ref @e1
taskersctl browser screenshot
taskersctl browser clear-data --origin-filter duckduckgo.com
```

Use `--selector` when you already know a CSS target. Use `--ref` after a `snapshot` when you want to act on the exact node Taskers returned.

### Read Terminal State

```bash
taskersctl debug terminal is-focused
taskersctl debug terminal read-text --tail-lines 40
taskersctl debug terminal render-stats
```

This is useful for agent integrations, test harnesses, and debugging embedded terminal behavior.

### Create And Reshape Structure

Create a workspace:

```bash
taskersctl workspace new --label "Workspace 2"
```

Split a pane in a specific workspace:

```bash
taskersctl pane split --workspace <workspace-id> --pane <pane-id> --axis vertical --kind terminal
```

Create a new browser surface in a pane:

```bash
taskersctl surface new --workspace <workspace-id> --pane <pane-id> --kind browser --url https://duckduckgo.com/
taskersctl surface new --workspace <workspace-id> --pane <pane-id> --kind browser --url https://example.com/ --ephemeral
```

## Hook And Script Integration

If your tool already emits lifecycle events, use `agent-hook` instead of rebuilding the state model yourself:

```bash
taskersctl agent-hook waiting --title "Codex" --message "Need review"
taskersctl agent-hook stop --title "Codex" --message "Turn complete"
taskersctl agent-hook stop --message "Finished"
```

Use `notification` for contextual agent output that should update the live pane/workspace summary without finalizing the run. Use `stop` for the final successful completion edge; non-zero shell exits still surface as error stops automatically. This is the best fit for wrappers around coding agents, CI helpers, and long-running scripts.

## Shell Completions

Generate completions directly from the live CLI parser:

```bash
taskersctl completion bash
taskersctl completion zsh
taskersctl completion fish
```

Install them in the usual shell-specific locations:

```bash
mkdir -p ~/.local/share/bash-completion/completions
taskersctl completion bash > ~/.local/share/bash-completion/completions/taskersctl

mkdir -p ~/.zsh/completions
taskersctl completion zsh > ~/.zsh/completions/_taskersctl

mkdir -p ~/.config/fish/completions
taskersctl completion fish > ~/.config/fish/completions/taskersctl.fish
```

## Advanced Notes

- Use `--socket` if you need to target a non-default Taskers control socket.
- `browser` commands are strict about selectors and refs. Resolve a target with `snapshot` first when in doubt.
- The browser automation surface is intentionally broader than the rest of the CLI. Start with `snapshot`, `get`, `wait`, and `click`, then expand only as needed.
- Browser creation commands accept `--ephemeral` for a private in-memory session that does not reuse the shared persistent browser profile.
- `browser clear-data` removes WebKit site data for the selected browser session. Use `--origin-filter` to target one site when you do not want to clear the whole session.
