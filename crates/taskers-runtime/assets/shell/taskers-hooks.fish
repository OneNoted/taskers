set -q TASKERS_HOOKS_FISH_LOADED; and return 0
set -gx TASKERS_HOOKS_FISH_LOADED 1

function taskers__repo_root
    type -q git; or return 0
    git -C "$PWD" rev-parse --show-toplevel 2>/dev/null; or true
end

function taskers__repo_branch
    type -q git; or return 0
    git -C "$PWD" symbolic-ref --quiet --short HEAD 2>/dev/null
    or git -C "$PWD" rev-parse --short HEAD 2>/dev/null
    or true
end

function taskers__classify_token --argument token
    switch "$token"
        case codex
            printf '%s' codex
        case claude claude-code
            printf '%s' claude
        case opencode
            printf '%s' opencode
        case aider
            printf '%s' aider
        case '*'
            return 1
    end
end

function taskers__classify_command --argument commandline
    set -l words (string split ' ' -- "$commandline")

    while test (count $words) -gt 0
        set -l head $words[1]
        switch "$head"
            case '*=*' env
                set words $words[2..-1]
            case npx pnpx bunx uvx
                test (count $words) -gt 1; or return 1
                taskers__classify_token "$words[2]"
                return $status
            case pnpm yarn
                if test (count $words) -gt 2; and test "$words[2]" = dlx
                    taskers__classify_token "$words[3]"
                    return $status
                end
                return 1
            case '*'
                taskers__classify_token "$head"
                return $status
        end
    end

    return 1
end

function taskers__collect_metadata
    set -g TASKERS_META_CWD "$PWD"
    set -g TASKERS_META_REPO_ROOT (taskers__repo_root)
    if test -n "$TASKERS_META_REPO_ROOT"
        set -g TASKERS_META_REPO_NAME (path basename "$TASKERS_META_REPO_ROOT")
        test -n "$TASKERS_META_REPO_NAME"; or set -g TASKERS_META_REPO_NAME /
        set -g TASKERS_META_BRANCH (taskers__repo_branch)
    else
        set -g TASKERS_META_REPO_NAME
        set -g TASKERS_META_BRANCH
    end

    if set -q TASKERS_ACTIVE_AGENT_KIND
        set -g TASKERS_META_AGENT "$TASKERS_ACTIVE_AGENT_KIND"
    else if set -q TASKERS_PANE_AGENT_KIND
        set -g TASKERS_META_AGENT "$TASKERS_PANE_AGENT_KIND"
    else
        set -g TASKERS_META_AGENT shell
    end

    set -g TASKERS_META_LABEL "$TASKERS_META_REPO_NAME"
    if test -z "$TASKERS_META_LABEL"
        set -g TASKERS_META_LABEL (path basename "$PWD")
        test -n "$TASKERS_META_LABEL"; or set -g TASKERS_META_LABEL /
    end

    if test "$TASKERS_META_AGENT" = shell
        set -g TASKERS_META_TITLE "$TASKERS_META_LABEL"
    else
        set -g TASKERS_META_TITLE "$TASKERS_META_AGENT :: $TASKERS_META_LABEL"
    end
end

function taskers__agent_active_for_kind --argument kind
    switch "$kind"
        case started progress waiting_input
            printf '%s' 1
        case completed error
            printf '%s' 0
        case '*'
            if set -q TASKERS_ACTIVE_AGENT_KIND
                printf '%s' 1
            else
                printf '%s' 0
            end
    end
end

function taskers__context_tty_matches
    set -q TASKERS_TTY_NAME; or return 1
    set -l current_tty (tty 2>/dev/null)
    string match -qr '^/dev/' -- "$current_tty"; or return 1
    test "$current_tty" = "$TASKERS_TTY_NAME"
end

function taskers__emit_with_metadata --argument kind message
    taskers__collect_metadata
    set -l agent_active (taskers__agent_active_for_kind "$kind")
    test -x "$TASKERS_CTL_PATH"; or return 0
    test -n "$TASKERS_WORKSPACE_ID"; or return 0
    test -n "$TASKERS_PANE_ID"; or return 0
    test -n "$TASKERS_SURFACE_ID"; or return 0
    taskers__context_tty_matches; or return 0

    set -l argv \
        "$TASKERS_CTL_PATH" \
        signal \
        --source shell \
        --kind "$kind" \
        --title "$TASKERS_META_TITLE" \
        --cwd "$TASKERS_META_CWD" \
        --agent "$TASKERS_META_AGENT" \
        --agent-active "$agent_active"

    test -n "$TASKERS_META_REPO_NAME"; and set -a argv --repo "$TASKERS_META_REPO_NAME"
    test -n "$TASKERS_META_BRANCH"; and set -a argv --branch "$TASKERS_META_BRANCH"
    test -n "$message"; and set -a argv --message "$message"

    $argv >/dev/null 2>&1 &
end

function taskers__emit_metadata_if_changed
    taskers__collect_metadata
    set -l agent_active (taskers__agent_active_for_kind metadata)

    if test "$TASKERS_LAST_META_CWD" = "$TASKERS_META_CWD" \
        -a "$TASKERS_LAST_META_REPO_NAME" = "$TASKERS_META_REPO_NAME" \
        -a "$TASKERS_LAST_META_BRANCH" = "$TASKERS_META_BRANCH" \
        -a "$TASKERS_LAST_META_AGENT" = "$TASKERS_META_AGENT" \
        -a "$TASKERS_LAST_META_TITLE" = "$TASKERS_META_TITLE" \
        -a "$TASKERS_LAST_META_AGENT_ACTIVE" = "$agent_active"
        return 0
    end

    set -gx TASKERS_LAST_META_CWD "$TASKERS_META_CWD"
    set -gx TASKERS_LAST_META_REPO_NAME "$TASKERS_META_REPO_NAME"
    set -gx TASKERS_LAST_META_BRANCH "$TASKERS_META_BRANCH"
    set -gx TASKERS_LAST_META_AGENT "$TASKERS_META_AGENT"
    set -gx TASKERS_LAST_META_TITLE "$TASKERS_META_TITLE"
    set -gx TASKERS_LAST_META_AGENT_ACTIVE "$agent_active"
    taskers__emit_with_metadata metadata
end

function taskers__on_preexec --on-event fish_preexec
    set -l agent (taskers__classify_command "$argv[1]" 2>/dev/null)
    if test -n "$agent"
        set -gx TASKERS_PANE_AGENT_KIND "$agent"
        set -gx TASKERS_ACTIVE_AGENT_KIND "$agent"
        taskers__emit_with_metadata started
    end
end

function taskers__on_postexec --on-event fish_postexec
    set -l exit_status $status
    if set -q TASKERS_ACTIVE_AGENT_KIND
        if test "$exit_status" -eq 0
            taskers__emit_with_metadata completed
        else
            taskers__emit_with_metadata error "Exited with status $exit_status"
        end
        set -e TASKERS_ACTIVE_AGENT_KIND
    end

    taskers__emit_metadata_if_changed
end

function taskers__on_prompt --on-event fish_prompt
    taskers__emit_metadata_if_changed
end

function taskers__on_pwd_change --on-variable PWD
    taskers__emit_metadata_if_changed
end

function taskers_signal
    test (count $argv) -gt 0; or return 1
    set -l kind $argv[1]
    set -e argv[1]
    taskers__emit_with_metadata "$kind" (string join ' ' -- $argv)
end

function taskers_waiting
    taskers_signal waiting_input $argv
end

function taskers_done
    taskers_signal completed $argv
end

function taskers_error
    taskers_signal error $argv
end

set -gx TASKERS_LAST_META_CWD ''
set -gx TASKERS_LAST_META_REPO_NAME ''
set -gx TASKERS_LAST_META_BRANCH ''
set -gx TASKERS_LAST_META_AGENT ''
set -gx TASKERS_LAST_META_TITLE ''
set -gx TASKERS_LAST_META_AGENT_ACTIVE ''
if not set -q TASKERS_TTY_NAME
    set -l current_tty (tty 2>/dev/null)
    if string match -qr '^/dev/' -- "$current_tty"
        set -gx TASKERS_TTY_NAME "$current_tty"
    end
end
taskers__emit_metadata_if_changed
