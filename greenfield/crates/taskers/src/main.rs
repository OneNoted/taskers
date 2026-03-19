use clap::{Parser, ValueEnum};
use dioxus::LaunchBuilder;
use dioxus_desktop::{
    Config, WindowBuilder,
    tao::{
        dpi::LogicalSize,
        event::{Event, WindowEvent},
    },
};
use gtk::glib;
use std::{
    collections::BTreeMap,
    fs::File,
    io::{self, Write},
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};
use taskers_core::{
    BootstrapModel, LayoutNodeSnapshot, PixelSize, RuntimeCapability, RuntimeStatus, SharedCore,
    SurfaceKind, TerminalDefaults,
};
use taskers_host::{DiagnosticCategory, DiagnosticRecord, DiagnosticsSink};
use taskers_paths::default_ghostty_runtime_dir;
use taskers_runtime::{ShellLaunchSpec, install_shell_integration, scrub_inherited_terminal_env};

#[derive(Debug, Clone, Parser)]
#[command(name = "taskers")]
#[command(about = "Greenfield Taskers desktop baseline")]
struct Cli {
    #[arg(long, value_enum)]
    smoke_script: Option<SmokeScript>,
    #[arg(long)]
    diagnostic_log: Option<String>,
    #[arg(long)]
    quit_after_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum SmokeScript {
    Baseline,
}

fn main() {
    let cli = Cli::parse();
    scrub_inherited_terminal_env();

    let diagnostics = DiagnosticsWriter::from_cli(&cli);
    let smoke_script = cli.smoke_script;
    let smoke_quit_after_ms = cli.quit_after_ms.unwrap_or(8_000);

    let (terminal_defaults, runtime_status) = bootstrap_runtime();
    log_runtime_status(diagnostics.as_ref(), &runtime_status);
    let core = SharedCore::bootstrap(BootstrapModel {
        runtime_status,
        terminal_defaults,
    });
    let core_for_window = core.clone();
    let core_for_events = core.clone();
    let diagnostics_for_window = diagnostics.clone();
    let diagnostics_for_events = diagnostics.clone();
    spawn_revision_sync_relay(core.clone(), diagnostics.clone());

    LaunchBuilder::desktop()
        .with_context(core.clone())
        .with_cfg(
            Config::new()
                .with_window(
                    WindowBuilder::new()
                        .with_title("Taskers")
                        .with_inner_size(LogicalSize::new(1440.0, 900.0)),
                )
                .with_on_window(move |window, _dom| {
                    let size = window.inner_size();
                    core_for_window
                        .set_window_size(PixelSize::new(size.width as i32, size.height as i32));

                    let event_sink = Arc::new({
                        let core = core_for_window.clone();
                        move |event| {
                            core.apply_host_event(event);
                        }
                    });
                    let diagnostics_sink = diagnostics_for_window.as_ref().map(DiagnosticsWriter::sink);

                    if let Err(error) =
                        taskers_host::attach_window(window.clone(), event_sink, diagnostics_sink)
                    {
                        log_diagnostic(
                            diagnostics_for_window.as_ref(),
                            DiagnosticRecord::new(
                                DiagnosticCategory::Window,
                                None,
                                format!("host attach failed: {error}"),
                            ),
                        );
                        eprintln!("taskers host attach failed: {error}");
                    }

                    let snapshot = core_for_window.snapshot();
                    log_diagnostic(
                        diagnostics_for_window.as_ref(),
                        DiagnosticRecord::new(
                            DiagnosticCategory::Window,
                            Some(snapshot.revision),
                            format!(
                                "initial snapshot panes={} active={}",
                                snapshot.portal.panes.len(),
                                snapshot.active_pane
                            ),
                        ),
                    );
                    if let Err(error) = taskers_host::sync_snapshot(&snapshot) {
                        log_diagnostic(
                            diagnostics_for_window.as_ref(),
                            DiagnosticRecord::new(
                                DiagnosticCategory::Sync,
                                Some(snapshot.revision),
                                format!("initial sync failed: {error}"),
                            ),
                        );
                        eprintln!("taskers host initial sync failed: {error}");
                    }

                    if let Some(script) = smoke_script {
                        spawn_smoke_script(
                            script,
                            core_for_window.clone(),
                            diagnostics_for_window.clone(),
                            smoke_quit_after_ms,
                        );
                    }
                })
                .with_custom_event_handler(move |event, _target| {
                    if let Event::WindowEvent {
                        event: WindowEvent::Resized(size),
                        ..
                    } = event
                    {
                        core_for_events
                            .set_window_size(PixelSize::new(size.width as i32, size.height as i32));
                        log_diagnostic(
                            diagnostics_for_events.as_ref(),
                            DiagnosticRecord::new(
                                DiagnosticCategory::Window,
                                Some(core_for_events.revision()),
                                format!(
                                    "window resized width={} height={}",
                                    size.width, size.height
                                ),
                            ),
                        );
                    }
                }),
        )
        .launch(taskers_shell::app);
}

fn bootstrap_runtime() -> (TerminalDefaults, RuntimeStatus) {
    let ghostty_runtime = probe_ghostty_runtime();

    let (shell_launch, shell_integration) = match install_shell_integration(None) {
        Ok(integration) => (integration.launch_spec(), RuntimeCapability::Ready),
        Err(error) => (
            ShellLaunchSpec::fallback(),
            RuntimeCapability::Fallback {
                message: format!("Shell integration unavailable: {error}"),
            },
        ),
    };

    (
        terminal_defaults_from(shell_launch),
        RuntimeStatus {
            ghostty_runtime,
            shell_integration,
            terminal_host: taskers_host::terminal_host_capability(),
        },
    )
}

fn probe_ghostty_runtime() -> RuntimeCapability {
    let runtime_dir = default_ghostty_runtime_dir();
    let bridge = runtime_dir.join("lib").join("libtaskers_ghostty_bridge.so");

    if bridge.exists() {
        RuntimeCapability::Ready
    } else {
        RuntimeCapability::Fallback {
            message: format!(
                "Ghostty runtime bootstrap is deferred in this checkpoint to avoid mixing GTK3 and GTK4 in one process. Expected runtime asset: {}",
                bridge.display()
            ),
        }
    }
}

fn terminal_defaults_from(shell_launch: ShellLaunchSpec) -> TerminalDefaults {
    let mut argv = Vec::with_capacity(shell_launch.args.len() + 1);
    argv.push(shell_launch.program.display().to_string());
    argv.extend(shell_launch.args);

    let mut env = BTreeMap::new();
    env.extend(shell_launch.env);

    TerminalDefaults {
        cols: 120,
        rows: 40,
        command_argv: argv,
        env,
    }
}

fn spawn_revision_sync_relay(core: SharedCore, diagnostics: Option<DiagnosticsWriter>) {
    let mut revisions = core.subscribe_revision_events();
    thread::spawn(move || loop {
        match revisions.blocking_recv() {
            Ok(revision) => {
                let snapshot = core.snapshot();
                let diagnostics = diagnostics.clone();
                glib::MainContext::default().invoke(move || {
                    log_diagnostic(
                        diagnostics.as_ref(),
                        DiagnosticRecord::new(
                            DiagnosticCategory::Sync,
                            Some(revision),
                            format!(
                                "syncing snapshot panes={} active={}",
                                snapshot.portal.panes.len(),
                                snapshot.active_pane
                            ),
                        ),
                    );
                    if let Err(error) = taskers_host::sync_snapshot(&snapshot) {
                        log_diagnostic(
                            diagnostics.as_ref(),
                            DiagnosticRecord::new(
                                DiagnosticCategory::Sync,
                                Some(revision),
                                format!("snapshot sync failed: {error}"),
                            ),
                        );
                        eprintln!("taskers host sync failed for revision {revision}: {error}");
                    }
                });
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                log_diagnostic(
                    diagnostics.as_ref(),
                    DiagnosticRecord::new(
                        DiagnosticCategory::Sync,
                        None,
                        format!("revision relay lagged; skipped {skipped} events"),
                    ),
                );
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
        }
    });
}

fn spawn_smoke_script(
    script: SmokeScript,
    core: SharedCore,
    diagnostics: Option<DiagnosticsWriter>,
    quit_after_ms: u64,
) {
    thread::spawn(move || {
        let started_at = Instant::now();
        match script {
            SmokeScript::Baseline => run_baseline_smoke(core.clone(), diagnostics.as_ref()),
        }

        let remaining = Duration::from_millis(quit_after_ms).saturating_sub(started_at.elapsed());
        if !remaining.is_zero() {
            thread::sleep(remaining);
        }

        log_diagnostic(
            diagnostics.as_ref(),
            DiagnosticRecord::new(
                DiagnosticCategory::Smoke,
                Some(core.revision()),
                format!("smoke script exiting after {}ms", quit_after_ms),
            ),
        );
        let _ = io::stderr().lock().flush();
        std::process::exit(0);
    });
}

fn run_baseline_smoke(core: SharedCore, diagnostics: Option<&DiagnosticsWriter>) {
    log_diagnostic(
        diagnostics,
        DiagnosticRecord::new(DiagnosticCategory::Smoke, Some(core.revision()), "baseline smoke started"),
    );

    thread::sleep(Duration::from_millis(300));
    core.split_with_browser();
    log_diagnostic(
        diagnostics,
        DiagnosticRecord::new(
            DiagnosticCategory::Smoke,
            Some(core.revision()),
            "split browser pane",
        ),
    );

    if let Some(title) = wait_for_browser_title(&core, Duration::from_secs(4)) {
        log_diagnostic(
            diagnostics,
            DiagnosticRecord::new(
                DiagnosticCategory::Smoke,
                Some(core.revision()),
                format!("browser metadata observed title={title}"),
            ),
        );
    } else {
        log_diagnostic(
            diagnostics,
            DiagnosticRecord::new(
                DiagnosticCategory::Smoke,
                Some(core.revision()),
                "browser metadata timed out",
            ),
        );
    }

    core.split_with_terminal();
    let snapshot = core.snapshot();
    let terminal_status = snapshot.runtime_status.terminal_host.label();
    let terminal_message = snapshot
        .runtime_status
        .terminal_host
        .message()
        .unwrap_or("no terminal host note");
    log_diagnostic(
        diagnostics,
        DiagnosticRecord::new(
            DiagnosticCategory::Smoke,
            Some(snapshot.revision),
            format!(
                "split terminal pane terminal_host={} message={terminal_message}",
                terminal_status
            ),
        ),
    );

    let (browser_count, terminal_count) = surface_counts(&snapshot.layout);
    log_diagnostic(
        diagnostics,
        DiagnosticRecord::new(
            DiagnosticCategory::Smoke,
            Some(snapshot.revision),
            format!(
                "final snapshot panes={} browsers={} terminals={} active={}",
                snapshot.portal.panes.len(),
                browser_count,
                terminal_count,
                snapshot.active_pane
            ),
        ),
    );
}

fn wait_for_browser_title(core: &SharedCore, timeout: Duration) -> Option<String> {
    let started_at = Instant::now();
    while started_at.elapsed() < timeout {
        let snapshot = core.snapshot();
        if let Some(title) = first_browser_title(&snapshot.layout)
            .filter(|title| title != "Browser")
        {
            return Some(title);
        }
        thread::sleep(Duration::from_millis(100));
    }
    None
}

fn first_browser_title(node: &LayoutNodeSnapshot) -> Option<String> {
    match node {
        LayoutNodeSnapshot::Pane(pane) if pane.surface.kind == SurfaceKind::Browser => {
            Some(pane.surface.title.clone())
        }
        LayoutNodeSnapshot::Pane(_) => None,
        LayoutNodeSnapshot::Split { first, second, .. } => {
            first_browser_title(first).or_else(|| first_browser_title(second))
        }
    }
}

fn surface_counts(node: &LayoutNodeSnapshot) -> (usize, usize) {
    match node {
        LayoutNodeSnapshot::Pane(pane) => match pane.surface.kind {
            SurfaceKind::Browser => (1, 0),
            SurfaceKind::Terminal => (0, 1),
        },
        LayoutNodeSnapshot::Split { first, second, .. } => {
            let (first_browser, first_terminal) = surface_counts(first);
            let (second_browser, second_terminal) = surface_counts(second);
            (
                first_browser + second_browser,
                first_terminal + second_terminal,
            )
        }
    }
}

fn log_runtime_status(diagnostics: Option<&DiagnosticsWriter>, status: &RuntimeStatus) {
    let summary = format!(
        "runtime status ghostty={} shell={} terminal={}",
        status.ghostty_runtime.label(),
        status.shell_integration.label(),
        status.terminal_host.label(),
    );
    log_diagnostic(
        diagnostics,
        DiagnosticRecord::new(DiagnosticCategory::Startup, None, summary),
    );
}

fn log_diagnostic(diagnostics: Option<&DiagnosticsWriter>, record: DiagnosticRecord) {
    if let Some(diagnostics) = diagnostics {
        diagnostics.write(record);
    }
}

#[derive(Clone)]
struct DiagnosticsWriter {
    target: DiagnosticsTarget,
}

#[derive(Clone)]
enum DiagnosticsTarget {
    Stderr,
    File(Arc<Mutex<File>>),
}

impl DiagnosticsWriter {
    fn from_cli(cli: &Cli) -> Option<Self> {
        let target = cli
            .diagnostic_log
            .clone()
            .or_else(|| {
                std::env::var("TASKERS_GREENFIELD_DIAGNOSTIC_LOG")
                    .ok()
                    .filter(|value| !value.is_empty())
            })
            .or_else(|| cli.smoke_script.map(|_| "stderr".into()))?;

        if target == "stderr" {
            return Some(Self {
                target: DiagnosticsTarget::Stderr,
            });
        }

        match File::create(PathBuf::from(&target)) {
            Ok(file) => Some(Self {
                target: DiagnosticsTarget::File(Arc::new(Mutex::new(file))),
            }),
            Err(error) => {
                eprintln!("taskers diagnostics log path failed: {error}");
                None
            }
        }
    }

    fn sink(&self) -> DiagnosticsSink {
        let diagnostics = self.clone();
        Arc::new(move |record| diagnostics.write(record))
    }

    fn write(&self, record: DiagnosticRecord) {
        let line = format!("{}\n", record.format_line());
        match &self.target {
            DiagnosticsTarget::Stderr => {
                let _ = io::stderr().lock().write_all(line.as_bytes());
            }
            DiagnosticsTarget::File(file) => {
                if let Ok(mut file) = file.lock() {
                    let _ = file.write_all(line.as_bytes());
                }
            }
        }
    }
}
