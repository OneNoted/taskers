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
    AppModel, Direction, KEYBOARD_RESIZE_STEP, PaneId, PaneKind, PaneMetadataPatch, SignalEvent,
    SignalKind, SplitAxis, SurfaceId, WorkspaceId,
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
    Workspace {
        #[command(subcommand)]
        command: WorkspaceCommand,
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
}

#[derive(Debug, Subcommand)]
enum WorkspaceCommand {
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
            query: QueryCommand::Status { socket },
        } => {
            let client = ControlClient::new(resolve_socket_path(socket));
            let response = client
                .send(ControlCommand::QueryStatus {
                    query: ControlQuery::All,
                })
                .await?;
            println!("{}", serde_json::to_string_pretty(&response)?);
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
            agent,
        } => {
            let workspace_id = workspace
                .or_else(env_workspace_id)
                .context("missing workspace id; pass --workspace or run from inside Taskers")?;
            let pane_id = pane
                .or_else(env_pane_id)
                .context("missing pane id; pass --pane or run from inside Taskers")?;
            let surface_id = surface.or_else(env_surface_id);
            let client = ControlClient::new(resolve_socket_path(socket));
            let normalized_title = title.trim();
            let normalized_body = body
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned);
            let message = normalized_body.unwrap_or_else(|| normalized_title.to_string());
            let inferred_agent = agent.or_else(|| infer_agent_kind(normalized_title));
            let metadata = Some(taskers_domain::SignalPaneMetadata {
                title: Some(normalized_title.to_string()),
                cwd: None,
                repo_name: None,
                git_branch: None,
                ports: Vec::new(),
                agent_kind: inferred_agent,
                agent_active: None,
            });
            let response = client
                .send(ControlCommand::EmitSignal {
                    workspace_id,
                    pane_id,
                    surface_id,
                    event: SignalEvent {
                        source: format!("notify:{normalized_title}"),
                        kind: SignalKind::Notification,
                        message: Some(message),
                        metadata,
                        timestamp: OffsetDateTime::now_utc(),
                    },
                })
                .await?;
            println!("{}", serde_json::to_string_pretty(&response)?);
        }
        Command::Workspace { command } => match command {
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
