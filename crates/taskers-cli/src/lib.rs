use std::{
    env,
    future::pending,
    io::{self, Write},
    path::PathBuf,
    process::Command as ProcessCommand,
};

use anyhow::{Context, anyhow, bail};
use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};
use taskers_control::{
    BrowserControlCommand, BrowserGetCommand, BrowserLoadState, BrowserPredicateCommand,
    BrowserTarget, BrowserWaitCondition, ControlClient, ControlCommand, ControlQuery,
    ControlResponse, InMemoryController, ScreenshotCommand, ScreenshotTarget, TerminalDebugCommand,
    bind_socket, default_socket_path, serve,
};
use taskers_domain::{
    AgentTarget, AppModel, AttentionState, BrowserProfileMode, Direction, KEYBOARD_RESIZE_STEP,
    PaneId, PaneKind, PaneMetadataPatch, ProgressState, SignalEvent, SignalKind, SplitAxis,
    SurfaceId, WorkspaceId, WorkspaceLogEntry,
};
use taskers_paths::default_terminal_socket_path;
use taskers_runtime::TerminalSessionClient;
use time::OffsetDateTime;

#[derive(Debug, Parser)]
#[command(name = "taskersctl")]
#[command(about = "Local control CLI for the taskers workspace app")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

fn parse_boolish(value: &str) -> Result<bool, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Ok(true),
        "false" | "0" | "no" | "off" => Ok(false),
        _ => Err(format!("invalid boolean value: {value}")),
    }
}

#[cfg(test)]
mod bool_parse_tests {
    use super::parse_boolish;

    #[test]
    fn parse_boolish_accepts_numeric_and_text_booleans() {
        assert_eq!(parse_boolish("1"), Ok(true));
        assert_eq!(parse_boolish("0"), Ok(false));
        assert_eq!(parse_boolish("true"), Ok(true));
        assert_eq!(parse_boolish("false"), Ok(false));
    }
}

#[derive(Debug, Subcommand)]
enum Command {
    Serve {
        #[arg(long)]
        socket: Option<PathBuf>,
        #[arg(long, default_value_t = true)]
        demo: bool,
    },
    Query {
        #[command(subcommand)]
        query: QueryCommand,
    },
    Signal {
        #[arg(long)]
        socket: Option<PathBuf>,
        #[arg(long)]
        workspace: Option<WorkspaceId>,
        #[arg(long)]
        pane: Option<PaneId>,
        #[arg(long)]
        surface: Option<SurfaceId>,
        #[arg(long)]
        kind: CliSignalKind,
        #[arg(long)]
        message: Option<String>,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        cwd: Option<String>,
        #[arg(long)]
        repo: Option<String>,
        #[arg(long)]
        branch: Option<String>,
        #[arg(long)]
        agent: Option<String>,
        #[arg(long, value_parser = parse_boolish)]
        agent_active: Option<bool>,
        #[arg(long)]
        command: Option<String>,
        #[arg(long, hide = true)]
        source: Option<String>,
    },
    Notify {
        #[arg(long)]
        socket: Option<PathBuf>,
        #[arg(long)]
        workspace: Option<WorkspaceId>,
        #[arg(long)]
        pane: Option<PaneId>,
        #[arg(long)]
        surface: Option<SurfaceId>,
        #[arg(long)]
        title: String,
        #[arg(long)]
        subtitle: Option<String>,
        #[arg(long)]
        body: Option<String>,
        #[arg(long = "notification-id")]
        notification_id: Option<String>,
        #[arg(long)]
        agent: Option<String>,
    },
    Agent {
        #[command(subcommand)]
        command: AgentCommand,
    },
    Workspace {
        #[command(subcommand)]
        command: WorkspaceCommand,
    },
    AgentHook {
        #[command(subcommand)]
        command: AgentHookCommand,
    },
    Browser {
        #[command(subcommand)]
        command: BrowserCommand,
    },
    Screenshot {
        #[command(flatten)]
        screenshot: ScreenshotArgs,
    },
    Completion {
        #[arg(value_enum)]
        shell: CompletionShell,
    },
    #[command(name = "completion-query", hide = true)]
    CompletionQuery {
        #[command(flatten)]
        query: CompletionQueryArgs,
    },
    Identify {
        #[arg(long)]
        socket: Option<PathBuf>,
        #[arg(long)]
        workspace: Option<WorkspaceId>,
        #[arg(long)]
        pane: Option<PaneId>,
        #[arg(long)]
        surface: Option<SurfaceId>,
    },
    Debug {
        #[command(subcommand)]
        command: DebugCommand,
    },
    Pane {
        #[command(subcommand)]
        command: PaneCommand,
    },
    Surface {
        #[command(subcommand)]
        command: SurfaceCommand,
    },
    #[command(hide = true)]
    Session {
        #[command(subcommand)]
        command: SessionCommand,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum CompletionShell {
    Bash,
    Fish,
    Zsh,
}

#[derive(Debug, Clone, Default, Args)]
struct CompletionQueryArgs {
    #[arg(long)]
    path: Option<String>,
    #[arg(long, allow_hyphen_values = true)]
    flag: Option<String>,
    #[arg(long)]
    positional: Option<usize>,
    #[arg(long)]
    socket: Option<PathBuf>,
    #[arg(long)]
    workspace: Option<WorkspaceId>,
    #[arg(long)]
    pane: Option<PaneId>,
    #[arg(long)]
    surface: Option<SurfaceId>,
}

#[derive(Debug, Subcommand)]
enum QueryCommand {
    Status {
        #[arg(long)]
        socket: Option<PathBuf>,
    },
    Agents {
        #[arg(long)]
        socket: Option<PathBuf>,
    },
    Notifications {
        #[arg(long)]
        socket: Option<PathBuf>,
    },
    Tree {
        #[arg(long)]
        socket: Option<PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
enum WorkspaceCommand {
    List {
        #[arg(long)]
        socket: Option<PathBuf>,
    },
    New {
        #[arg(long)]
        socket: Option<PathBuf>,
        #[arg(long)]
        label: String,
    },
    Switch {
        #[arg(long)]
        socket: Option<PathBuf>,
        #[arg(long)]
        workspace: WorkspaceId,
    },
    Rename {
        #[arg(long)]
        socket: Option<PathBuf>,
        #[arg(long)]
        workspace: WorkspaceId,
        #[arg(long)]
        label: String,
    },
    Close {
        #[arg(long)]
        socket: Option<PathBuf>,
        #[arg(long)]
        workspace: WorkspaceId,
    },
}

#[derive(Debug, Subcommand)]
enum AgentHookCommand {
    SessionStart {
        #[arg(long)]
        socket: Option<PathBuf>,
        #[arg(long)]
        workspace: Option<WorkspaceId>,
        #[arg(long)]
        pane: Option<PaneId>,
        #[arg(long)]
        surface: Option<SurfaceId>,
        #[arg(long)]
        agent: Option<String>,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        message: Option<String>,
    },
    Active {
        #[arg(long)]
        socket: Option<PathBuf>,
        #[arg(long)]
        workspace: Option<WorkspaceId>,
        #[arg(long)]
        pane: Option<PaneId>,
        #[arg(long)]
        surface: Option<SurfaceId>,
        #[arg(long)]
        agent: Option<String>,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        message: Option<String>,
    },
    Progress {
        #[arg(long)]
        socket: Option<PathBuf>,
        #[arg(long)]
        workspace: Option<WorkspaceId>,
        #[arg(long)]
        pane: Option<PaneId>,
        #[arg(long)]
        surface: Option<SurfaceId>,
        #[arg(long)]
        agent: Option<String>,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        message: Option<String>,
    },
    Waiting {
        #[arg(long)]
        socket: Option<PathBuf>,
        #[arg(long)]
        workspace: Option<WorkspaceId>,
        #[arg(long)]
        pane: Option<PaneId>,
        #[arg(long)]
        surface: Option<SurfaceId>,
        #[arg(long)]
        agent: Option<String>,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        message: Option<String>,
    },
    Notification {
        #[arg(long)]
        socket: Option<PathBuf>,
        #[arg(long)]
        workspace: Option<WorkspaceId>,
        #[arg(long)]
        pane: Option<PaneId>,
        #[arg(long)]
        surface: Option<SurfaceId>,
        #[arg(long)]
        agent: Option<String>,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        message: Option<String>,
    },
    Stop {
        #[arg(long)]
        socket: Option<PathBuf>,
        #[arg(long)]
        workspace: Option<WorkspaceId>,
        #[arg(long)]
        pane: Option<PaneId>,
        #[arg(long)]
        surface: Option<SurfaceId>,
        #[arg(long)]
        agent: Option<String>,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        message: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum AgentCommand {
    Status {
        #[command(subcommand)]
        command: AgentStatusCommand,
    },
    Progress {
        #[command(subcommand)]
        command: AgentProgressCommand,
    },
    Log {
        #[command(subcommand)]
        command: AgentLogCommand,
    },
    Notify {
        #[command(subcommand)]
        command: AgentNotifyCommand,
    },
    Flash {
        #[arg(long)]
        socket: Option<PathBuf>,
        #[arg(long)]
        workspace: Option<WorkspaceId>,
        #[arg(long)]
        pane: Option<PaneId>,
        #[arg(long)]
        surface: Option<SurfaceId>,
    },
    FocusUnread {
        #[arg(long)]
        socket: Option<PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
enum AgentStatusCommand {
    Set {
        #[arg(long)]
        socket: Option<PathBuf>,
        #[arg(long)]
        workspace: Option<WorkspaceId>,
        #[arg(long)]
        text: String,
    },
    Clear {
        #[arg(long)]
        socket: Option<PathBuf>,
        #[arg(long)]
        workspace: Option<WorkspaceId>,
    },
}

#[derive(Debug, Subcommand)]
enum AgentProgressCommand {
    Set {
        #[arg(long)]
        socket: Option<PathBuf>,
        #[arg(long)]
        workspace: Option<WorkspaceId>,
        #[arg(long)]
        value: u16,
        #[arg(long)]
        label: Option<String>,
    },
    Clear {
        #[arg(long)]
        socket: Option<PathBuf>,
        #[arg(long)]
        workspace: Option<WorkspaceId>,
    },
}

#[derive(Debug, Subcommand)]
enum AgentLogCommand {
    Append {
        #[arg(long)]
        socket: Option<PathBuf>,
        #[arg(long)]
        workspace: Option<WorkspaceId>,
        #[arg(long)]
        message: String,
        #[arg(long)]
        source: Option<String>,
    },
    List {
        #[arg(long)]
        socket: Option<PathBuf>,
        #[arg(long)]
        workspace: Option<WorkspaceId>,
    },
    Clear {
        #[arg(long)]
        socket: Option<PathBuf>,
        #[arg(long)]
        workspace: Option<WorkspaceId>,
    },
}

#[derive(Debug, Subcommand)]
enum AgentNotifyCommand {
    Create {
        #[arg(long)]
        socket: Option<PathBuf>,
        #[arg(long)]
        workspace: Option<WorkspaceId>,
        #[arg(long)]
        pane: Option<PaneId>,
        #[arg(long)]
        surface: Option<SurfaceId>,
        #[arg(long, value_enum, default_value_t = CliAgentTargetScope::Surface)]
        scope: CliAgentTargetScope,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        subtitle: Option<String>,
        #[arg(long = "notification-id")]
        notification_id: Option<String>,
        #[arg(long)]
        message: String,
        #[arg(long, value_enum, default_value_t = CliAttentionState::Waiting)]
        state: CliAttentionState,
    },
    List {
        #[arg(long)]
        socket: Option<PathBuf>,
        #[arg(long)]
        workspace: Option<WorkspaceId>,
    },
    Clear {
        #[arg(long)]
        socket: Option<PathBuf>,
        #[arg(long)]
        workspace: Option<WorkspaceId>,
        #[arg(long)]
        pane: Option<PaneId>,
        #[arg(long)]
        surface: Option<SurfaceId>,
        #[arg(long, value_enum, default_value_t = CliAgentTargetScope::Surface)]
        scope: CliAgentTargetScope,
    },
}

#[derive(Debug, Subcommand)]
enum BrowserCommand {
    Open {
        #[arg(long)]
        socket: Option<PathBuf>,
        #[arg(long)]
        workspace: Option<WorkspaceId>,
        #[arg(long)]
        pane: Option<PaneId>,
        #[arg(long)]
        url: Option<String>,
        #[arg(long, default_value_t = false)]
        ephemeral: bool,
    },
    Navigate {
        #[command(flatten)]
        browser: BrowserSurfaceArgs,
        #[arg(long)]
        url: String,
    },
    Back {
        #[command(flatten)]
        browser: BrowserSurfaceArgs,
    },
    Forward {
        #[command(flatten)]
        browser: BrowserSurfaceArgs,
    },
    Reload {
        #[command(flatten)]
        browser: BrowserSurfaceArgs,
    },
    Snapshot {
        #[command(flatten)]
        browser: BrowserSurfaceArgs,
    },
    Eval {
        #[command(flatten)]
        browser: BrowserSurfaceArgs,
        #[arg(long)]
        script: String,
    },
    Wait {
        #[command(flatten)]
        browser: BrowserSurfaceArgs,
        #[arg(long)]
        selector: Option<String>,
        #[arg(long)]
        text: Option<String>,
        #[arg(long)]
        url_contains: Option<String>,
        #[arg(long, value_enum)]
        load_state: Option<CliBrowserLoadState>,
        #[arg(long)]
        script: Option<String>,
        #[arg(long)]
        delay_ms: Option<u64>,
        #[arg(long, default_value_t = 3_000)]
        timeout_ms: u64,
        #[arg(long, default_value_t = 100)]
        poll_interval_ms: u64,
    },
    Click {
        #[command(flatten)]
        browser: BrowserSurfaceArgs,
        #[command(flatten)]
        target: BrowserTargetArgs,
        #[arg(long, default_value_t = false)]
        snapshot_after: bool,
    },
    Dblclick {
        #[command(flatten)]
        browser: BrowserSurfaceArgs,
        #[command(flatten)]
        target: BrowserTargetArgs,
        #[arg(long, default_value_t = false)]
        snapshot_after: bool,
    },
    Type {
        #[command(flatten)]
        browser: BrowserSurfaceArgs,
        #[command(flatten)]
        target: BrowserTargetArgs,
        #[arg(long)]
        text: String,
        #[arg(long, default_value_t = false)]
        snapshot_after: bool,
    },
    Fill {
        #[command(flatten)]
        browser: BrowserSurfaceArgs,
        #[command(flatten)]
        target: BrowserTargetArgs,
        #[arg(long)]
        text: String,
        #[arg(long, default_value_t = false)]
        snapshot_after: bool,
    },
    Press {
        #[command(flatten)]
        browser: BrowserSurfaceArgs,
        #[command(flatten)]
        target: BrowserOptionalTargetArgs,
        #[arg(long)]
        key: String,
        #[arg(long, default_value_t = false)]
        snapshot_after: bool,
    },
    Keydown {
        #[command(flatten)]
        browser: BrowserSurfaceArgs,
        #[command(flatten)]
        target: BrowserOptionalTargetArgs,
        #[arg(long)]
        key: String,
        #[arg(long, default_value_t = false)]
        snapshot_after: bool,
    },
    Keyup {
        #[command(flatten)]
        browser: BrowserSurfaceArgs,
        #[command(flatten)]
        target: BrowserOptionalTargetArgs,
        #[arg(long)]
        key: String,
        #[arg(long, default_value_t = false)]
        snapshot_after: bool,
    },
    Hover {
        #[command(flatten)]
        browser: BrowserSurfaceArgs,
        #[command(flatten)]
        target: BrowserTargetArgs,
        #[arg(long, default_value_t = false)]
        snapshot_after: bool,
    },
    Focus {
        #[command(flatten)]
        browser: BrowserSurfaceArgs,
        #[command(flatten)]
        target: BrowserTargetArgs,
        #[arg(long, default_value_t = false)]
        snapshot_after: bool,
    },
    Check {
        #[command(flatten)]
        browser: BrowserSurfaceArgs,
        #[command(flatten)]
        target: BrowserTargetArgs,
        #[arg(long, default_value_t = false)]
        snapshot_after: bool,
    },
    Uncheck {
        #[command(flatten)]
        browser: BrowserSurfaceArgs,
        #[command(flatten)]
        target: BrowserTargetArgs,
        #[arg(long, default_value_t = false)]
        snapshot_after: bool,
    },
    Select {
        #[command(flatten)]
        browser: BrowserSurfaceArgs,
        #[command(flatten)]
        target: BrowserTargetArgs,
        #[arg(long = "value")]
        values: Vec<String>,
        #[arg(long, default_value_t = false)]
        snapshot_after: bool,
    },
    Scroll {
        #[command(flatten)]
        browser: BrowserSurfaceArgs,
        #[command(flatten)]
        target: BrowserOptionalTargetArgs,
        #[arg(long, default_value_t = 0)]
        dx: i32,
        #[arg(long, default_value_t = 0)]
        dy: i32,
        #[arg(long, default_value_t = false)]
        snapshot_after: bool,
    },
    ScrollIntoView {
        #[command(flatten)]
        browser: BrowserSurfaceArgs,
        #[command(flatten)]
        target: BrowserTargetArgs,
        #[arg(long, default_value_t = false)]
        snapshot_after: bool,
    },
    Get {
        #[command(flatten)]
        browser: BrowserSurfaceArgs,
        #[command(subcommand)]
        command: BrowserGetSubcommand,
    },
    Is {
        #[command(flatten)]
        browser: BrowserSurfaceArgs,
        #[command(subcommand)]
        command: BrowserIsSubcommand,
    },
    Screenshot {
        #[command(flatten)]
        browser: BrowserSurfaceArgs,
        #[arg(long)]
        out: Option<String>,
        #[arg(long, short = 'f', default_value_t = false)]
        full: bool,
    },
    FocusWebview {
        #[command(flatten)]
        browser: BrowserSurfaceArgs,
    },
    IsWebviewFocused {
        #[command(flatten)]
        browser: BrowserSurfaceArgs,
    },
    ClearData {
        #[command(flatten)]
        browser: BrowserSurfaceArgs,
        #[arg(long)]
        origin_filter: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum DebugCommand {
    Terminal {
        #[command(subcommand)]
        command: TerminalDebugCliCommand,
    },
}

#[derive(Debug, Subcommand)]
enum TerminalDebugCliCommand {
    IsFocused {
        #[command(flatten)]
        terminal: TerminalSurfaceArgs,
    },
    ReadText {
        #[command(flatten)]
        terminal: TerminalSurfaceArgs,
        #[arg(long)]
        tail_lines: Option<usize>,
    },
    RenderStats {
        #[command(flatten)]
        terminal: TerminalSurfaceArgs,
    },
}

#[derive(Debug, Clone, Args)]
struct BrowserSurfaceArgs {
    #[arg(long)]
    socket: Option<PathBuf>,
    #[arg(long)]
    workspace: Option<WorkspaceId>,
    #[arg(long)]
    pane: Option<PaneId>,
    #[arg(long)]
    surface: Option<SurfaceId>,
}

#[derive(Debug, Clone, Args)]
struct TerminalSurfaceArgs {
    #[arg(long)]
    socket: Option<PathBuf>,
    #[arg(long)]
    workspace: Option<WorkspaceId>,
    #[arg(long)]
    pane: Option<PaneId>,
    #[arg(long)]
    surface: Option<SurfaceId>,
}

#[derive(Debug, Clone, Args)]
struct ScreenshotArgs {
    #[arg(long)]
    socket: Option<PathBuf>,
    #[arg(
        long,
        value_enum,
        help = "Taskers-owned screenshot target. V1 supports surface, pane, workspace_window, and workspace_canvas; app_window capture is deferred."
    )]
    target: CliScreenshotTarget,
    #[arg(long)]
    workspace: Option<WorkspaceId>,
    #[arg(long)]
    pane: Option<PaneId>,
    #[arg(long)]
    surface: Option<SurfaceId>,
    #[arg(long)]
    out: Option<String>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliScreenshotTarget {
    Surface,
    Pane,
    #[value(name = "workspace_window", alias = "workspace-window")]
    WorkspaceWindow,
    #[value(name = "workspace_canvas", alias = "workspace-canvas")]
    WorkspaceCanvas,
}

#[derive(Debug, Clone, Args)]
struct BrowserTargetArgs {
    #[arg(long = "ref")]
    reference: Option<String>,
    #[arg(long)]
    selector: Option<String>,
}

#[derive(Debug, Clone, Args)]
struct BrowserOptionalTargetArgs {
    #[arg(long = "ref")]
    reference: Option<String>,
    #[arg(long)]
    selector: Option<String>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliBrowserLoadState {
    Started,
    Redirected,
    Committed,
    Finished,
}

#[derive(Debug, Subcommand)]
enum BrowserGetSubcommand {
    Url,
    Title,
    Text {
        #[command(flatten)]
        target: BrowserTargetArgs,
    },
    Html {
        #[command(flatten)]
        target: BrowserTargetArgs,
    },
    Value {
        #[command(flatten)]
        target: BrowserTargetArgs,
    },
    Attr {
        #[command(flatten)]
        target: BrowserTargetArgs,
        #[arg(long)]
        name: String,
    },
    Count {
        #[arg(long)]
        selector: String,
    },
    Box {
        #[command(flatten)]
        target: BrowserTargetArgs,
    },
    Styles {
        #[command(flatten)]
        target: BrowserTargetArgs,
        #[arg(long = "property")]
        properties: Vec<String>,
    },
}

#[derive(Debug, Subcommand)]
enum BrowserIsSubcommand {
    Visible {
        #[command(flatten)]
        target: BrowserTargetArgs,
    },
    Enabled {
        #[command(flatten)]
        target: BrowserTargetArgs,
    },
    Checked {
        #[command(flatten)]
        target: BrowserTargetArgs,
    },
}

#[derive(Debug, Subcommand)]
enum PaneCommand {
    NewWindow {
        #[arg(long)]
        socket: Option<PathBuf>,
        #[arg(long)]
        workspace: WorkspaceId,
        #[arg(long, value_enum, default_value_t = CliDirection::Right)]
        direction: CliDirection,
    },
    Split {
        #[arg(long)]
        socket: Option<PathBuf>,
        #[arg(long)]
        workspace: WorkspaceId,
        #[arg(long)]
        pane: Option<PaneId>,
        #[arg(long, value_enum, default_value_t = CliAxis::Vertical)]
        axis: CliAxis,
        #[arg(long, value_enum, default_value_t = CliPaneKind::Terminal)]
        kind: CliPaneKind,
        #[arg(long)]
        url: Option<String>,
        #[arg(long, default_value_t = false)]
        ephemeral: bool,
    },
    Focus {
        #[arg(long)]
        socket: Option<PathBuf>,
        #[arg(long)]
        workspace: WorkspaceId,
        #[arg(long)]
        pane: PaneId,
    },
    FocusDirection {
        #[arg(long)]
        socket: Option<PathBuf>,
        #[arg(long)]
        workspace: WorkspaceId,
        #[arg(long, value_enum)]
        direction: CliDirection,
    },
    ResizeWindow {
        #[arg(long)]
        socket: Option<PathBuf>,
        #[arg(long)]
        workspace: WorkspaceId,
        #[arg(long, value_enum)]
        direction: CliDirection,
        #[arg(long, default_value_t = KEYBOARD_RESIZE_STEP)]
        amount: i32,
    },
    ResizeSplit {
        #[arg(long)]
        socket: Option<PathBuf>,
        #[arg(long)]
        workspace: WorkspaceId,
        #[arg(long, value_enum)]
        direction: CliDirection,
        #[arg(long, default_value_t = KEYBOARD_RESIZE_STEP)]
        amount: i32,
    },
    Close {
        #[arg(long)]
        socket: Option<PathBuf>,
        #[arg(long)]
        workspace: WorkspaceId,
        #[arg(long)]
        pane: PaneId,
    },
    Update {
        #[arg(long)]
        socket: Option<PathBuf>,
        #[arg(long)]
        pane: PaneId,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        cwd: Option<String>,
        #[arg(long)]
        repo: Option<String>,
        #[arg(long)]
        branch: Option<String>,
        #[arg(long)]
        agent: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum SurfaceCommand {
    New {
        #[arg(long)]
        socket: Option<PathBuf>,
        #[arg(long)]
        workspace: WorkspaceId,
        #[arg(long)]
        pane: PaneId,
        #[arg(long, value_enum, default_value_t = CliPaneKind::Terminal)]
        kind: CliPaneKind,
        #[arg(long)]
        url: Option<String>,
        #[arg(long, default_value_t = false)]
        ephemeral: bool,
    },
    Focus {
        #[arg(long)]
        socket: Option<PathBuf>,
        #[arg(long)]
        workspace: WorkspaceId,
        #[arg(long)]
        pane: PaneId,
        #[arg(long)]
        surface: SurfaceId,
    },
    Complete {
        #[arg(long)]
        socket: Option<PathBuf>,
        #[arg(long)]
        workspace: WorkspaceId,
        #[arg(long)]
        pane: PaneId,
        #[arg(long)]
        surface: SurfaceId,
    },
    AgentStart {
        #[arg(long)]
        socket: Option<PathBuf>,
        #[arg(long)]
        workspace: WorkspaceId,
        #[arg(long)]
        pane: PaneId,
        #[arg(long)]
        surface: SurfaceId,
        #[arg(long)]
        agent: String,
    },
    AgentStop {
        #[arg(long)]
        socket: Option<PathBuf>,
        #[arg(long)]
        workspace: WorkspaceId,
        #[arg(long)]
        pane: PaneId,
        #[arg(long)]
        surface: SurfaceId,
        #[arg(long = "exit-status")]
        exit_status: i32,
    },
    DismissAlert {
        #[arg(long)]
        socket: Option<PathBuf>,
        #[arg(long)]
        workspace: WorkspaceId,
        #[arg(long)]
        pane: PaneId,
        #[arg(long)]
        surface: SurfaceId,
    },
    Close {
        #[arg(long)]
        socket: Option<PathBuf>,
        #[arg(long)]
        workspace: WorkspaceId,
        #[arg(long)]
        pane: PaneId,
        #[arg(long)]
        surface: SurfaceId,
    },
}

#[derive(Debug, Subcommand)]
enum SessionCommand {
    Attach {
        #[arg(long)]
        socket: Option<PathBuf>,
        #[arg(long)]
        session: String,
        #[arg(
            trailing_var_arg = true,
            allow_hyphen_values = true,
            num_args = 0..
        )]
        shell_args: Vec<String>,
    },
    Terminate {
        #[arg(long)]
        socket: Option<PathBuf>,
        #[arg(long)]
        session: String,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliSignalKind {
    Metadata,
    Started,
    Progress,
    Completed,
    WaitingInput,
    Error,
    Notification,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliAxis {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliDirection {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum CliPaneKind {
    Terminal,
    Browser,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum CliAgentTargetScope {
    Workspace,
    Pane,
    Surface,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum CliAttentionState {
    Normal,
    Busy,
    Completed,
    Waiting,
    Error,
}

impl From<CliSignalKind> for SignalKind {
    fn from(value: CliSignalKind) -> Self {
        match value {
            CliSignalKind::Metadata => SignalKind::Metadata,
            CliSignalKind::Started => SignalKind::Started,
            CliSignalKind::Progress => SignalKind::Progress,
            CliSignalKind::Completed => SignalKind::Completed,
            CliSignalKind::WaitingInput => SignalKind::WaitingInput,
            CliSignalKind::Error => SignalKind::Error,
            CliSignalKind::Notification => SignalKind::Notification,
        }
    }
}

impl From<CliAxis> for SplitAxis {
    fn from(value: CliAxis) -> Self {
        match value {
            CliAxis::Horizontal => SplitAxis::Horizontal,
            CliAxis::Vertical => SplitAxis::Vertical,
        }
    }
}

impl From<CliDirection> for Direction {
    fn from(value: CliDirection) -> Self {
        match value {
            CliDirection::Left => Direction::Left,
            CliDirection::Right => Direction::Right,
            CliDirection::Up => Direction::Up,
            CliDirection::Down => Direction::Down,
        }
    }
}

impl From<CliPaneKind> for PaneKind {
    fn from(value: CliPaneKind) -> Self {
        match value {
            CliPaneKind::Terminal => PaneKind::Terminal,
            CliPaneKind::Browser => PaneKind::Browser,
        }
    }
}

fn browser_profile_mode(ephemeral: bool) -> BrowserProfileMode {
    if ephemeral {
        BrowserProfileMode::Ephemeral
    } else {
        BrowserProfileMode::PersistentDefault
    }
}

impl From<CliAttentionState> for AttentionState {
    fn from(value: CliAttentionState) -> Self {
        match value {
            CliAttentionState::Normal => AttentionState::Normal,
            CliAttentionState::Busy => AttentionState::Busy,
            CliAttentionState::Completed => AttentionState::Completed,
            CliAttentionState::Waiting => AttentionState::WaitingInput,
            CliAttentionState::Error => AttentionState::Error,
        }
    }
}

impl From<CliBrowserLoadState> for BrowserLoadState {
    fn from(value: CliBrowserLoadState) -> Self {
        match value {
            CliBrowserLoadState::Started => BrowserLoadState::Started,
            CliBrowserLoadState::Redirected => BrowserLoadState::Redirected,
            CliBrowserLoadState::Committed => BrowserLoadState::Committed,
            CliBrowserLoadState::Finished => BrowserLoadState::Finished,
        }
    }
}

#[derive(Debug, Clone)]
struct CompletionNode {
    path: Vec<String>,
    subcommands: Vec<String>,
    flags: Vec<String>,
    value_flags: Vec<String>,
}

fn cli_command() -> clap::Command {
    Cli::command()
}

fn render_completion(shell: CompletionShell) -> String {
    let nodes = completion_nodes(&cli_command());
    match shell {
        CompletionShell::Bash => render_bash_completion(&nodes),
        CompletionShell::Fish => render_fish_completion(&nodes),
        CompletionShell::Zsh => render_zsh_completion(&nodes),
    }
}

fn write_completion(shell: CompletionShell, mut writer: impl Write) -> anyhow::Result<()> {
    writer.write_all(render_completion(shell).as_bytes())?;
    writer.flush()?;
    Ok(())
}

fn completion_nodes(root: &clap::Command) -> Vec<CompletionNode> {
    let mut nodes = Vec::new();
    collect_completion_nodes(root, Vec::new(), &mut nodes);
    nodes
}

fn collect_completion_nodes(
    command: &clap::Command,
    path: Vec<String>,
    nodes: &mut Vec<CompletionNode>,
) {
    let subcommands = command
        .get_subcommands()
        .filter(|subcommand| !subcommand.is_hide_set())
        .map(|subcommand| subcommand.get_name().to_string())
        .collect::<Vec<_>>();
    let mut flags = Vec::new();
    let mut value_flags = Vec::new();

    for arg in command.get_arguments().filter(|arg| !arg.is_hide_set()) {
        let takes_values = arg.get_action().takes_values();

        if let Some(long) = arg.get_long() {
            let flag = format!("--{long}");
            push_unique(&mut flags, flag.clone());
            if takes_values {
                push_unique(&mut value_flags, flag);
            }
        }

        if let Some(short) = arg.get_short() {
            let flag = format!("-{short}");
            push_unique(&mut flags, flag.clone());
            if takes_values {
                push_unique(&mut value_flags, flag);
            }
        }
    }

    nodes.push(CompletionNode {
        path: path.clone(),
        subcommands,
        flags,
        value_flags,
    });

    for subcommand in command
        .get_subcommands()
        .filter(|subcommand| !subcommand.is_hide_set())
    {
        let mut child_path = path.clone();
        child_path.push(subcommand.get_name().to_string());
        collect_completion_nodes(subcommand, child_path, nodes);
    }
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.contains(&value) {
        values.push(value);
    }
}

async fn completion_query_candidates(query: &CompletionQueryArgs) -> Vec<String> {
    let Some(command) = completion_command_for_path(query.path.as_deref().unwrap_or_default())
    else {
        return Vec::new();
    };

    let Some(arg) = completion_arg_for_query(&command, query.flag.as_deref(), query.positional)
    else {
        return Vec::new();
    };

    let mut candidates = completion_static_candidates(arg);
    let dynamic = completion_dynamic_candidates(
        arg.get_id().as_str(),
        query.socket.clone(),
        query.workspace,
        query.pane,
        query.surface,
    )
    .await;
    for candidate in dynamic {
        push_unique(&mut candidates, candidate);
    }
    candidates
}

fn completion_command_for_path(path: &str) -> Option<clap::Command> {
    let mut command = cli_command();
    for segment in path
        .split_whitespace()
        .filter(|segment| !segment.is_empty())
    {
        let next = {
            command
                .get_subcommands()
                .find(|subcommand| !subcommand.is_hide_set() && subcommand.get_name() == segment)?
                .clone()
        };
        command = next;
    }
    Some(command)
}

fn completion_arg_for_query<'a>(
    command: &'a clap::Command,
    flag: Option<&str>,
    positional: Option<usize>,
) -> Option<&'a clap::Arg> {
    if let Some(flag) = flag {
        if let Some(long) = flag.strip_prefix("--") {
            return command
                .get_arguments()
                .find(|arg| !arg.is_hide_set() && arg.get_long() == Some(long));
        }
        if let Some(short) = flag.strip_prefix('-') {
            let mut chars = short.chars();
            let short = chars.next()?;
            if chars.next().is_some() {
                return None;
            }
            return command
                .get_arguments()
                .find(|arg| !arg.is_hide_set() && arg.get_short() == Some(short));
        }
    }

    positional.and_then(|index| {
        command
            .get_positionals()
            .filter(|arg| !arg.is_hide_set())
            .nth(index)
    })
}

fn completion_static_candidates(arg: &clap::Arg) -> Vec<String> {
    arg.get_possible_values()
        .into_iter()
        .filter(|value| !value.is_hide_set())
        .map(|value| value.get_name().to_string())
        .collect()
}

async fn completion_dynamic_candidates(
    arg_id: &str,
    socket: Option<PathBuf>,
    workspace: Option<WorkspaceId>,
    pane: Option<PaneId>,
    _surface: Option<SurfaceId>,
) -> Vec<String> {
    let client = ControlClient::new(resolve_socket_path(socket));
    let Ok(model) = query_model(&client).await else {
        return Vec::new();
    };

    match arg_id {
        "workspace" => {
            let mut values = model
                .workspaces
                .keys()
                .map(ToString::to_string)
                .collect::<Vec<_>>();
            values.sort();
            values
        }
        "pane" => {
            let Ok(workspace_id) = resolve_workspace_id_from_model(&model, workspace) else {
                return Vec::new();
            };
            let Some(workspace_record) = model.workspaces.get(&workspace_id) else {
                return Vec::new();
            };
            let mut values = workspace_record
                .panes
                .keys()
                .map(ToString::to_string)
                .collect::<Vec<_>>();
            values.sort();
            values
        }
        "surface" => {
            let Ok(workspace_id) = resolve_workspace_id_from_model(&model, workspace) else {
                return Vec::new();
            };
            let Some(workspace_record) = model.workspaces.get(&workspace_id) else {
                return Vec::new();
            };
            let pane_id = pane
                .or_else(env_pane_id)
                .unwrap_or(workspace_record.active_pane);
            let Some(pane_record) = workspace_record.panes.get(&pane_id) else {
                return Vec::new();
            };
            let mut values = pane_record
                .surfaces
                .keys()
                .map(ToString::to_string)
                .collect::<Vec<_>>();
            values.sort();
            values
        }
        _ => Vec::new(),
    }
}

fn render_bash_completion(nodes: &[CompletionNode]) -> String {
    format!(
        r#"_taskersctl_subcommands() {{
  case "$1" in
{subcommands_cases}    * ) ;;
  esac
}}

_taskersctl_flags() {{
  case "$1" in
{flags_cases}    * ) ;;
  esac
}}

_taskersctl_value_flags() {{
  case "$1" in
{value_flags_cases}    * ) ;;
  esac
}}

_taskersctl_query_values() {{
  local path="$1" flag="$2" positional="$3" socket="$4" workspace="$5" pane="$6" surface="$7"
  local args=(completion-query)
  [[ -n "$path" ]] && args+=(--path "$path")
  [[ -n "$flag" ]] && args+=("--flag=$flag")
  [[ -n "$positional" ]] && args+=(--positional "$positional")
  [[ -n "$socket" ]] && args+=(--socket "$socket")
  [[ -n "$workspace" ]] && args+=(--workspace "$workspace")
  [[ -n "$pane" ]] && args+=(--pane "$pane")
  [[ -n "$surface" ]] && args+=(--surface "$surface")
  taskersctl "${{args[@]}}" 2>/dev/null
}}

_taskersctl() {{
  local cur path subcommands flags value_flags word expect_value=0 expect_flag=""
  local selected_socket="" selected_workspace="" selected_pane="" selected_surface=""
  local positionals_used=0
  local i eq_flag eq_value joined
  local -a dynamic
  COMPREPLY=()
  cur="${{COMP_WORDS[COMP_CWORD]}}"
  path=""

  for ((i=1; i<COMP_CWORD; i++)); do
    word="${{COMP_WORDS[i]}}"
    if (( expect_value )); then
      case "$expect_flag" in
        --socket) selected_socket="$word" ;;
        --workspace) selected_workspace="$word" ;;
        --pane) selected_pane="$word" ;;
        --surface) selected_surface="$word" ;;
      esac
      expect_value=0
      expect_flag=""
      continue
    fi
    [[ -z "$word" ]] && continue

    if [[ "$word" == --*=* ]]; then
      eq_flag="${{word%%=*}}"
      eq_value="${{word#*=}}"
      case "$eq_flag" in
        --socket) selected_socket="$eq_value" ;;
        --workspace) selected_workspace="$eq_value" ;;
        --pane) selected_pane="$eq_value" ;;
        --surface) selected_surface="$eq_value" ;;
      esac
      value_flags="$(_taskersctl_value_flags "$path")"
      case " $value_flags " in
        *" $eq_flag "*) continue ;;
      esac
    fi

    if [[ "$word" == -* ]]; then
      value_flags="$(_taskersctl_value_flags "$path")"
      case " $value_flags " in
        *" $word "*) expect_value=1; expect_flag="$word" ;;
      esac
      continue
    fi

    subcommands="$(_taskersctl_subcommands "$path")"
    case " $subcommands " in
      *" $word "*) path="${{path:+$path }}$word" ;;
      *) positionals_used=$((positionals_used + 1)) ;;
    esac
  done

  if (( expect_value )); then
    mapfile -t dynamic < <(_taskersctl_query_values "$path" "$expect_flag" "" "$selected_socket" "$selected_workspace" "$selected_pane" "$selected_surface")
    joined="${{dynamic[*]}}"
    COMPREPLY=( $(compgen -W "$joined" -- "$cur") )
    return 0
  fi

  subcommands="$(_taskersctl_subcommands "$path")"
  flags="$(_taskersctl_flags "$path")"
  mapfile -t dynamic < <(_taskersctl_query_values "$path" "" "$positionals_used" "$selected_socket" "$selected_workspace" "$selected_pane" "$selected_surface")
  joined="${{dynamic[*]}}"
  if [[ "$cur" == -* ]]; then
    COMPREPLY=( $(compgen -W "$flags" -- "$cur") )
  else
    COMPREPLY=( $(compgen -W "$subcommands $flags $joined" -- "$cur") )
  fi
}}

complete -F _taskersctl taskersctl
"#,
        subcommands_cases = render_bash_case_body(nodes, |node| &node.subcommands),
        flags_cases = render_bash_case_body(nodes, |node| &node.flags),
        value_flags_cases = render_bash_case_body(nodes, |node| &node.value_flags),
    )
}

fn render_zsh_completion(nodes: &[CompletionNode]) -> String {
    format!(
        r#"#compdef taskersctl

__taskersctl_subcommands() {{
  case "$1" in
{subcommands_cases}    * ) ;;
  esac
}}

__taskersctl_flags() {{
  case "$1" in
{flags_cases}    * ) ;;
  esac
}}

__taskersctl_value_flags() {{
  case "$1" in
{value_flags_cases}    * ) ;;
  esac
}}

__taskersctl_query_values() {{
  local path="$1" flag="$2" positional="$3" socket="$4" workspace="$5" pane="$6" surface="$7"
  local -a args
  args=(completion-query)
  [[ -n "$path" ]] && args+=(--path "$path")
  [[ -n "$flag" ]] && args+=("--flag=$flag")
  [[ -n "$positional" ]] && args+=(--positional "$positional")
  [[ -n "$socket" ]] && args+=(--socket "$socket")
  [[ -n "$workspace" ]] && args+=(--workspace "$workspace")
  [[ -n "$pane" ]] && args+=(--pane "$pane")
  [[ -n "$surface" ]] && args+=(--surface "$surface")
  taskersctl $args 2>/dev/null
}}

_taskersctl() {{
  local cur path subcommands_text flags_text value_flags_text word expect_value=0 expect_flag=""
  local selected_socket="" selected_workspace="" selected_pane="" selected_surface=""
  local positionals_used=0
  local eq_flag eq_value
  local -a candidates
  local -a dynamic
  local i
  cur="${{words[CURRENT]}}"
  path=""

  for ((i=2; i<CURRENT; i++)); do
    word="${{words[i]}}"
    if (( expect_value )); then
      case "$expect_flag" in
        --socket) selected_socket="$word" ;;
        --workspace) selected_workspace="$word" ;;
        --pane) selected_pane="$word" ;;
        --surface) selected_surface="$word" ;;
      esac
      expect_value=0
      expect_flag=""
      continue
    fi
    [[ -z "$word" ]] && continue

    if [[ "$word" == --*=* ]]; then
      eq_flag="${{word%%=*}}"
      eq_value="${{word#*=}}"
      case "$eq_flag" in
        --socket) selected_socket="$eq_value" ;;
        --workspace) selected_workspace="$eq_value" ;;
        --pane) selected_pane="$eq_value" ;;
        --surface) selected_surface="$eq_value" ;;
      esac
      value_flags_text="$(__taskersctl_value_flags "$path")"
      [[ " $value_flags_text " == *" $eq_flag "* ]] && continue
    fi

    if [[ "$word" == -* ]]; then
      value_flags_text="$(__taskersctl_value_flags "$path")"
      if [[ " $value_flags_text " == *" $word "* ]]; then
        expect_value=1
        expect_flag="$word"
      fi
      continue
    fi

    subcommands_text="$(__taskersctl_subcommands "$path")"
    if [[ " $subcommands_text " == *" $word "* ]]; then
      path="${{path:+$path }}$word"
    else
      (( positionals_used += 1 ))
    fi
  done

  if (( expect_value )); then
    candidates=("${{(@f)$(__taskersctl_query_values "$path" "$expect_flag" "" "$selected_socket" "$selected_workspace" "$selected_pane" "$selected_surface")}}")
    (( $#candidates )) && compadd -- $candidates
    return 0
  fi

  subcommands_text="$(__taskersctl_subcommands "$path")"
  flags_text="$(__taskersctl_flags "$path")"
  dynamic=("${{(@f)$(__taskersctl_query_values "$path" "" "$positionals_used" "$selected_socket" "$selected_workspace" "$selected_pane" "$selected_surface")}}")
  if [[ "$cur" == -* ]]; then
    candidates=(${{=flags_text}})
  else
    candidates=(${{=subcommands_text}} ${{=flags_text}} $dynamic)
  fi
  compadd -- $candidates
}}

(( $+functions[compdef] )) && compdef _taskersctl taskersctl
"#,
        subcommands_cases = render_bash_case_body(nodes, |node| &node.subcommands),
        flags_cases = render_bash_case_body(nodes, |node| &node.flags),
        value_flags_cases = render_bash_case_body(nodes, |node| &node.value_flags),
    )
}

fn render_fish_completion(nodes: &[CompletionNode]) -> String {
    format!(
        r#"function __taskersctl_subcommands
  switch "$argv[1]"
{subcommands_cases}    case '*'
  end
end

function __taskersctl_flags
  switch "$argv[1]"
{flags_cases}    case '*'
  end
end

function __taskersctl_value_flags
  switch "$argv[1]"
{value_flags_cases}    case '*'
  end
end

function __taskersctl_query_values
  set -l args completion-query
  test -n "$argv[1]"; and set -a args --path "$argv[1]"
  test -n "$argv[2]"; and set -a args --flag="$argv[2]"
  test -n "$argv[3]"; and set -a args --positional "$argv[3]"
  test -n "$argv[4]"; and set -a args --socket "$argv[4]"
  test -n "$argv[5]"; and set -a args --workspace "$argv[5]"
  test -n "$argv[6]"; and set -a args --pane "$argv[6]"
  test -n "$argv[7]"; and set -a args --surface "$argv[7]"
  taskersctl $args 2>/dev/null
end

function __taskersctl_complete
  set -l tokens (commandline -opc)
  set -e tokens[1]
  set -l path
  set -l expect_value 0
  set -l expect_flag
  set -l selected_socket
  set -l selected_workspace
  set -l selected_pane
  set -l selected_surface
  set -l positionals_used 0

  for word in $tokens
    if test $expect_value -eq 1
      switch $expect_flag
        case --socket
          set selected_socket $word
        case --workspace
          set selected_workspace $word
        case --pane
          set selected_pane $word
        case --surface
          set selected_surface $word
      end
      set expect_value 0
      set expect_flag
      continue
    end
    if test -z "$word"
      continue
    end

    if string match -qr '^--[^=]+=.*$' -- $word
      set -l opt (string replace -r '=.*$' '' -- $word)
      set -l opt_value (string replace -r '^[^=]*=' '' -- $word)
      switch $opt
        case --socket
          set selected_socket $opt_value
        case --workspace
          set selected_workspace $opt_value
        case --pane
          set selected_pane $opt_value
        case --surface
          set selected_surface $opt_value
      end
      set -l value_flags (__taskersctl_value_flags "$path")
      if contains -- $opt $value_flags
        continue
      end
    end

    if string match -qr '^-' -- $word
      set -l value_flags (__taskersctl_value_flags "$path")
      if contains -- $word $value_flags
        set expect_value 1
        set expect_flag $word
      end
      continue
    end

    set -l subcommands (__taskersctl_subcommands "$path")
    if contains -- $word $subcommands
      if test -n "$path"
        set path "$path $word"
      else
        set path "$word"
      end
    else
      set positionals_used (math $positionals_used + 1)
    end
  end

  if test $expect_value -eq 1
    __taskersctl_query_values "$path" "$expect_flag" "" "$selected_socket" "$selected_workspace" "$selected_pane" "$selected_surface"
    return
  end

  set -l token (commandline -ct)
  set -l subcommands (__taskersctl_subcommands "$path")
  set -l flags (__taskersctl_flags "$path")
  set -l dynamic (__taskersctl_query_values "$path" "" "$positionals_used" "$selected_socket" "$selected_workspace" "$selected_pane" "$selected_surface")
  if string match -qr '^-' -- $token
    printf '%s\n' $flags
  else
    printf '%s\n' $subcommands $flags $dynamic
  end
end

complete -f -c taskersctl -a '(__taskersctl_complete)'
"#,
        subcommands_cases = render_fish_case_body(nodes, |node| &node.subcommands),
        flags_cases = render_fish_case_body(nodes, |node| &node.flags),
        value_flags_cases = render_fish_case_body(nodes, |node| &node.value_flags),
    )
}

fn render_bash_case_body<'a>(
    nodes: &'a [CompletionNode],
    values: impl Fn(&'a CompletionNode) -> &'a [String],
) -> String {
    let mut output = String::new();
    for node in nodes {
        output.push_str("    ");
        output.push_str(&completion_case_key(&node.path));
        output.push_str(" ) printf '%s' '");
        output.push_str(&shell_words(values(node)));
        output.push_str("' ;;\n");
    }
    output
}

fn render_fish_case_body<'a>(
    nodes: &'a [CompletionNode],
    values: impl Fn(&'a CompletionNode) -> &'a [String],
) -> String {
    let mut output = String::new();
    for node in nodes {
        output.push_str("    case '");
        output.push_str(&completion_path(&node.path));
        output.push_str("'\n");
        for value in values(node) {
            output.push_str("      echo '");
            output.push_str(value);
            output.push_str("'\n");
        }
    }
    output
}

fn completion_case_key(path: &[String]) -> String {
    let path = completion_path(path);
    if path.is_empty() {
        "''".into()
    } else {
        format!("'{path}'")
    }
}

fn completion_path(path: &[String]) -> String {
    path.join(" ")
}

fn shell_words(values: &[String]) -> String {
    values.join(" ")
}

pub async fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Serve { socket, demo } => {
            let socket = resolve_socket_path(socket);
            let listener = bind_socket(&socket)
                .with_context(|| format!("failed to bind socket at {}", socket.display()))?;
            let initial_model = if demo {
                AppModel::demo()
            } else {
                AppModel::new("Main")
            };
            let controller = InMemoryController::new(initial_model);
            eprintln!("serving taskers control API on {}", socket.display());
            serve(listener, controller, pending()).await?;
        }
        Command::Session { command } => match command {
            SessionCommand::Attach {
                socket,
                session,
                shell_args,
            } => {
                let client = TerminalSessionClient::new(resolve_terminal_socket_path(socket));
                client.attach_or_create(&session, &shell_args)?;
            }
            SessionCommand::Terminate { socket, session } => {
                let client = TerminalSessionClient::new(resolve_terminal_socket_path(socket));
                client.terminate_session(&session)?;
            }
        },
        Command::Query { query } => match query {
            QueryCommand::Status { socket } => {
                let client = ControlClient::new(resolve_socket_path(socket));
                let response = client
                    .send(ControlCommand::QueryStatus {
                        query: ControlQuery::All,
                    })
                    .await?;
                println!("{}", serde_json::to_string_pretty(&response)?);
            }
            QueryCommand::Agents { socket } => {
                let client = ControlClient::new(resolve_socket_path(socket));
                let model = query_model(&client).await?;
                let payload = model
                    .workspace_summaries(model.active_window)?
                    .into_iter()
                    .flat_map(|workspace| {
                        let workspace_id = workspace.workspace_id;
                        let workspace_label = workspace.label.clone();
                        workspace.agent_summaries.into_iter().map(move |agent| {
                            serde_json::json!({
                                "workspace_id": workspace_id,
                                "workspace_label": workspace_label,
                                "workspace_window_id": agent.workspace_window_id,
                                "pane_id": agent.pane_id,
                                "surface_id": agent.surface_id,
                                "agent_kind": agent.agent_kind,
                                "title": agent.title,
                                "state": format!("{:?}", agent.state).to_lowercase(),
                                "last_signal_at": agent.last_signal_at,
                            })
                        })
                    })
                    .collect::<Vec<_>>();
                println!("{}", serde_json::to_string_pretty(&payload)?);
            }
            QueryCommand::Notifications { socket } => {
                let client = ControlClient::new(resolve_socket_path(socket));
                let model = query_model(&client).await?;
                let payload = model
                    .activity_items()
                    .into_iter()
                    .map(|item| {
                        serde_json::json!({
                            "workspace_id": item.workspace_id,
                            "workspace_window_id": item.workspace_window_id,
                            "pane_id": item.pane_id,
                            "surface_id": item.surface_id,
                            "kind": format!("{:?}", item.kind).to_lowercase(),
                            "state": format!("{:?}", item.state).to_lowercase(),
                            "title": item.title,
                            "message": item.message,
                            "created_at": item.created_at,
                        })
                    })
                    .collect::<Vec<_>>();
                println!("{}", serde_json::to_string_pretty(&payload)?);
            }
            QueryCommand::Tree { socket } => {
                let client = ControlClient::new(resolve_socket_path(socket));
                let model = query_model(&client).await?;
                println!("{}", serde_json::to_string_pretty(&model)?);
            }
        },
        Command::Signal {
            socket,
            workspace,
            pane,
            surface,
            kind,
            message,
            title,
            cwd,
            repo,
            branch,
            agent,
            agent_active,
            command,
            source,
        } => {
            let workspace_id = workspace
                .or_else(env_workspace_id)
                .context("missing workspace id; pass --workspace or run from inside Taskers")?;
            let pane_id = pane
                .or_else(env_pane_id)
                .context("missing pane id; pass --pane or run from inside Taskers")?;
            let surface_id = surface.or_else(env_surface_id);
            let client = ControlClient::new(resolve_socket_path(socket));
            let metadata = if title.is_some()
                || cwd.is_some()
                || repo.is_some()
                || branch.is_some()
                || agent.is_some()
                || agent_active.is_some()
                || command.is_some()
            {
                Some(taskers_domain::SignalPaneMetadata {
                    title,
                    agent_title: None,
                    cwd,
                    repo_name: repo,
                    git_branch: branch,
                    ports: Vec::new(),
                    agent_kind: agent,
                    agent_active,
                    agent_command: command,
                })
            } else {
                None
            };
            let response = client
                .send(ControlCommand::EmitSignal {
                    workspace_id,
                    pane_id,
                    surface_id,
                    event: SignalEvent {
                        source: source.unwrap_or_else(|| "taskers-cli".into()),
                        kind: kind.into(),
                        message,
                        metadata,
                        timestamp: OffsetDateTime::now_utc(),
                    },
                })
                .await?;
            println!("{}", serde_json::to_string_pretty(&response)?);
        }
        Command::Notify {
            socket,
            workspace,
            pane,
            surface,
            title,
            subtitle,
            body,
            notification_id,
            agent: _agent,
        } => {
            let client = ControlClient::new(resolve_socket_path(socket));
            let model = query_model(&client).await?;
            ensure_implicit_notify_target_context(workspace, pane, surface)?;
            let target = resolve_agent_target(
                &model,
                workspace,
                pane,
                surface,
                CliAgentTargetScope::Surface,
            )?;
            let normalized_title = title.trim();
            let normalized_body = body
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned);
            let message = normalized_body.unwrap_or_else(|| normalized_title.to_string());
            let response = client
                .send(ControlCommand::AgentCreateNotification {
                    target,
                    kind: SignalKind::Notification,
                    title: Some(normalized_title.to_string()),
                    subtitle,
                    external_id: notification_id,
                    message,
                    state: AttentionState::WaitingInput,
                })
                .await?;
            println!("{}", serde_json::to_string_pretty(&response)?);
        }
        Command::Agent { command } => match command {
            AgentCommand::Status { command } => match command {
                AgentStatusCommand::Set {
                    socket,
                    workspace,
                    text,
                } => {
                    let client = ControlClient::new(resolve_socket_path(socket));
                    let model = query_model(&client).await?;
                    let workspace_id = resolve_workspace_id_from_model(&model, workspace)?;
                    let response = send_control_command(
                        &client,
                        ControlCommand::AgentSetStatus { workspace_id, text },
                    )
                    .await?;
                    println!("{}", serde_json::to_string_pretty(&response)?);
                }
                AgentStatusCommand::Clear { socket, workspace } => {
                    let client = ControlClient::new(resolve_socket_path(socket));
                    let model = query_model(&client).await?;
                    let workspace_id = resolve_workspace_id_from_model(&model, workspace)?;
                    let response = send_control_command(
                        &client,
                        ControlCommand::AgentClearStatus { workspace_id },
                    )
                    .await?;
                    println!("{}", serde_json::to_string_pretty(&response)?);
                }
            },
            AgentCommand::Progress { command } => match command {
                AgentProgressCommand::Set {
                    socket,
                    workspace,
                    value,
                    label,
                } => {
                    let client = ControlClient::new(resolve_socket_path(socket));
                    let model = query_model(&client).await?;
                    let workspace_id = resolve_workspace_id_from_model(&model, workspace)?;
                    let response = send_control_command(
                        &client,
                        ControlCommand::AgentSetProgress {
                            workspace_id,
                            progress: ProgressState { value, label },
                        },
                    )
                    .await?;
                    println!("{}", serde_json::to_string_pretty(&response)?);
                }
                AgentProgressCommand::Clear { socket, workspace } => {
                    let client = ControlClient::new(resolve_socket_path(socket));
                    let model = query_model(&client).await?;
                    let workspace_id = resolve_workspace_id_from_model(&model, workspace)?;
                    let response = send_control_command(
                        &client,
                        ControlCommand::AgentClearProgress { workspace_id },
                    )
                    .await?;
                    println!("{}", serde_json::to_string_pretty(&response)?);
                }
            },
            AgentCommand::Log { command } => match command {
                AgentLogCommand::Append {
                    socket,
                    workspace,
                    message,
                    source,
                } => {
                    let client = ControlClient::new(resolve_socket_path(socket));
                    let model = query_model(&client).await?;
                    let workspace_id = resolve_workspace_id_from_model(&model, workspace)?;
                    let response = send_control_command(
                        &client,
                        ControlCommand::AgentAppendLog {
                            workspace_id,
                            entry: WorkspaceLogEntry {
                                source,
                                message,
                                created_at: OffsetDateTime::now_utc(),
                            },
                        },
                    )
                    .await?;
                    println!("{}", serde_json::to_string_pretty(&response)?);
                }
                AgentLogCommand::List { socket, workspace } => {
                    let client = ControlClient::new(resolve_socket_path(socket));
                    let model = query_model(&client).await?;
                    let workspace_id = resolve_workspace_id_from_model(&model, workspace)?;
                    let workspace = model
                        .workspaces
                        .get(&workspace_id)
                        .ok_or_else(|| anyhow!("workspace {workspace_id} not found"))?;
                    println!("{}", serde_json::to_string_pretty(&workspace.log_entries)?);
                }
                AgentLogCommand::Clear { socket, workspace } => {
                    let client = ControlClient::new(resolve_socket_path(socket));
                    let model = query_model(&client).await?;
                    let workspace_id = resolve_workspace_id_from_model(&model, workspace)?;
                    let response = send_control_command(
                        &client,
                        ControlCommand::AgentClearLog { workspace_id },
                    )
                    .await?;
                    println!("{}", serde_json::to_string_pretty(&response)?);
                }
            },
            AgentCommand::Notify { command } => match command {
                AgentNotifyCommand::Create {
                    socket,
                    workspace,
                    pane,
                    surface,
                    scope,
                    title,
                    subtitle,
                    notification_id,
                    message,
                    state,
                } => {
                    let client = ControlClient::new(resolve_socket_path(socket));
                    let model = query_model(&client).await?;
                    let target = resolve_agent_target(&model, workspace, pane, surface, scope)?;
                    let response = send_control_command(
                        &client,
                        ControlCommand::AgentCreateNotification {
                            target,
                            kind: SignalKind::Notification,
                            title,
                            subtitle,
                            external_id: notification_id,
                            message,
                            state: state.into(),
                        },
                    )
                    .await?;
                    println!("{}", serde_json::to_string_pretty(&response)?);
                }
                AgentNotifyCommand::List { socket, workspace } => {
                    let client = ControlClient::new(resolve_socket_path(socket));
                    let model = query_model(&client).await?;
                    let workspace_filter = workspace.or_else(env_workspace_id);
                    let payload = model
                        .activity_items()
                        .into_iter()
                        .filter(|item| {
                            workspace_filter
                                .is_none_or(|workspace_id| item.workspace_id == workspace_id)
                        })
                        .map(|item| {
                            serde_json::json!({
                                "workspace_id": item.workspace_id,
                                "workspace_window_id": item.workspace_window_id,
                                "notification_id": item.notification_id,
                                "pane_id": item.pane_id,
                                "surface_id": item.surface_id,
                                "kind": format!("{:?}", item.kind).to_lowercase(),
                                "state": format!("{:?}", item.state).to_lowercase(),
                                "title": item.title,
                                "subtitle": item.subtitle,
                                "message": item.message,
                                "read_at": item.read_at,
                                "created_at": item.created_at,
                            })
                        })
                        .collect::<Vec<_>>();
                    println!("{}", serde_json::to_string_pretty(&payload)?);
                }
                AgentNotifyCommand::Clear {
                    socket,
                    workspace,
                    pane,
                    surface,
                    scope,
                } => {
                    let client = ControlClient::new(resolve_socket_path(socket));
                    let model = query_model(&client).await?;
                    let target = resolve_agent_target(&model, workspace, pane, surface, scope)?;
                    let response = send_control_command(
                        &client,
                        ControlCommand::AgentClearNotifications { target },
                    )
                    .await?;
                    println!("{}", serde_json::to_string_pretty(&response)?);
                }
            },
            AgentCommand::Flash {
                socket,
                workspace,
                pane,
                surface,
            } => {
                let client = ControlClient::new(resolve_socket_path(socket));
                let model = query_model(&client).await?;
                let target = resolve_agent_target(
                    &model,
                    workspace,
                    pane,
                    surface,
                    CliAgentTargetScope::Surface,
                )?;
                let AgentTarget::Surface {
                    workspace_id,
                    pane_id,
                    surface_id,
                } = target
                else {
                    bail!("surface flash requires a surface target");
                };
                let response = send_control_command(
                    &client,
                    ControlCommand::AgentTriggerFlash {
                        workspace_id,
                        pane_id,
                        surface_id,
                    },
                )
                .await?;
                println!("{}", serde_json::to_string_pretty(&response)?);
            }
            AgentCommand::FocusUnread { socket } => {
                let client = ControlClient::new(resolve_socket_path(socket));
                let response = send_control_command(
                    &client,
                    ControlCommand::AgentFocusLatestUnread { window_id: None },
                )
                .await?;
                println!("{}", serde_json::to_string_pretty(&response)?);
            }
        },
        Command::Workspace { command } => match command {
            WorkspaceCommand::List { socket } => {
                let client = ControlClient::new(resolve_socket_path(socket));
                let model = query_model(&client).await?;
                let active_workspace = model.active_workspace_id();
                let payload = model
                    .workspace_summaries(model.active_window)?
                    .into_iter()
                    .map(|workspace| {
                        serde_json::json!({
                            "workspace_id": workspace.workspace_id,
                            "label": workspace.label,
                            "active": active_workspace == Some(workspace.workspace_id),
                            "unread_count": workspace.unread_count,
                            "highest_attention": format!("{:?}", workspace.highest_attention).to_lowercase(),
                            "display_attention": format!("{:?}", workspace.display_attention).to_lowercase(),
                            "agent_count": workspace.agent_summaries.len(),
                            "repo_hint": workspace.repo_hint,
                            "latest_notification": workspace.latest_notification,
                        })
                    })
                    .collect::<Vec<_>>();
                println!("{}", serde_json::to_string_pretty(&payload)?);
            }
            WorkspaceCommand::New { socket, label } => {
                let client = ControlClient::new(resolve_socket_path(socket));
                let response = client
                    .send(ControlCommand::CreateWorkspace { label })
                    .await?;
                println!("{}", serde_json::to_string_pretty(&response)?);
            }
            WorkspaceCommand::Switch { socket, workspace } => {
                let client = ControlClient::new(resolve_socket_path(socket));
                let response = client
                    .send(ControlCommand::SwitchWorkspace {
                        window_id: None,
                        workspace_id: workspace,
                    })
                    .await?;
                println!("{}", serde_json::to_string_pretty(&response)?);
            }
            WorkspaceCommand::Rename {
                socket,
                workspace,
                label,
            } => {
                let client = ControlClient::new(resolve_socket_path(socket));
                let response = client
                    .send(ControlCommand::RenameWorkspace {
                        workspace_id: workspace,
                        label,
                    })
                    .await?;
                println!("{}", serde_json::to_string_pretty(&response)?);
            }
            WorkspaceCommand::Close { socket, workspace } => {
                let client = ControlClient::new(resolve_socket_path(socket));
                let response = client
                    .send(ControlCommand::CloseWorkspace {
                        workspace_id: workspace,
                    })
                    .await?;
                println!("{}", serde_json::to_string_pretty(&response)?);
            }
        },
        Command::AgentHook { command } => match command {
            AgentHookCommand::SessionStart {
                socket,
                workspace,
                pane,
                surface,
                agent,
                title,
                message,
            } => {
                emit_agent_hook(
                    socket,
                    workspace,
                    pane,
                    surface,
                    agent,
                    title,
                    message,
                    CliSignalKind::Started,
                )
                .await?;
            }
            AgentHookCommand::Active {
                socket,
                workspace,
                pane,
                surface,
                agent,
                title,
                message,
            }
            | AgentHookCommand::Progress {
                socket,
                workspace,
                pane,
                surface,
                agent,
                title,
                message,
            } => {
                emit_agent_hook(
                    socket,
                    workspace,
                    pane,
                    surface,
                    agent,
                    title,
                    message,
                    CliSignalKind::Progress,
                )
                .await?;
            }
            AgentHookCommand::Waiting {
                socket,
                workspace,
                pane,
                surface,
                agent,
                title,
                message,
            } => {
                emit_agent_hook(
                    socket,
                    workspace,
                    pane,
                    surface,
                    agent,
                    title,
                    message,
                    CliSignalKind::WaitingInput,
                )
                .await?;
            }
            AgentHookCommand::Notification {
                socket,
                workspace,
                pane,
                surface,
                agent,
                title,
                message,
            } => {
                emit_agent_hook(
                    socket,
                    workspace,
                    pane,
                    surface,
                    agent,
                    title,
                    message,
                    CliSignalKind::Notification,
                )
                .await?;
            }
            AgentHookCommand::Stop {
                socket,
                workspace,
                pane,
                surface,
                agent,
                title,
                message,
            } => {
                emit_agent_hook(
                    socket,
                    workspace,
                    pane,
                    surface,
                    agent,
                    title,
                    message,
                    CliSignalKind::Completed,
                )
                .await?;
            }
        },
        Command::Browser { command } => {
            handle_browser_cli_command(command).await?;
        }
        Command::Screenshot { screenshot } => {
            handle_screenshot_cli_command(screenshot).await?;
        }
        Command::Completion { shell } => {
            write_completion(shell, io::stdout())?;
        }
        Command::CompletionQuery { query } => {
            for candidate in completion_query_candidates(&query).await {
                println!("{candidate}");
            }
        }
        Command::Identify {
            socket,
            workspace,
            pane,
            surface,
        } => {
            let client = ControlClient::new(resolve_socket_path(socket));
            let response = send_control_command(
                &client,
                ControlCommand::QueryStatus {
                    query: ControlQuery::Identify {
                        workspace_id: workspace.or_else(env_workspace_id),
                        pane_id: pane.or_else(env_pane_id),
                        surface_id: surface.or_else(env_surface_id),
                    },
                },
            )
            .await?;
            match response {
                ControlResponse::Identify { result } => {
                    println!("{}", serde_json::to_string_pretty(&result)?);
                }
                other => bail!("unexpected identify response: {other:?}"),
            }
        }
        Command::Debug { command } => match command {
            DebugCommand::Terminal { command } => {
                handle_terminal_debug_cli_command(command).await?;
            }
        },
        Command::Pane { command } => match command {
            PaneCommand::NewWindow {
                socket,
                workspace,
                direction,
            } => {
                let client = ControlClient::new(resolve_socket_path(socket));
                let response = client
                    .send(ControlCommand::CreateWorkspaceWindow {
                        workspace_id: workspace,
                        direction: direction.into(),
                    })
                    .await?;
                println!("{}", serde_json::to_string_pretty(&response)?);
            }
            PaneCommand::Split {
                socket,
                workspace,
                pane,
                axis,
                kind,
                url,
                ephemeral,
            } => {
                if url.is_some() && kind != CliPaneKind::Browser {
                    bail!("--url requires --kind browser");
                }
                if ephemeral && kind != CliPaneKind::Browser {
                    bail!("--ephemeral requires --kind browser");
                }

                let client = ControlClient::new(resolve_socket_path(socket));
                if kind == CliPaneKind::Terminal {
                    let response = client
                        .send(ControlCommand::SplitPane {
                            workspace_id: workspace,
                            pane_id: pane,
                            axis: axis.into(),
                        })
                        .await?;
                    println!("{}", serde_json::to_string_pretty(&response)?);
                } else {
                    let response = send_control_command(
                        &client,
                        ControlCommand::SplitPane {
                            workspace_id: workspace,
                            pane_id: pane,
                            axis: axis.into(),
                        },
                    )
                    .await?;
                    let pane_id = match response {
                        ControlResponse::PaneSplit { pane_id } => pane_id,
                        other => bail!("unexpected split response: {other:?}"),
                    };
                    let placeholder_surface_id =
                        active_surface_for_pane(&query_model(&client).await?, workspace, pane_id)?;
                    let surface_id = create_surface(
                        &client,
                        workspace,
                        pane_id,
                        kind.into(),
                        Some(browser_profile_mode(ephemeral)),
                        url.clone(),
                    )
                    .await?;
                    send_control_command(
                        &client,
                        ControlCommand::CloseSurface {
                            workspace_id: workspace,
                            pane_id,
                            surface_id: placeholder_surface_id,
                        },
                    )
                    .await?;
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "status": "browser_surface_opened",
                            "workspace_id": workspace,
                            "pane_id": pane_id,
                            "surface_id": surface_id,
                            "replaced_surface_id": placeholder_surface_id,
                            "url": url,
                        }))?
                    );
                }
            }
            PaneCommand::Focus {
                socket,
                workspace,
                pane,
            } => {
                let client = ControlClient::new(resolve_socket_path(socket));
                let response = client
                    .send(ControlCommand::FocusPane {
                        workspace_id: workspace,
                        pane_id: pane,
                    })
                    .await?;
                println!("{}", serde_json::to_string_pretty(&response)?);
            }
            PaneCommand::FocusDirection {
                socket,
                workspace,
                direction,
            } => {
                let client = ControlClient::new(resolve_socket_path(socket));
                let response = client
                    .send(ControlCommand::FocusPaneDirection {
                        workspace_id: workspace,
                        direction: direction.into(),
                    })
                    .await?;
                println!("{}", serde_json::to_string_pretty(&response)?);
            }
            PaneCommand::ResizeWindow {
                socket,
                workspace,
                direction,
                amount,
            } => {
                let client = ControlClient::new(resolve_socket_path(socket));
                let response = client
                    .send(ControlCommand::ResizeActiveWindow {
                        workspace_id: workspace,
                        direction: direction.into(),
                        amount,
                    })
                    .await?;
                println!("{}", serde_json::to_string_pretty(&response)?);
            }
            PaneCommand::ResizeSplit {
                socket,
                workspace,
                direction,
                amount,
            } => {
                let client = ControlClient::new(resolve_socket_path(socket));
                let response = client
                    .send(ControlCommand::ResizeActivePaneSplit {
                        workspace_id: workspace,
                        direction: direction.into(),
                        amount,
                    })
                    .await?;
                println!("{}", serde_json::to_string_pretty(&response)?);
            }
            PaneCommand::Close {
                socket,
                workspace,
                pane,
            } => {
                let client = ControlClient::new(resolve_socket_path(socket));
                let response = client
                    .send(ControlCommand::ClosePane {
                        workspace_id: workspace,
                        pane_id: pane,
                    })
                    .await?;
                println!("{}", serde_json::to_string_pretty(&response)?);
            }
            PaneCommand::Update {
                socket,
                pane,
                title,
                cwd,
                repo,
                branch,
                agent,
            } => {
                let client = ControlClient::new(resolve_socket_path(socket));
                let response = client
                    .send(ControlCommand::UpdatePaneMetadata {
                        pane_id: pane,
                        patch: PaneMetadataPatch {
                            title,
                            cwd,
                            url: None,
                            browser_profile_mode: None,
                            repo_name: repo,
                            git_branch: branch,
                            ports: None,
                            agent_kind: agent,
                        },
                    })
                    .await?;
                println!("{}", serde_json::to_string_pretty(&response)?);
            }
        },
        Command::Surface { command } => match command {
            SurfaceCommand::New {
                socket,
                workspace,
                pane,
                kind,
                url,
                ephemeral,
            } => {
                if url.is_some() && kind != CliPaneKind::Browser {
                    bail!("--url requires --kind browser");
                }
                if ephemeral && kind != CliPaneKind::Browser {
                    bail!("--ephemeral requires --kind browser");
                }

                let client = ControlClient::new(resolve_socket_path(socket));
                if kind == CliPaneKind::Terminal {
                    let response = client
                        .send(ControlCommand::CreateSurface {
                            workspace_id: workspace,
                            pane_id: pane,
                            kind: PaneKind::Terminal,
                            browser_profile_mode: None,
                        })
                        .await?;
                    println!("{}", serde_json::to_string_pretty(&response)?);
                } else {
                    let surface_id = create_surface(
                        &client,
                        workspace,
                        pane,
                        kind.into(),
                        Some(browser_profile_mode(ephemeral)),
                        url.clone(),
                    )
                    .await?;
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "status": "surface_created",
                            "workspace_id": workspace,
                            "pane_id": pane,
                            "surface_id": surface_id,
                            "kind": "browser",
                            "url": url,
                            "profile_mode": browser_profile_mode(ephemeral),
                        }))?
                    );
                }
            }
            SurfaceCommand::Focus {
                socket,
                workspace,
                pane,
                surface,
            } => {
                let client = ControlClient::new(resolve_socket_path(socket));
                let response = client
                    .send(ControlCommand::FocusSurface {
                        workspace_id: workspace,
                        pane_id: pane,
                        surface_id: surface,
                    })
                    .await?;
                println!("{}", serde_json::to_string_pretty(&response)?);
            }
            SurfaceCommand::Complete {
                socket,
                workspace,
                pane,
                surface,
            } => {
                let client = ControlClient::new(resolve_socket_path(socket));
                let response = client
                    .send(ControlCommand::MarkSurfaceCompleted {
                        workspace_id: workspace,
                        pane_id: pane,
                        surface_id: surface,
                    })
                    .await?;
                println!("{}", serde_json::to_string_pretty(&response)?);
            }
            SurfaceCommand::AgentStart {
                socket,
                workspace,
                pane,
                surface,
                agent,
            } => {
                let client = ControlClient::new(resolve_socket_path(socket));
                let response = client
                    .send(ControlCommand::StartSurfaceAgentSession {
                        workspace_id: workspace,
                        pane_id: pane,
                        surface_id: surface,
                        agent_kind: agent,
                    })
                    .await?;
                println!("{}", serde_json::to_string_pretty(&response)?);
            }
            SurfaceCommand::AgentStop {
                socket,
                workspace,
                pane,
                surface,
                exit_status,
            } => {
                let client = ControlClient::new(resolve_socket_path(socket));
                let response = client
                    .send(ControlCommand::StopSurfaceAgentSession {
                        workspace_id: workspace,
                        pane_id: pane,
                        surface_id: surface,
                        exit_status,
                    })
                    .await?;
                println!("{}", serde_json::to_string_pretty(&response)?);
            }
            SurfaceCommand::DismissAlert {
                socket,
                workspace,
                pane,
                surface,
            } => {
                let client = ControlClient::new(resolve_socket_path(socket));
                let response = client
                    .send(ControlCommand::DismissSurfaceAlert {
                        workspace_id: workspace,
                        pane_id: pane,
                        surface_id: surface,
                    })
                    .await?;
                println!("{}", serde_json::to_string_pretty(&response)?);
            }
            SurfaceCommand::Close {
                socket,
                workspace,
                pane,
                surface,
            } => {
                let client = ControlClient::new(resolve_socket_path(socket));
                let response = client
                    .send(ControlCommand::CloseSurface {
                        workspace_id: workspace,
                        pane_id: pane,
                        surface_id: surface,
                    })
                    .await?;
                println!("{}", serde_json::to_string_pretty(&response)?);
            }
        },
    }

    Ok(())
}

fn env_workspace_id() -> Option<WorkspaceId> {
    if !taskers_env_context_matches_current_tty() {
        return None;
    }
    env::var("TASKERS_WORKSPACE_ID")
        .ok()
        .and_then(|value| value.parse().ok())
}

fn env_pane_id() -> Option<PaneId> {
    if !taskers_env_context_matches_current_tty() {
        return None;
    }
    env::var("TASKERS_PANE_ID")
        .ok()
        .and_then(|value| value.parse().ok())
}

fn env_surface_id() -> Option<SurfaceId> {
    if !taskers_env_context_matches_current_tty() {
        return None;
    }
    env::var("TASKERS_SURFACE_ID")
        .ok()
        .and_then(|value| value.parse().ok())
}

fn env_tty_name() -> Option<String> {
    env::var("TASKERS_TTY_NAME")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn current_process_tty_name() -> Option<String> {
    let output = ProcessCommand::new("ps")
        .args(["-o", "tty=", "-p", &std::process::id().to_string()])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if raw.is_empty() || raw == "?" {
        return None;
    }
    if raw.starts_with('/') {
        Some(raw)
    } else {
        Some(format!("/dev/{raw}"))
    }
}

fn taskers_env_context_matches_current_tty() -> bool {
    match env_tty_name() {
        Some(expected) => current_process_tty_name().is_some_and(|current| current == expected),
        None => true,
    }
}

fn has_implicit_notify_target_context() -> bool {
    env_workspace_id().is_some() && env_pane_id().is_some() && env_surface_id().is_some()
}

fn ensure_implicit_notify_target_context(
    workspace: Option<WorkspaceId>,
    pane: Option<PaneId>,
    surface: Option<SurfaceId>,
) -> anyhow::Result<()> {
    if workspace.is_some()
        || pane.is_some()
        || surface.is_some()
        || has_implicit_notify_target_context()
    {
        return Ok(());
    }

    bail!(
        "notify requires embedded Taskers pane context; pass --workspace/--pane/--surface when running outside Taskers"
    )
}

fn resolve_socket_path(socket: Option<PathBuf>) -> PathBuf {
    socket
        .or_else(|| env::var_os("TASKERS_SOCKET").map(PathBuf::from))
        .unwrap_or_else(default_socket_path)
}

fn resolve_terminal_socket_path(socket: Option<PathBuf>) -> PathBuf {
    socket
        .or_else(|| env::var_os("TASKERS_TERMINAL_SOCKET").map(PathBuf::from))
        .unwrap_or_else(default_terminal_socket_path)
}

async fn send_control_command(
    client: &ControlClient,
    command: ControlCommand,
) -> anyhow::Result<ControlResponse> {
    let response = client.send(command).await?;
    response.response.map_err(|error| anyhow!(error))
}

async fn query_model(client: &ControlClient) -> anyhow::Result<AppModel> {
    let response = send_control_command(
        client,
        ControlCommand::QueryStatus {
            query: ControlQuery::All,
        },
    )
    .await?;
    match response {
        ControlResponse::Status { session } => Ok(session.model),
        other => bail!("unexpected query response: {other:?}"),
    }
}

async fn resolve_surface_context(
    client: &ControlClient,
    surface_id: SurfaceId,
) -> anyhow::Result<(WorkspaceId, PaneId, SurfaceId)> {
    let response = send_control_command(
        client,
        ControlCommand::QueryStatus {
            query: ControlQuery::Identify {
                workspace_id: None,
                pane_id: None,
                surface_id: Some(surface_id),
            },
        },
    )
    .await?;

    let ControlResponse::Identify { result } = response else {
        bail!("unexpected identify response: {response:?}");
    };
    let caller = result
        .caller
        .ok_or_else(|| anyhow!("missing identify caller context for surface {surface_id}"))?;
    Ok((caller.workspace_id, caller.pane_id, caller.surface_id))
}

fn active_surface_for_pane(
    model: &AppModel,
    workspace_id: WorkspaceId,
    pane_id: PaneId,
) -> anyhow::Result<SurfaceId> {
    model
        .workspaces
        .get(&workspace_id)
        .and_then(|workspace| workspace.panes.get(&pane_id))
        .map(|pane| pane.active_surface)
        .ok_or_else(|| anyhow!("pane {pane_id} is not present in workspace {workspace_id}"))
}

fn resolve_workspace_id_from_model(
    model: &AppModel,
    workspace: Option<WorkspaceId>,
) -> anyhow::Result<WorkspaceId> {
    workspace
        .or_else(env_workspace_id)
        .or_else(|| model.active_workspace_id())
        .context("missing workspace id; pass --workspace or run from inside Taskers")
}

fn resolve_workspace_window_screenshot_target(
    model: &AppModel,
    workspace: Option<WorkspaceId>,
) -> anyhow::Result<ScreenshotTarget> {
    let workspace_id = resolve_workspace_id_from_model(model, workspace)?;
    let workspace = model
        .workspaces
        .get(&workspace_id)
        .ok_or_else(|| anyhow!("workspace {workspace_id} not found"))?;
    if !workspace.windows.contains_key(&workspace.active_window) {
        bail!("workspace {workspace_id} has no active workspace window");
    }
    Ok(ScreenshotTarget::WorkspaceWindow { workspace_id })
}

fn resolve_agent_target(
    model: &AppModel,
    workspace: Option<WorkspaceId>,
    pane: Option<PaneId>,
    surface: Option<SurfaceId>,
    scope: CliAgentTargetScope,
) -> anyhow::Result<AgentTarget> {
    let workspace_id = resolve_workspace_id_from_model(model, workspace)?;
    let workspace_record = model
        .workspaces
        .get(&workspace_id)
        .ok_or_else(|| anyhow!("workspace {workspace_id} not found"))?;

    let resolved_pane = pane
        .or_else(env_pane_id)
        .unwrap_or(workspace_record.active_pane);
    let pane_record = workspace_record.panes.get(&resolved_pane).ok_or_else(|| {
        anyhow!("pane {resolved_pane} is not present in workspace {workspace_id}")
    })?;
    let resolved_surface = surface
        .or_else(env_surface_id)
        .unwrap_or(pane_record.active_surface);

    match scope {
        CliAgentTargetScope::Workspace => Ok(AgentTarget::Workspace { workspace_id }),
        CliAgentTargetScope::Pane => Ok(AgentTarget::Pane {
            workspace_id,
            pane_id: resolved_pane,
        }),
        CliAgentTargetScope::Surface => Ok(AgentTarget::Surface {
            workspace_id,
            pane_id: resolved_pane,
            surface_id: resolved_surface,
        }),
    }
}

async fn create_surface(
    client: &ControlClient,
    workspace_id: WorkspaceId,
    pane_id: PaneId,
    kind: PaneKind,
    browser_profile_mode: Option<BrowserProfileMode>,
    url: Option<String>,
) -> anyhow::Result<SurfaceId> {
    let response = send_control_command(
        client,
        ControlCommand::CreateSurface {
            workspace_id,
            pane_id,
            kind,
            browser_profile_mode,
        },
    )
    .await?;
    let surface_id = match response {
        ControlResponse::SurfaceCreated { surface_id } => surface_id,
        other => bail!("unexpected create surface response: {other:?}"),
    };

    if let Some(url) = url {
        send_control_command(
            client,
            ControlCommand::UpdateSurfaceMetadata {
                surface_id,
                patch: PaneMetadataPatch {
                    title: None,
                    cwd: None,
                    url: Some(url),
                    browser_profile_mode: None,
                    repo_name: None,
                    git_branch: None,
                    ports: None,
                    agent_kind: None,
                },
            },
        )
        .await?;
    }

    Ok(surface_id)
}

async fn handle_browser_cli_command(command: BrowserCommand) -> anyhow::Result<()> {
    match command {
        BrowserCommand::Open {
            socket,
            workspace,
            pane,
            url,
            ephemeral,
        } => {
            let client = ControlClient::new(resolve_socket_path(socket));
            let model = query_model(&client).await?;
            let workspace_id = resolve_workspace_id_from_model(&model, workspace)?;
            let target_pane = pane.or_else(env_pane_id).or_else(|| {
                model
                    .workspaces
                    .get(&workspace_id)
                    .map(|workspace| workspace.active_pane)
            });
            let response = send_control_command(
                &client,
                ControlCommand::SplitPane {
                    workspace_id,
                    pane_id: target_pane,
                    axis: SplitAxis::Horizontal,
                },
            )
            .await?;
            let pane_id = match response {
                ControlResponse::PaneSplit { pane_id } => pane_id,
                other => bail!("unexpected browser open response: {other:?}"),
            };
            let placeholder_surface_id =
                active_surface_for_pane(&query_model(&client).await?, workspace_id, pane_id)?;
            let surface_id = create_surface(
                &client,
                workspace_id,
                pane_id,
                PaneKind::Browser,
                Some(browser_profile_mode(ephemeral)),
                url.clone(),
            )
            .await?;
            send_control_command(
                &client,
                ControlCommand::CloseSurface {
                    workspace_id,
                    pane_id,
                    surface_id: placeholder_surface_id,
                },
            )
            .await?;
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "status": "browser_opened",
                    "workspace_id": workspace_id,
                    "pane_id": pane_id,
                    "surface_id": surface_id,
                    "url": url,
                    "profile_mode": browser_profile_mode(ephemeral),
                }))?
            );
        }
        BrowserCommand::Navigate { browser, url } => {
            let client = ControlClient::new(resolve_socket_path(browser.socket.clone()));
            let (_, _, surface_id) = resolve_browser_surface(&client, &browser).await?;
            let result =
                send_browser_command(&client, BrowserControlCommand::Navigate { surface_id, url })
                    .await?;
            print_browser_result(&result)?;
        }
        BrowserCommand::Back { browser } => {
            run_browser_surface_command(&browser, |surface_id| BrowserControlCommand::Back {
                surface_id,
            })
            .await?;
        }
        BrowserCommand::Forward { browser } => {
            run_browser_surface_command(&browser, |surface_id| BrowserControlCommand::Forward {
                surface_id,
            })
            .await?;
        }
        BrowserCommand::Reload { browser } => {
            run_browser_surface_command(&browser, |surface_id| BrowserControlCommand::Reload {
                surface_id,
            })
            .await?;
        }
        BrowserCommand::Snapshot { browser } => {
            run_browser_surface_command(&browser, |surface_id| BrowserControlCommand::Snapshot {
                surface_id,
            })
            .await?;
        }
        BrowserCommand::Eval { browser, script } => {
            run_browser_surface_command(&browser, |surface_id| BrowserControlCommand::Eval {
                surface_id,
                script,
            })
            .await?;
        }
        BrowserCommand::Wait {
            browser,
            selector,
            text,
            url_contains,
            load_state,
            script,
            delay_ms,
            timeout_ms,
            poll_interval_ms,
        } => {
            let condition =
                resolve_wait_condition(selector, text, url_contains, load_state, script, delay_ms)?;
            run_browser_surface_command(&browser, move |surface_id| BrowserControlCommand::Wait {
                surface_id,
                condition,
                timeout_ms,
                poll_interval_ms,
            })
            .await?;
        }
        BrowserCommand::Click {
            browser,
            target,
            snapshot_after,
        } => {
            let target = resolve_required_browser_target(target)?;
            run_browser_surface_command(&browser, move |surface_id| BrowserControlCommand::Click {
                surface_id,
                target,
                snapshot_after,
            })
            .await?;
        }
        BrowserCommand::Dblclick {
            browser,
            target,
            snapshot_after,
        } => {
            let target = resolve_required_browser_target(target)?;
            run_browser_surface_command(&browser, move |surface_id| {
                BrowserControlCommand::Dblclick {
                    surface_id,
                    target,
                    snapshot_after,
                }
            })
            .await?;
        }
        BrowserCommand::Type {
            browser,
            target,
            text,
            snapshot_after,
        } => {
            let target = resolve_required_browser_target(target)?;
            run_browser_surface_command(&browser, move |surface_id| BrowserControlCommand::Type {
                surface_id,
                target,
                text,
                snapshot_after,
            })
            .await?;
        }
        BrowserCommand::Fill {
            browser,
            target,
            text,
            snapshot_after,
        } => {
            let target = resolve_required_browser_target(target)?;
            run_browser_surface_command(&browser, move |surface_id| BrowserControlCommand::Fill {
                surface_id,
                target,
                text,
                snapshot_after,
            })
            .await?;
        }
        BrowserCommand::Press {
            browser,
            target,
            key,
            snapshot_after,
        } => {
            let target = resolve_optional_browser_target(target)?;
            run_browser_surface_command(&browser, move |surface_id| BrowserControlCommand::Press {
                surface_id,
                target,
                key,
                snapshot_after,
            })
            .await?;
        }
        BrowserCommand::Keydown {
            browser,
            target,
            key,
            snapshot_after,
        } => {
            let target = resolve_optional_browser_target(target)?;
            run_browser_surface_command(&browser, move |surface_id| {
                BrowserControlCommand::Keydown {
                    surface_id,
                    target,
                    key,
                    snapshot_after,
                }
            })
            .await?;
        }
        BrowserCommand::Keyup {
            browser,
            target,
            key,
            snapshot_after,
        } => {
            let target = resolve_optional_browser_target(target)?;
            run_browser_surface_command(&browser, move |surface_id| BrowserControlCommand::Keyup {
                surface_id,
                target,
                key,
                snapshot_after,
            })
            .await?;
        }
        BrowserCommand::Hover {
            browser,
            target,
            snapshot_after,
        } => {
            let target = resolve_required_browser_target(target)?;
            run_browser_surface_command(&browser, move |surface_id| BrowserControlCommand::Hover {
                surface_id,
                target,
                snapshot_after,
            })
            .await?;
        }
        BrowserCommand::Focus {
            browser,
            target,
            snapshot_after,
        } => {
            let target = resolve_required_browser_target(target)?;
            run_browser_surface_command(&browser, move |surface_id| BrowserControlCommand::Focus {
                surface_id,
                target,
                snapshot_after,
            })
            .await?;
        }
        BrowserCommand::Check {
            browser,
            target,
            snapshot_after,
        } => {
            let target = resolve_required_browser_target(target)?;
            run_browser_surface_command(&browser, move |surface_id| BrowserControlCommand::Check {
                surface_id,
                target,
                snapshot_after,
            })
            .await?;
        }
        BrowserCommand::Uncheck {
            browser,
            target,
            snapshot_after,
        } => {
            let target = resolve_required_browser_target(target)?;
            run_browser_surface_command(&browser, move |surface_id| {
                BrowserControlCommand::Uncheck {
                    surface_id,
                    target,
                    snapshot_after,
                }
            })
            .await?;
        }
        BrowserCommand::Select {
            browser,
            target,
            values,
            snapshot_after,
        } => {
            let target = resolve_required_browser_target(target)?;
            run_browser_surface_command(&browser, move |surface_id| {
                BrowserControlCommand::Select {
                    surface_id,
                    target,
                    values,
                    snapshot_after,
                }
            })
            .await?;
        }
        BrowserCommand::Scroll {
            browser,
            target,
            dx,
            dy,
            snapshot_after,
        } => {
            let target = resolve_optional_browser_target(target)?;
            run_browser_surface_command(&browser, move |surface_id| {
                BrowserControlCommand::Scroll {
                    surface_id,
                    target,
                    dx,
                    dy,
                    snapshot_after,
                }
            })
            .await?;
        }
        BrowserCommand::ScrollIntoView {
            browser,
            target,
            snapshot_after,
        } => {
            let target = resolve_required_browser_target(target)?;
            run_browser_surface_command(&browser, move |surface_id| {
                BrowserControlCommand::ScrollIntoView {
                    surface_id,
                    target,
                    snapshot_after,
                }
            })
            .await?;
        }
        BrowserCommand::Get { browser, command } => {
            let query = match command {
                BrowserGetSubcommand::Url => BrowserGetCommand::Url,
                BrowserGetSubcommand::Title => BrowserGetCommand::Title,
                BrowserGetSubcommand::Text { target } => BrowserGetCommand::Text {
                    target: resolve_required_browser_target(target)?,
                },
                BrowserGetSubcommand::Html { target } => BrowserGetCommand::Html {
                    target: resolve_required_browser_target(target)?,
                },
                BrowserGetSubcommand::Value { target } => BrowserGetCommand::Value {
                    target: resolve_required_browser_target(target)?,
                },
                BrowserGetSubcommand::Attr { target, name } => BrowserGetCommand::Attr {
                    target: resolve_required_browser_target(target)?,
                    name,
                },
                BrowserGetSubcommand::Count { selector } => BrowserGetCommand::Count { selector },
                BrowserGetSubcommand::Box { target } => BrowserGetCommand::Box {
                    target: resolve_required_browser_target(target)?,
                },
                BrowserGetSubcommand::Styles { target, properties } => BrowserGetCommand::Styles {
                    target: resolve_required_browser_target(target)?,
                    properties,
                },
            };
            run_browser_surface_command(&browser, move |surface_id| BrowserControlCommand::Get {
                surface_id,
                query,
            })
            .await?;
        }
        BrowserCommand::Is { browser, command } => {
            let query = match command {
                BrowserIsSubcommand::Visible { target } => BrowserPredicateCommand::Visible {
                    target: resolve_required_browser_target(target)?,
                },
                BrowserIsSubcommand::Enabled { target } => BrowserPredicateCommand::Enabled {
                    target: resolve_required_browser_target(target)?,
                },
                BrowserIsSubcommand::Checked { target } => BrowserPredicateCommand::Checked {
                    target: resolve_required_browser_target(target)?,
                },
            };
            run_browser_surface_command(&browser, move |surface_id| BrowserControlCommand::Is {
                surface_id,
                query,
            })
            .await?;
        }
        BrowserCommand::Screenshot { browser, out, full } => {
            run_browser_surface_command(&browser, move |surface_id| {
                BrowserControlCommand::Screenshot {
                    surface_id,
                    path: out,
                    full_document: full,
                }
            })
            .await?;
        }
        BrowserCommand::FocusWebview { browser } => {
            run_browser_surface_command(&browser, |surface_id| {
                BrowserControlCommand::FocusWebview { surface_id }
            })
            .await?;
        }
        BrowserCommand::IsWebviewFocused { browser } => {
            run_browser_surface_command(&browser, |surface_id| {
                BrowserControlCommand::IsWebviewFocused { surface_id }
            })
            .await?;
        }
        BrowserCommand::ClearData {
            browser,
            origin_filter,
        } => {
            run_browser_surface_command(&browser, move |surface_id| {
                BrowserControlCommand::ClearData {
                    surface_id,
                    origin_filter,
                    reload: true,
                }
            })
            .await?;
        }
    }

    Ok(())
}

async fn handle_screenshot_cli_command(screenshot: ScreenshotArgs) -> anyhow::Result<()> {
    let client = ControlClient::new(resolve_socket_path(screenshot.socket.clone()));
    let command = resolve_screenshot_command(&client, &screenshot).await?;
    let result = send_screenshot_command(&client, command).await?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

async fn resolve_screenshot_command(
    client: &ControlClient,
    screenshot: &ScreenshotArgs,
) -> anyhow::Result<ScreenshotCommand> {
    let model = query_model(client).await?;
    let target = match screenshot.target {
        CliScreenshotTarget::Surface => {
            let (_, _, surface_id) = resolve_terminal_surface(
                client,
                &TerminalSurfaceArgs {
                    socket: screenshot.socket.clone(),
                    workspace: screenshot.workspace,
                    pane: screenshot.pane,
                    surface: screenshot.surface,
                },
            )
            .await?;
            ScreenshotTarget::Surface { surface_id }
        }
        CliScreenshotTarget::Pane => {
            let workspace_id = resolve_workspace_id_from_model(&model, screenshot.workspace)?;
            let workspace = model
                .workspaces
                .get(&workspace_id)
                .ok_or_else(|| anyhow!("workspace {workspace_id} not found"))?;
            let pane_id = screenshot
                .pane
                .or_else(env_pane_id)
                .unwrap_or(workspace.active_pane);
            workspace.panes.get(&pane_id).ok_or_else(|| {
                anyhow!("pane {pane_id} is not present in workspace {workspace_id}")
            })?;
            ScreenshotTarget::Pane {
                workspace_id,
                pane_id,
            }
        }
        CliScreenshotTarget::WorkspaceWindow => {
            resolve_workspace_window_screenshot_target(&model, screenshot.workspace)?
        }
        CliScreenshotTarget::WorkspaceCanvas => {
            let workspace_id = resolve_workspace_id_from_model(&model, screenshot.workspace)?;
            model
                .workspaces
                .get(&workspace_id)
                .ok_or_else(|| anyhow!("workspace {workspace_id} not found"))?;
            ScreenshotTarget::WorkspaceCanvas { workspace_id }
        }
    };

    Ok(ScreenshotCommand::Capture {
        target,
        path: screenshot.out.clone(),
    })
}

async fn run_browser_surface_command<F>(
    browser: &BrowserSurfaceArgs,
    build: F,
) -> anyhow::Result<()>
where
    F: FnOnce(SurfaceId) -> BrowserControlCommand,
{
    let client = ControlClient::new(resolve_socket_path(browser.socket.clone()));
    let (_, _, surface_id) = resolve_browser_surface(&client, browser).await?;
    let result = send_browser_command(&client, build(surface_id)).await?;
    print_browser_result(&result)
}

async fn handle_terminal_debug_cli_command(command: TerminalDebugCliCommand) -> anyhow::Result<()> {
    match command {
        TerminalDebugCliCommand::IsFocused { terminal } => {
            run_terminal_surface_command(&terminal, |surface_id| TerminalDebugCommand::IsFocused {
                surface_id,
            })
            .await
        }
        TerminalDebugCliCommand::ReadText {
            terminal,
            tail_lines,
        } => {
            run_terminal_surface_command(&terminal, |surface_id| TerminalDebugCommand::ReadText {
                surface_id,
                tail_lines,
            })
            .await
        }
        TerminalDebugCliCommand::RenderStats { terminal } => {
            run_terminal_surface_command(&terminal, |surface_id| {
                TerminalDebugCommand::RenderStats { surface_id }
            })
            .await
        }
    }
}

async fn run_terminal_surface_command<F>(
    terminal: &TerminalSurfaceArgs,
    build: F,
) -> anyhow::Result<()>
where
    F: FnOnce(SurfaceId) -> TerminalDebugCommand,
{
    let client = ControlClient::new(resolve_socket_path(terminal.socket.clone()));
    let (_, _, surface_id) = resolve_terminal_surface(&client, terminal).await?;
    let result = send_terminal_debug_command(&client, build(surface_id)).await?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

async fn send_browser_command(
    client: &ControlClient,
    browser_command: BrowserControlCommand,
) -> anyhow::Result<serde_json::Value> {
    let response =
        send_control_command(client, ControlCommand::Browser { browser_command }).await?;
    match response {
        ControlResponse::Browser { result } => Ok(result),
        other => bail!("unexpected browser response: {other:?}"),
    }
}

fn print_browser_result(result: &serde_json::Value) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(result)?);
    Ok(())
}

async fn send_terminal_debug_command(
    client: &ControlClient,
    command: TerminalDebugCommand,
) -> anyhow::Result<serde_json::Value> {
    let response = send_control_command(
        client,
        ControlCommand::TerminalDebug {
            debug_command: command,
        },
    )
    .await?;
    match response {
        ControlResponse::TerminalDebug { result } => Ok(serde_json::to_value(result)?),
        other => bail!("unexpected terminal debug response: {other:?}"),
    }
}

async fn send_screenshot_command(
    client: &ControlClient,
    screenshot_command: ScreenshotCommand,
) -> anyhow::Result<serde_json::Value> {
    let response =
        send_control_command(client, ControlCommand::Screenshot { screenshot_command }).await?;
    match response {
        ControlResponse::Screenshot { result } => Ok(serde_json::to_value(result)?),
        other => bail!("unexpected screenshot response: {other:?}"),
    }
}

async fn resolve_browser_surface(
    client: &ControlClient,
    browser: &BrowserSurfaceArgs,
) -> anyhow::Result<(WorkspaceId, PaneId, SurfaceId)> {
    let model = query_model(client).await?;
    if let Some(surface_id) = browser.surface.or_else(env_surface_id) {
        let (workspace_id, pane_id, kind) = find_surface_location(&model, surface_id)
            .ok_or_else(|| anyhow!("surface {surface_id} is not present in the current session"))?;
        if kind != PaneKind::Browser {
            bail!("surface {surface_id} is not a browser");
        }
        if let Some(workspace_id_arg) = browser.workspace
            && workspace_id_arg != workspace_id
        {
            bail!(
                "surface {surface_id} belongs to workspace {workspace_id}, not {workspace_id_arg}"
            );
        }
        if let Some(pane_id_arg) = browser.pane
            && pane_id_arg != pane_id
        {
            bail!("surface {surface_id} belongs to pane {pane_id}, not {pane_id_arg}");
        }
        return Ok((workspace_id, pane_id, surface_id));
    }

    let workspace_id = resolve_workspace_id_from_model(&model, browser.workspace)?;
    let workspace = model
        .workspaces
        .get(&workspace_id)
        .ok_or_else(|| anyhow!("workspace {workspace_id} not found"))?;
    let pane_id = browser
        .pane
        .or_else(env_pane_id)
        .unwrap_or(workspace.active_pane);
    let pane = workspace
        .panes
        .get(&pane_id)
        .ok_or_else(|| anyhow!("pane {pane_id} is not present in workspace {workspace_id}"))?;
    let surface_id = pane.active_surface;
    let surface = pane
        .surfaces
        .get(&surface_id)
        .ok_or_else(|| anyhow!("surface {surface_id} is not present in pane {pane_id}"))?;
    if surface.kind != PaneKind::Browser {
        bail!(
            "active surface {surface_id} in pane {pane_id} is not a browser; pass --surface or activate a browser pane"
        );
    }
    Ok((workspace_id, pane_id, surface_id))
}

async fn resolve_terminal_surface(
    client: &ControlClient,
    terminal: &TerminalSurfaceArgs,
) -> anyhow::Result<(WorkspaceId, PaneId, SurfaceId)> {
    let model = query_model(client).await?;
    if let Some(surface_id) = terminal.surface.or_else(env_surface_id) {
        let (workspace_id, pane_id, kind) = find_surface_location(&model, surface_id)
            .ok_or_else(|| anyhow!("surface {surface_id} is not present in the current session"))?;
        if kind != PaneKind::Terminal {
            bail!("surface {surface_id} is not a terminal");
        }
        if let Some(workspace_id_arg) = terminal.workspace
            && workspace_id_arg != workspace_id
        {
            bail!(
                "surface {surface_id} belongs to workspace {workspace_id}, not {workspace_id_arg}"
            );
        }
        if let Some(pane_id_arg) = terminal.pane
            && pane_id_arg != pane_id
        {
            bail!("surface {surface_id} belongs to pane {pane_id}, not {pane_id_arg}");
        }
        return Ok((workspace_id, pane_id, surface_id));
    }

    let workspace_id = resolve_workspace_id_from_model(&model, terminal.workspace)?;
    let workspace = model
        .workspaces
        .get(&workspace_id)
        .ok_or_else(|| anyhow!("workspace {workspace_id} not found"))?;
    let pane_id = terminal
        .pane
        .or_else(env_pane_id)
        .unwrap_or(workspace.active_pane);
    let pane = workspace
        .panes
        .get(&pane_id)
        .ok_or_else(|| anyhow!("pane {pane_id} is not present in workspace {workspace_id}"))?;
    let surface_id = pane.active_surface;
    let surface = pane
        .surfaces
        .get(&surface_id)
        .ok_or_else(|| anyhow!("surface {surface_id} is not present in pane {pane_id}"))?;
    if surface.kind != PaneKind::Terminal {
        bail!(
            "active surface {surface_id} in pane {pane_id} is not a terminal; pass --surface or activate a terminal pane"
        );
    }
    Ok((workspace_id, pane_id, surface_id))
}

fn find_surface_location(
    model: &AppModel,
    surface_id: SurfaceId,
) -> Option<(WorkspaceId, PaneId, PaneKind)> {
    model
        .workspaces
        .iter()
        .find_map(|(workspace_id, workspace)| {
            workspace.panes.iter().find_map(|(pane_id, pane)| {
                pane.surfaces
                    .get(&surface_id)
                    .map(|surface| (*workspace_id, *pane_id, surface.kind.clone()))
            })
        })
}

fn resolve_required_browser_target(target: BrowserTargetArgs) -> anyhow::Result<BrowserTarget> {
    resolve_browser_target(target.reference, target.selector, true)
        .map(|target| target.expect("required browser target"))
}

fn resolve_optional_browser_target(
    target: BrowserOptionalTargetArgs,
) -> anyhow::Result<Option<BrowserTarget>> {
    resolve_browser_target(target.reference, target.selector, false)
}

fn resolve_browser_target(
    reference: Option<String>,
    selector: Option<String>,
    required: bool,
) -> anyhow::Result<Option<BrowserTarget>> {
    match (reference, selector) {
        (Some(reference), None) => Ok(Some(BrowserTarget::Ref { value: reference })),
        (None, Some(selector)) => Ok(Some(BrowserTarget::Selector { value: selector })),
        (None, None) if !required => Ok(None),
        (None, None) => bail!("missing browser target; pass --ref or --selector"),
        (Some(_), Some(_)) => bail!("pass only one of --ref or --selector"),
    }
}

fn resolve_wait_condition(
    selector: Option<String>,
    text: Option<String>,
    url_contains: Option<String>,
    load_state: Option<CliBrowserLoadState>,
    script: Option<String>,
    delay_ms: Option<u64>,
) -> anyhow::Result<BrowserWaitCondition> {
    let mut condition = None;
    let mut set = |next| -> anyhow::Result<()> {
        if condition.is_some() {
            bail!(
                "browser wait requires exactly one of --selector, --text, --url-contains, --load-state, --script, or --delay-ms"
            );
        }
        condition = Some(next);
        Ok(())
    };

    if let Some(selector) = selector {
        set(BrowserWaitCondition::Selector { selector })?;
    }
    if let Some(text) = text {
        set(BrowserWaitCondition::Text { text })?;
    }
    if let Some(pattern) = url_contains {
        set(BrowserWaitCondition::UrlMatches { pattern })?;
    }
    if let Some(state) = load_state {
        set(BrowserWaitCondition::LoadState {
            state: state.into(),
        })?;
    }
    if let Some(script) = script {
        set(BrowserWaitCondition::Function { script })?;
    }
    if let Some(duration_ms) = delay_ms {
        set(BrowserWaitCondition::Delay { duration_ms })?;
    }

    condition.context(
        "browser wait requires one of --selector, --text, --url-contains, --load-state, --script, or --delay-ms",
    )
}

#[allow(clippy::too_many_arguments)]
async fn emit_agent_hook(
    socket: Option<PathBuf>,
    workspace: Option<WorkspaceId>,
    pane: Option<PaneId>,
    surface: Option<SurfaceId>,
    agent: Option<String>,
    title: Option<String>,
    message: Option<String>,
    kind: CliSignalKind,
) -> anyhow::Result<()> {
    let workspace_id = workspace
        .or_else(env_workspace_id)
        .context("missing workspace id; pass --workspace or run from inside Taskers")?;
    let pane_id = pane
        .or_else(env_pane_id)
        .context("missing pane id; pass --pane or run from inside Taskers")?;
    let surface_id = surface.or_else(env_surface_id);
    let client = ControlClient::new(resolve_socket_path(socket));

    let normalized_agent = agent
        .or_else(|| title.as_deref().and_then(infer_agent_kind))
        .unwrap_or_else(|| "shell".into());
    let normalized_title = title.unwrap_or_else(|| normalized_agent.clone());
    let metadata = Some(taskers_domain::SignalPaneMetadata {
        title: None,
        agent_title: Some(normalized_title.clone()),
        cwd: None,
        repo_name: None,
        git_branch: None,
        ports: Vec::new(),
        agent_kind: Some(normalized_agent.clone()),
        agent_active: Some(matches!(
            kind,
            CliSignalKind::Started
                | CliSignalKind::Progress
                | CliSignalKind::WaitingInput
                | CliSignalKind::Notification
        )),
        agent_command: None,
    });
    let normalized_message = message
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let status_text = normalized_message
        .clone()
        .unwrap_or_else(|| normalized_title.clone());
    let signal_response = send_control_command(
        &client,
        ControlCommand::EmitSignal {
            workspace_id,
            pane_id,
            surface_id,
            event: SignalEvent {
                source: format!("agent-hook:{normalized_agent}"),
                kind: kind.into(),
                message,
                metadata,
                timestamp: OffsetDateTime::now_utc(),
            },
        },
    )
    .await?;

    let (resolved_workspace_id, resolved_pane_id, resolved_surface_id) = match surface_id {
        Some(surface_id) => {
            let (workspace_id, pane_id, surface_id) =
                resolve_surface_context(&client, surface_id).await?;
            (workspace_id, pane_id, Some(surface_id))
        }
        None => (workspace_id, pane_id, surface_id),
    };

    if let Some(log_message) = normalized_message.clone() {
        let _ = send_control_command(
            &client,
            ControlCommand::AgentAppendLog {
                workspace_id: resolved_workspace_id,
                entry: WorkspaceLogEntry {
                    source: Some(normalized_agent.clone()),
                    message: log_message,
                    created_at: OffsetDateTime::now_utc(),
                },
            },
        )
        .await?;
    }

    match kind {
        CliSignalKind::Started | CliSignalKind::Progress => {
            let _ = send_control_command(
                &client,
                ControlCommand::AgentSetStatus {
                    workspace_id: resolved_workspace_id,
                    text: status_text,
                },
            )
            .await?;
        }
        CliSignalKind::WaitingInput | CliSignalKind::Notification => {
            let _ = send_control_command(
                &client,
                ControlCommand::AgentSetStatus {
                    workspace_id: resolved_workspace_id,
                    text: status_text.clone(),
                },
            )
            .await?;
        }
        CliSignalKind::Completed | CliSignalKind::Error => {
            if matches!(kind, CliSignalKind::Completed) {
                let _ = send_control_command(
                    &client,
                    ControlCommand::AgentClearStatus {
                        workspace_id: resolved_workspace_id,
                    },
                )
                .await?;
                let _ = send_control_command(
                    &client,
                    ControlCommand::AgentClearProgress {
                        workspace_id: resolved_workspace_id,
                    },
                )
                .await?;
            } else {
                let _ = send_control_command(
                    &client,
                    ControlCommand::AgentSetStatus {
                        workspace_id: resolved_workspace_id,
                        text: status_text,
                    },
                )
                .await?;
                let _ = send_control_command(
                    &client,
                    ControlCommand::AgentClearProgress {
                        workspace_id: resolved_workspace_id,
                    },
                )
                .await?;
            }
        }
        CliSignalKind::Metadata => {}
    }

    if matches!(
        kind,
        CliSignalKind::WaitingInput | CliSignalKind::Notification | CliSignalKind::Error
    ) {
        let flash_surface_id = match resolved_surface_id.or_else(env_surface_id) {
            Some(surface_id) => Some(surface_id),
            None => {
                let model = query_model(&client).await?;
                Some(active_surface_for_pane(
                    &model,
                    resolved_workspace_id,
                    resolved_pane_id,
                )?)
            }
        };
        if let Some(surface_id) = flash_surface_id {
            let _ = send_control_command(
                &client,
                ControlCommand::AgentTriggerFlash {
                    workspace_id: resolved_workspace_id,
                    pane_id: resolved_pane_id,
                    surface_id,
                },
            )
            .await?;
        }
    }

    println!("{}", serde_json::to_string_pretty(&signal_response)?);
    Ok(())
}

fn infer_agent_kind(value: &str) -> Option<String> {
    let normalized = value.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "codex" => Some("codex".into()),
        "claude" | "claude code" | "claude-code" => Some("claude".into()),
        "opencode" => Some("opencode".into()),
        "aider" => Some("aider".into()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        path::PathBuf,
        sync::Mutex,
        time::{SystemTime, UNIX_EPOCH},
    };

    use clap::Parser;
    use taskers_control::{
        BrowserTarget, BrowserWaitCondition, ControlClient, ControlCommand, InMemoryController,
        ScreenshotCommand, ScreenshotTarget, bind_socket, serve,
    };
    use taskers_domain::{AppModel, BrowserProfileMode, PaneKind, SplitAxis, WorkspaceWindowId};
    use tokio::sync::oneshot;

    use super::{
        Cli, CliBrowserLoadState, CliScreenshotTarget, CliSignalKind, CompletionQueryArgs,
        CompletionShell, ScreenshotArgs, completion_query_candidates, emit_agent_hook,
        ensure_implicit_notify_target_context, env_pane_id, env_surface_id, env_workspace_id,
        infer_agent_kind, query_model, render_completion, resolve_browser_target,
        resolve_screenshot_command, resolve_wait_condition,
        resolve_workspace_window_screenshot_target, send_screenshot_command,
    };

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{unique}"))
    }

    #[test]
    fn infers_known_agent_names() {
        assert_eq!(infer_agent_kind("Codex"), Some("codex".into()));
        assert_eq!(infer_agent_kind("Claude Code"), Some("claude".into()));
        assert_eq!(infer_agent_kind("opencode"), Some("opencode".into()));
        assert_eq!(infer_agent_kind("unknown"), None);
    }

    #[test]
    fn parses_completion_subcommand() {
        let cli = Cli::try_parse_from(["taskersctl", "completion", "fish"])
            .expect("completion subcommand should parse");
        let debug = format!("{cli:?}");
        assert!(debug.contains("Completion"));
        assert!(debug.contains("Fish"));
    }

    #[test]
    fn generated_completion_scripts_include_public_commands_only() {
        for shell in [
            CompletionShell::Bash,
            CompletionShell::Fish,
            CompletionShell::Zsh,
        ] {
            let script = render_completion(shell);
            assert!(
                script.contains("browser"),
                "expected browser command in {shell:?} completion"
            );
            assert!(
                script.contains("workspace"),
                "expected workspace command in {shell:?} completion"
            );
            assert!(
                script.contains("--socket"),
                "expected socket flag in {shell:?} completion"
            );
            assert!(
                !script.contains(" session "),
                "hidden session command leaked into {shell:?} completion"
            );
        }
    }

    #[tokio::test]
    async fn completion_query_returns_static_flag_values() {
        let values = completion_query_candidates(&CompletionQueryArgs {
            path: Some("screenshot".into()),
            flag: Some("--target".into()),
            ..CompletionQueryArgs::default()
        })
        .await;

        assert_eq!(
            values,
            vec![
                "surface".to_string(),
                "pane".to_string(),
                "workspace_window".to_string(),
                "workspace_canvas".to_string()
            ]
        );
    }

    #[tokio::test]
    async fn completion_query_returns_static_positional_values() {
        let values = completion_query_candidates(&CompletionQueryArgs {
            path: Some("completion".into()),
            positional: Some(0),
            ..CompletionQueryArgs::default()
        })
        .await;

        assert_eq!(
            values,
            vec!["bash".to_string(), "fish".to_string(), "zsh".to_string()]
        );
    }

    #[tokio::test]
    async fn completion_query_returns_dynamic_taskers_ids() {
        let tempdir = unique_temp_dir("taskers-cli-completion-query");
        std::fs::create_dir_all(&tempdir).expect("tempdir");
        let socket_path = tempdir.join("taskers.sock");
        let listener = bind_socket(&socket_path).expect("listener");
        let controller = InMemoryController::new(AppModel::new("Main"));
        let snapshot = controller.snapshot();
        let workspace = snapshot.model.active_workspace().expect("workspace");
        let workspace_id = workspace.id;
        let active_pane_id = workspace.active_pane;
        let initial_surface_id = workspace
            .panes
            .get(&active_pane_id)
            .expect("pane")
            .active_surface;

        controller
            .handle(ControlCommand::SplitPane {
                workspace_id,
                pane_id: Some(active_pane_id),
                axis: SplitAxis::Horizontal,
            })
            .expect("split pane");
        let second_pane_id = controller
            .snapshot()
            .model
            .workspaces
            .get(&workspace_id)
            .and_then(|workspace| {
                workspace
                    .panes
                    .keys()
                    .copied()
                    .find(|pane_id| *pane_id != active_pane_id)
            })
            .expect("second pane");

        controller
            .handle(ControlCommand::CreateSurface {
                workspace_id,
                pane_id: active_pane_id,
                kind: PaneKind::Browser,
                browser_profile_mode: Some(BrowserProfileMode::PersistentDefault),
            })
            .expect("create surface");
        let browser_surface_id = controller
            .snapshot()
            .model
            .workspaces
            .get(&workspace_id)
            .and_then(|workspace| workspace.panes.get(&active_pane_id))
            .and_then(|pane| {
                pane.surfaces
                    .keys()
                    .copied()
                    .find(|surface_id| *surface_id != initial_surface_id)
            })
            .expect("browser surface");

        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        let server = tokio::spawn(async move {
            serve(listener, controller, async move {
                let _ = shutdown_rx.await;
            })
            .await
        });

        let workspace_values = completion_query_candidates(&CompletionQueryArgs {
            path: Some("browser click".into()),
            flag: Some("--workspace".into()),
            socket: Some(socket_path.clone()),
            ..CompletionQueryArgs::default()
        })
        .await;
        assert!(workspace_values.contains(&workspace_id.to_string()));

        let pane_values = completion_query_candidates(&CompletionQueryArgs {
            path: Some("browser click".into()),
            flag: Some("--pane".into()),
            socket: Some(socket_path.clone()),
            workspace: Some(workspace_id),
            ..CompletionQueryArgs::default()
        })
        .await;
        assert!(pane_values.contains(&active_pane_id.to_string()));
        assert!(pane_values.contains(&second_pane_id.to_string()));

        let surface_values = completion_query_candidates(&CompletionQueryArgs {
            path: Some("browser click".into()),
            flag: Some("--surface".into()),
            socket: Some(socket_path.clone()),
            workspace: Some(workspace_id),
            pane: Some(active_pane_id),
            ..CompletionQueryArgs::default()
        })
        .await;
        assert!(surface_values.contains(&initial_surface_id.to_string()));
        assert!(surface_values.contains(&browser_surface_id.to_string()));

        shutdown_tx.send(()).expect("shutdown");
        server.await.expect("server task").expect("serve cleanly");
        std::fs::remove_dir_all(&tempdir).expect("cleanup tempdir");
    }

    #[test]
    fn codex_notify_helper_requires_embedded_surface_context() {
        let asset = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/taskers-codex-notify.sh"
        ));

        for expected in [
            "TASKERS_WORKSPACE_ID",
            "TASKERS_PANE_ID",
            "TASKERS_SURFACE_ID",
            "TASKERS_TTY_NAME",
            "tty 2>/dev/null",
            "agent-hook stop",
            "--workspace \"$TASKERS_WORKSPACE_ID\"",
            "--pane \"$TASKERS_PANE_ID\"",
            "--surface \"$TASKERS_SURFACE_ID\"",
        ] {
            assert!(
                asset.contains(expected),
                "expected helper asset to contain {expected:?}"
            );
        }
    }

    #[test]
    fn reads_runtime_context_ids_from_env() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        unsafe {
            std::env::set_var(
                "TASKERS_WORKSPACE_ID",
                "019cede5-2843-7da1-a281-dd6b5d1cfbe6",
            );
            std::env::set_var("TASKERS_PANE_ID", "019cede5-2843-7da1-a281-dd4f2de73c9c");
            std::env::set_var("TASKERS_SURFACE_ID", "019cede5-2843-7da1-a281-dd2119ae9b83");
        }

        assert!(env_workspace_id().is_some());
        assert!(env_pane_id().is_some());
        assert!(env_surface_id().is_some());

        unsafe {
            std::env::remove_var("TASKERS_WORKSPACE_ID");
            std::env::remove_var("TASKERS_PANE_ID");
            std::env::remove_var("TASKERS_SURFACE_ID");
        }
    }

    #[test]
    fn implicit_notify_requires_embedded_taskers_context() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        unsafe {
            std::env::remove_var("TASKERS_WORKSPACE_ID");
            std::env::remove_var("TASKERS_PANE_ID");
            std::env::remove_var("TASKERS_SURFACE_ID");
        }

        assert!(ensure_implicit_notify_target_context(None, None, None).is_err());
        assert!(ensure_implicit_notify_target_context(env_workspace_id(), None, None).is_err());
    }

    #[test]
    fn implicit_notify_accepts_embedded_context_or_explicit_target() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        unsafe {
            std::env::set_var(
                "TASKERS_WORKSPACE_ID",
                "019cede5-2843-7da1-a281-dd6b5d1cfbe6",
            );
            std::env::set_var("TASKERS_PANE_ID", "019cede5-2843-7da1-a281-dd4f2de73c9c");
            std::env::set_var("TASKERS_SURFACE_ID", "019cede5-2843-7da1-a281-dd2119ae9b83");
            std::env::remove_var("TASKERS_TTY_NAME");
        }

        assert!(ensure_implicit_notify_target_context(None, None, None).is_ok());

        unsafe {
            std::env::remove_var("TASKERS_WORKSPACE_ID");
            std::env::remove_var("TASKERS_PANE_ID");
            std::env::remove_var("TASKERS_SURFACE_ID");
        }

        let workspace = "019cede5-2843-7da1-a281-dd6b5d1cfbe6"
            .parse()
            .expect("workspace id");
        assert!(ensure_implicit_notify_target_context(Some(workspace), None, None).is_ok());
    }

    #[test]
    fn runtime_context_ids_are_ignored_when_tty_mismatches() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        unsafe {
            std::env::set_var(
                "TASKERS_WORKSPACE_ID",
                "019cede5-2843-7da1-a281-dd6b5d1cfbe6",
            );
            std::env::set_var("TASKERS_PANE_ID", "019cede5-2843-7da1-a281-dd4f2de73c9c");
            std::env::set_var("TASKERS_SURFACE_ID", "019cede5-2843-7da1-a281-dd2119ae9b83");
            std::env::set_var("TASKERS_TTY_NAME", "/dev/pts/taskers-mismatch");
        }

        assert!(env_workspace_id().is_none());
        assert!(env_pane_id().is_none());
        assert!(env_surface_id().is_none());

        unsafe {
            std::env::remove_var("TASKERS_WORKSPACE_ID");
            std::env::remove_var("TASKERS_PANE_ID");
            std::env::remove_var("TASKERS_SURFACE_ID");
            std::env::remove_var("TASKERS_TTY_NAME");
        }
    }

    #[tokio::test]
    async fn agent_hook_status_and_logs_follow_surface_workspace_after_move() {
        let tempdir = unique_temp_dir("taskers-cli-agent-hook");
        std::fs::create_dir_all(&tempdir).expect("tempdir");
        let socket_path = tempdir.join("taskers.sock");
        let listener = bind_socket(&socket_path).expect("listener");
        let controller = InMemoryController::new(AppModel::new("Main"));
        let snapshot = controller.snapshot();
        let source_workspace = snapshot.model.active_workspace().expect("workspace");
        let source_workspace_id = source_workspace.id;
        let source_pane_id = source_workspace.active_pane;

        controller
            .handle(ControlCommand::CreateSurface {
                workspace_id: source_workspace_id,
                pane_id: source_pane_id,
                kind: PaneKind::Browser,
                browser_profile_mode: Some(BrowserProfileMode::PersistentDefault),
            })
            .expect("create surface");
        let moved_surface_id = controller
            .snapshot()
            .model
            .workspaces
            .get(&source_workspace_id)
            .and_then(|workspace| workspace.panes.get(&source_pane_id))
            .map(|pane| pane.active_surface)
            .expect("moved surface");

        controller
            .handle(ControlCommand::CreateWorkspace {
                label: "Docs".into(),
            })
            .expect("create target workspace");
        let target_workspace_id = controller
            .snapshot()
            .model
            .active_workspace_id()
            .expect("target workspace");

        controller
            .handle(ControlCommand::MoveSurfaceToWorkspace {
                source_workspace_id,
                source_pane_id,
                surface_id: moved_surface_id,
                target_workspace_id,
            })
            .expect("move surface");

        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        let server = tokio::spawn(serve(listener, controller.clone(), async move {
            let _ = shutdown_rx.await;
        }));

        emit_agent_hook(
            Some(socket_path.clone()),
            Some(source_workspace_id),
            Some(source_pane_id),
            Some(moved_surface_id),
            Some("codex".into()),
            Some("Codex".into()),
            Some("Turn complete".into()),
            CliSignalKind::Notification,
        )
        .await
        .expect("emit agent hook");

        let snapshot = controller.snapshot();
        let source_workspace_after = snapshot
            .model
            .workspaces
            .get(&source_workspace_id)
            .expect("source workspace");
        let target_workspace_after = snapshot
            .model
            .workspaces
            .get(&target_workspace_id)
            .expect("target workspace");

        assert_eq!(source_workspace_after.status_text, None);
        assert!(
            source_workspace_after.log_entries.is_empty(),
            "expected source workspace log to stay empty"
        );
        assert_eq!(
            target_workspace_after.status_text.as_deref(),
            Some("Turn complete")
        );
        assert_eq!(target_workspace_after.log_entries.len(), 1);
        assert_eq!(
            target_workspace_after.log_entries[0].message,
            "Turn complete"
        );

        let target_surface = target_workspace_after
            .panes
            .values()
            .flat_map(|pane| pane.surfaces.values())
            .find(|surface| surface.id == moved_surface_id)
            .expect("target surface");
        assert_eq!(
            target_surface.metadata.latest_agent_message.as_deref(),
            Some("Turn complete")
        );
        assert!(
            target_workspace_after
                .surface_flash_tokens
                .contains_key(&moved_surface_id),
            "expected flash token on moved target surface"
        );

        shutdown_tx.send(()).expect("shutdown");
        server.await.expect("server task").expect("serve cleanly");
        std::fs::remove_dir_all(&tempdir).expect("cleanup tempdir");
    }

    #[tokio::test]
    async fn screenshot_workspace_window_resolves_to_selected_workspace() {
        let tempdir = unique_temp_dir("taskers-cli-screenshot-window");
        std::fs::create_dir_all(&tempdir).expect("tempdir");
        let socket_path = tempdir.join("taskers.sock");
        let listener = bind_socket(&socket_path).expect("listener");
        let controller = InMemoryController::new(AppModel::new("Main"));
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        let server = tokio::spawn(async move {
            serve(listener, controller, async move {
                let _ = shutdown_rx.await;
            })
            .await
        });

        let client = ControlClient::new(socket_path.clone());
        let workspace_id = query_model(&client)
            .await
            .expect("model")
            .active_workspace_id()
            .expect("workspace");

        let command = resolve_screenshot_command(
            &client,
            &ScreenshotArgs {
                socket: Some(socket_path),
                target: CliScreenshotTarget::WorkspaceWindow,
                workspace: Some(workspace_id),
                pane: None,
                surface: None,
                out: Some(tempdir.join("window.png").display().to_string()),
            },
        )
        .await
        .expect("resolve screenshot");

        match command {
            ScreenshotCommand::Capture {
                target:
                    ScreenshotTarget::WorkspaceWindow {
                        workspace_id: resolved_workspace_id,
                    },
                ..
            } => assert_eq!(resolved_workspace_id, workspace_id),
            other => panic!("unexpected screenshot command: {other:?}"),
        }

        shutdown_tx.send(()).expect("shutdown");
        server.await.expect("server task").expect("serve cleanly");
        std::fs::remove_dir_all(&tempdir).expect("cleanup tempdir");
    }

    #[tokio::test]
    async fn screenshot_surface_rejects_non_terminal_surface() {
        let tempdir = unique_temp_dir("taskers-cli-screenshot-surface");
        std::fs::create_dir_all(&tempdir).expect("tempdir");
        let socket_path = tempdir.join("taskers.sock");
        let listener = bind_socket(&socket_path).expect("listener");
        let controller = InMemoryController::new(AppModel::new("Main"));
        let snapshot = controller.snapshot();
        let workspace = snapshot.model.active_workspace().expect("workspace");

        controller
            .handle(ControlCommand::CreateSurface {
                workspace_id: workspace.id,
                pane_id: workspace.active_pane,
                kind: PaneKind::Browser,
                browser_profile_mode: Some(BrowserProfileMode::PersistentDefault),
            })
            .expect("create browser surface");
        let browser_surface_id = controller
            .snapshot()
            .model
            .workspaces
            .get(&workspace.id)
            .and_then(|workspace| workspace.panes.get(&workspace.active_pane))
            .map(|pane| pane.active_surface)
            .expect("browser surface");

        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        let server = tokio::spawn(async move {
            serve(listener, controller, async move {
                let _ = shutdown_rx.await;
            })
            .await
        });

        let client = ControlClient::new(socket_path.clone());
        let error = resolve_screenshot_command(
            &client,
            &ScreenshotArgs {
                socket: Some(socket_path),
                target: CliScreenshotTarget::Surface,
                workspace: Some(workspace.id),
                pane: Some(workspace.active_pane),
                surface: Some(browser_surface_id),
                out: None,
            },
        )
        .await
        .expect_err("browser surface should not resolve as a terminal screenshot target");

        assert!(
            error.to_string().contains("not a terminal"),
            "unexpected error: {error}"
        );

        shutdown_tx.send(()).expect("shutdown");
        server.await.expect("server task").expect("serve cleanly");
        std::fs::remove_dir_all(&tempdir).expect("cleanup tempdir");
    }

    #[test]
    fn screenshot_workspace_window_errors_when_workspace_has_no_active_window() {
        let mut model = AppModel::new("Main");
        let workspace_id = model.active_workspace_id().expect("workspace");
        let workspace = model.workspaces.get_mut(&workspace_id).expect("workspace");
        workspace.active_window = WorkspaceWindowId::new();
        let error = resolve_workspace_window_screenshot_target(&model, Some(workspace_id))
            .expect_err("workspace without active window should fail");

        assert!(
            error.to_string().contains("no active workspace window"),
            "unexpected error: {error}"
        );
    }

    #[tokio::test]
    async fn screenshot_bridge_unavailable_does_not_create_output() {
        let tempdir = unique_temp_dir("taskers-cli-screenshot-unavailable");
        std::fs::create_dir_all(&tempdir).expect("tempdir");
        let socket_path = tempdir.join("missing.sock");
        let output_path = tempdir.join("missing.png");
        let client = ControlClient::new(socket_path);

        let error = send_screenshot_command(
            &client,
            ScreenshotCommand::Capture {
                target: ScreenshotTarget::WorkspaceCanvas {
                    workspace_id: taskers_domain::WorkspaceId::new(),
                },
                path: Some(output_path.display().to_string()),
            },
        )
        .await
        .expect_err("missing host bridge should fail");

        assert!(
            !output_path.exists(),
            "unexpected screenshot artifact at {}",
            output_path.display()
        );
        assert!(
            error.to_string().contains("No such file")
                || error.to_string().contains("os error")
                || error.to_string().contains("connect"),
            "unexpected error: {error}"
        );

        std::fs::remove_dir_all(&tempdir).expect("cleanup tempdir");
    }

    #[test]
    fn browser_targets_require_exactly_one_selector_or_ref() {
        let target = resolve_browser_target(Some("@e1".into()), None, true).expect("target");
        assert_eq!(
            target,
            Some(BrowserTarget::Ref {
                value: "@e1".into()
            })
        );
        assert!(resolve_browser_target(None, None, true).is_err());
        assert!(resolve_browser_target(Some("@e1".into()), Some("a".into()), true).is_err());
        assert_eq!(
            resolve_browser_target(None, None, false).expect("optional target"),
            None
        );
    }

    #[test]
    fn browser_wait_conditions_require_one_clause() {
        let wait = resolve_wait_condition(None, Some("hello".into()), None, None, None, None)
            .expect("wait");
        assert_eq!(
            wait,
            BrowserWaitCondition::Text {
                text: "hello".into()
            }
        );

        let wait = resolve_wait_condition(
            None,
            None,
            None,
            Some(CliBrowserLoadState::Committed),
            None,
            None,
        )
        .expect("load state");
        assert_eq!(
            wait,
            BrowserWaitCondition::LoadState {
                state: taskers_control::BrowserLoadState::Committed
            }
        );

        assert!(
            resolve_wait_condition(
                Some("body".into()),
                Some("hello".into()),
                None,
                None,
                None,
                None,
            )
            .is_err()
        );
        assert!(resolve_wait_condition(None, None, None, None, None, None).is_err());
    }
}
