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
};
use taskers_core::{
    BootstrapModel, PixelSize, RuntimeCapability, RuntimeStatus, SharedCore, TerminalDefaults,
};
use taskers_host::{DiagnosticCategory, DiagnosticRecord, DiagnosticsSink};
use taskers_paths::default_ghostty_runtime_dir;
use taskers_runtime::{ShellLaunchSpec, install_shell_integration, scrub_inherited_terminal_env};

fn main() {
    scrub_inherited_terminal_env();

    let diagnostics = DiagnosticsWriter::from_env();
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
    fn from_env() -> Option<Self> {
        let value = std::env::var_os("TASKERS_GREENFIELD_DIAGNOSTIC_LOG")?;
        if value == "stderr" {
            return Some(Self {
                target: DiagnosticsTarget::Stderr,
            });
        }

        let path = PathBuf::from(value);
        let file = File::create(path).ok()?;
        Some(Self {
            target: DiagnosticsTarget::File(Arc::new(Mutex::new(file))),
        })
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
