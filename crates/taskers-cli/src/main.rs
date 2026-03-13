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
    AppModel, PaneId, PaneMetadataPatch, SignalEvent, SignalKind, SplitAxis, WorkspaceId,
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
        workspace: WorkspaceId,
        #[arg(long)]
        pane: PaneId,
        #[arg(long)]
        kind: CliSignalKind,
        #[arg(long)]
        message: Option<String>,
    },
    Workspace {
        #[command(subcommand)]
        command: WorkspaceCommand,
    },
    Pane {
        #[command(subcommand)]
        command: PaneCommand,
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

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliSignalKind {
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

impl From<CliSignalKind> for SignalKind {
    fn from(value: CliSignalKind) -> Self {
        match value {
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Install { skip_build } => {
            install_app(skip_build)?;
        }
        Command::Serve { socket, demo } => {
            let socket = socket.unwrap_or_else(default_socket_path);
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
            let client = ControlClient::new(socket.unwrap_or_else(default_socket_path));
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
            kind,
            message,
        } => {
            let client = ControlClient::new(socket.unwrap_or_else(default_socket_path));
            let response = client
                .send(ControlCommand::EmitSignal {
                    workspace_id: workspace,
                    pane_id: pane,
                    event: SignalEvent {
                        source: "taskers-cli".into(),
                        kind: kind.into(),
                        message,
                        timestamp: OffsetDateTime::now_utc(),
                    },
                })
                .await?;
            println!("{}", serde_json::to_string_pretty(&response)?);
        }
        Command::Workspace { command } => match command {
            WorkspaceCommand::New { socket, label } => {
                let client = ControlClient::new(socket.unwrap_or_else(default_socket_path));
                let response = client
                    .send(ControlCommand::CreateWorkspace { label })
                    .await?;
                println!("{}", serde_json::to_string_pretty(&response)?);
            }
            WorkspaceCommand::Switch { socket, workspace } => {
                let client = ControlClient::new(socket.unwrap_or_else(default_socket_path));
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
                let client = ControlClient::new(socket.unwrap_or_else(default_socket_path));
                let response = client
                    .send(ControlCommand::RenameWorkspace {
                        workspace_id: workspace,
                        label,
                    })
                    .await?;
                println!("{}", serde_json::to_string_pretty(&response)?);
            }
            WorkspaceCommand::Close { socket, workspace } => {
                let client = ControlClient::new(socket.unwrap_or_else(default_socket_path));
                let response = client
                    .send(ControlCommand::CloseWorkspace {
                        workspace_id: workspace,
                    })
                    .await?;
                println!("{}", serde_json::to_string_pretty(&response)?);
            }
        },
        Command::Pane { command } => match command {
            PaneCommand::Split {
                socket,
                workspace,
                pane,
                axis,
            } => {
                let client = ControlClient::new(socket.unwrap_or_else(default_socket_path));
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
                let client = ControlClient::new(socket.unwrap_or_else(default_socket_path));
                let response = client
                    .send(ControlCommand::FocusPane {
                        workspace_id: workspace,
                        pane_id: pane,
                    })
                    .await?;
                println!("{}", serde_json::to_string_pretty(&response)?);
            }
            PaneCommand::Close {
                socket,
                workspace,
                pane,
            } => {
                let client = ControlClient::new(socket.unwrap_or_else(default_socket_path));
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
                let client = ControlClient::new(socket.unwrap_or_else(default_socket_path));
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
    }

    Ok(())
}

fn install_app(skip_build: bool) -> anyhow::Result<()> {
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .context("failed to resolve workspace root")?;
    let cargo_bin_dir = cargo_bin_dir()?;
    let app_binary = cargo_bin_dir.join("taskers");

    if !skip_build {
        let status = ProcessCommand::new("cargo")
            .arg("install")
            .arg("--path")
            .arg("crates/taskers-app")
            .arg("--bin")
            .arg("taskers")
            .arg("--force")
            .arg("--locked")
            .current_dir(&workspace_root)
            .status()
            .context("failed to invoke cargo install for taskers")?;
        if !status.success() {
            anyhow::bail!("cargo install for taskers exited with status {status}");
        }
    }

    if !app_binary.exists() {
        anyhow::bail!(
            "expected installed binary at {}, but it was not found",
            app_binary.display()
        );
    }

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
        "/../../assets/taskers.desktop.in"
    ));
    let desktop_entry = desktop_template.replace("{{EXEC}}", &desktop_exec(&app_binary));
    let desktop_path = applications_dir.join("dev.taskers.app.desktop");
    std::fs::write(&desktop_path, desktop_entry)
        .with_context(|| format!("failed to write {}", desktop_path.display()))?;

    let icon_path = icons_dir.join("taskers.svg");
    std::fs::write(
        &icon_path,
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/taskers.svg"
        )),
    )
    .with_context(|| format!("failed to write {}", icon_path.display()))?;

    refresh_desktop_indexes(&applications_dir);

    println!("Installed taskers");
    println!("Binary: {}", app_binary.display());
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
