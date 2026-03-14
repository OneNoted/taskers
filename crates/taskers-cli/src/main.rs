use std::{
    env,
    future::pending,
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
};

use anyhow::Context;
use clap::{Parser, Subcommand, ValueEnum};
use taskers_control::{
    ControlClient, ControlCommand, ControlQuery, InMemoryController, bind_socket,
    default_socket_path, serve,
};
use taskers_domain::{
    AppModel, Direction, KEYBOARD_RESIZE_STEP, PaneId, PaneKind, PaneMetadataPatch, SignalEvent,
    SignalKind, SplitAxis, SurfaceId, WorkspaceId,
};
use time::OffsetDateTime;

#[derive(Debug, Parser)]
#[command(name = "taskers")]
#[command(about = "Local control CLI for the taskers workspace app")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Install {
        #[arg(long, default_value_t = false)]
        skip_build: bool,
    },
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Install { skip_build } => {
            install_app(skip_build)?;
        }
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
            } => {
                let client = ControlClient::new(resolve_socket_path(socket));
                let response = client
                    .send(ControlCommand::SplitPane {
                        workspace_id: workspace,
                        pane_id: pane,
                        axis: axis.into(),
                    })
                    .await?;
                println!("{}", serde_json::to_string_pretty(&response)?);
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
            } => {
                let client = ControlClient::new(resolve_socket_path(socket));
                let response = client
                    .send(ControlCommand::CreateSurface {
                        workspace_id: workspace,
                        pane_id: pane,
                        kind: PaneKind::Terminal,
                    })
                    .await?;
                println!("{}", serde_json::to_string_pretty(&response)?);
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

fn install_app(skip_build: bool) -> anyhow::Result<()> {
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .context("failed to resolve workspace root")?;
    let cargo_bin_dir = cargo_bin_dir()?;
    let app_binary = cargo_bin_dir.join("taskers");
    let cli_binary = cargo_bin_dir.join("taskersctl");

    if !skip_build {
        for (path, bin_name) in [
            ("crates/taskers-app", "taskers"),
            ("crates/taskers-cli", "taskersctl"),
        ] {
            let status = ProcessCommand::new("cargo")
                .arg("install")
                .arg("--path")
                .arg(path)
                .arg("--bin")
                .arg(bin_name)
                .arg("--force")
                .arg("--locked")
                .current_dir(&workspace_root)
                .status()
                .with_context(|| format!("failed to invoke cargo install for {bin_name}"))?;
            if !status.success() {
                anyhow::bail!("cargo install for {bin_name} exited with status {status}");
            }
        }
    }

    if !app_binary.exists() {
        anyhow::bail!(
            "expected installed binary at {}, but it was not found",
            app_binary.display()
        );
    }
    if !cli_binary.exists() {
        anyhow::bail!(
            "expected installed binary at {}, but it was not found",
            cli_binary.display()
        );
    }

    let launcher_binary = install_launcher_binary(&app_binary, "taskers")?;
    let control_binary = install_launcher_binary(&cli_binary, "taskersctl")?;
    let codex_notify_script = install_executable_asset(
        "taskers-codex-notify",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/taskers-codex-notify.sh"
        )),
    )?;

    let xdg_data_home = xdg_data_home()?;
    let applications_dir = xdg_data_home.join("applications");
    let icons_dir = xdg_data_home
        .join("icons")
        .join("hicolor")
        .join("scalable")
        .join("apps");
    let taskers_data_dir = xdg_data_home.join("taskers");
    let ghostty_bundle_dir = taskers_data_dir.join("ghostty");
    let terminfo_dir = xdg_data_home.join("terminfo");
    std::fs::create_dir_all(&applications_dir)?;
    std::fs::create_dir_all(&icons_dir)?;

    install_ghostty_runtime(
        &workspace_root,
        &ghostty_bundle_dir,
        &taskers_data_dir.join("locale"),
        &terminfo_dir,
    )?;

    let desktop_template = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/taskers.desktop.in"
    ));
    let desktop_entry = desktop_template.replace("{{EXEC}}", &desktop_exec(&launcher_binary));
    let desktop_path = applications_dir.join("dev.taskers.app.desktop");
    std::fs::write(&desktop_path, desktop_entry)
        .with_context(|| format!("failed to write {}", desktop_path.display()))?;

    let icon_path = icons_dir.join("taskers.svg");
    std::fs::write(
        &icon_path,
        include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/taskers.svg")),
    )
    .with_context(|| format!("failed to write {}", icon_path.display()))?;

    refresh_desktop_indexes(&applications_dir);

    println!("Installed taskers");
    println!("Binary: {}", app_binary.display());
    println!("Launcher binary: {}", launcher_binary.display());
    println!("Control binary: {}", control_binary.display());
    println!("Codex notify helper: {}", codex_notify_script.display());
    println!("Desktop entry: {}", desktop_path.display());
    println!("Icon: {}", icon_path.display());
    println!("Ghostty resources: {}", ghostty_bundle_dir.display());
    println!(
        "Ghostty bridge: {}",
        ghostty_bundle_dir
            .join("lib")
            .join("libtaskers_ghostty_bridge.so")
            .display()
    );
    println!(
        "Ghostty locale: {}",
        taskers_data_dir.join("locale").display()
    );

    Ok(())
}

fn cargo_bin_dir() -> anyhow::Result<PathBuf> {
    if let Some(path) = env::var_os("CARGO_HOME").map(PathBuf::from) {
        return Ok(path.join("bin"));
    }

    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is not set and CARGO_HOME is unavailable")?;
    Ok(home.join(".cargo").join("bin"))
}

fn xdg_bin_dir() -> anyhow::Result<PathBuf> {
    if let Some(path) = env::var_os("XDG_BIN_HOME").map(PathBuf::from) {
        return Ok(path);
    }

    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is not set and XDG_BIN_HOME is unavailable")?;
    Ok(home.join(".local").join("bin"))
}

fn xdg_data_home() -> anyhow::Result<PathBuf> {
    if let Some(path) = env::var_os("XDG_DATA_HOME").map(PathBuf::from) {
        return Ok(path);
    }

    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is not set and XDG_DATA_HOME is unavailable")?;
    Ok(home.join(".local").join("share"))
}

fn desktop_exec(path: &Path) -> String {
    let raw = path.display().to_string();
    raw.replace('\\', "\\\\").replace(' ', "\\ ")
}

fn install_launcher_binary(app_binary: &Path, target_name: &str) -> anyhow::Result<PathBuf> {
    let bin_dir = xdg_bin_dir()?;
    std::fs::create_dir_all(&bin_dir)
        .with_context(|| format!("failed to create {}", bin_dir.display()))?;
    let launcher_binary = bin_dir.join(target_name);

    if launcher_binary == app_binary {
        return Ok(launcher_binary);
    }

    if launcher_binary.symlink_metadata().is_ok() {
        std::fs::remove_file(&launcher_binary)
            .with_context(|| format!("failed to remove {}", launcher_binary.display()))?;
    }

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(app_binary, &launcher_binary).with_context(|| {
            format!(
                "failed to symlink {} -> {}",
                launcher_binary.display(),
                app_binary.display()
            )
        })?;
    }

    #[cfg(not(unix))]
    {
        std::fs::copy(app_binary, &launcher_binary).with_context(|| {
            format!(
                "failed to copy {} -> {}",
                app_binary.display(),
                launcher_binary.display()
            )
        })?;
    }

    Ok(launcher_binary)
}

fn install_executable_asset(target_name: &str, content: &str) -> anyhow::Result<PathBuf> {
    let bin_dir = xdg_bin_dir()?;
    std::fs::create_dir_all(&bin_dir)
        .with_context(|| format!("failed to create {}", bin_dir.display()))?;
    let target_path = bin_dir.join(target_name);
    std::fs::write(&target_path, content)
        .with_context(|| format!("failed to write {}", target_path.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = std::fs::metadata(&target_path)
            .with_context(|| format!("failed to stat {}", target_path.display()))?
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&target_path, permissions)
            .with_context(|| format!("failed to chmod {}", target_path.display()))?;
    }

    Ok(target_path)
}

fn refresh_desktop_indexes(applications_dir: &Path) {
    run_if_available("update-desktop-database", [applications_dir.as_os_str()]);
}

fn install_ghostty_runtime(
    workspace_root: &Path,
    resources_dir: &Path,
    locale_dir: &Path,
    terminfo_dir: &Path,
) -> anyhow::Result<()> {
    let staging_dir = workspace_root
        .join("target")
        .join("taskers-ghostty-install");
    let status = ProcessCommand::new("zig")
        .current_dir(workspace_root.join("vendor").join("ghostty"))
        .args([
            "build",
            "taskers-bridge",
            "-Dapp-runtime=gtk",
            "-Demit-exe=false",
            "-Dgtk-wayland=false",
            "-Dstrip=true",
            "-Di18n=false",
            "--summary",
            "none",
            "--prefix",
        ])
        .arg(&staging_dir)
        .status()
        .context("failed to invoke zig build for Ghostty runtime assets")?;
    if !status.success() {
        anyhow::bail!("zig build for Ghostty runtime assets exited with status {status}");
    }

    copy_directory(&staging_dir.join("share").join("ghostty"), resources_dir).with_context(
        || {
            format!(
                "failed to copy Ghostty resources to {}",
                resources_dir.display()
            )
        },
    )?;
    copy_directory(&staging_dir.join("lib"), &resources_dir.join("lib")).with_context(|| {
        format!(
            "failed to copy Ghostty bridge libraries to {}",
            resources_dir.join("lib").display()
        )
    })?;
    copy_directory(&staging_dir.join("share").join("locale"), locale_dir)
        .with_context(|| format!("failed to copy locale files to {}", locale_dir.display()))?;
    copy_directory(&staging_dir.join("share").join("terminfo"), terminfo_dir)
        .with_context(|| format!("failed to copy terminfo to {}", terminfo_dir.display()))?;
    let embedded_terminfo_dir = resources_dir
        .parent()
        .map(|path| path.join("terminfo"))
        .ok_or_else(|| anyhow::anyhow!("ghostty resources dir has no parent"))?;
    copy_directory(
        &staging_dir.join("share").join("terminfo"),
        &embedded_terminfo_dir,
    )
    .with_context(|| {
        format!(
            "failed to copy embedded terminfo to {}",
            embedded_terminfo_dir.display()
        )
    })?;
    std::fs::write(
        resources_dir.join(".taskers-runtime-version"),
        env!("CARGO_PKG_VERSION"),
    )
    .with_context(|| {
        format!(
            "failed to write Ghostty runtime version marker to {}",
            resources_dir.join(".taskers-runtime-version").display()
        )
    })?;

    Ok(())
}

fn copy_directory(source: &Path, destination: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(destination)?;

    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry.file_type()?;

        if file_type.is_dir() {
            copy_directory(&source_path, &destination_path)?;
        } else if file_type.is_symlink() {
            #[cfg(unix)]
            {
                if let Some(parent) = destination_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                if destination_path.exists() {
                    std::fs::remove_file(&destination_path)?;
                }
                let target = std::fs::read_link(&source_path)?;
                std::os::unix::fs::symlink(target, &destination_path)?;
            }
        } else if file_type.is_file() {
            if let Some(parent) = destination_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(&source_path, &destination_path)?;
        }
    }

    Ok(())
}

fn run_if_available<I, S>(program: &str, args: I)
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let Some(path) = env::var_os("PATH") else {
        return;
    };
    let Some(resolved) = env::split_paths(&path)
        .map(|entry| entry.join(program))
        .find(|candidate| candidate.exists())
    else {
        return;
    };

    let mut command = ProcessCommand::new(resolved);
    for arg in args {
        command.arg(arg);
    }
    let _ = command.status();
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
