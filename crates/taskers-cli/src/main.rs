use std::{
    env,
    future::pending,
    path::PathBuf,
};

use anyhow::{Context, anyhow, bail};
use clap::{Parser, Subcommand, ValueEnum};
use taskers_control::{
    ControlClient, ControlCommand, ControlQuery, ControlResponse, InMemoryController, bind_socket,
    default_socket_path, serve,
};
use taskers_domain::{
    AgentTarget, AppModel, AttentionState, Direction, KEYBOARD_RESIZE_STEP, PaneId, PaneKind,
    PaneMetadataPatch, ProgressState, SignalEvent, SignalKind, SplitAxis, SurfaceId,
    WorkspaceId, WorkspaceLogEntry,
};
use time::OffsetDateTime;

#[derive(Debug, Parser)]
#[command(name = "taskersctl")]
#[command(about = "Local control CLI for the taskers workspace app")]
struct Cli {
    #[command(subcommand)]
    command: Command,
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
        #[arg(long)]
        agent_active: Option<bool>,
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
        body: Option<String>,
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
    Pane {
        #[command(subcommand)]
        command: PaneCommand,
    },
    Surface {
        #[command(subcommand)]
        command: SurfaceCommand,
    },
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
    },
    Navigate {
        #[arg(long)]
        socket: Option<PathBuf>,
        #[arg(long)]
        workspace: Option<WorkspaceId>,
        #[arg(long)]
        pane: Option<PaneId>,
        #[arg(long)]
        surface: SurfaceId,
        #[arg(long)]
        url: String,
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
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
        Command::Query {
            query,
        } => match query {
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
        }
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
            body,
            agent: _agent,
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
                    title: Some(normalized_title.to_string()),
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
                            progress: ProgressState {
                                value,
                                label,
                            },
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
                            title,
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
                        .filter(|item| workspace_filter.is_none_or(|workspace_id| item.workspace_id == workspace_id))
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
        Command::Browser { command } => match command {
            BrowserCommand::Open {
                socket,
                workspace,
                pane,
                url,
            } => {
                let client = ControlClient::new(resolve_socket_path(socket));
                let model = query_model(&client).await?;
                let workspace_id = workspace
                    .or_else(env_workspace_id)
                    .or_else(|| model.active_workspace_id())
                    .context("missing workspace id; pass --workspace or run from inside Taskers")?;
                let target_pane = pane
                    .or_else(env_pane_id)
                    .or_else(|| {
                        model.workspaces
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
                let surface_id =
                    create_surface(&client, workspace_id, pane_id, PaneKind::Browser, url.clone())
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
                    }))?
                );
            }
            BrowserCommand::Navigate {
                socket,
                workspace,
                pane,
                surface,
                url,
            } => {
                let client = ControlClient::new(resolve_socket_path(socket));
                if let Some(pane_id) = pane {
                    let workspace_id = workspace
                        .or_else(env_workspace_id)
                        .context("missing workspace id; pass --workspace or run from inside Taskers")?;
                    let _ = send_control_command(
                        &client,
                        ControlCommand::FocusSurface {
                            workspace_id,
                            pane_id,
                            surface_id: surface,
                        },
                    )
                    .await;
                }
                let response = client
                    .send(ControlCommand::UpdateSurfaceMetadata {
                        surface_id: surface,
                        patch: PaneMetadataPatch {
                            title: None,
                            cwd: None,
                            url: Some(url),
                            repo_name: None,
                            git_branch: None,
                            ports: None,
                            agent_kind: None,
                        },
                    })
                    .await?;
                println!("{}", serde_json::to_string_pretty(&response)?);
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
            } => {
                if url.is_some() && kind != CliPaneKind::Browser {
                    bail!("--url requires --kind browser");
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
                    let surface_id =
                        create_surface(&client, workspace, pane_id, kind.into(), url.clone())
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
            } => {
                if url.is_some() && kind != CliPaneKind::Browser {
                    bail!("--url requires --kind browser");
                }

                let client = ControlClient::new(resolve_socket_path(socket));
                if kind == CliPaneKind::Terminal {
                    let response = client
                        .send(ControlCommand::CreateSurface {
                            workspace_id: workspace,
                            pane_id: pane,
                            kind: PaneKind::Terminal,
                        })
                        .await?;
                    println!("{}", serde_json::to_string_pretty(&response)?);
                } else {
                    let surface_id =
                        create_surface(&client, workspace, pane, kind.into(), url.clone()).await?;
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "status": "surface_created",
                            "workspace_id": workspace,
                            "pane_id": pane,
                            "surface_id": surface_id,
                            "kind": "browser",
                            "url": url,
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
    env::var("TASKERS_WORKSPACE_ID")
        .ok()
        .and_then(|value| value.parse().ok())
}

fn env_pane_id() -> Option<PaneId> {
    env::var("TASKERS_PANE_ID")
        .ok()
        .and_then(|value| value.parse().ok())
}

fn env_surface_id() -> Option<SurfaceId> {
    env::var("TASKERS_SURFACE_ID")
        .ok()
        .and_then(|value| value.parse().ok())
}

fn resolve_socket_path(socket: Option<PathBuf>) -> PathBuf {
    socket
        .or_else(|| env::var_os("TASKERS_SOCKET").map(PathBuf::from))
        .unwrap_or_else(default_socket_path)
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
    let pane_record = workspace_record
        .panes
        .get(&resolved_pane)
        .ok_or_else(|| anyhow!("pane {resolved_pane} is not present in workspace {workspace_id}"))?;
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
    url: Option<String>,
) -> anyhow::Result<SurfaceId> {
    let response = send_control_command(
        client,
        ControlCommand::CreateSurface {
            workspace_id,
            pane_id,
            kind,
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

    if let Some(log_message) = normalized_message.clone() {
        let _ = send_control_command(
            &client,
            ControlCommand::AgentAppendLog {
                workspace_id,
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
        CliSignalKind::Started
        | CliSignalKind::Progress
        | CliSignalKind::WaitingInput
        | CliSignalKind::Notification => {
            let _ = send_control_command(
                &client,
                ControlCommand::AgentSetStatus {
                    workspace_id,
                    text: status_text,
                },
            )
            .await?;
        }
        CliSignalKind::Completed => {
            let _ = send_control_command(
                &client,
                ControlCommand::AgentClearStatus { workspace_id },
            )
            .await?;
            let _ = send_control_command(
                &client,
                ControlCommand::AgentClearProgress { workspace_id },
            )
            .await?;
        }
        CliSignalKind::Metadata | CliSignalKind::Error => {}
    }

    if matches!(
        kind,
        CliSignalKind::WaitingInput | CliSignalKind::Notification | CliSignalKind::Error
    ) {
        let flash_surface_id = match surface_id.or_else(env_surface_id) {
            Some(surface_id) => Some(surface_id),
            None => {
                let model = query_model(&client).await?;
                Some(active_surface_for_pane(&model, workspace_id, pane_id)?)
            }
        };
        if let Some(surface_id) = flash_surface_id {
            let _ = send_control_command(
                &client,
                ControlCommand::AgentTriggerFlash {
                    workspace_id,
                    pane_id,
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
    use std::sync::Mutex;

    use super::{env_pane_id, env_surface_id, env_workspace_id, infer_agent_kind};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn infers_known_agent_names() {
        assert_eq!(infer_agent_kind("Codex"), Some("codex".into()));
        assert_eq!(infer_agent_kind("Claude Code"), Some("claude".into()));
        assert_eq!(infer_agent_kind("opencode"), Some("opencode".into()));
        assert_eq!(infer_agent_kind("unknown"), None);
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
}
