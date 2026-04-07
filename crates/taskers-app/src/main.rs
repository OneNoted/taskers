#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
compile_error!(
    "taskers on crates.io currently supports x86_64 Linux only. Mainline macOS support is not shipped from this repo root."
);

use adw::prelude::*;
use anyhow::{Context, Result, bail};
use axum::{Router, extract::ws::WebSocketUpgrade, response::Html, routing::get};
use clap::{Parser, ValueEnum};
use gtk::{EventControllerKey, gdk, gio, glib};
use serde::{Deserialize, Serialize};
use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
    fs::{File, OpenOptions, create_dir_all, read_to_string, remove_file, write},
    future::pending,
    io::{self, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    rc::Rc,
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, Sender, TryRecvError},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use taskers_control::{
    BrowserControlCommand, ControlCommand, ControlError, ControlResponse, ScreenshotCommand,
    TerminalDebugCommand, bind_socket, default_socket_path, serve_with_handler,
};
use taskers_core::{AppState, default_session_path, load_or_bootstrap};
use taskers_domain::{AppModel, NotificationDeliveryState, NotificationId, SignalKind};
use taskers_ghostty::{
    BackendChoice, EmbeddedTerminalAppearance, GhosttyHost, GhosttyHostOptions,
    ensure_runtime_installed,
};
use taskers_host::{DiagnosticCategory, DiagnosticRecord, DiagnosticsSink, TaskersHost};
use taskers_runtime::{
    ShellLaunchSpec, TerminalSessionClient, install_shell_integration, scrub_inherited_terminal_env,
};
use taskers_shell_core::{
    BootstrapModel, LayoutNodeSnapshot, NotificationPreferencesSnapshot, PaneTabLayoutSnapshot,
    PixelSize, RuntimeCapability, RuntimeStatus, SharedCore, ShellAction, ShellSection,
    ShortcutAction, ShortcutPreset, SurfaceKind,
};
use webkit6::{Settings as WebKitSettings, WebView, prelude::*};

use glib::variant::ToVariant;
use taskers_paths::{TaskersPaths, default_terminal_socket_path};

const APP_ID: &str = taskers_paths::APP_ID;
const GHOSTTY_PROBE_WINDOW_SIZE_PX: i32 = 64;
const DEV_DIAGNOSTIC_LOG_NAME: &str = "taskers-gtk.latest.log";

#[derive(Debug, Clone, Parser)]
#[command(name = "taskers")]
#[command(about = "Taskers workspace shell")]
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
    app_state: AppState,
    socket_path: PathBuf,
    ghostty_host: Option<GhosttyHost>,
    config: TaskersConfig,
    startup_notes: Vec<String>,
}

struct RuntimeBootstrap {
    ghostty_runtime: RuntimeCapability,
    shell_integration: RuntimeCapability,
    terminal_persistence: RuntimeCapability,
    shell_launch: ShellLaunchSpec,
    host_options: GhosttyHostOptions,
    socket_path: PathBuf,
    terminal_session_client: Option<TerminalSessionClient>,
    startup_notes: Vec<String>,
}

#[derive(Debug, Clone)]
struct RuntimePathOverrides {
    root_dir: PathBuf,
    session_path: PathBuf,
    socket_path: PathBuf,
    terminal_socket_path: PathBuf,
}

fn should_skip_terminal_sidecar_in_smoke(path_overrides: Option<&RuntimePathOverrides>) -> bool {
    path_overrides.is_some()
        && matches!(
            std::env::var("TASKERS_TERMINAL_BACKEND").ok().as_deref(),
            Some("mock")
        )
}

enum HostAutomationCommand {
    Browser(BrowserControlCommand),
    Screenshot(ScreenshotCommand),
    TerminalDebug(TerminalDebugCommand),
}

struct HostAutomationRequest {
    command: HostAutomationCommand,
    response_tx: tokio::sync::oneshot::Sender<Result<ControlResponse, ControlError>>,
}

#[derive(Debug, Clone)]
struct PendingDesktopNotification {
    id: NotificationId,
    workspace_id: taskers_shell_core::WorkspaceId,
    pane_id: taskers_shell_core::PaneId,
    surface_id: taskers_shell_core::SurfaceId,
    title: String,
    body: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct TaskersConfig {
    #[serde(default = "default_theme_id")]
    selected_theme_id: String,
    #[serde(default = "default_shortcut_preset_id")]
    selected_shortcut_preset: String,
    #[serde(default)]
    notification_preferences: NotificationPreferencesConfig,
    #[serde(default = "default_true")]
    render_live_surfaces_in_overview: bool,
    #[serde(default)]
    embedded_terminal_appearance: EmbeddedTerminalAppearance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct NotificationPreferencesConfig {
    #[serde(default = "default_true")]
    alerts_on_waiting: bool,
    #[serde(default = "default_true")]
    alerts_on_error: bool,
    #[serde(default = "default_true")]
    alerts_on_completed: bool,
    #[serde(default = "default_true")]
    suppress_when_visible: bool,
}

impl Default for TaskersConfig {
    fn default() -> Self {
        Self {
            selected_theme_id: default_theme_id(),
            selected_shortcut_preset: default_shortcut_preset_id(),
            notification_preferences: NotificationPreferencesConfig::default(),
            render_live_surfaces_in_overview: true,
            embedded_terminal_appearance: EmbeddedTerminalAppearance::Taskers,
        }
    }
}

impl Default for NotificationPreferencesConfig {
    fn default() -> Self {
        Self {
            alerts_on_waiting: true,
            alerts_on_error: true,
            alerts_on_completed: true,
            suppress_when_visible: true,
        }
    }
}

impl NotificationPreferencesConfig {
    fn to_snapshot(self) -> NotificationPreferencesSnapshot {
        NotificationPreferencesSnapshot {
            alerts_on_waiting: self.alerts_on_waiting,
            alerts_on_error: self.alerts_on_error,
            alerts_on_completed: self.alerts_on_completed,
            suppress_when_visible: self.suppress_when_visible,
        }
    }

    fn from_snapshot(snapshot: NotificationPreferencesSnapshot) -> Self {
        Self {
            alerts_on_waiting: snapshot.alerts_on_waiting,
            alerts_on_error: snapshot.alerts_on_error,
            alerts_on_completed: snapshot.alerts_on_completed,
            suppress_when_visible: snapshot.suppress_when_visible,
        }
    }
}

fn default_theme_id() -> String {
    "dark".into()
}

fn default_shortcut_preset_id() -> String {
    ShortcutPreset::PowerUser.id().into()
}

fn default_true() -> bool {
    true
}

fn safe_eprintln(message: impl std::fmt::Display) {
    let mut stderr = io::stderr().lock();
    let _ = writeln!(stderr, "{message}");
}

fn push_startup_note(
    startup_notes: &mut Vec<String>,
    diagnostics: Option<&DiagnosticsWriter>,
    note: impl Into<String>,
) {
    let note = note.into();
    if let Some(diagnostics) = diagnostics {
        log_diagnostic(
            Some(diagnostics),
            DiagnosticRecord::new(DiagnosticCategory::Startup, None, note.clone()),
        );
    }
    startup_notes.push(note);
}

impl TaskersConfig {
    fn load() -> Result<Self> {
        let path = taskers_paths::default_config_path();
        let data = match read_to_string(&path) {
            Ok(data) => data,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("failed to read config {}", path.display()));
            }
        };
        let config: Self = serde_json::from_str(&data)
            .with_context(|| format!("failed to parse config {}", path.display()))?;
        Ok(config)
    }

    fn save(&self) -> Result<()> {
        let path = taskers_paths::default_config_path();
        if let Some(parent) = path.parent() {
            create_dir_all(parent).with_context(|| {
                format!("failed to create config directory {}", parent.display())
            })?;
        }
        let data = serde_json::to_string_pretty(self).context("failed to serialize config")?;
        write(&path, data).with_context(|| format!("failed to write config {}", path.display()))
    }

    fn shortcut_preset(&self) -> ShortcutPreset {
        ShortcutPreset::parse(&self.selected_shortcut_preset).unwrap_or(ShortcutPreset::PowerUser)
    }

    fn from_settings(settings: &taskers_shell_core::SettingsSnapshot, current: &Self) -> Self {
        Self {
            selected_theme_id: settings.selected_theme_id.clone(),
            selected_shortcut_preset: settings
                .shortcut_presets
                .iter()
                .find(|preset| preset.active)
                .map(|preset| preset.id.clone())
                .unwrap_or_else(default_shortcut_preset_id),
            notification_preferences: NotificationPreferencesConfig::from_snapshot(
                settings.notification_preferences,
            ),
            render_live_surfaces_in_overview: settings.render_live_surfaces_in_overview,
            embedded_terminal_appearance: current.embedded_terminal_appearance,
        }
    }
}

fn main() -> glib::ExitCode {
    let cli = Cli::parse();
    scrub_inherited_terminal_env();
    if let Some(mode) = cli.internal_ghostty_probe {
        return run_internal_ghostty_probe(mode);
    }

    let bootstrap = match bootstrap_runtime(None, cli.smoke_script) {
        Ok(bootstrap) => bootstrap,
        Err(error) => {
            safe_eprintln(format!("failed to bootstrap Taskers host: {error:?}"));
            return glib::ExitCode::FAILURE;
        }
    };

    let app = adw::Application::builder().application_id(APP_ID).build();
    let bootstrap = Rc::new(RefCell::new(Some(bootstrap)));
    let hold_guard = Rc::new(RefCell::new(None));
    // A unique GTK application can receive activate before its first window is
    // fully presentable. Track launch-in-progress explicitly so detached
    // desktop-entry startups do not get mistaken for stale no-window instances.
    let launch_in_progress = Rc::new(Cell::new(true));
    let bootstrap_for_startup = bootstrap.clone();
    let hold_guard_for_startup = hold_guard.clone();
    let launch_in_progress_for_startup = launch_in_progress.clone();
    let cli_for_startup = cli.clone();
    app.connect_startup(move |app| {
        ensure_application_hold(app, &hold_guard_for_startup);
        let Some(bootstrap) = bootstrap_for_startup.borrow_mut().take() else {
            launch_in_progress_for_startup.set(false);
            release_application_hold(&hold_guard_for_startup);
            app.quit();
            return;
        };

        if let Err(error) = build_ui_result(
            app,
            bootstrap,
            hold_guard_for_startup.clone(),
            cli_for_startup.clone(),
        ) {
            safe_eprintln(format!("failed to launch Taskers host: {error:?}"));
            launch_in_progress_for_startup.set(false);
            release_application_hold(&hold_guard_for_startup);
            app.quit();
            return;
        }

        let launch_in_progress = launch_in_progress_for_startup.clone();
        glib::idle_add_local_once(move || launch_in_progress.set(false));
    });
    app.connect_activate({
        let hold_guard = hold_guard.clone();
        let launch_in_progress = launch_in_progress.clone();
        move |app| {
            if present_existing_window(app) || launch_in_progress.get() {
                return;
            }

            release_application_hold(&hold_guard);
            app.quit();
        }
    });
    app.connect_window_removed({
        let hold_guard = hold_guard.clone();
        move |app, _| {
            if app.windows().is_empty() {
                release_application_hold(&hold_guard);
                app.quit();
            }
        }
    });
    app.run_with_args::<&str>(&[])
}

fn present_existing_window(app: &adw::Application) -> bool {
    let window = app
        .active_window()
        .or_else(|| app.windows().into_iter().next());
    if let Some(window) = window {
        window.present();
        true
    } else {
        false
    }
}

fn ensure_application_hold(
    app: &adw::Application,
    hold_guard: &Rc<RefCell<Option<gtk::gio::ApplicationHoldGuard>>>,
) {
    if let Ok(mut hold_guard) = hold_guard.try_borrow_mut()
        && hold_guard.is_none()
    {
        *hold_guard = Some(app.hold());
    }
}

fn release_application_hold(hold_guard: &Rc<RefCell<Option<gtk::gio::ApplicationHoldGuard>>>) {
    if let Ok(mut hold_guard) = hold_guard.try_borrow_mut() {
        drop(hold_guard.take());
    }
}

fn shutdown_host_bridge(host: &Rc<RefCell<TaskersHost>>, diagnostics: Option<&DiagnosticsWriter>) {
    if let Ok(mut host) = host.try_borrow_mut() {
        host.shutdown();
    }
    quiesce_host_bridge(host, diagnostics, Duration::from_millis(250));
    if let Ok(host) = host.try_borrow()
        && let Some(health) = host.bridge_health_snapshot()
    {
        log_diagnostic(
            diagnostics,
            DiagnosticRecord::new(
                DiagnosticCategory::Bridge,
                None,
                format!(
                    "ghostty shutdown summary state={} surface_count={}",
                    health.state.label(),
                    health.surface_count
                ),
            ),
        );
    }
}

fn quiesce_host_bridge(
    host: &Rc<RefCell<TaskersHost>>,
    diagnostics: Option<&DiagnosticsWriter>,
    timeout: Duration,
) {
    let deadline = Instant::now() + timeout;
    let context = glib::MainContext::default();
    loop {
        let Some(health) = host
            .try_borrow()
            .ok()
            .and_then(|host| host.bridge_health_snapshot())
        else {
            return;
        };
        if health.surface_count == 0 {
            log_diagnostic(
                diagnostics,
                DiagnosticRecord::new(
                    DiagnosticCategory::Bridge,
                    None,
                    "ghostty bridge quiesced surface_count=0",
                ),
            );
            return;
        }
        if Instant::now() >= deadline {
            log_diagnostic(
                diagnostics,
                DiagnosticRecord::new(
                    DiagnosticCategory::Bridge,
                    None,
                    format!(
                        "ghostty bridge quiesce timed out surface_count={}",
                        health.surface_count
                    ),
                ),
            );
            return;
        }

        let mut progressed = false;
        while context.pending() {
            progressed = true;
            let _ = context.iteration(false);
        }
        if !progressed {
            thread::sleep(Duration::from_millis(8));
        }
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
    let shell_action_sink = Rc::new({
        let core = core.clone();
        move |action| core.dispatch_shell_action(action)
    });
    let diagnostics_sink = diagnostics.as_ref().map(DiagnosticsWriter::sink);
    let host = Rc::new(RefCell::new(TaskersHost::new(
        &shell_view,
        bootstrap.ghostty_host,
        event_sink,
        shell_action_sink,
        diagnostics_sink,
    )));
    if let Some(bridge_info) = host.borrow().bridge_info() {
        let note = format!(
            "Ghostty bridge version={} build_id={}",
            bridge_info.version, bridge_info.build_id
        );
        log_diagnostic(
            diagnostics.as_ref(),
            DiagnosticRecord::new(
                DiagnosticCategory::Startup,
                Some(core.revision()),
                note.clone(),
            ),
        );
        safe_eprintln(note);
    }
    if let Some(health) = host.borrow().bridge_health_snapshot() {
        let note = format!(
            "Ghostty bridge lifecycle={} surface_count={}",
            health.state.label(),
            health.surface_count
        );
        log_diagnostic(
            diagnostics.as_ref(),
            DiagnosticRecord::new(
                DiagnosticCategory::Bridge,
                Some(core.revision()),
                note.clone(),
            ),
        );
        safe_eprintln(note);
    }
    let persisted_config = Rc::new(RefCell::new(bootstrap.config.clone()));

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Taskers")
        .default_width(1440)
        .default_height(900)
        .build();
    let shutdown_done = Rc::new(Cell::new(false));
    let app_for_close = app.clone();
    let host_for_close = host.clone();
    let hold_guard_for_close = hold_guard.clone();
    let close_diagnostics = diagnostics.clone();
    let shutdown_done_for_close = shutdown_done.clone();
    window.connect_close_request(move |_| {
        if !shutdown_done_for_close.replace(true) {
            shutdown_host_bridge(&host_for_close, close_diagnostics.as_ref());
        }
        release_application_hold(&hold_guard_for_close);
        app_for_close.quit();
        glib::Propagation::Proceed
    });
    let shutdown_done_for_app = shutdown_done.clone();
    let shutdown_host = host.clone();
    let shutdown_diagnostics = diagnostics.clone();
    app.connect_shutdown(move |_| {
        if !shutdown_done_for_app.replace(true) {
            shutdown_host_bridge(&shutdown_host, shutdown_diagnostics.as_ref());
        }
    });
    let host_widget = host.borrow().widget();
    window.set_content(Some(&host_widget));
    connect_navigation_shortcuts(&window, &shell_view, &core);
    install_notification_action(app, &window, &core, &bootstrap.app_state);
    let last_revision = Rc::new(Cell::new(0_u64));
    let last_size = Rc::new(Cell::new((0_i32, 0_i32)));
    let (host_request_tx, host_request_rx) = mpsc::channel::<HostAutomationRequest>();
    let control_server_note = spawn_control_server(
        bootstrap.app_state.clone(),
        bootstrap.socket_path,
        host_request_tx,
    );
    install_host_bridge(
        host_request_rx,
        &window,
        &core,
        &host,
        &last_revision,
        &last_size,
        diagnostics.clone(),
    );

    for note in bootstrap.startup_notes {
        log_diagnostic(
            diagnostics.as_ref(),
            DiagnosticRecord::new(DiagnosticCategory::Startup, None, note.clone()),
        );
        safe_eprintln(note);
    }
    log_diagnostic(
        diagnostics.as_ref(),
        DiagnosticRecord::new(
            DiagnosticCategory::Startup,
            Some(core.revision()),
            control_server_note.clone(),
        ),
    );
    safe_eprintln(control_server_note);
    log_diagnostic(
        diagnostics.as_ref(),
        DiagnosticRecord::new(
            DiagnosticCategory::Startup,
            Some(core.revision()),
            format!("shared shell listening on {shell_url}"),
        ),
    );
    safe_eprintln(format!("shared shell listening on {shell_url}"));

    let smoke_script = cli.smoke_script;
    let quit_after_ms = cli.quit_after_ms.unwrap_or(8_000);
    let tick_window = window.clone();
    let tick_core = core.clone();
    let tick_host = host.clone();
    let tick_revision = last_revision.clone();
    let tick_size = last_size.clone();
    let tick_diagnostics = diagnostics.clone();
    let tick_app = app.clone();
    let tick_app_state = bootstrap.app_state.clone();
    let tick_config = persisted_config.clone();
    glib::timeout_add_local(Duration::from_millis(16), move || {
        sync_window(
            &tick_window,
            &tick_core,
            &tick_host,
            &tick_revision,
            &tick_size,
            tick_diagnostics.as_ref(),
        );
        process_pending_notifications(
            &tick_app,
            &tick_window,
            &tick_core,
            &tick_app_state,
            tick_diagnostics.as_ref(),
        );
        persist_settings_if_needed(&tick_core, &tick_config, tick_diagnostics.as_ref());
        glib::ControlFlow::Continue
    });

    window.present();

    let initial_window = window.clone();
    let initial_core = core.clone();
    let initial_host = host.clone();
    let initial_revision = last_revision.clone();
    let initial_size = last_size.clone();
    let initial_diagnostics = diagnostics.clone();
    let initial_app = app.clone();
    let initial_app_state = bootstrap.app_state.clone();
    glib::timeout_add_local_once(Duration::from_millis(80), move || {
        sync_window(
            &initial_window,
            &initial_core,
            &initial_host,
            &initial_revision,
            &initial_size,
            initial_diagnostics.as_ref(),
        );
        process_pending_notifications(
            &initial_app,
            &initial_window,
            &initial_core,
            &initial_app_state,
            initial_diagnostics.as_ref(),
        );
    });

    if let Some(script) = smoke_script {
        let (smoke_quit_tx, smoke_quit_rx) = mpsc::channel::<()>();
        let smoke_app = app.clone();
        glib::timeout_add_local(Duration::from_millis(16), move || {
            match smoke_quit_rx.try_recv() {
                Ok(()) => {
                    smoke_app.quit();
                    glib::ControlFlow::Break
                }
                Err(TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(TryRecvError::Disconnected) => glib::ControlFlow::Break,
            }
        });
        spawn_smoke_script(script, core, diagnostics, quit_after_ms, smoke_quit_tx);
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

fn install_notification_action(
    app: &adw::Application,
    window: &adw::ApplicationWindow,
    core: &SharedCore,
    app_state: &AppState,
) {
    let action = gio::SimpleAction::new("open-notification", Some(&String::static_variant_type()));
    let action_window = window.clone();
    let action_core = core.clone();
    let action_state = app_state.clone();
    action.connect_activate(move |_, parameter| {
        let Some(notification_id) = parameter
            .and_then(|value| value.str())
            .and_then(|value| value.parse::<NotificationId>().ok())
        else {
            return;
        };

        let _ = action_state.dispatch(ControlCommand::OpenNotification {
            window_id: None,
            notification_id,
        });
        action_window.present();
        action_core.sync_external_changes();
    });
    app.add_action(&action);
}

fn process_pending_notifications(
    app: &adw::Application,
    window: &adw::ApplicationWindow,
    core: &SharedCore,
    app_state: &AppState,
    diagnostics: Option<&DiagnosticsWriter>,
) {
    let model = app_state.snapshot_model();
    let settings = core.snapshot().settings;
    let prefs = settings.notification_preferences;
    let pending = pending_desktop_notifications_with_prefs(&model, prefs);
    if pending.is_empty() {
        return;
    }

    for notification in pending {
        let delivery = if prefs.suppress_when_visible
            && notification_target_visible(&model, window, &notification)
        {
            NotificationDeliveryState::Suppressed
        } else {
            let desktop = gio::Notification::new(&notification.title);
            if let Some(body) = &notification.body {
                desktop.set_body(Some(body));
            }
            desktop.set_default_action_and_target_value(
                "app.open-notification",
                Some(&notification.id.to_string().to_variant()),
            );
            app.send_notification(Some(&notification.id.to_string()), &desktop);
            NotificationDeliveryState::Shown
        };

        if let Err(error) = app_state.dispatch(ControlCommand::MarkNotificationDelivery {
            notification_id: notification.id,
            delivery,
        }) {
            log_diagnostic(
                diagnostics,
                DiagnosticRecord::new(
                    DiagnosticCategory::Sync,
                    Some(core.revision()),
                    format!("notification delivery update failed: {error:?}"),
                )
                .with_pane(notification.pane_id)
                .with_surface(notification.surface_id),
            );
            safe_eprintln(format!(
                "taskers notification delivery update failed: {error:?}"
            ));
        }
    }

    core.sync_external_changes();
}

fn pending_desktop_notifications_with_prefs(
    model: &AppModel,
    prefs: NotificationPreferencesSnapshot,
) -> Vec<PendingDesktopNotification> {
    model
        .workspaces
        .values()
        .flat_map(|workspace| {
            workspace
                .notifications
                .iter()
                .filter_map(move |notification| {
                    if notification.cleared_at.is_some()
                        || !matches!(
                            notification.desktop_delivery,
                            NotificationDeliveryState::Pending
                        )
                        || !notification_alerts_enabled(&notification.kind, prefs)
                    {
                        return None;
                    }

                    let title = notification
                        .title
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(str::to_owned)
                        .unwrap_or_else(|| notification.message.clone());
                    Some(PendingDesktopNotification {
                        id: notification.id,
                        workspace_id: workspace.id,
                        pane_id: notification.pane_id,
                        surface_id: notification.surface_id,
                        title,
                        body: notification_body(
                            notification.subtitle.as_deref(),
                            &notification.message,
                        ),
                    })
                })
        })
        .collect()
}

fn notification_alerts_enabled(kind: &SignalKind, prefs: NotificationPreferencesSnapshot) -> bool {
    match kind {
        SignalKind::WaitingInput => prefs.alerts_on_waiting,
        SignalKind::Error => prefs.alerts_on_error,
        SignalKind::Completed => prefs.alerts_on_completed,
        SignalKind::Notification => true,
        SignalKind::Started | SignalKind::Progress | SignalKind::Metadata => false,
    }
}

fn notification_body(subtitle: Option<&str>, message: &str) -> Option<String> {
    let subtitle = subtitle
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let message = message.trim();

    match (subtitle, message.is_empty()) {
        (Some(subtitle), false) if subtitle != message => Some(format!("{subtitle}\n{message}")),
        (Some(subtitle), _) => Some(subtitle),
        (None, false) => Some(message.to_owned()),
        (None, true) => None,
    }
}

fn notification_target_visible(
    model: &AppModel,
    window: &adw::ApplicationWindow,
    notification: &PendingDesktopNotification,
) -> bool {
    if !window.is_active() || model.active_workspace_id() != Some(notification.workspace_id) {
        return false;
    }

    model
        .workspaces
        .get(&notification.workspace_id)
        .and_then(|workspace| {
            if workspace.active_pane != notification.pane_id {
                return None;
            }
            workspace.panes.get(&notification.pane_id)
        })
        .is_some_and(|pane| pane.active_surface == notification.surface_id)
}

fn persist_settings_if_needed(
    core: &SharedCore,
    persisted_config: &Rc<RefCell<TaskersConfig>>,
    diagnostics: Option<&DiagnosticsWriter>,
) {
    let snapshot = core.snapshot();
    let current = persisted_config.borrow().clone();
    let next = TaskersConfig::from_settings(&snapshot.settings, &current);
    if current == next {
        return;
    }

    if let Err(error) = next.save() {
        log_diagnostic(
            diagnostics,
            DiagnosticRecord::new(
                DiagnosticCategory::Startup,
                Some(core.revision()),
                format!("failed to persist config: {error:?}"),
            ),
        );
        safe_eprintln(format!("taskers config save failed: {error:?}"));
        return;
    }

    *persisted_config.borrow_mut() = next;
}

#[cfg(test)]
mod notification_tests {
    use super::{
        notification_alerts_enabled, notification_body, pending_desktop_notifications_with_prefs,
    };
    use taskers_domain::{
        AppModel, AttentionState, NotificationDeliveryState, NotificationId, NotificationItem,
        SignalKind,
    };
    use taskers_shell_core::NotificationPreferencesSnapshot;
    use time::OffsetDateTime;

    #[test]
    fn desktop_alert_filter_includes_user_facing_notification_kinds() {
        let prefs = NotificationPreferencesSnapshot::default();
        assert!(notification_alerts_enabled(
            &SignalKind::WaitingInput,
            prefs
        ));
        assert!(notification_alerts_enabled(&SignalKind::Error, prefs));
        assert!(notification_alerts_enabled(&SignalKind::Completed, prefs));
        assert!(notification_alerts_enabled(
            &SignalKind::Notification,
            prefs
        ));
        assert!(!notification_alerts_enabled(&SignalKind::Started, prefs));
        assert!(!notification_alerts_enabled(&SignalKind::Progress, prefs));
    }

    #[test]
    fn notification_body_prefers_subtitle_then_message() {
        assert_eq!(
            notification_body(Some("Codex"), "Finished"),
            Some("Codex\nFinished".into())
        );
        assert_eq!(
            notification_body(Some("Finished"), "Finished"),
            Some("Finished".into())
        );
        assert_eq!(notification_body(None, "Done"), Some("Done".into()));
        assert_eq!(notification_body(None, "   "), None);
    }

    #[test]
    fn pending_desktop_notifications_only_returns_pending_active_items() {
        let mut model = AppModel::new("Main");
        let workspace = model.active_workspace_id().expect("workspace");
        let pane_id = model.active_workspace().expect("workspace").active_pane;
        let surface_id = model
            .active_workspace()
            .and_then(|workspace| workspace.panes.get(&pane_id))
            .and_then(|pane| pane.active_surface())
            .map(|surface| surface.id)
            .expect("surface");
        let now = OffsetDateTime::now_utc();
        let workspace = model.workspaces.get_mut(&workspace).expect("workspace");
        workspace.notifications.push(NotificationItem {
            id: NotificationId::new(),
            pane_id,
            surface_id,
            kind: SignalKind::Notification,
            state: AttentionState::WaitingInput,
            title: Some("Codex".into()),
            subtitle: Some("Waiting".into()),
            external_id: None,
            message: "Need input".into(),
            created_at: now,
            read_at: None,
            cleared_at: None,
            desktop_delivery: NotificationDeliveryState::Pending,
        });
        workspace.notifications.push(NotificationItem {
            id: NotificationId::new(),
            pane_id,
            surface_id,
            kind: SignalKind::Notification,
            state: AttentionState::WaitingInput,
            title: Some("Old".into()),
            subtitle: None,
            external_id: None,
            message: "Shown already".into(),
            created_at: now,
            read_at: None,
            cleared_at: None,
            desktop_delivery: NotificationDeliveryState::Shown,
        });

        let pending = pending_desktop_notifications_with_prefs(
            &model,
            NotificationPreferencesSnapshot::default(),
        );
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].title, "Codex");
        assert_eq!(pending[0].body.as_deref(), Some("Waiting\nNeed input"));
    }

    #[test]
    fn pending_desktop_notifications_respects_waiting_toggle() {
        let mut model = AppModel::new("Main");
        let workspace = model.active_workspace_id().expect("workspace");
        let pane_id = model.active_workspace().expect("workspace").active_pane;
        let surface_id = model
            .active_workspace()
            .and_then(|workspace| workspace.panes.get(&pane_id))
            .and_then(|pane| pane.active_surface())
            .map(|surface| surface.id)
            .expect("surface");
        let workspace = model.workspaces.get_mut(&workspace).expect("workspace");
        workspace.notifications.push(NotificationItem {
            id: NotificationId::new(),
            pane_id,
            surface_id,
            kind: SignalKind::WaitingInput,
            state: AttentionState::WaitingInput,
            title: Some("Codex".into()),
            subtitle: None,
            external_id: None,
            message: "Need input".into(),
            created_at: OffsetDateTime::now_utc(),
            read_at: None,
            cleared_at: None,
            desktop_delivery: NotificationDeliveryState::Pending,
        });

        let pending = pending_desktop_notifications_with_prefs(
            &model,
            NotificationPreferencesSnapshot {
                alerts_on_waiting: false,
                ..NotificationPreferencesSnapshot::default()
            },
        );

        assert!(pending.is_empty());
    }
}

#[cfg(test)]
mod config_tests {
    use super::{NotificationPreferencesConfig, TaskersConfig};
    use taskers_ghostty::EmbeddedTerminalAppearance;
    use taskers_shell_core::{NotificationPreferencesSnapshot, SettingsSnapshot};

    #[test]
    fn taskers_config_defaults_to_taskers_embedded_terminal_appearance() {
        assert_eq!(
            TaskersConfig::default().embedded_terminal_appearance,
            EmbeddedTerminalAppearance::Taskers
        );
    }

    #[test]
    fn taskers_config_from_settings_preserves_embedded_terminal_appearance() {
        let current = TaskersConfig {
            embedded_terminal_appearance: EmbeddedTerminalAppearance::Ghostty,
            ..TaskersConfig::default()
        };

        let settings = SettingsSnapshot {
            selected_theme_id: "gruvbox-dark".into(),
            theme_options: Vec::new(),
            shortcut_presets: Vec::new(),
            shortcuts: Vec::new(),
            notification_preferences: NotificationPreferencesSnapshot {
                alerts_on_waiting: false,
                alerts_on_error: true,
                alerts_on_completed: true,
                suppress_when_visible: false,
            },
            render_live_surfaces_in_overview: false,
        };

        let next = TaskersConfig::from_settings(&settings, &current);

        assert_eq!(next.selected_theme_id, "gruvbox-dark");
        assert_eq!(
            next.embedded_terminal_appearance,
            EmbeddedTerminalAppearance::Ghostty
        );
        assert_eq!(
            next.notification_preferences,
            NotificationPreferencesConfig::from_snapshot(settings.notification_preferences)
        );
        assert!(!next.render_live_surfaces_in_overview);
    }
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

fn bootstrap_runtime(
    diagnostics: Option<&DiagnosticsWriter>,
    smoke_script: Option<SmokeScript>,
) -> Result<BootstrapContext> {
    let (config, config_note) = match TaskersConfig::load() {
        Ok(config) => (config, None),
        Err(error) => (
            TaskersConfig::default(),
            Some(format!("Taskers config unavailable: {error}")),
        ),
    };
    let path_overrides = smoke_script
        .map(|_| smoke_runtime_path_overrides())
        .transpose()?;
    let runtime =
        resolve_runtime_bootstrap(config.embedded_terminal_appearance, path_overrides.as_ref());
    let mut startup_notes = runtime.startup_notes;
    if let Some(note) = config_note {
        push_startup_note(&mut startup_notes, diagnostics, note);
    }
    if let Some(path_overrides) = path_overrides.as_ref() {
        push_startup_note(
            &mut startup_notes,
            diagnostics,
            format!(
                "Smoke mode using isolated runtime root {}",
                path_overrides.root_dir.display()
            ),
        );
    }
    let session_path = path_overrides
        .as_ref()
        .map(|overrides| overrides.session_path.clone())
        .unwrap_or_else(default_session_path);
    let mut initial_model = load_or_bootstrap(&session_path, false).with_context(|| {
        format!(
            "failed to load or bootstrap Taskers session at {}",
            session_path.display()
        )
    })?;
    if let Some(terminal_session_client) = runtime.terminal_session_client.as_ref() {
        match terminal_session_client.list_sessions() {
            Ok(live_sessions) => {
                let live_sessions = live_sessions.into_iter().collect::<HashSet<_>>();
                initial_model.recover_interrupted_agent_resumes_for_missing_sessions(
                    |session_id| live_sessions.contains(&session_id.to_string()),
                );
            }
            Err(error) => {
                push_startup_note(
                    &mut startup_notes,
                    diagnostics,
                    format!(
                        "Terminal session recovery unavailable; leaving persisted sessions untouched: {error}"
                    ),
                );
            }
        }
    } else {
        initial_model.recover_interrupted_agent_resumes();
    }

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
        terminal_persistence: runtime.terminal_persistence,
    };
    let app_state = AppState::new(
        initial_model,
        session_path,
        backend_choice,
        runtime.shell_launch,
        runtime.terminal_session_client.clone(),
    )
    .context("failed to initialize Taskers app state")?;
    let core = SharedCore::bootstrap(BootstrapModel {
        app_state: app_state.clone(),
        runtime_status,
        selected_theme_id: config.selected_theme_id.clone(),
        selected_shortcut_preset: config.shortcut_preset(),
        notification_preferences: config.notification_preferences.to_snapshot(),
        render_live_surfaces_in_overview: config.render_live_surfaces_in_overview,
    });

    log_runtime_status(diagnostics, &core.snapshot().runtime_status);

    Ok(BootstrapContext {
        core,
        app_state,
        socket_path: runtime.socket_path,
        ghostty_host,
        config,
        startup_notes,
    })
}

fn resolve_runtime_bootstrap(
    embedded_terminal_appearance: EmbeddedTerminalAppearance,
    path_overrides: Option<&RuntimePathOverrides>,
) -> RuntimeBootstrap {
    scrub_inherited_terminal_env();

    let mut startup_notes = Vec::new();
    let socket_path = path_overrides
        .map(|overrides| overrides.socket_path.clone())
        .unwrap_or_else(default_socket_path);
    let terminal_socket_path = path_overrides
        .map(|overrides| overrides.terminal_socket_path.clone())
        .unwrap_or_else(default_terminal_socket_path);
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
    let terminal_session_client = if should_skip_terminal_sidecar_in_smoke(path_overrides) {
        startup_notes.push(
            "Smoke mode with mock terminal backend skips the terminal session sidecar.".into(),
        );
        None
    } else {
        match ensure_terminal_session_daemon(&terminal_socket_path) {
            Ok(()) => {
                shell_launch.env.insert(
                    "TASKERS_TERMINAL_SOCKET".into(),
                    terminal_socket_path.display().to_string(),
                );
                Some(TerminalSessionClient::new(terminal_socket_path))
            }
            Err(error) => {
                startup_notes.push(format!(
                    "terminal session sidecar unavailable; terminals will start fresh shells and will not survive Taskers restart ({error})"
                ));
                None
            }
        }
    };
    let terminal_persistence = if terminal_session_client.is_some() {
        RuntimeCapability::Ready
    } else {
        RuntimeCapability::Fallback {
            message:
                "Terminal sidecar unavailable; terminals will start fresh shells and will not survive Taskers restart."
                    .into(),
        }
    };

    let host_options = GhosttyHostOptions::from_shell_launch(&shell_launch)
        .with_embedded_terminal_appearance(embedded_terminal_appearance);

    RuntimeBootstrap {
        ghostty_runtime,
        shell_integration,
        terminal_persistence,
        shell_launch,
        host_options,
        socket_path,
        terminal_session_client,
        startup_notes,
    }
}

fn smoke_runtime_path_overrides() -> Result<RuntimePathOverrides> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    let root_dir =
        std::env::temp_dir().join(format!("taskers-smoke-{}-{timestamp}", std::process::id()));
    create_dir_all(&root_dir)
        .with_context(|| format!("failed to create smoke runtime root {}", root_dir.display()))?;
    Ok(RuntimePathOverrides {
        session_path: root_dir.join("session.json"),
        socket_path: root_dir.join("control.sock"),
        terminal_socket_path: root_dir.join("terminal.sock"),
        root_dir,
    })
}

fn taskers_probe_session_path(mode: GhosttyProbeMode) -> PathBuf {
    std::env::temp_dir().join(format!(
        "taskers-probe-{}-{}.json",
        mode.as_arg(),
        std::process::id()
    ))
}

fn run_internal_ghostty_probe(mode: GhosttyProbeMode) -> glib::ExitCode {
    let config = TaskersConfig::load().unwrap_or_default();
    let runtime = resolve_runtime_bootstrap(config.embedded_terminal_appearance, None);
    let host = match GhosttyHost::new_with_options(&runtime.host_options) {
        Ok(host) => {
            let _ = host.tick();
            host
        }
        Err(error) => {
            safe_eprintln(format!(
                "ghostty {} self-probe failed during host init: {error}",
                mode.as_arg()
            ));
            return glib::ExitCode::FAILURE;
        }
    };

    if matches!(mode, GhosttyProbeMode::Surface) {
        return run_internal_surface_probe(host, runtime.shell_launch, mode, config);
    }

    spin_probe_main_context(Duration::from_millis(350));
    glib::ExitCode::SUCCESS
}

fn run_internal_surface_probe(
    host: GhosttyHost,
    shell_launch: ShellLaunchSpec,
    mode: GhosttyProbeMode,
    config: TaskersConfig,
) -> glib::ExitCode {
    if !gtk::is_initialized_main_thread()
        && let Err(error) = gtk::init()
    {
        safe_eprintln(format!(
            "ghostty {} self-probe failed during gtk init: {error}",
            mode.as_arg()
        ));
        return glib::ExitCode::FAILURE;
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
        taskers_probe_session_path(mode),
        BackendChoice::GhosttyEmbedded,
        shell_launch,
        None,
    ) {
        Ok(app_state) => app_state,
        Err(error) => {
            safe_eprintln(format!(
                "ghostty {} self-probe failed during app state bootstrap: {error}",
                mode.as_arg()
            ));
            return glib::ExitCode::FAILURE;
        }
    };

    let selected_theme_id = config.selected_theme_id.clone();
    let selected_shortcut_preset = config.shortcut_preset();
    let notification_preferences = config.notification_preferences.to_snapshot();
    let core = SharedCore::bootstrap(BootstrapModel {
        app_state,
        runtime_status: RuntimeStatus {
            ghostty_runtime: RuntimeCapability::Ready,
            shell_integration: RuntimeCapability::Ready,
            terminal_host: RuntimeCapability::Ready,
            terminal_persistence: RuntimeCapability::Ready,
        },
        selected_theme_id,
        selected_shortcut_preset,
        notification_preferences,
        render_live_surfaces_in_overview: config.render_live_surfaces_in_overview,
    });
    core.set_window_size(PixelSize::new(
        GHOSTTY_PROBE_WINDOW_SIZE_PX,
        GHOSTTY_PROBE_WINDOW_SIZE_PX,
    ));

    let event_sink = Rc::new(|_| {});
    let shell_action_sink = Rc::new(|_| {});
    let mut taskers_host =
        TaskersHost::new(&shell_view, Some(host), event_sink, shell_action_sink, None);
    let host_widget = taskers_host.widget();
    let window = gtk::Window::builder()
        .default_width(GHOSTTY_PROBE_WINDOW_SIZE_PX)
        .default_height(GHOSTTY_PROBE_WINDOW_SIZE_PX)
        .child(&host_widget)
        .build();
    configure_probe_window(&window);
    window.show();

    spin_probe_main_context(Duration::from_millis(80));
    if let Err(error) = taskers_host.sync_snapshot(&core.snapshot()) {
        safe_eprintln(format!(
            "ghostty {} self-probe failed during snapshot sync: {error}",
            mode.as_arg()
        ));
        return glib::ExitCode::FAILURE;
    }

    let deadline = Instant::now() + Duration::from_millis(350);
    let context = glib::MainContext::default();
    while Instant::now() < deadline {
        taskers_host.tick(None);
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

fn configure_probe_window(window: &gtk::Window) {
    // The probe still needs a mapped GTK toplevel so Ghostty can realize an
    // embedded surface, but it should not flash a visible window at startup.
    window.set_decorated(false);
    window.set_deletable(false);
    window.set_resizable(false);
    window.set_focusable(false);
    window.set_can_target(false);
    window.set_opacity(0.0);
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
            safe_eprintln(format!("taskers host command failed: {error}"));
        }
    }

    let width = window.width();
    let height = window.height();
    if should_defer_initial_sync(last_size.get(), width, height) {
        host.borrow_mut().tick(Some(core.revision()));
        return;
    }

    let size = PixelSize::new(width.max(1), height.max(1));
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
            safe_eprintln(format!(
                "taskers host sync failed for revision {revision}: {error}"
            ));
        }
        last_revision.set(revision);
    }

    host.borrow_mut().tick(Some(core.revision()));
}

fn should_defer_initial_sync(last_size: (i32, i32), width: i32, height: i32) -> bool {
    last_size == (0, 0) && (width <= 1 || height <= 1)
}

fn install_host_bridge(
    receiver: Receiver<HostAutomationRequest>,
    window: &adw::ApplicationWindow,
    core: &SharedCore,
    host: &Rc<RefCell<TaskersHost>>,
    last_revision: &Rc<Cell<u64>>,
    last_size: &Rc<Cell<(i32, i32)>>,
    diagnostics: Option<DiagnosticsWriter>,
) {
    let receiver = Rc::new(receiver);
    let bridge_window = window.clone();
    let bridge_core = core.clone();
    let bridge_host = host.clone();
    let bridge_revision = last_revision.clone();
    let bridge_size = last_size.clone();
    glib::timeout_add_local(Duration::from_millis(8), move || {
        loop {
            let request = match receiver.try_recv() {
                Ok(request) => request,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return glib::ControlFlow::Break,
            };
            let request_window = bridge_window.clone();
            let request_core = bridge_core.clone();
            let request_host = bridge_host.clone();
            let request_revision = bridge_revision.clone();
            let request_size = bridge_size.clone();
            let request_diagnostics = diagnostics.clone();
            glib::MainContext::default().spawn_local(async move {
                let response = handle_host_request(
                    &request_window,
                    &request_core,
                    &request_host,
                    &request_revision,
                    &request_size,
                    request_diagnostics.as_ref(),
                    request.command,
                )
                .await;
                let _ = request.response_tx.send(response);
            });
        }
        glib::ControlFlow::Continue
    });
}

async fn handle_host_request(
    window: &adw::ApplicationWindow,
    core: &SharedCore,
    host: &Rc<RefCell<TaskersHost>>,
    last_revision: &Rc<Cell<u64>>,
    last_size: &Rc<Cell<(i32, i32)>>,
    diagnostics: Option<&DiagnosticsWriter>,
    command: HostAutomationCommand,
) -> Result<ControlResponse, ControlError> {
    match command {
        HostAutomationCommand::Browser(command) => {
            handle_browser_request(
                window,
                core,
                host,
                last_revision,
                last_size,
                diagnostics,
                command,
            )
            .await
        }
        HostAutomationCommand::Screenshot(command) => handle_screenshot_request(
            window,
            core,
            host,
            last_revision,
            last_size,
            diagnostics,
            command,
        ),
        HostAutomationCommand::TerminalDebug(command) => handle_terminal_debug_request(
            window,
            core,
            host,
            last_revision,
            last_size,
            diagnostics,
            command,
        ),
    }
}

async fn handle_browser_request(
    window: &adw::ApplicationWindow,
    core: &SharedCore,
    host: &Rc<RefCell<TaskersHost>>,
    last_revision: &Rc<Cell<u64>>,
    last_size: &Rc<Cell<(i32, i32)>>,
    diagnostics: Option<&DiagnosticsWriter>,
    command: BrowserControlCommand,
) -> Result<ControlResponse, ControlError> {
    sync_window(window, core, host, last_revision, last_size, diagnostics);

    if let BrowserControlCommand::FocusWebview { surface_id } = &command {
        let snapshot = core.snapshot();
        let Some(entry) = snapshot
            .browser_catalog
            .iter()
            .find(|entry| entry.surface_id == *surface_id)
        else {
            return Err(ControlError::not_found(format!(
                "browser surface {surface_id} not found"
            )));
        };
        core.dispatch_shell_action(ShellAction::FocusSurface {
            pane_id: entry.pane_id,
            surface_id: *surface_id,
        });
        sync_window(window, core, host, last_revision, last_size, diagnostics);
    }

    let handle = {
        let host_ref = host.borrow();
        host_ref.browser_surface_handle(browser_surface_id(&command))?
    };
    let result = handle.execute(command).await?;
    sync_window(window, core, host, last_revision, last_size, diagnostics);
    Ok(ControlResponse::Browser { result })
}

fn handle_terminal_debug_request(
    window: &adw::ApplicationWindow,
    core: &SharedCore,
    host: &Rc<RefCell<TaskersHost>>,
    last_revision: &Rc<Cell<u64>>,
    last_size: &Rc<Cell<(i32, i32)>>,
    diagnostics: Option<&DiagnosticsWriter>,
    command: TerminalDebugCommand,
) -> Result<ControlResponse, ControlError> {
    sync_window(window, core, host, last_revision, last_size, diagnostics);
    let result = host.borrow_mut().execute_terminal_debug(command)?;
    sync_window(window, core, host, last_revision, last_size, diagnostics);
    Ok(ControlResponse::TerminalDebug { result })
}

fn handle_screenshot_request(
    window: &adw::ApplicationWindow,
    core: &SharedCore,
    host: &Rc<RefCell<TaskersHost>>,
    last_revision: &Rc<Cell<u64>>,
    last_size: &Rc<Cell<(i32, i32)>>,
    diagnostics: Option<&DiagnosticsWriter>,
    command: ScreenshotCommand,
) -> Result<ControlResponse, ControlError> {
    sync_window(window, core, host, last_revision, last_size, diagnostics);
    let result = host.borrow_mut().execute_screenshot(command)?;
    sync_window(window, core, host, last_revision, last_size, diagnostics);
    Ok(ControlResponse::Screenshot { result })
}

fn browser_surface_id(command: &BrowserControlCommand) -> taskers_shell_core::SurfaceId {
    match command {
        BrowserControlCommand::Navigate { surface_id, .. }
        | BrowserControlCommand::Back { surface_id }
        | BrowserControlCommand::Forward { surface_id }
        | BrowserControlCommand::Reload { surface_id }
        | BrowserControlCommand::FocusWebview { surface_id }
        | BrowserControlCommand::IsWebviewFocused { surface_id }
        | BrowserControlCommand::Snapshot { surface_id }
        | BrowserControlCommand::Eval { surface_id, .. }
        | BrowserControlCommand::Wait { surface_id, .. }
        | BrowserControlCommand::Click { surface_id, .. }
        | BrowserControlCommand::Dblclick { surface_id, .. }
        | BrowserControlCommand::Type { surface_id, .. }
        | BrowserControlCommand::Fill { surface_id, .. }
        | BrowserControlCommand::Press { surface_id, .. }
        | BrowserControlCommand::Keydown { surface_id, .. }
        | BrowserControlCommand::Keyup { surface_id, .. }
        | BrowserControlCommand::Hover { surface_id, .. }
        | BrowserControlCommand::Focus { surface_id, .. }
        | BrowserControlCommand::Check { surface_id, .. }
        | BrowserControlCommand::Uncheck { surface_id, .. }
        | BrowserControlCommand::Select { surface_id, .. }
        | BrowserControlCommand::Scroll { surface_id, .. }
        | BrowserControlCommand::ScrollIntoView { surface_id, .. }
        | BrowserControlCommand::Get { surface_id, .. }
        | BrowserControlCommand::Is { surface_id, .. }
        | BrowserControlCommand::Screenshot { surface_id, .. }
        | BrowserControlCommand::ClearData { surface_id, .. } => *surface_id,
    }
}

fn spawn_control_server(
    app_state: AppState,
    socket_path: PathBuf,
    host_tx: Sender<HostAutomationRequest>,
) -> String {
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
                        let app_state = app_state.clone();
                        let host_tx = host_tx.clone();
                        async move {
                            match command {
                                ControlCommand::Browser { browser_command } => {
                                    let (response_tx, response_rx) =
                                        tokio::sync::oneshot::channel();
                                    host_tx
                                        .send(HostAutomationRequest {
                                            command: HostAutomationCommand::Browser(
                                                browser_command,
                                            ),
                                            response_tx,
                                        })
                                        .map_err(|_| {
                                            ControlError::internal(
                                                "host automation bridge is unavailable",
                                            )
                                        })?;
                                    response_rx.await.map_err(|_| {
                                        ControlError::internal(
                                            "host automation bridge dropped the response",
                                        )
                                    })?
                                }
                                ControlCommand::Screenshot { screenshot_command } => {
                                    let (response_tx, response_rx) =
                                        tokio::sync::oneshot::channel();
                                    host_tx
                                        .send(HostAutomationRequest {
                                            command: HostAutomationCommand::Screenshot(
                                                screenshot_command,
                                            ),
                                            response_tx,
                                        })
                                        .map_err(|_| {
                                            ControlError::internal(
                                                "host automation bridge is unavailable",
                                            )
                                        })?;
                                    response_rx.await.map_err(|_| {
                                        ControlError::internal(
                                            "host automation bridge dropped the response",
                                        )
                                    })?
                                }
                                ControlCommand::TerminalDebug { debug_command } => {
                                    let (response_tx, response_rx) =
                                        tokio::sync::oneshot::channel();
                                    host_tx
                                        .send(HostAutomationRequest {
                                            command: HostAutomationCommand::TerminalDebug(
                                                debug_command,
                                            ),
                                            response_tx,
                                        })
                                        .map_err(|_| {
                                            ControlError::internal(
                                                "host automation bridge is unavailable",
                                            )
                                        })?;
                                    response_rx.await.map_err(|_| {
                                        ControlError::internal(
                                            "host automation bridge dropped the response",
                                        )
                                    })?
                                }
                                other => app_state
                                    .dispatch(other)
                                    .map_err(|error| ControlError::internal(error.to_string())),
                            }
                        }
                    };
                    if let Err(error) = serve_with_handler(listener, handler, pending::<()>()).await
                    {
                        safe_eprintln(format!("control server error: {error}"));
                    }
                }
                Err(error) => {
                    safe_eprintln(format!(
                        "control server unavailable at {}: {error}",
                        socket_path.display()
                    ));
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
                safe_eprintln(format!("liveview server failed: {error}"));
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
    quit_tx: Sender<()>,
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
        let _ = quit_tx.send(());
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
        LayoutNodeSnapshot::Pane(pane) => first_browser_ready_in_pane_layout(&pane.layout),
        LayoutNodeSnapshot::Split { first, second, .. } => {
            first_browser_ready(first).or_else(|| first_browser_ready(second))
        }
    }
}

fn surface_counts(node: &LayoutNodeSnapshot) -> (usize, usize) {
    match node {
        LayoutNodeSnapshot::Pane(pane) => surface_counts_in_pane_layout(&pane.layout),
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

fn first_browser_ready_in_pane_layout(node: &PaneTabLayoutSnapshot) -> Option<String> {
    match node {
        PaneTabLayoutSnapshot::Pane(pane) => pane
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
        PaneTabLayoutSnapshot::Split { first, second, .. } => {
            first_browser_ready_in_pane_layout(first)
                .or_else(|| first_browser_ready_in_pane_layout(second))
        }
    }
}

fn surface_counts_in_pane_layout(node: &PaneTabLayoutSnapshot) -> (usize, usize) {
    match node {
        PaneTabLayoutSnapshot::Pane(pane) => pane.surfaces.iter().fold(
            (0usize, 0usize),
            |(browser_count, terminal_count), surface| match surface.kind {
                SurfaceKind::Browser => (browser_count + 1, terminal_count),
                SurfaceKind::Terminal => (browser_count, terminal_count + 1),
            },
        ),
        PaneTabLayoutSnapshot::Split { first, second, .. } => {
            let (first_browser, first_terminal) = surface_counts_in_pane_layout(first);
            let (second_browser, second_terminal) = surface_counts_in_pane_layout(second);
            (
                first_browser + second_browser,
                first_terminal + second_terminal,
            )
        }
    }
}

fn log_runtime_status(diagnostics: Option<&DiagnosticsWriter>, status: &RuntimeStatus) {
    let summary = format!(
        "runtime status ghostty={} shell={} terminal={} persistence={}",
        status.ghostty_runtime.label(),
        status.shell_integration.label(),
        status.terminal_host.label(),
        status.terminal_persistence.label(),
    );
    log_diagnostic(
        diagnostics,
        DiagnosticRecord::new(DiagnosticCategory::Startup, None, summary),
    );
}

fn ensure_terminal_session_daemon(socket_path: &PathBuf) -> Result<()> {
    let client = TerminalSessionClient::new(socket_path.clone());
    if client.ping().is_ok() {
        return Ok(());
    }

    let terminald = std::env::current_exe()
        .context("failed to resolve current executable for terminal sidecar launch")?
        .with_file_name("taskers-terminald");
    Command::new(&terminald)
        .arg("--socket")
        .arg(socket_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("failed to launch {}", terminald.display()))?;

    for _ in 0..20 {
        if client.ping().is_ok() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    bail!(
        "terminal sidecar did not become ready at {}",
        socket_path.display()
    )
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
        let explicit_target = cli
            .diagnostic_log
            .clone()
            .or_else(|| {
                std::env::var("TASKERS_DIAGNOSTIC_LOG")
                    .ok()
                    .filter(|value| !value.is_empty())
            })
            .or_else(|| cli.smoke_script.map(|_| "stderr".into()));
        let auto_target = explicit_target
            .is_none()
            .then(default_dev_diagnostic_log_path)
            .flatten();
        let target = explicit_target
            .or_else(|| auto_target.as_ref().map(|path| path.display().to_string()))?;

        if target == "stderr" {
            return Some(Self {
                target: DiagnosticsTarget::Stderr,
            });
        }

        let target_path = PathBuf::from(&target);
        if let Some(parent) = target_path.parent()
            && let Err(error) = create_dir_all(parent)
        {
            safe_eprintln(format!(
                "taskers diagnostics log directory failed: {} ({error})",
                parent.display()
            ));
            return None;
        }

        match File::create(&target_path) {
            Ok(file) => Some(Self {
                target: DiagnosticsTarget::File(Arc::new(Mutex::new(file))),
            }),
            Err(error) => {
                safe_eprintln(format!("taskers diagnostics log path failed: {error}"));
                None
            }
        }
        .inspect(|_| {
            if auto_target.as_deref() == Some(target_path.as_path()) {
                safe_eprintln(format!(
                    "taskers diagnostics logging to {}",
                    target_path.display()
                ));
            }
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

fn default_dev_diagnostic_log_path() -> Option<PathBuf> {
    let current_exe = std::env::current_exe().ok()?;
    looks_like_dev_install(&current_exe).then(|| {
        TaskersPaths::detect()
            .state_dir()
            .join("diagnostics")
            .join(DEV_DIAGNOSTIC_LOG_NAME)
    })
}

fn looks_like_dev_install(path: &Path) -> bool {
    let path = path.to_string_lossy();
    path.contains("/.cargo/bin/taskers-gtk")
        || path.contains("/target/debug/taskers-gtk")
        || path.contains("/target/release/taskers-gtk")
}

#[cfg(test)]
mod startup_tests {
    use super::{
        RuntimePathOverrides, looks_like_dev_install, should_defer_initial_sync,
        should_skip_terminal_sidecar_in_smoke, smoke_runtime_path_overrides,
    };
    use std::path::Path;

    #[test]
    fn initial_sync_waits_for_real_allocation() {
        assert!(should_defer_initial_sync((0, 0), 1, 1));
        assert!(should_defer_initial_sync((0, 0), 1440, 1));
        assert!(!should_defer_initial_sync((0, 0), 1440, 900));
    }

    #[test]
    fn later_resizes_do_not_get_blocked() {
        assert!(!should_defer_initial_sync((1440, 900), 1, 1));
    }

    #[test]
    fn cargo_install_binary_uses_dev_diagnostics() {
        assert!(looks_like_dev_install(Path::new(
            "/home/notes/.cargo/bin/taskers-gtk"
        )));
    }

    #[test]
    fn target_build_binary_uses_dev_diagnostics() {
        assert!(looks_like_dev_install(Path::new(
            "/home/notes/Projects/taskers/target/debug/taskers-gtk"
        )));
    }

    #[test]
    fn launcher_bundle_binary_does_not_use_dev_diagnostics() {
        assert!(!looks_like_dev_install(Path::new(
            "/home/notes/.local/share/taskers/releases/0.6.0/x86_64-unknown-linux-gnu/taskers-gtk"
        )));
    }

    #[test]
    fn smoke_runtime_paths_are_isolated_from_user_state() {
        let overrides = smoke_runtime_path_overrides().expect("smoke paths");
        assert!(overrides.root_dir.starts_with(std::env::temp_dir()));
        assert!(overrides.session_path.starts_with(&overrides.root_dir));
        assert!(overrides.socket_path.starts_with(&overrides.root_dir));
        assert!(
            overrides
                .terminal_socket_path
                .starts_with(&overrides.root_dir)
        );
    }

    #[test]
    fn smoke_with_mock_backend_skips_terminal_sidecar() {
        let overrides = RuntimePathOverrides {
            root_dir: Path::new("/tmp/taskers-smoke").to_path_buf(),
            session_path: Path::new("/tmp/taskers-smoke/session.json").to_path_buf(),
            socket_path: Path::new("/tmp/taskers-smoke/control.sock").to_path_buf(),
            terminal_socket_path: Path::new("/tmp/taskers-smoke/terminal.sock").to_path_buf(),
        };

        unsafe { std::env::set_var("TASKERS_TERMINAL_BACKEND", "mock") };
        assert!(should_skip_terminal_sidecar_in_smoke(Some(&overrides)));
        unsafe { std::env::set_var("TASKERS_TERMINAL_BACKEND", "ghostty") };
        assert!(!should_skip_terminal_sidecar_in_smoke(Some(&overrides)));
        unsafe { std::env::remove_var("TASKERS_TERMINAL_BACKEND") };
        assert!(!should_skip_terminal_sidecar_in_smoke(Some(&overrides)));
        unsafe { std::env::set_var("TASKERS_TERMINAL_BACKEND", "mock") };
        assert!(!should_skip_terminal_sidecar_in_smoke(None));
        unsafe { std::env::remove_var("TASKERS_TERMINAL_BACKEND") };
    }
}
