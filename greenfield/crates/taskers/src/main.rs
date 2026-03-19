use adw::prelude::*;
use anyhow::{Context, Result};
use axum::{Router, extract::ws::WebSocketUpgrade, response::Html, routing::get};
use clap::{Parser, ValueEnum};
use gtk::glib;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    fs::File,
    io::{self, Write},
    net::TcpListener,
    path::PathBuf,
    rc::Rc,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};
use taskers_core::{
    BootstrapModel, LayoutNodeSnapshot, PixelSize, RuntimeCapability, RuntimeStatus, SharedCore,
    SurfaceKind, TerminalDefaults,
};
use taskers_ghostty::{GhosttyHost, ensure_runtime_installed};
use taskers_host::{DiagnosticCategory, DiagnosticRecord, DiagnosticsSink, TaskersHost};
use taskers_runtime::{ShellLaunchSpec, install_shell_integration, scrub_inherited_terminal_env};
use webkit6::{Settings as WebKitSettings, WebView, prelude::*};

const APP_ID: &str = "dev.onenoted.Taskers.Greenfield";

#[derive(Debug, Clone, Parser)]
#[command(name = "taskers")]
#[command(about = "Greenfield Taskers unified shell")]
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

struct BootstrapContext {
    core: SharedCore,
    ghostty_host: Option<GhosttyHost>,
    startup_notes: Vec<String>,
}

fn main() -> glib::ExitCode {
    let cli = Cli::parse();
    let app = adw::Application::builder().application_id(APP_ID).build();
    let hold_guard = Rc::new(RefCell::new(None));
    let hold_guard_for_startup = hold_guard.clone();
    let cli_for_startup = cli.clone();
    app.connect_startup(move |app| {
        *hold_guard_for_startup.borrow_mut() = Some(app.hold());
        build_ui(app, hold_guard_for_startup.clone(), cli_for_startup.clone());
    });
    app.connect_activate(|app| {
        if let Some(window) = app.active_window() {
            window.present();
        }
    });
    app.run_with_args::<&str>(&[])
}

fn build_ui(
    app: &adw::Application,
    hold_guard: Rc<RefCell<Option<gtk::gio::ApplicationHoldGuard>>>,
    cli: Cli,
) {
    if let Err(error) = build_ui_result(app, hold_guard, cli) {
        eprintln!("failed to launch greenfield Taskers host: {error:?}");
    }
}

fn build_ui_result(
    app: &adw::Application,
    hold_guard: Rc<RefCell<Option<gtk::gio::ApplicationHoldGuard>>>,
    cli: Cli,
) -> Result<()> {
    let diagnostics = DiagnosticsWriter::from_cli(&cli);
    let bootstrap = bootstrap_runtime(diagnostics.as_ref());
    log_runtime_status(diagnostics.as_ref(), &bootstrap.core.snapshot().runtime_status);

    let shell_url = launch_liveview_server(bootstrap.core.clone())?;
    let settings = WebKitSettings::builder()
        .enable_developer_extras(true)
        .build();
    let shell_view = WebView::builder()
        .hexpand(true)
        .vexpand(true)
        .focusable(true)
        .settings(&settings)
        .build();
    shell_view.load_uri(&shell_url);

    let core = bootstrap.core.clone();
    let event_sink = Rc::new({
        let core = core.clone();
        move |event| core.apply_host_event(event)
    });
    let diagnostics_sink = diagnostics.as_ref().map(DiagnosticsWriter::sink);
    let host = Rc::new(RefCell::new(TaskersHost::new(
        &shell_view,
        bootstrap.ghostty_host,
        event_sink,
        diagnostics_sink,
    )));

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Taskers")
        .default_width(1440)
        .default_height(900)
        .build();
    window.connect_close_request(move |_| {
        drop(hold_guard.borrow_mut().take());
        glib::Propagation::Proceed
    });
    let host_widget = host.borrow().widget();
    window.set_content(Some(&host_widget));

    for note in bootstrap.startup_notes {
        log_diagnostic(
            diagnostics.as_ref(),
            DiagnosticRecord::new(DiagnosticCategory::Startup, None, note.clone()),
        );
        eprintln!("{note}");
    }

    let smoke_script = cli.smoke_script;
    let quit_after_ms = cli.quit_after_ms.unwrap_or(8_000);
    let last_revision = Rc::new(Cell::new(0_u64));
    let last_size = Rc::new(Cell::new((0_i32, 0_i32)));
    let tick_window = window.clone();
    let tick_core = core.clone();
    let tick_host = host.clone();
    let tick_revision = last_revision.clone();
    let tick_size = last_size.clone();
    let tick_diagnostics = diagnostics.clone();
    glib::timeout_add_local(Duration::from_millis(16), move || {
        sync_window(
            &tick_window,
            &tick_core,
            &tick_host,
            &tick_revision,
            &tick_size,
            tick_diagnostics.as_ref(),
        );
        glib::ControlFlow::Continue
    });

    window.present();

    let initial_window = window.clone();
    let initial_core = core.clone();
    let initial_host = host.clone();
    let initial_revision = last_revision.clone();
    let initial_size = last_size.clone();
    let initial_diagnostics = diagnostics.clone();
    glib::timeout_add_local_once(Duration::from_millis(80), move || {
        sync_window(
            &initial_window,
            &initial_core,
            &initial_host,
            &initial_revision,
            &initial_size,
            initial_diagnostics.as_ref(),
        );
    });

    if let Some(script) = smoke_script {
        spawn_smoke_script(script, core, diagnostics, quit_after_ms);
    }

    Ok(())
}

fn bootstrap_runtime(diagnostics: Option<&DiagnosticsWriter>) -> BootstrapContext {
    scrub_inherited_terminal_env();

    let mut startup_notes = Vec::new();
    let ghostty_runtime = match ensure_runtime_installed() {
        Ok(Some(runtime)) => {
            startup_notes.push(format!(
                "Installed Ghostty runtime assets to {}",
                runtime.runtime_dir.display()
            ));
            RuntimeCapability::Ready
        }
        Ok(None) => RuntimeCapability::Ready,
        Err(error) => RuntimeCapability::Fallback {
            message: format!("Ghostty runtime bootstrap unavailable: {error}"),
        },
    };

    let (shell_launch, shell_integration) = match install_shell_integration(None) {
        Ok(integration) => (integration.launch_spec(), RuntimeCapability::Ready),
        Err(error) => (
            ShellLaunchSpec::fallback(),
            RuntimeCapability::Fallback {
                message: format!("Shell integration unavailable: {error}"),
            },
        ),
    };

    let (ghostty_host, terminal_host, terminal_note) = match GhosttyHost::new() {
        Ok(host) => {
            let _ = host.tick();
            (Some(host), RuntimeCapability::Ready, None)
        }
        Err(error) => (
            None,
            RuntimeCapability::Fallback {
                message: format!("Ghostty host unavailable: {error}"),
            },
            Some(format!("Ghostty host unavailable: {error}")),
        ),
    };

    if let Some(note) = terminal_note {
        startup_notes.push(note);
    }

    let runtime_status = RuntimeStatus {
        ghostty_runtime,
        shell_integration,
        terminal_host,
    };
    let terminal_defaults = terminal_defaults_from(shell_launch);
    let core = SharedCore::bootstrap(BootstrapModel {
        runtime_status,
        terminal_defaults,
    });

    log_runtime_status(diagnostics, &core.snapshot().runtime_status);

    BootstrapContext {
        core,
        ghostty_host,
        startup_notes,
    }
}

fn terminal_defaults_from(shell_launch: ShellLaunchSpec) -> TerminalDefaults {
    let mut command_argv = Vec::with_capacity(shell_launch.args.len() + 1);
    command_argv.push(shell_launch.program.display().to_string());
    command_argv.extend(shell_launch.args);

    let mut env = BTreeMap::new();
    env.extend(shell_launch.env);

    TerminalDefaults {
        cols: 120,
        rows: 40,
        command_argv,
        env,
    }
}

fn sync_window(
    window: &adw::ApplicationWindow,
    core: &SharedCore,
    host: &Rc<RefCell<TaskersHost>>,
    last_revision: &Cell<u64>,
    last_size: &Cell<(i32, i32)>,
    diagnostics: Option<&DiagnosticsWriter>,
) {
    let size = PixelSize::new(window.width().max(1), window.height().max(1));
    if last_size.get() != (size.width, size.height) {
        core.set_window_size(size);
        last_size.set((size.width, size.height));
        log_diagnostic(
            diagnostics,
            DiagnosticRecord::new(
                DiagnosticCategory::Window,
                Some(core.revision()),
                format!("window resized width={} height={}", size.width, size.height),
            ),
        );
    }

    let revision = core.revision();
    if last_revision.get() != revision {
        let snapshot = core.snapshot();
        log_diagnostic(
            diagnostics,
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
        if let Err(error) = host.borrow_mut().sync_snapshot(&snapshot) {
            log_diagnostic(
                diagnostics,
                DiagnosticRecord::new(
                    DiagnosticCategory::Sync,
                    Some(revision),
                    format!("snapshot sync failed: {error}"),
                ),
            );
            eprintln!("taskers host sync failed for revision {revision}: {error}");
        }
        last_revision.set(revision);
    }

    host.borrow().tick();
}

fn launch_liveview_server(core: SharedCore) -> Result<String> {
    let listener = TcpListener::bind("127.0.0.1:0").context("failed to bind loopback port")?;
    listener
        .set_nonblocking(true)
        .context("failed to set loopback listener nonblocking")?;
    let addr = listener.local_addr().context("failed to read loopback addr")?;
    let url = format!("http://{addr}/");

    thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        runtime.block_on(async move {
            let listener = tokio::net::TcpListener::from_std(listener).expect("tokio listener");
            let view = dioxus_liveview::LiveViewPool::new();
            let router = Router::new()
                .route(
                    "/",
                    get(|| async move {
                        Html(format!(
                            r#"<!DOCTYPE html>
<html>
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>Taskers</title>
  </head>
  <body>
    <div id="main"></div>
  </body>
  {}
</html>"#,
                            dioxus_liveview::interpreter_glue("/ws")
                        ))
                    }),
                )
                .route(
                    "/ws",
                    get(move |ws: WebSocketUpgrade| {
                        let view = view.clone();
                        let core = core.clone();
                        async move {
                            ws.on_upgrade(move |socket| async move {
                                let _ = view
                                    .launch_with_props(
                                        dioxus_liveview::axum_socket(socket),
                                        taskers_shell::TaskersShell,
                                        taskers_shell::TaskersShellProps { core },
                                    )
                                    .await;
                            })
                        }
                    }),
                );

            if let Err(error) = axum::serve(listener, router.into_make_service()).await {
                eprintln!("liveview server failed: {error}");
            }
        });
    });

    Ok(url)
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
        DiagnosticRecord::new(
            DiagnosticCategory::Smoke,
            Some(core.revision()),
            "baseline smoke started",
        ),
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
        if let Some(title) = first_browser_title(&snapshot.layout).filter(|title| title != "Browser")
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
