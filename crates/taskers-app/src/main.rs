use adw::prelude::*;
use anyhow::{Context, Result};
use axum::{Router, extract::ws::WebSocketUpgrade, response::Html, routing::get};
use clap::{Parser, ValueEnum};
use gtk::{EventControllerKey, gdk, glib};
use std::{
    cell::{Cell, RefCell},
    fs::{File, OpenOptions, remove_file},
    future::pending,
    io::{self, Write},
    net::TcpListener,
    path::PathBuf,
    process::{Command, Stdio},
    rc::Rc,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use taskers_core::{AppState, load_or_bootstrap};
use taskers_control::{bind_socket, default_socket_path, serve_with_handler};
use taskers_shell_core::{
    BootstrapModel, LayoutNodeSnapshot, PixelSize, RuntimeCapability, RuntimeStatus, SharedCore,
    ShellSection, ShortcutAction, ShortcutPreset, SurfaceKind,
};
use taskers_domain::AppModel;
use taskers_ghostty::{BackendChoice, GhosttyHost, GhosttyHostOptions, ensure_runtime_installed};
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
    #[arg(long, hide = true, value_enum)]
    internal_ghostty_probe: Option<GhosttyProbeMode>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum SmokeScript {
    Baseline,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum GhosttyProbeMode {
    Host,
    Surface,
}

impl GhosttyProbeMode {
    fn as_arg(self) -> &'static str {
        match self {
            Self::Host => "host",
            Self::Surface => "surface",
        }
    }
}

struct BootstrapContext {
    core: SharedCore,
    ghostty_host: Option<GhosttyHost>,
    startup_notes: Vec<String>,
}

struct RuntimeBootstrap {
    ghostty_runtime: RuntimeCapability,
    shell_integration: RuntimeCapability,
    shell_launch: ShellLaunchSpec,
    host_options: GhosttyHostOptions,
    socket_path: PathBuf,
    startup_notes: Vec<String>,
}

fn main() -> glib::ExitCode {
    let cli = Cli::parse();
    scrub_inherited_terminal_env();
    if let Some(mode) = cli.internal_ghostty_probe {
        return run_internal_ghostty_probe(mode);
    }

    let bootstrap = match bootstrap_runtime(None) {
        Ok(bootstrap) => bootstrap,
        Err(error) => {
            eprintln!("failed to bootstrap greenfield Taskers host: {error:?}");
            return glib::ExitCode::FAILURE;
        }
    };

    let app = adw::Application::builder().application_id(APP_ID).build();
    let bootstrap = Rc::new(RefCell::new(Some(bootstrap)));
    let hold_guard = Rc::new(RefCell::new(None));
    let bootstrap_for_startup = bootstrap.clone();
    let hold_guard_for_startup = hold_guard.clone();
    let cli_for_startup = cli.clone();
    app.connect_startup(move |app| {
        *hold_guard_for_startup.borrow_mut() = Some(app.hold());
        if let Some(bootstrap) = bootstrap_for_startup.borrow_mut().take() {
            build_ui(
                app,
                bootstrap,
                hold_guard_for_startup.clone(),
                cli_for_startup.clone(),
            );
        }
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
    bootstrap: BootstrapContext,
    hold_guard: Rc<RefCell<Option<gtk::gio::ApplicationHoldGuard>>>,
    cli: Cli,
) {
    if let Err(error) = build_ui_result(app, bootstrap, hold_guard, cli) {
        eprintln!("failed to launch greenfield Taskers host: {error:?}");
    }
}

fn build_ui_result(
    app: &adw::Application,
    bootstrap: BootstrapContext,
    hold_guard: Rc<RefCell<Option<gtk::gio::ApplicationHoldGuard>>>,
    cli: Cli,
) -> Result<()> {
    let diagnostics = DiagnosticsWriter::from_cli(&cli);
    log_runtime_status(
        diagnostics.as_ref(),
        &bootstrap.core.snapshot().runtime_status,
    );

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
    shell_view.set_can_target(true);
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
    connect_navigation_shortcuts(&window, &shell_view, &core);

    for note in bootstrap.startup_notes {
        log_diagnostic(
            diagnostics.as_ref(),
            DiagnosticRecord::new(DiagnosticCategory::Startup, None, note.clone()),
        );
        eprintln!("{note}");
    }
    log_diagnostic(
        diagnostics.as_ref(),
        DiagnosticRecord::new(
            DiagnosticCategory::Startup,
            Some(core.revision()),
            format!("shared shell listening on {shell_url}"),
        ),
    );
    eprintln!("shared shell listening on {shell_url}");

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

fn normalize_shortcut_modifiers(state: gdk::ModifierType) -> gdk::ModifierType {
    state
        & (gdk::ModifierType::CONTROL_MASK
            | gdk::ModifierType::SHIFT_MASK
            | gdk::ModifierType::ALT_MASK
            | gdk::ModifierType::META_MASK
            | gdk::ModifierType::SUPER_MASK
            | gdk::ModifierType::HYPER_MASK)
}

fn is_modifier_key(key: gdk::Key) -> bool {
    matches!(
        key,
        gdk::Key::Control_L
            | gdk::Key::Control_R
            | gdk::Key::Shift_L
            | gdk::Key::Shift_R
            | gdk::Key::Alt_L
            | gdk::Key::Alt_R
            | gdk::Key::Meta_L
            | gdk::Key::Meta_R
            | gdk::Key::Super_L
            | gdk::Key::Super_R
            | gdk::Key::Hyper_L
            | gdk::Key::Hyper_R
    )
}

fn shortcut_matches(
    preset: ShortcutPreset,
    action: ShortcutAction,
    key: gdk::Key,
    state: gdk::ModifierType,
) -> bool {
    action
        .accelerators(preset)
        .iter()
        .filter_map(|accelerator| gtk::accelerator_parse(*accelerator))
        .any(|(expected_key, expected_modifiers)| {
            key == expected_key && normalize_shortcut_modifiers(state) == expected_modifiers
        })
}

fn focus_active_browser_address(shell_view: &WebView) {
    shell_view.evaluate_javascript(
        "(() => {
            const address = document.querySelector('.browser-address');
            if (!(address instanceof HTMLInputElement)) return false;
            address.focus();
            address.select();
            return true;
        })();",
        None,
        None,
        None::<&gtk::gio::Cancellable>,
        |_| {},
    );
}

fn connect_navigation_shortcuts(
    window: &adw::ApplicationWindow,
    shell_view: &WebView,
    core: &SharedCore,
) {
    let controller = EventControllerKey::new();
    controller.set_propagation_phase(gtk::PropagationPhase::Capture);
    let shortcuts_core = core.clone();
    let shortcuts_shell = shell_view.clone();
    controller.connect_key_pressed(move |_, key, _, state| {
        if is_modifier_key(key) {
            return glib::Propagation::Proceed;
        }

        let preset = shortcuts_core.selected_shortcut_preset();

        if shortcut_matches(preset, ShortcutAction::FocusBrowserAddress, key, state) {
            let snapshot = shortcuts_core.snapshot();
            if snapshot.section == ShellSection::Workspace && snapshot.browser_chrome.is_some() {
                focus_active_browser_address(&shortcuts_shell);
                return glib::Propagation::Stop;
            }
            return glib::Propagation::Proceed;
        }

        for action in ShortcutAction::ALL {
            if action == ShortcutAction::FocusBrowserAddress {
                continue;
            }
            if shortcut_matches(preset, action, key, state)
                && shortcuts_core.dispatch_shortcut_action(action)
            {
                return glib::Propagation::Stop;
            }
        }

        glib::Propagation::Proceed
    });
    window.add_controller(controller);
}

fn bootstrap_runtime(diagnostics: Option<&DiagnosticsWriter>) -> Result<BootstrapContext> {
    let runtime = resolve_runtime_bootstrap();
    let mut startup_notes = runtime.startup_notes;
    let session_path = greenfield_session_path();
    let initial_model = load_or_bootstrap(&session_path, false).with_context(|| {
        format!(
            "failed to load or bootstrap greenfield session at {}",
            session_path.display()
        )
    })?;

    let (ghostty_host, backend_choice, terminal_host, terminal_note) =
        match probe_ghostty_backend_process(GhosttyProbeMode::Surface) {
            Ok(()) => match GhosttyHost::new_with_options(&runtime.host_options) {
                Ok(host) => {
                    let _ = host.tick();
                    (
                        Some(host),
                        BackendChoice::GhosttyEmbedded,
                        RuntimeCapability::Ready,
                        None,
                    )
                }
                Err(error) => (
                    None,
                    BackendChoice::Mock,
                    RuntimeCapability::Fallback {
                        message: format!("Ghostty host unavailable: {error}"),
                    },
                    Some(format!("Ghostty host unavailable after probe: {error}")),
                ),
            },
            Err(error) => (
                None,
                BackendChoice::Mock,
                RuntimeCapability::Fallback {
                    message: format!("Ghostty surface self-probe failed: {error}"),
                },
                Some(format!("Ghostty surface self-probe failed: {error}")),
            ),
        };

    if let Some(note) = terminal_note {
        startup_notes.push(note);
    }

    let runtime_status = RuntimeStatus {
        ghostty_runtime: runtime.ghostty_runtime,
        shell_integration: runtime.shell_integration,
        terminal_host,
    };
    let app_state = AppState::new(
        initial_model,
        session_path,
        backend_choice,
        runtime.shell_launch,
    )
    .context("failed to initialize greenfield app state")?;
    startup_notes.push(spawn_control_server(
        app_state.clone(),
        runtime.socket_path.clone(),
    ));
    let core = SharedCore::bootstrap(BootstrapModel {
        app_state,
        runtime_status,
        selected_theme_id: "dark".into(),
        selected_shortcut_preset: ShortcutPreset::PowerUser,
    });

    log_runtime_status(diagnostics, &core.snapshot().runtime_status);

    Ok(BootstrapContext {
        core,
        ghostty_host,
        startup_notes,
    })
}

fn resolve_runtime_bootstrap() -> RuntimeBootstrap {
    scrub_inherited_terminal_env();

    let mut startup_notes = Vec::new();
    let socket_path = default_socket_path();
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

    let (mut shell_launch, shell_integration) = match install_shell_integration(None) {
        Ok(integration) => (integration.launch_spec(), RuntimeCapability::Ready),
        Err(error) => (
            ShellLaunchSpec::fallback(),
            RuntimeCapability::Fallback {
                message: format!("Shell integration unavailable: {error}"),
            },
        ),
    };
    shell_launch
        .env
        .insert("TASKERS_SOCKET".into(), socket_path.display().to_string());

    let host_options = GhosttyHostOptions::from_shell_launch(&shell_launch);

    RuntimeBootstrap {
        ghostty_runtime,
        shell_integration,
        shell_launch,
        host_options,
        socket_path,
        startup_notes,
    }
}

fn greenfield_session_path() -> PathBuf {
    taskers_paths::TaskersPaths::detect()
        .state_dir()
        .join("greenfield-session.json")
}

fn greenfield_probe_session_path(mode: GhosttyProbeMode) -> PathBuf {
    std::env::temp_dir().join(format!(
        "taskers-greenfield-probe-{}-{}.json",
        mode.as_arg(),
        std::process::id()
    ))
}

fn run_internal_ghostty_probe(mode: GhosttyProbeMode) -> glib::ExitCode {
    let runtime = resolve_runtime_bootstrap();
    let host = match GhosttyHost::new_with_options(&runtime.host_options) {
        Ok(host) => {
            let _ = host.tick();
            host
        }
        Err(error) => {
            eprintln!(
                "ghostty {} self-probe failed during host init: {error}",
                mode.as_arg()
            );
            return glib::ExitCode::FAILURE;
        }
    };

    if matches!(mode, GhosttyProbeMode::Surface) {
        return run_internal_surface_probe(host, runtime.shell_launch, mode);
    }

    spin_probe_main_context(Duration::from_millis(350));
    glib::ExitCode::SUCCESS
}

fn run_internal_surface_probe(
    host: GhosttyHost,
    shell_launch: ShellLaunchSpec,
    mode: GhosttyProbeMode,
) -> glib::ExitCode {
    if !gtk::is_initialized_main_thread() {
        if let Err(error) = gtk::init() {
            eprintln!(
                "ghostty {} self-probe failed during gtk init: {error}",
                mode.as_arg()
            );
            return glib::ExitCode::FAILURE;
        }
    }

    let settings = WebKitSettings::builder()
        .enable_developer_extras(true)
        .build();
    let shell_view = WebView::builder()
        .hexpand(true)
        .vexpand(true)
        .focusable(true)
        .settings(&settings)
        .build();
    shell_view.set_can_target(true);
    shell_view.load_html(
        "<!DOCTYPE html><html><body></body></html>",
        Some("http://127.0.0.1/"),
    );

    let app_state = match AppState::new(
        AppModel::new("Ghostty Probe"),
        greenfield_probe_session_path(mode),
        BackendChoice::GhosttyEmbedded,
        shell_launch,
    ) {
        Ok(app_state) => app_state,
        Err(error) => {
            eprintln!(
                "ghostty {} self-probe failed during app state bootstrap: {error}",
                mode.as_arg()
            );
            return glib::ExitCode::FAILURE;
        }
    };

    let core = SharedCore::bootstrap(BootstrapModel {
        app_state,
        runtime_status: RuntimeStatus {
            ghostty_runtime: RuntimeCapability::Ready,
            shell_integration: RuntimeCapability::Ready,
            terminal_host: RuntimeCapability::Ready,
        },
        selected_theme_id: "dark".into(),
        selected_shortcut_preset: ShortcutPreset::PowerUser,
    });
    core.set_window_size(PixelSize::new(1200, 800));

    let event_sink = Rc::new(|_| {});
    let mut taskers_host = TaskersHost::new(&shell_view, Some(host), event_sink, None);
    let host_widget = taskers_host.widget();
    let window = gtk::Window::builder()
        .title("Taskers Ghostty Probe")
        .default_width(1200)
        .default_height(800)
        .child(&host_widget)
        .build();
    window.present();

    spin_probe_main_context(Duration::from_millis(80));
    if let Err(error) = taskers_host.sync_snapshot(&core.snapshot()) {
        eprintln!(
            "ghostty {} self-probe failed during snapshot sync: {error}",
            mode.as_arg()
        );
        return glib::ExitCode::FAILURE;
    }

    let deadline = Instant::now() + Duration::from_millis(350);
    let context = glib::MainContext::default();
    while Instant::now() < deadline {
        taskers_host.tick();
        while context.pending() {
            let _ = context.iteration(false);
        }
        thread::sleep(Duration::from_millis(16));
    }

    // The probe only needs to prove that an embedded surface can initialize
    // and stay alive briefly. Tearing the GTK/GL stack back down inside the
    // child has been the flaky part on Linux, so exit immediately on success
    // and let the parent make the real startup decision.
    let _ = window;
    std::process::exit(0);
}

fn spin_probe_main_context(duration: Duration) {
    let deadline = Instant::now() + duration;
    let context = glib::MainContext::default();
    while Instant::now() < deadline {
        while context.pending() {
            let _ = context.iteration(false);
        }
        thread::sleep(Duration::from_millis(16));
    }
}

fn probe_ghostty_backend_process(mode: GhosttyProbeMode) -> Result<()> {
    let current_exe = std::env::current_exe().context("failed to resolve current executable")?;
    let log_path = ghostty_probe_log_path(mode);
    let stdout = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&log_path)
        .with_context(|| format!("failed to open probe log {}", log_path.display()))?;
    let stderr = stdout
        .try_clone()
        .with_context(|| format!("failed to clone probe log {}", log_path.display()))?;

    let mut child = Command::new(current_exe)
        .arg("--internal-ghostty-probe")
        .arg(mode.as_arg())
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .context("failed to launch Ghostty self-probe")?;

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => {
                let _ = remove_file(&log_path);
                return Ok(());
            }
            Ok(Some(status)) => {
                anyhow::bail!(
                    "{}; probe log: {}",
                    describe_exit_status(status),
                    log_path.display()
                );
            }
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(50)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                anyhow::bail!(
                    "Ghostty self-probe timed out; probe log: {}",
                    log_path.display()
                );
            }
            Err(error) => {
                anyhow::bail!(
                    "failed to wait for Ghostty self-probe: {error}; probe log: {}",
                    log_path.display()
                );
            }
        }
    }
}

fn ghostty_probe_log_path(mode: GhosttyProbeMode) -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    std::env::temp_dir().join(format!(
        "taskers-ghostty-probe-{}-{}-{timestamp}.log",
        mode.as_arg(),
        std::process::id()
    ))
}

fn describe_exit_status(status: std::process::ExitStatus) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;

        if let Some(signal) = status.signal() {
            return format!("Ghostty self-probe crashed with signal {signal}");
        }
    }

    match status.code() {
        Some(code) => format!("Ghostty self-probe exited with status {code}"),
        None => "Ghostty self-probe exited unsuccessfully".into(),
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
    core.sync_external_changes();

    for command in core.drain_host_commands() {
        if let Err(error) = host.borrow_mut().handle_command(command) {
            log_diagnostic(
                diagnostics,
                DiagnosticRecord::new(
                    DiagnosticCategory::HostEvent,
                    Some(core.revision()),
                    format!("host command failed: {error}"),
                ),
            );
            eprintln!("taskers host command failed: {error}");
        }
    }

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
                    snapshot.current_workspace.active_pane
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

fn spawn_control_server(app_state: AppState, socket_path: PathBuf) -> String {
    if let Some(parent) = socket_path.parent()
        && let Err(error) = std::fs::create_dir_all(parent)
    {
        return format!(
            "Control server disabled: failed to prepare socket directory for {} ({error})",
            socket_path.display()
        );
    }

    let note = format!("Control server starting on {}", socket_path.display());
    thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        runtime.block_on(async move {
            match bind_socket(&socket_path) {
                Ok(listener) => {
                    let handler = move |command| {
                        app_state
                            .dispatch(command)
                            .map_err(|error| error.to_string())
                    };
                    if let Err(error) = serve_with_handler(listener, handler, pending::<()>()).await
                    {
                        eprintln!("control server error: {error}");
                    }
                }
                Err(error) => {
                    eprintln!(
                        "control server unavailable at {}: {error}",
                        socket_path.display()
                    );
                }
            }
        });
    });

    note
}

fn launch_liveview_server(core: SharedCore) -> Result<String> {
    let listener = TcpListener::bind("127.0.0.1:0").context("failed to bind loopback port")?;
    listener
        .set_nonblocking(true)
        .context("failed to set loopback listener nonblocking")?;
    let addr = listener
        .local_addr()
        .context("failed to read loopback addr")?;
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

    if let Some(metadata) = wait_for_browser_ready(&core, Duration::from_secs(4)) {
        log_diagnostic(
            diagnostics,
            DiagnosticRecord::new(
                DiagnosticCategory::Smoke,
                Some(core.revision()),
                format!("browser metadata observed {metadata}"),
            ),
        );
    } else {
        log_diagnostic(
            diagnostics,
            DiagnosticRecord::new(
                DiagnosticCategory::Smoke,
                Some(core.revision()),
                "browser surface did not appear before timeout",
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

    let (browser_count, terminal_count) = surface_counts(&snapshot.current_workspace.layout);
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
                snapshot.current_workspace.active_pane
            ),
        ),
    );
}

fn wait_for_browser_ready(core: &SharedCore, timeout: Duration) -> Option<String> {
    let started_at = Instant::now();
    while started_at.elapsed() < timeout {
        let snapshot = core.snapshot();
        if let Some(metadata) = first_browser_ready(&snapshot.current_workspace.layout) {
            return Some(metadata);
        }
        thread::sleep(Duration::from_millis(100));
    }
    None
}

fn first_browser_ready(node: &LayoutNodeSnapshot) -> Option<String> {
    match node {
        LayoutNodeSnapshot::Pane(pane) => pane
            .surfaces
            .iter()
            .find(|surface| surface.id == pane.active_surface)
            .or_else(|| pane.surfaces.first())
            .filter(|surface| surface.kind == SurfaceKind::Browser)
            .map(|surface| {
                if surface.title != "Browser" {
                    format!("title={}", surface.title)
                } else if let Some(url) = surface.url.as_deref() {
                    format!("url={url}")
                } else {
                    "surface-present".into()
                }
            }),
        LayoutNodeSnapshot::Split { first, second, .. } => {
            first_browser_ready(first).or_else(|| first_browser_ready(second))
        }
    }
}

fn surface_counts(node: &LayoutNodeSnapshot) -> (usize, usize) {
    match node {
        LayoutNodeSnapshot::Pane(pane) => pane.surfaces.iter().fold(
            (0usize, 0usize),
            |(browser_count, terminal_count), surface| match surface.kind {
                SurfaceKind::Browser => (browser_count + 1, terminal_count),
                SurfaceKind::Terminal => (browser_count, terminal_count + 1),
            },
        ),
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
