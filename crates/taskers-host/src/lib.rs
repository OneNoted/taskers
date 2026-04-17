mod browser_automation;

use anyhow::{Result, anyhow};
use gtk::{
    Align, Box as GtkBox, CssProvider, DrawingArea, EventControllerFocus, EventControllerScroll,
    EventControllerScrollFlags, Fixed, GestureDrag, Orientation, Overflow, Overlay,
    STYLE_PROVIDER_PRIORITY_APPLICATION, Snapshot, Widget, WidgetPaintable, gdk, glib, graphene,
    gsk, prelude::*,
};
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    f64::consts::{FRAC_PI_2, PI, TAU},
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use taskers_control::{
    BrowserControlCommand, BrowserLoadState, ControlError, ControlErrorCode, ScreenshotCommand,
    ScreenshotResult, ScreenshotTarget, ScreenshotTargetResult, TerminalDebugCommand,
    TerminalDebugResult, TerminalRenderStats,
};
use taskers_core::{
    BrowserSurfaceCatalogEntry, HostCommand, HostEvent, PaneId, PortalSurfacePlan, ShellDragMode,
    ShellSection, ShellSnapshot, SurfaceId, SurfaceMountSpec, SurfacePortalPlan, TerminalMountSpec,
    TerminalSurfaceCatalogEntry, WorkspaceId, WorkspaceViewSnapshot,
};
use taskers_domain::{
    BrowserProfileMode, MIN_WORKSPACE_WINDOW_HEIGHT, MIN_WORKSPACE_WINDOW_WIDTH, PaneKind,
};
use taskers_ghostty::{GhosttyBridgeInfo, GhosttyHost, SurfaceDescriptor};
use taskers_shell_core as taskers_core;
use webkit6::{
    HardwareAccelerationPolicy, LoadEvent, NetworkSession, Settings as WebKitSettings, WebView,
    prelude::*,
};

pub type HostEventSink = Rc<dyn Fn(HostEvent) + 'static>;
pub type ShellActionSink = Rc<dyn Fn(taskers_core::ShellAction) + 'static>;
pub type DiagnosticsSink = Arc<dyn Fn(DiagnosticRecord) + Send + Sync + 'static>;

// Extremely thin viewport-edge slivers have been enough to trip native
// GTK/Ghostty rendering during teardown on Linux/NVIDIA. Keep only a narrow
// safety valve here so moderately visible edge slices still render.
const MIN_CLIPPED_NATIVE_SURFACE_WIDTH_PX: i32 = 48;
const MIN_CLIPPED_NATIVE_SURFACE_HEIGHT_PX: i32 = 120;
const MIN_RESIZE_SPLIT_RATIO: u16 = 150;
const MAX_RESIZE_SPLIT_RATIO: u16 = 850;
const GHOSTTY_BRIDGE_WARN_THRESHOLD: Duration = Duration::from_secs(2);
const GHOSTTY_BRIDGE_FATAL_THRESHOLD: Duration = Duration::from_secs(5);
const GHOSTTY_BRIDGE_WATCHDOG_POLL_INTERVAL: Duration = Duration::from_millis(200);
const MAX_CONCURRENT_GHOSTTY_SURFACES: usize = 3;
const WEBKIT_DISABLE_DMABUF_RENDERER_ENV: &str = "WEBKIT_DISABLE_DMABUF_RENDERER";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticCategory {
    Startup,
    Window,
    Sync,
    Bridge,
    HostEvent,
    SurfaceLifecycle,
    BrowserMetadata,
    Smoke,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticRecord {
    pub timestamp_ms: u128,
    pub revision: Option<u64>,
    pub category: DiagnosticCategory,
    pub message: String,
    pub pane_id: Option<taskers_core::PaneId>,
    pub surface_id: Option<SurfaceId>,
}

impl DiagnosticRecord {
    pub fn new(
        category: DiagnosticCategory,
        revision: Option<u64>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            timestamp_ms: current_timestamp_ms(),
            revision,
            category,
            message: message.into(),
            pane_id: None,
            surface_id: None,
        }
    }

    pub fn with_pane(mut self, pane_id: taskers_core::PaneId) -> Self {
        self.pane_id = Some(pane_id);
        self
    }

    pub fn with_surface(mut self, surface_id: SurfaceId) -> Self {
        self.surface_id = Some(surface_id);
        self
    }

    fn with_optional_pane(mut self, pane_id: Option<taskers_core::PaneId>) -> Self {
        self.pane_id = pane_id;
        self
    }

    fn with_optional_surface(mut self, surface_id: Option<SurfaceId>) -> Self {
        self.surface_id = surface_id;
        self
    }

    pub fn format_line(&self) -> String {
        let revision = self
            .revision
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".into());
        let pane = self
            .pane_id
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".into());
        let surface = self
            .surface_id
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".into());

        format!(
            "ts_ms={} category={:?} revision={} pane={} surface={} message={}",
            self.timestamp_ms, self.category, revision, pane, surface, self.message
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GhosttyLifecycleState {
    Starting,
    Running,
    ShuttingDown,
    Failed,
}

impl GhosttyLifecycleState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Running => "running",
            Self::ShuttingDown => "shutting-down",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeHealthSnapshot {
    pub bridge_info: GhosttyBridgeInfo,
    pub state: GhosttyLifecycleState,
    pub surface_count: usize,
    pub last_tick_duration_ms: Option<u128>,
    pub last_mutation_duration_ms: Option<u128>,
    pub last_operation: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BridgeOperationKind {
    Tick,
    SurfaceSync,
    Shutdown,
    TerminalCommand,
    TerminalDebug,
}

impl BridgeOperationKind {
    fn label(self) -> &'static str {
        match self {
            Self::Tick => "tick",
            Self::SurfaceSync => "surface-sync",
            Self::Shutdown => "shutdown",
            Self::TerminalCommand => "terminal-command",
            Self::TerminalDebug => "terminal-debug",
        }
    }

    fn tracks_tick_duration(self) -> bool {
        matches!(self, Self::Tick)
    }

    fn tracks_mutation_duration(self) -> bool {
        matches!(self, Self::SurfaceSync | Self::Shutdown)
    }
}

struct BridgeWatchdog {
    shared: Arc<Mutex<BridgeWatchdogShared>>,
    stop_tx: mpsc::Sender<()>,
    join_handle: Option<thread::JoinHandle<()>>,
}

struct BridgeWatchdogShared {
    next_token: u64,
    lifecycle_state: GhosttyLifecycleState,
    active: Option<ActiveBridgeOperation>,
    last_tick_duration_ms: Option<u128>,
    last_mutation_duration_ms: Option<u128>,
    last_operation: Option<CompletedBridgeOperation>,
}

struct ActiveBridgeOperation {
    token: u64,
    kind: BridgeOperationKind,
    revision: Option<u64>,
    pane_id: Option<PaneId>,
    surface_id: Option<SurfaceId>,
    started_at: Instant,
    warned: bool,
    fatal_logged: bool,
}

struct CompletedBridgeOperation {
    kind: BridgeOperationKind,
    duration_ms: u128,
}

struct BridgeOperationGuard {
    shared: Arc<Mutex<BridgeWatchdogShared>>,
    token: u64,
    kind: BridgeOperationKind,
    started_at: Instant,
}

impl BridgeWatchdog {
    fn new(diagnostics: Option<DiagnosticsSink>) -> Self {
        let shared = Arc::new(Mutex::new(BridgeWatchdogShared {
            next_token: 1,
            lifecycle_state: GhosttyLifecycleState::Starting,
            active: None,
            last_tick_duration_ms: None,
            last_mutation_duration_ms: None,
            last_operation: None,
        }));
        let (stop_tx, stop_rx) = mpsc::channel();
        let thread_shared = shared.clone();
        let join_handle = thread::spawn(move || {
            loop {
                match stop_rx.recv_timeout(GHOSTTY_BRIDGE_WATCHDOG_POLL_INTERVAL) {
                    Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }

                let mut records = Vec::new();
                {
                    let mut shared = thread_shared.lock().expect("bridge watchdog lock");
                    let Some(active) = shared.active.as_mut() else {
                        continue;
                    };
                    let elapsed_ms = active.started_at.elapsed().as_millis();
                    let mut fatal_transition = None;
                    if elapsed_ms >= GHOSTTY_BRIDGE_FATAL_THRESHOLD.as_millis()
                        && !active.fatal_logged
                    {
                        active.fatal_logged = true;
                        fatal_transition = Some((
                            active.revision,
                            active.kind.label(),
                            active.pane_id,
                            active.surface_id,
                        ));
                        records.push(
                            DiagnosticRecord::new(
                                DiagnosticCategory::Bridge,
                                active.revision,
                                format!(
                                    "ghostty bridge hang suspected operation={} elapsed_ms={elapsed_ms}",
                                    active.kind.label()
                                ),
                            )
                            .with_optional_pane(active.pane_id)
                            .with_optional_surface(active.surface_id),
                        );
                    } else if elapsed_ms >= GHOSTTY_BRIDGE_WARN_THRESHOLD.as_millis()
                        && !active.warned
                    {
                        active.warned = true;
                        records.push(
                            DiagnosticRecord::new(
                                DiagnosticCategory::Bridge,
                                active.revision,
                                format!(
                                    "ghostty bridge operation stalled operation={} elapsed_ms={elapsed_ms}",
                                    active.kind.label()
                                ),
                            )
                            .with_optional_pane(active.pane_id)
                            .with_optional_surface(active.surface_id),
                        );
                    }

                    if let Some((revision, operation, pane_id, surface_id)) = fatal_transition {
                        let state_changed = shared.lifecycle_state != GhosttyLifecycleState::Failed;
                        shared.lifecycle_state = GhosttyLifecycleState::Failed;
                        if state_changed {
                            records.push(DiagnosticRecord::new(
                                DiagnosticCategory::Bridge,
                                revision,
                                format!(
                                    "ghostty lifecycle state={} reason=watchdog observed hung {} operation",
                                    GhosttyLifecycleState::Failed.label(),
                                    operation
                                ),
                            )
                            .with_optional_pane(pane_id)
                            .with_optional_surface(surface_id));
                        }
                    }
                }

                if let Some(diagnostics) = diagnostics.as_ref() {
                    for record in records {
                        diagnostics(record);
                    }
                }
            }
        });

        Self {
            shared,
            stop_tx,
            join_handle: Some(join_handle),
        }
    }

    fn begin(
        &self,
        kind: BridgeOperationKind,
        revision: Option<u64>,
        pane_id: Option<PaneId>,
        surface_id: Option<SurfaceId>,
    ) -> BridgeOperationGuard {
        let started_at = Instant::now();
        let token = {
            let mut shared = self.shared.lock().expect("bridge watchdog lock");
            let token = shared.next_token;
            shared.next_token += 1;
            shared.active = Some(ActiveBridgeOperation {
                token,
                kind,
                revision,
                pane_id,
                surface_id,
                started_at,
                warned: false,
                fatal_logged: false,
            });
            token
        };

        BridgeOperationGuard {
            shared: self.shared.clone(),
            token,
            kind,
            started_at,
        }
    }

    fn lifecycle_state(&self) -> GhosttyLifecycleState {
        self.shared
            .lock()
            .expect("bridge watchdog lock")
            .lifecycle_state
    }

    fn transition_state(
        &self,
        diagnostics: Option<&DiagnosticsSink>,
        state: GhosttyLifecycleState,
        revision: Option<u64>,
        reason: impl Into<String>,
    ) {
        let reason = reason.into();
        let changed = {
            let mut shared = self.shared.lock().expect("bridge watchdog lock");
            if shared.lifecycle_state == state {
                false
            } else {
                shared.lifecycle_state = state;
                true
            }
        };

        if changed {
            emit_diagnostic(
                diagnostics,
                DiagnosticRecord::new(
                    DiagnosticCategory::Bridge,
                    revision,
                    format!("ghostty lifecycle state={} reason={reason}", state.label()),
                ),
            );
        }
    }

    fn snapshot(
        &self,
        bridge_info: GhosttyBridgeInfo,
        surface_count: usize,
    ) -> BridgeHealthSnapshot {
        let shared = self.shared.lock().expect("bridge watchdog lock");
        BridgeHealthSnapshot {
            bridge_info,
            state: shared.lifecycle_state,
            surface_count,
            last_tick_duration_ms: shared.last_tick_duration_ms,
            last_mutation_duration_ms: shared.last_mutation_duration_ms,
            last_operation: shared
                .last_operation
                .as_ref()
                .map(|operation| format!("{}:{}ms", operation.kind.label(), operation.duration_ms)),
        }
    }
}

impl Drop for BridgeWatchdog {
    fn drop(&mut self) {
        let _ = self.stop_tx.send(());
        if let Some(handle) = self.join_handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for BridgeOperationGuard {
    fn drop(&mut self) {
        let duration_ms = self.started_at.elapsed().as_millis();
        let mut shared = self.shared.lock().expect("bridge watchdog lock");
        if shared.active.as_ref().map(|active| active.token) == Some(self.token) {
            shared.active = None;
        }
        if self.kind.tracks_tick_duration() {
            shared.last_tick_duration_ms = Some(duration_ms);
        }
        if self.kind.tracks_mutation_duration() {
            shared.last_mutation_duration_ms = Some(duration_ms);
        }
        shared.last_operation = Some(CompletedBridgeOperation {
            kind: self.kind,
            duration_ms,
        });
    }
}

fn redacted_browser_url_for_diagnostics(url: &str) -> String {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let without_fragment = trimmed.split('#').next().unwrap_or(trimmed);
    let without_query = without_fragment
        .split('?')
        .next()
        .unwrap_or(without_fragment);

    if let Some((scheme, remainder)) = without_query.split_once("://") {
        let mut parts = remainder.splitn(2, '/');
        let authority = parts.next().unwrap_or_default();
        let path = parts.next().unwrap_or_default();
        if path.is_empty() {
            format!("{scheme}://{authority}")
        } else {
            format!("{scheme}://{authority}/...")
        }
    } else {
        without_query.to_string()
    }
}

pub struct TaskersHost {
    root: Overlay,
    native_surface_viewport: Fixed,
    native_surface_scene: Fixed,
    event_sink: HostEventSink,
    shell_action_sink: ShellActionSink,
    diagnostics: Option<DiagnosticsSink>,
    ghostty_host: Option<GhosttyHost>,
    ghostty_bridge_info: Option<GhosttyBridgeInfo>,
    ghostty_watchdog: Option<BridgeWatchdog>,
    skip_next_ghostty_tick: bool,
    pending_terminal_create_retry: bool,
    native_surface_provider: CssProvider,
    selected_theme_id: String,
    browser_surfaces: HashMap<SurfaceId, BrowserSurface>,
    persistent_browser_session: Option<NetworkSession>,
    terminal_surfaces: HashMap<SurfaceId, TerminalSurface>,
    resize_handles: HashMap<String, ResizeHandleOverlay>,
    current_portal: Option<SurfacePortalPlan>,
    current_workspace: Option<WorkspaceViewSnapshot>,
}

struct ResizeHandleOverlay {
    widget: GtkBox,
    target: Rc<RefCell<taskers_core::ResizeHandleTarget>>,
    split_gap: Rc<Cell<i32>>,
}

#[derive(Clone, Copy)]
struct DragAnchor {
    start_abs_x: i32,
    start_abs_y: i32,
    pointer_offset_x: i32,
    pointer_offset_y: i32,
}

#[derive(Clone)]
pub struct BrowserSurfaceHandle {
    surface_id: SurfaceId,
    workspace_id: Rc<Cell<WorkspaceId>>,
    pane_id: Rc<Cell<PaneId>>,
    webview: WebView,
    profile_mode: BrowserProfileMode,
    network_session: NetworkSession,
    last_load_state: Rc<Cell<Option<BrowserLoadState>>>,
}

impl BrowserSurfaceHandle {
    pub fn surface_id(&self) -> SurfaceId {
        self.surface_id
    }

    pub fn workspace_id(&self) -> WorkspaceId {
        self.workspace_id.get()
    }

    pub fn pane_id(&self) -> PaneId {
        self.pane_id.get()
    }

    pub(crate) fn webview(&self) -> &WebView {
        &self.webview
    }

    pub(crate) fn url(&self) -> String {
        self.webview
            .uri()
            .map(|uri| uri.to_string())
            .unwrap_or_default()
    }

    pub(crate) fn title(&self) -> String {
        self.webview
            .title()
            .map(|title| title.to_string())
            .unwrap_or_default()
    }

    pub(crate) fn is_loading(&self) -> bool {
        self.webview.is_loading()
    }

    pub(crate) fn load_state(&self) -> Option<BrowserLoadState> {
        self.last_load_state.get()
    }

    pub(crate) fn focus_webview(&self) {
        self.webview.grab_focus();
    }

    pub(crate) fn is_webview_focused(&self) -> bool {
        self.webview.has_focus()
    }

    pub(crate) fn navigate(&self, url: &str) {
        self.webview.load_uri(url);
    }

    pub(crate) fn go_back(&self) {
        if self.webview.can_go_back() {
            self.webview.go_back();
        }
    }

    pub(crate) fn go_forward(&self) {
        if self.webview.can_go_forward() {
            self.webview.go_forward();
        }
    }

    pub(crate) fn reload(&self) {
        self.webview.reload();
    }
}

impl TaskersHost {
    pub fn new(
        shell_widget: &impl IsA<Widget>,
        ghostty_host: Option<GhosttyHost>,
        event_sink: HostEventSink,
        shell_action_sink: ShellActionSink,
        diagnostics: Option<DiagnosticsSink>,
    ) -> Self {
        let ghostty_bridge_info = ghostty_host.as_ref().map(GhosttyHost::bridge_info);
        let ghostty_watchdog = ghostty_bridge_info
            .as_ref()
            .map(|_| BridgeWatchdog::new(diagnostics.clone()));
        let root = Overlay::new();
        root.set_hexpand(true);
        root.set_vexpand(true);
        root.set_child(Some(shell_widget));
        let (native_surface_viewport, native_surface_scene) = build_native_surface_scene_layers();
        root.add_overlay(&native_surface_viewport);
        root.set_measure_overlay(&native_surface_viewport, false);
        root.set_clip_overlay(&native_surface_viewport, false);
        native_surface_viewport.put(&native_surface_scene, 0.0, 0.0);
        let native_surface_provider = install_native_surface_css("dark");

        let pan_sink = event_sink.clone();
        let pan_diagnostics = diagnostics.clone();
        let workspace_pan = EventControllerScroll::new(EventControllerScrollFlags::BOTH_AXES);
        workspace_pan.set_propagation_phase(gtk::PropagationPhase::Capture);
        workspace_pan.connect_scroll(move |_, dx, dy| {
            let Some((dx, dy)) = workspace_pan_delta(dx, dy) else {
                return glib::Propagation::Proceed;
            };
            emit_diagnostic(
                pan_diagnostics.as_ref(),
                DiagnosticRecord::new(
                    DiagnosticCategory::HostEvent,
                    None,
                    format!("workspace pan gesture dx={dx} dy={dy}"),
                ),
            );
            (pan_sink)(HostEvent::ViewportScrolled { dx, dy });
            glib::Propagation::Proceed
        });
        shell_widget.add_controller(workspace_pan);

        emit_diagnostic(
            diagnostics.as_ref(),
            DiagnosticRecord::new(
                DiagnosticCategory::Window,
                None,
                "created GTK4 host overlay",
            ),
        );

        if let Some(watchdog) = ghostty_watchdog.as_ref() {
            watchdog.transition_state(
                diagnostics.as_ref(),
                GhosttyLifecycleState::Running,
                None,
                "ghostty host initialized",
            );
        }

        Self {
            root,
            native_surface_viewport,
            native_surface_scene,
            event_sink,
            shell_action_sink,
            diagnostics,
            ghostty_host,
            ghostty_bridge_info,
            ghostty_watchdog,
            skip_next_ghostty_tick: false,
            pending_terminal_create_retry: false,
            native_surface_provider,
            selected_theme_id: "dark".into(),
            browser_surfaces: HashMap::new(),
            persistent_browser_session: None,
            terminal_surfaces: HashMap::new(),
            resize_handles: HashMap::new(),
            current_portal: None,
            current_workspace: None,
        }
    }

    pub fn widget(&self) -> Overlay {
        self.root.clone()
    }

    pub fn needs_sync_retry(&self) -> bool {
        self.pending_terminal_create_retry
    }

    pub fn sync_snapshot(&mut self, snapshot: &ShellSnapshot) -> Result<()> {
        self.pending_terminal_create_retry = false;
        let interactive = native_surfaces_interactive(
            snapshot.section,
            snapshot.drag_mode,
            snapshot.overview_mode,
        );
        let visible = native_surfaces_visible(snapshot.section, snapshot.drag_mode);
        self.current_portal = Some(snapshot.portal.clone());
        self.current_workspace = Some(snapshot.current_workspace.clone());
        sync_native_surface_scene(
            &self.root,
            &self.native_surface_viewport,
            &self.native_surface_scene,
            &snapshot.portal,
            &snapshot.current_workspace,
            visible,
            interactive,
        );
        if self.selected_theme_id != snapshot.settings.selected_theme_id {
            self.selected_theme_id = snapshot.settings.selected_theme_id.clone();
            update_native_surface_css(&self.native_surface_provider, &self.selected_theme_id);
        }
        emit_diagnostic(
            self.diagnostics.as_ref(),
            DiagnosticRecord::new(
                DiagnosticCategory::Sync,
                Some(snapshot.revision),
                format!("host sync start panes={}", snapshot.portal.panes.len()),
            ),
        );
        self.sync_browser_surfaces(
            snapshot,
            interactive,
            visible,
            snapshot.resize_preview_active,
        )?;
        let _bridge_guard = self.begin_bridge_operation(
            BridgeOperationKind::SurfaceSync,
            Some(snapshot.revision),
            None,
            None,
        );
        let terminal_mutated = match self.sync_terminal_surfaces(
            &snapshot.portal,
            &snapshot.current_workspace,
            &snapshot.terminal_catalog,
            &snapshot.settings.selected_theme_id,
            snapshot.revision,
            interactive,
            visible,
            snapshot.resize_preview_active,
        ) {
            Ok(terminal_mutated) => terminal_mutated,
            Err(error) => {
                self.mark_bridge_failed(
                    Some(snapshot.revision),
                    format!("terminal surface sync failed: {error}"),
                );
                return Err(error);
            }
        };
        self.sync_resize_handles(snapshot);
        if terminal_mutated {
            self.skip_next_ghostty_tick = true;
            emit_diagnostic(
                self.diagnostics.as_ref(),
                DiagnosticRecord::new(
                    DiagnosticCategory::Bridge,
                    Some(snapshot.revision),
                    "deferred ghostty tick after terminal surface mutation",
                ),
            );
        }
        Ok(())
    }

    pub fn tick(&mut self, revision: Option<u64>) {
        if !self.bridge_running() {
            return;
        }
        if self.skip_next_ghostty_tick {
            self.skip_next_ghostty_tick = false;
            emit_diagnostic(
                self.diagnostics.as_ref(),
                DiagnosticRecord::new(
                    DiagnosticCategory::Bridge,
                    revision,
                    "skipping ghostty tick for mutation cooldown",
                ),
            );
            return;
        }

        let Some(host) = &self.ghostty_host else {
            return;
        };
        let _bridge_guard =
            self.begin_bridge_operation(BridgeOperationKind::Tick, revision, None, None);
        if let Err(error) = host.tick() {
            self.mark_bridge_failed(revision, format!("ghostty tick failed: {error}"));
        }
    }

    pub fn bridge_info(&self) -> Option<GhosttyBridgeInfo> {
        self.ghostty_bridge_info.clone()
    }

    pub fn bridge_health_snapshot(&self) -> Option<BridgeHealthSnapshot> {
        let bridge_info = self.ghostty_bridge_info.clone()?;
        let surface_count = self
            .ghostty_host
            .as_ref()
            .map(GhosttyHost::surface_count)
            .unwrap_or_default();
        self.ghostty_watchdog
            .as_ref()
            .map(|watchdog| watchdog.snapshot(bridge_info, surface_count))
    }

    pub fn shutdown(&mut self) {
        let bridge_was_running = self.bridge_running();
        if let Some(watchdog) = self.ghostty_watchdog.as_ref() {
            watchdog.transition_state(
                self.diagnostics.as_ref(),
                GhosttyLifecycleState::ShuttingDown,
                None,
                "window close requested",
            );
        }
        let _bridge_guard =
            self.begin_bridge_operation(BridgeOperationKind::Shutdown, None, None, None);
        self.skip_next_ghostty_tick = false;
        if bridge_was_running {
            if let Some(host) = &self.ghostty_host {
                host.begin_shutdown();
            }
        } else {
            emit_diagnostic(
                self.diagnostics.as_ref(),
                DiagnosticRecord::new(
                    DiagnosticCategory::Bridge,
                    None,
                    "skipping ghostty shutdown calls because bridge is not running",
                ),
            );
        }

        let terminal_surfaces = self.terminal_surfaces.drain().collect::<Vec<_>>();
        for (surface_id, surface) in terminal_surfaces {
            surface.shell.detach(&self.native_surface_scene);
            surface.attention_ring.detach(&self.native_surface_scene);
            if bridge_was_running {
                if let Some(host) = &self.ghostty_host {
                    host.destroy_surface(&surface.widget);
                }
            } else {
                emit_diagnostic(
                    self.diagnostics.as_ref(),
                    DiagnosticRecord::new(
                        DiagnosticCategory::Bridge,
                        None,
                        "skipped terminal surface destroy during shutdown because bridge is not running",
                    )
                    .with_surface(surface_id),
                );
            }
            emit_diagnostic(
                self.diagnostics.as_ref(),
                DiagnosticRecord::new(
                    DiagnosticCategory::SurfaceLifecycle,
                    None,
                    "terminal surface shutdown",
                )
                .with_surface(surface_id),
            );
        }

        let browser_surfaces = self.browser_surfaces.drain().collect::<Vec<_>>();
        for (surface_id, surface) in browser_surfaces {
            surface.shell.detach(&self.native_surface_scene);
            surface.attention_ring.detach(&self.native_surface_scene);
            emit_diagnostic(
                self.diagnostics.as_ref(),
                DiagnosticRecord::new(
                    DiagnosticCategory::SurfaceLifecycle,
                    None,
                    "browser surface shutdown",
                )
                .with_surface(surface_id),
            );
        }

        if let Some(health) = self.bridge_health_snapshot() {
            emit_diagnostic(
                self.diagnostics.as_ref(),
                DiagnosticRecord::new(
                    DiagnosticCategory::Bridge,
                    None,
                    format!(
                        "ghostty bridge shutdown complete state={} surface_count={} last_tick_ms={} last_mutation_ms={}",
                        health.state.label(),
                        health.surface_count,
                        health
                            .last_tick_duration_ms
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "-".into()),
                        health
                            .last_mutation_duration_ms
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "-".into()),
                    ),
                ),
            );
        }
    }

    pub fn handle_command(&mut self, command: HostCommand) -> Result<()> {
        match command {
            HostCommand::BrowserNavigate { surface_id, url } => {
                self.with_browser_surface(surface_id, "browser navigate", |surface| {
                    surface.navigate(&url)
                })
            }
            HostCommand::BrowserBack { surface_id } => {
                self.with_browser_surface(surface_id, "browser back", |surface| surface.go_back())
            }
            HostCommand::BrowserForward { surface_id } => {
                self.with_browser_surface(surface_id, "browser forward", |surface| {
                    surface.go_forward()
                })
            }
            HostCommand::BrowserReload { surface_id } => {
                self.with_browser_surface(surface_id, "browser reload", |surface| surface.reload())
            }
            HostCommand::BrowserToggleDevtools { surface_id } => {
                self.with_browser_surface(surface_id, "browser devtools toggle", |surface| {
                    surface.toggle_devtools()
                })
            }
            HostCommand::BrowserClearData { surface_id } => {
                let handle = self.browser_surface_handle(surface_id)?;
                glib::spawn_future_local(async move {
                    let _ = handle.clear_data(None, true).await;
                });
                Ok(())
            }
            HostCommand::TerminalSendText { surface_id, text } => {
                if !self.bridge_running() {
                    emit_diagnostic(
                        self.diagnostics.as_ref(),
                        DiagnosticRecord::new(
                            DiagnosticCategory::Bridge,
                            None,
                            "skipping terminal send text because ghostty bridge is not running",
                        )
                        .with_surface(surface_id),
                    );
                    return Ok(());
                }
                let Some(host) = self.ghostty_host.as_ref() else {
                    return Ok(());
                };
                let Some(surface) = self.terminal_surfaces.get(&surface_id) else {
                    return Ok(());
                };
                let pane_id = surface.pane_id.get();
                let widget = surface.widget.clone();
                let _bridge_guard = self.begin_bridge_operation(
                    BridgeOperationKind::TerminalCommand,
                    None,
                    Some(pane_id),
                    Some(surface_id),
                );
                if let Err(error) = host.send_surface_text(&widget, &text) {
                    self.mark_bridge_failed(None, format!("terminal send text failed: {error}"));
                    return Err(anyhow!(error.to_string()));
                }
                emit_diagnostic(
                    self.diagnostics.as_ref(),
                    DiagnosticRecord::new(
                        DiagnosticCategory::HostEvent,
                        None,
                        "terminal send text command handled",
                    )
                    .with_surface(surface_id),
                );
                Ok(())
            }
        }
    }

    pub async fn execute_browser_command(
        &self,
        command: BrowserControlCommand,
    ) -> Result<serde_json::Value, ControlError> {
        let surface_id = browser_command_surface_id(&command);
        let handle = self.browser_surface_handle(surface_id)?;
        handle.execute(command).await
    }

    pub fn execute_screenshot(
        &mut self,
        command: ScreenshotCommand,
    ) -> Result<ScreenshotResult, ControlError> {
        let portal = self.current_portal.as_ref().ok_or_else(|| {
            ControlError::not_supported("screenshot capture is unavailable before first host sync")
        })?;
        let workspace = self.current_workspace.as_ref().ok_or_else(|| {
            ControlError::not_supported(
                "workspace screenshot metadata is unavailable before first host sync",
            )
        })?;

        match command {
            ScreenshotCommand::Capture { target, path } => match target {
                ScreenshotTarget::Surface { surface_id } => {
                    let surface = self.terminal_surfaces.get(&surface_id).ok_or_else(|| {
                        ControlError::not_found(format!(
                            "terminal surface {surface_id} is not present in the current host snapshot"
                        ))
                    })?;
                    ensure_visible_workspace(workspace.id, surface.workspace_id.get())?;
                    let plan = portal
                        .panes
                        .iter()
                        .find(|plan| plan.surface_id == surface_id)
                        .ok_or_else(|| {
                            ControlError::not_found(format!(
                                "surface {surface_id} is not visible in the current host snapshot"
                            ))
                        })?;
                    capture_widget_to_png(
                        self.root.upcast_ref(),
                        Some(plan.frame),
                        path,
                        ScreenshotTargetResult::Surface {
                            workspace_id: surface.workspace_id.get(),
                            pane_id: surface.pane_id.get(),
                            surface_id,
                        },
                    )
                }
                ScreenshotTarget::Pane {
                    workspace_id,
                    pane_id,
                } => {
                    ensure_visible_workspace(workspace.id, workspace_id)?;
                    let plan = portal
                        .panes
                        .iter()
                        .find(|plan| plan.pane_id == pane_id)
                        .ok_or_else(|| {
                            ControlError::not_found(format!(
                                "pane {pane_id} is not visible in the current host snapshot"
                            ))
                        })?;
                    capture_widget_to_png(
                        self.root.upcast_ref(),
                        Some(plan.pane_frame),
                        path,
                        ScreenshotTargetResult::Pane {
                            workspace_id,
                            pane_id,
                        },
                    )
                }
                ScreenshotTarget::WorkspaceWindow { workspace_id } => {
                    ensure_visible_workspace(workspace.id, workspace_id)?;
                    let window = workspace
                        .columns
                        .iter()
                        .flat_map(|column| column.windows.iter())
                        .find(|window| window.id == workspace.active_window_id)
                        .ok_or_else(|| {
                            ControlError::not_found(format!(
                                "workspace {workspace_id} has no active workspace window in the current host snapshot"
                            ))
                        })?;
                    capture_widget_to_png(
                        self.root.upcast_ref(),
                        Some(window.frame),
                        path,
                        ScreenshotTargetResult::WorkspaceWindow {
                            workspace_id,
                            workspace_window_id: window.id,
                        },
                    )
                }
                ScreenshotTarget::WorkspaceCanvas { workspace_id } => {
                    ensure_visible_workspace(workspace.id, workspace_id)?;
                    capture_widget_to_png(
                        self.root.upcast_ref(),
                        Some(portal.content),
                        path,
                        ScreenshotTargetResult::WorkspaceCanvas { workspace_id },
                    )
                }
            },
        }
    }

    pub fn execute_terminal_debug(
        &mut self,
        command: TerminalDebugCommand,
    ) -> Result<TerminalDebugResult, ControlError> {
        let Some(host) = self.ghostty_host.as_ref() else {
            return Err(ControlError::not_supported(
                "terminal debug requires the Ghostty host backend",
            ));
        };
        if !self.bridge_running() {
            return Err(ControlError::not_supported(
                "terminal debug is unavailable because the Ghostty bridge is not running",
            ));
        }

        let surface_id = terminal_debug_surface_id(&command);
        let surface = self.terminal_surfaces.get(&surface_id).ok_or_else(|| {
            ControlError::not_found(format!("terminal surface {surface_id} not found"))
        })?;
        let pane_id = surface.pane_id.get();
        let workspace_id = surface.workspace_id.get();
        let widget = surface.widget.clone();
        let focused = surface.is_focused();
        let visible = surface.visible;
        let cols = surface.spec.cols;
        let rows = surface.spec.rows;
        let width_px = surface.width_px;
        let height_px = surface.height_px;

        match command {
            TerminalDebugCommand::IsFocused { .. } => {
                Ok(TerminalDebugResult::IsFocused { focused })
            }
            TerminalDebugCommand::ReadText { tail_lines, .. } => {
                let _bridge_guard = self.begin_bridge_operation(
                    BridgeOperationKind::TerminalDebug,
                    None,
                    Some(pane_id),
                    Some(surface_id),
                );
                let text = match host.read_surface_text(&widget) {
                    Ok(text) => text,
                    Err(error) => {
                        self.mark_bridge_failed(
                            None,
                            format!("terminal debug read failed: {error}"),
                        );
                        return Err(ControlError::internal(error.to_string()));
                    }
                };
                Ok(TerminalDebugResult::ReadText {
                    text: trim_terminal_tail(text, tail_lines),
                })
            }
            TerminalDebugCommand::RenderStats { .. } => {
                let _bridge_guard = self.begin_bridge_operation(
                    BridgeOperationKind::TerminalDebug,
                    None,
                    Some(pane_id),
                    Some(surface_id),
                );
                let has_selection = match host.surface_has_selection(&widget) {
                    Ok(has_selection) => has_selection,
                    Err(error) => {
                        self.mark_bridge_failed(
                            None,
                            format!("terminal debug render stats failed: {error}"),
                        );
                        return Err(ControlError::internal(error.to_string()));
                    }
                };
                Ok(TerminalDebugResult::RenderStats {
                    stats: TerminalRenderStats {
                        surface_id,
                        workspace_id,
                        pane_id,
                        mounted: true,
                        visible,
                        focused,
                        backend: "ghostty".into(),
                        cols,
                        rows,
                        width_px,
                        height_px,
                        resize_count: surface.resize_count,
                        last_resize_revision: surface.last_resize_revision,
                        has_selection,
                    },
                })
            }
        }
    }

    fn bridge_running(&self) -> bool {
        self.ghostty_watchdog
            .as_ref()
            .is_some_and(|watchdog| watchdog.lifecycle_state() == GhosttyLifecycleState::Running)
    }

    fn begin_bridge_operation(
        &self,
        kind: BridgeOperationKind,
        revision: Option<u64>,
        pane_id: Option<PaneId>,
        surface_id: Option<SurfaceId>,
    ) -> Option<BridgeOperationGuard> {
        self.ghostty_watchdog
            .as_ref()
            .map(|watchdog| watchdog.begin(kind, revision, pane_id, surface_id))
    }

    fn mark_bridge_failed(&mut self, revision: Option<u64>, reason: impl Into<String>) {
        if let Some(watchdog) = self.ghostty_watchdog.as_ref() {
            watchdog.transition_state(
                self.diagnostics.as_ref(),
                GhosttyLifecycleState::Failed,
                revision,
                reason,
            );
        }
        self.skip_next_ghostty_tick = false;
    }

    fn with_browser_surface(
        &mut self,
        surface_id: SurfaceId,
        action: &'static str,
        callback: impl FnOnce(&mut BrowserSurface),
    ) -> Result<()> {
        let Some(surface) = self.browser_surfaces.get_mut(&surface_id) else {
            return Ok(());
        };
        callback(surface);
        emit_diagnostic(
            self.diagnostics.as_ref(),
            DiagnosticRecord::new(
                DiagnosticCategory::HostEvent,
                None,
                format!("{action} command handled"),
            )
            .with_surface(surface_id),
        );
        Ok(())
    }

    fn sync_resize_handles(&mut self, snapshot: &ShellSnapshot) {
        let desired_ids = snapshot
            .resize_handles
            .iter()
            .map(|handle| handle.id.clone())
            .collect::<HashSet<_>>();
        let stale_ids = self
            .resize_handles
            .keys()
            .filter(|id| !desired_ids.contains(*id))
            .cloned()
            .collect::<Vec<_>>();

        for handle_id in stale_ids {
            if let Some(handle) = self.resize_handles.remove(&handle_id) {
                handle.detach(&self.root);
            }
        }

        for handle in &snapshot.resize_handles {
            match self.resize_handles.get_mut(&handle.id) {
                Some(existing) => existing.sync(&self.root, handle, snapshot.metrics.split_gap),
                None => {
                    let overlay = ResizeHandleOverlay::new(
                        &self.root,
                        handle,
                        snapshot.metrics.split_gap,
                        self.shell_action_sink.clone(),
                        self.diagnostics.clone(),
                    );
                    self.resize_handles.insert(handle.id.clone(), overlay);
                }
            }
        }
    }

    pub fn browser_surface_handle(
        &self,
        surface_id: SurfaceId,
    ) -> Result<BrowserSurfaceHandle, ControlError> {
        self.browser_surfaces
            .get(&surface_id)
            .map(BrowserSurface::handle)
            .ok_or_else(|| {
                ControlError::not_found(format!("browser surface {surface_id} not found"))
            })
    }

    fn persistent_browser_session(&mut self) -> NetworkSession {
        if let Some(session) = self.persistent_browser_session.clone() {
            return session;
        }

        let session = match build_persistent_browser_session() {
            Ok(session) => session,
            Err(error) => {
                emit_diagnostic(
                    self.diagnostics.as_ref(),
                    DiagnosticRecord::new(
                        DiagnosticCategory::Startup,
                        None,
                        format!(
                            "failed to initialize persistent browser profile: {error}; using an ephemeral browser session"
                        ),
                    ),
                );
                build_ephemeral_browser_session()
            }
        };
        self.persistent_browser_session = Some(session.clone());
        session
    }

    fn browser_network_session_for(&mut self, profile_mode: BrowserProfileMode) -> NetworkSession {
        match profile_mode {
            BrowserProfileMode::PersistentDefault => self.persistent_browser_session(),
            BrowserProfileMode::Ephemeral => build_ephemeral_browser_session(),
        }
    }

    fn remove_browser_surface(&mut self, surface_id: SurfaceId, revision: u64, reason: &str) {
        if let Some(surface) = self.browser_surfaces.remove(&surface_id) {
            surface.shell.detach(&self.native_surface_scene);
            surface.attention_ring.detach(&self.native_surface_scene);
            emit_diagnostic(
                self.diagnostics.as_ref(),
                DiagnosticRecord::new(DiagnosticCategory::SurfaceLifecycle, Some(revision), reason)
                    .with_surface(surface_id),
            );
        }
    }

    fn sync_browser_surfaces(
        &mut self,
        snapshot: &ShellSnapshot,
        interactive: bool,
        visible: bool,
        resize_preview_active: bool,
    ) -> Result<()> {
        let desired = scene_plans_for_kind(
            &snapshot.portal,
            &snapshot.current_workspace,
            PaneKind::Browser,
        );
        let desired_by_id = desired
            .into_iter()
            .map(|plan| (plan.surface_id, plan))
            .collect::<HashMap<_, _>>();
        let catalog_by_id = snapshot
            .browser_catalog
            .iter()
            .map(|entry| (entry.surface_id, entry))
            .collect::<HashMap<_, _>>();
        let desired_ids = catalog_by_id.keys().copied().collect::<HashSet<_>>();

        let stale = self
            .browser_surfaces
            .keys()
            .copied()
            .filter(|surface_id| !desired_ids.contains(surface_id))
            .collect::<Vec<_>>();

        for surface_id in stale {
            self.remove_browser_surface(surface_id, snapshot.revision, "browser surface removed");
        }

        let profile_changed = self
            .browser_surfaces
            .iter()
            .filter_map(|(surface_id, surface)| {
                let entry = catalog_by_id.get(surface_id)?;
                (surface.profile_mode != entry.profile_mode).then_some(*surface_id)
            })
            .collect::<Vec<_>>();
        for surface_id in profile_changed {
            self.remove_browser_surface(
                surface_id,
                snapshot.revision,
                "browser surface recreated for profile mode change",
            );
        }

        for entry in snapshot.browser_catalog.iter() {
            let visible_plan = native_surface_visible_plan(
                if visible {
                    desired_by_id.get(&entry.surface_id)
                } else {
                    None
                },
                resize_preview_active,
            );
            match self.browser_surfaces.get_mut(&entry.surface_id) {
                Some(surface) => surface.sync(
                    &self.native_surface_scene,
                    entry,
                    visible_plan,
                    &snapshot.settings.selected_theme_id,
                    snapshot.revision,
                    interactive,
                    self.diagnostics.as_ref(),
                )?,
                None => {
                    let network_session = self.browser_network_session_for(entry.profile_mode);
                    let surface = BrowserSurface::new(
                        &self.native_surface_scene,
                        entry,
                        visible_plan,
                        &snapshot.settings.selected_theme_id,
                        snapshot.revision,
                        interactive,
                        network_session,
                        self.event_sink.clone(),
                        self.diagnostics.clone(),
                    )?;
                    self.browser_surfaces.insert(entry.surface_id, surface);
                }
            }
        }

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn sync_terminal_surfaces(
        &mut self,
        portal: &SurfacePortalPlan,
        workspace: &WorkspaceViewSnapshot,
        catalog: &[TerminalSurfaceCatalogEntry],
        theme_id: &str,
        revision: u64,
        interactive: bool,
        visible: bool,
        resize_preview_active: bool,
    ) -> Result<bool> {
        let desired = scene_plans_for_kind(portal, workspace, PaneKind::Terminal);
        let desired_by_id = desired
            .into_iter()
            .map(|plan| (plan.surface_id, plan))
            .collect::<HashMap<_, _>>();
        let catalog_by_id = catalog
            .iter()
            .map(|entry| (entry.surface_id, entry))
            .collect::<HashMap<_, _>>();
        let desired_ids = catalog_by_id.keys().copied().collect::<HashSet<_>>();

        let stale = self
            .terminal_surfaces
            .keys()
            .copied()
            .filter(|surface_id| !desired_ids.contains(surface_id))
            .collect::<Vec<_>>();
        let removed_any = !stale.is_empty();

        let host = self.ghostty_host.as_ref();
        let bridge_running = self.bridge_running();
        let mut terminal_mutated = false;

        for surface_id in stale {
            if let Some(surface) = self.terminal_surfaces.remove(&surface_id) {
                surface.shell.detach(&self.native_surface_scene);
                surface.attention_ring.detach(&self.native_surface_scene);
                if bridge_running {
                    if let Some(host) = host {
                        host.destroy_surface(&surface.widget);
                    }
                } else {
                    emit_diagnostic(
                        self.diagnostics.as_ref(),
                        DiagnosticRecord::new(
                            DiagnosticCategory::Bridge,
                            Some(revision),
                            "skipped terminal surface destroy because ghostty bridge is not running",
                        )
                        .with_surface(surface_id),
                    );
                }
                emit_diagnostic(
                    self.diagnostics.as_ref(),
                    DiagnosticRecord::new(
                        DiagnosticCategory::SurfaceLifecycle,
                        Some(revision),
                        "terminal surface removed",
                    )
                    .with_surface(surface_id),
                );
                terminal_mutated = true;
            }
        }

        for entry in catalog {
            let visible_plan = native_surface_visible_plan(
                if visible {
                    desired_by_id.get(&entry.surface_id)
                } else {
                    None
                },
                resize_preview_active,
            );
            match self.terminal_surfaces.get_mut(&entry.surface_id) {
                Some(surface) => surface.sync(
                    &self.native_surface_scene,
                    entry,
                    visible_plan,
                    theme_id,
                    revision,
                    interactive,
                    resize_preview_active,
                    host.filter(|_| bridge_running),
                    self.diagnostics.as_ref(),
                ),
                None => {
                    if !bridge_running {
                        emit_diagnostic(
                            self.diagnostics.as_ref(),
                            DiagnosticRecord::new(
                                DiagnosticCategory::Bridge,
                                Some(revision),
                                "skipping terminal surface create because ghostty bridge is not running",
                            )
                            .with_pane(entry.pane_id)
                            .with_surface(entry.surface_id),
                        );
                        continue;
                    }
                    let Some(host) = host else {
                        continue;
                    };
                    let bridge_surface_count = host.surface_count();
                    let live_surface_count = self.terminal_surfaces.len();
                    match terminal_surface_create_decision(
                        removed_any,
                        live_surface_count,
                        bridge_surface_count,
                    ) {
                        TerminalSurfaceCreateDecision::DeferUntilBridgeQuiesces => {
                            self.pending_terminal_create_retry = true;
                            emit_diagnostic(
                                self.diagnostics.as_ref(),
                                DiagnosticRecord::new(
                                    DiagnosticCategory::Bridge,
                                    Some(revision),
                                    format!(
                                        "deferring terminal surface create until ghostty bridge quiesces bridge_surface_count={} live_surface_count={}",
                                        bridge_surface_count, live_surface_count
                                    ),
                                )
                                .with_pane(entry.pane_id)
                                .with_surface(entry.surface_id),
                            );
                            continue;
                        }
                        TerminalSurfaceCreateDecision::SkipAtSurfaceBudget => {
                            emit_diagnostic(
                                self.diagnostics.as_ref(),
                                DiagnosticRecord::new(
                                    DiagnosticCategory::Bridge,
                                    Some(revision),
                                    format!(
                                        "skipping terminal surface create because embedded ghostty surface budget reached surface_limit={} bridge_surface_count={} live_surface_count={}",
                                        MAX_CONCURRENT_GHOSTTY_SURFACES,
                                        bridge_surface_count,
                                        live_surface_count
                                    ),
                                )
                                .with_pane(entry.pane_id)
                                .with_surface(entry.surface_id),
                            );
                            continue;
                        }
                        TerminalSurfaceCreateDecision::CreateNow => {}
                    }
                    let surface = TerminalSurface::new(
                        &self.native_surface_scene,
                        entry,
                        visible_plan,
                        theme_id,
                        revision,
                        interactive,
                        resize_preview_active,
                        self.event_sink.clone(),
                        self.diagnostics.clone(),
                        host,
                    )?;
                    self.terminal_surfaces.insert(entry.surface_id, surface);
                    terminal_mutated = true;
                }
            }
        }

        Ok(terminal_mutated)
    }
}

fn build_native_surface_scene_layers() -> (Fixed, Fixed) {
    let native_surface_viewport = Fixed::new();
    native_surface_viewport.set_overflow(Overflow::Hidden);
    native_surface_viewport.set_hexpand(false);
    native_surface_viewport.set_vexpand(false);
    native_surface_viewport.set_halign(Align::Start);
    native_surface_viewport.set_valign(Align::Start);
    native_surface_viewport.set_focusable(false);
    native_surface_viewport.set_can_target(false);

    let native_surface_scene = Fixed::new();
    native_surface_scene.set_hexpand(false);
    native_surface_scene.set_vexpand(false);
    native_surface_scene.set_halign(Align::Start);
    native_surface_scene.set_valign(Align::Start);
    native_surface_scene.set_focusable(false);
    native_surface_scene.set_can_target(false);

    (native_surface_viewport, native_surface_scene)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TerminalSurfaceCreateDecision {
    CreateNow,
    DeferUntilBridgeQuiesces,
    SkipAtSurfaceBudget,
}

fn terminal_surface_create_decision(
    removed_any: bool,
    live_surface_count: usize,
    bridge_surface_count: usize,
) -> TerminalSurfaceCreateDecision {
    if removed_any || bridge_surface_count > live_surface_count {
        return TerminalSurfaceCreateDecision::DeferUntilBridgeQuiesces;
    }
    if live_surface_count >= MAX_CONCURRENT_GHOSTTY_SURFACES {
        return TerminalSurfaceCreateDecision::SkipAtSurfaceBudget;
    }
    TerminalSurfaceCreateDecision::CreateNow
}

impl ResizeHandleOverlay {
    fn new(
        overlay: &Overlay,
        handle: &taskers_core::ResizeHandleSnapshot,
        split_gap: i32,
        shell_action_sink: ShellActionSink,
        diagnostics: Option<DiagnosticsSink>,
    ) -> Self {
        let widget = GtkBox::new(Orientation::Vertical, 0);
        widget.add_css_class("resize-handle");
        widget.add_css_class(resize_handle_class(handle.cursor));
        widget.set_focusable(false);
        widget.set_can_target(true);
        widget.set_cursor_from_name(Some(resize_cursor_name(handle.cursor)));

        let target = Rc::new(RefCell::new(handle.target.clone()));
        let split_gap_cell = Rc::new(Cell::new(split_gap));
        let active_target = Rc::new(RefCell::new(None::<taskers_core::ResizeHandleTarget>));
        let active_split_gap = Rc::new(Cell::new(split_gap));
        let drag_anchor = Rc::new(Cell::new(None::<DragAnchor>));

        let drag = GestureDrag::new();
        let active_widget = widget.clone();
        let active_target_for_begin = active_target.clone();
        let target_for_begin = target.clone();
        let split_gap_for_begin = split_gap_cell.clone();
        let active_split_gap_for_begin = active_split_gap.clone();
        let drag_anchor_for_begin = drag_anchor.clone();
        let begin_diagnostics = diagnostics.clone();
        let begin_id = handle.id.clone();
        drag.connect_drag_begin(move |_, start_x, start_y| {
            active_widget.add_css_class("resize-handle-active");
            *active_target_for_begin.borrow_mut() = Some(target_for_begin.borrow().clone());
            active_split_gap_for_begin.set(split_gap_for_begin.get());
            let pointer_offset_x = start_x.round() as i32;
            let pointer_offset_y = start_y.round() as i32;
            drag_anchor_for_begin.set(Some(DragAnchor {
                start_abs_x: active_widget.margin_start() + pointer_offset_x,
                start_abs_y: active_widget.margin_top() + pointer_offset_y,
                pointer_offset_x,
                pointer_offset_y,
            }));
            emit_diagnostic(
                begin_diagnostics.as_ref(),
                DiagnosticRecord::new(
                    DiagnosticCategory::HostEvent,
                    None,
                    format!("resize drag begin handle={begin_id}"),
                ),
            );
        });

        let preview_target = active_target.clone();
        let preview_split_gap = active_split_gap.clone();
        let preview_widget = widget.clone();
        let preview_anchor = drag_anchor.clone();
        let preview_sink = shell_action_sink.clone();
        drag.connect_drag_update(move |_, dx, dy| {
            let Some(target) = preview_target.borrow().as_ref().cloned() else {
                return;
            };
            let Some(anchor) = preview_anchor.get() else {
                return;
            };
            let (dx, dy) = corrected_drag_delta(&preview_widget, anchor, dx, dy);
            if let Some(preview) = preview_for_drag(&target, preview_split_gap.get(), dx, dy) {
                (preview_sink)(taskers_core::ShellAction::PreviewResize { preview });
            }
        });

        let end_widget = widget.clone();
        let end_target = active_target;
        let end_split_gap = active_split_gap;
        let end_anchor = drag_anchor;
        let end_sink = shell_action_sink;
        let end_diagnostics = diagnostics;
        let end_id = handle.id.clone();
        drag.connect_drag_end(move |_, dx, dy| {
            end_widget.remove_css_class("resize-handle-active");
            let anchor = end_anchor.take();
            let active_target = end_target.borrow_mut().take();
            let Some(target) = active_target else {
                (end_sink)(taskers_core::ShellAction::CancelResizePreview);
                return;
            };
            let (dx, dy) = anchor
                .map(|anchor| corrected_drag_delta(&end_widget, anchor, dx, dy))
                .unwrap_or((dx, dy));
            let preview = preview_for_drag(&target, end_split_gap.get(), dx, dy);
            emit_diagnostic(
                end_diagnostics.as_ref(),
                DiagnosticRecord::new(
                    DiagnosticCategory::HostEvent,
                    None,
                    format!("resize drag end handle={end_id}"),
                ),
            );
            if let Some(preview) = preview {
                (end_sink)(taskers_core::ShellAction::PreviewResize { preview });
                (end_sink)(taskers_core::ShellAction::CommitResizePreview);
            } else {
                (end_sink)(taskers_core::ShellAction::CancelResizePreview);
            }
        });
        widget.add_controller(drag);
        position_widget(overlay, widget.upcast_ref(), handle.frame);

        Self {
            widget,
            target,
            split_gap: split_gap_cell,
        }
    }

    fn sync(
        &mut self,
        overlay: &Overlay,
        handle: &taskers_core::ResizeHandleSnapshot,
        split_gap: i32,
    ) {
        *self.target.borrow_mut() = handle.target.clone();
        self.split_gap.set(split_gap);
        self.widget
            .set_cursor_from_name(Some(resize_cursor_name(handle.cursor)));
        position_widget(overlay, self.widget.upcast_ref(), handle.frame);
    }

    fn detach(self, overlay: &Overlay) {
        detach_from_overlay(overlay, self.widget.upcast_ref());
    }
}

struct BrowserSurface {
    shell: NativeSurfaceShell,
    attention_ring: AttentionRingOverlay,
    surface_id: SurfaceId,
    workspace_id: Rc<Cell<WorkspaceId>>,
    pane_id: Rc<Cell<PaneId>>,
    webview: WebView,
    focus_state: Rc<Cell<bool>>,
    network_session: NetworkSession,
    profile_mode: BrowserProfileMode,
    url: String,
    active: bool,
    interactive: bool,
    visible: bool,
    devtools_open: Rc<Cell<bool>>,
    last_load_state: Rc<Cell<Option<BrowserLoadState>>>,
    event_sink: HostEventSink,
    diagnostics: Option<DiagnosticsSink>,
}

impl BrowserSurface {
    #[allow(clippy::too_many_arguments)]
    fn new(
        scene: &Fixed,
        entry: &BrowserSurfaceCatalogEntry,
        visible_plan: Option<&PortalSurfacePlan>,
        theme_id: &str,
        revision: u64,
        interactive: bool,
        network_session: NetworkSession,
        event_sink: HostEventSink,
        diagnostics: Option<DiagnosticsSink>,
    ) -> Result<Self> {
        let url = entry.url.clone();

        let settings = WebKitSettings::builder()
            .enable_back_forward_navigation_gestures(true)
            .enable_developer_extras(true)
            .build();
        apply_taskers_webkit_graphics_fallback(&settings);
        let webview = WebView::builder()
            .hexpand(true)
            .vexpand(true)
            .focusable(true)
            .network_session(&network_session)
            .settings(&settings)
            .build();
        let (shell_class, widget_class) = native_surface_classes(PaneKind::Browser);
        webview.add_css_class("native-surface-widget");
        webview.add_css_class(widget_class);
        webview.set_can_target(visible_plan.is_some() && interactive);
        webview.load_uri(&url);
        (event_sink)(HostEvent::SurfaceUrlChanged {
            surface_id: entry.surface_id,
            url: url.clone(),
        });
        let shell = NativeSurfaceShell::new(
            webview.upcast_ref(),
            shell_class,
            visible_plan.is_some() && interactive,
        );
        let attention_ring = AttentionRingOverlay::new();
        match visible_plan {
            Some(plan) => {
                shell.show_at(scene, plan.frame);
                attention_ring.show_at(scene, plan.pane_frame, plan.notification_ring, theme_id);
            }
            None => {
                shell.park_hidden(scene);
                attention_ring.park_hidden(scene);
            }
        }
        let devtools_open = Rc::new(Cell::new(false));
        let workspace_id = Rc::new(Cell::new(entry.workspace_id));
        let pane_id = Rc::new(Cell::new(entry.pane_id));
        let focus_state = Rc::new(Cell::new(false));
        let last_load_state = Rc::new(Cell::new(None));

        let focus_pane_id = pane_id.clone();
        let surface_id = entry.surface_id;
        let focus_sink = event_sink.clone();
        let focus_diagnostics = diagnostics.clone();
        let focus_state_for_enter = focus_state.clone();
        let focus = EventControllerFocus::new();
        focus.connect_enter(move |_| {
            let pane_id = focus_pane_id.get();
            focus_state_for_enter.set(true);
            // Rely on the native widget's own focus transition instead of a
            // synthetic click handler so terminal/browser content keeps the
            // full mouse sequence for itself.
            emit_diagnostic(
                focus_diagnostics.as_ref(),
                DiagnosticRecord::new(
                    DiagnosticCategory::HostEvent,
                    None,
                    "browser focus event received",
                )
                .with_pane(pane_id)
                .with_surface(surface_id),
            );
            (focus_sink)(HostEvent::PaneFocused { pane_id });
        });
        let focus_state_for_leave = focus_state.clone();
        focus.connect_leave(move |_| {
            focus_state_for_leave.set(false);
        });
        webview.add_controller(focus);

        let surface_id = entry.surface_id;
        let title_sink = event_sink.clone();
        let title_diagnostics = diagnostics.clone();
        webview.connect_title_notify(move |web_view| {
            if let Some(title) = web_view.title() {
                emit_diagnostic(
                    title_diagnostics.as_ref(),
                    DiagnosticRecord::new(
                        DiagnosticCategory::BrowserMetadata,
                        None,
                        format!("browser title observed: {title}"),
                    )
                    .with_surface(surface_id),
                );
                (title_sink)(HostEvent::SurfaceTitleChanged {
                    surface_id,
                    title: title.to_string(),
                });
            }
        });

        let surface_id = entry.surface_id;
        let navigation_sink = event_sink.clone();
        let navigation_diagnostics = diagnostics.clone();
        let navigation_devtools = devtools_open.clone();
        let load_state_cell = last_load_state.clone();
        webview.connect_load_changed(move |web_view, load_event| {
            load_state_cell.set(Some(browser_load_state_from_webkit(load_event)));
            emit_browser_navigation_state(
                web_view,
                surface_id,
                navigation_devtools.get(),
                &navigation_sink,
                navigation_diagnostics.as_ref(),
            );
        });

        let url_surface_id = entry.surface_id;
        let url_sink = event_sink;
        let uri_sink = url_sink.clone();
        let url_diagnostics = diagnostics.clone();
        let navigation_sink = url_sink.clone();
        let navigation_diagnostics = diagnostics.clone();
        let navigation_devtools = devtools_open.clone();
        webview.connect_uri_notify(move |web_view| {
            if let Some(url) = web_view.uri() {
                emit_diagnostic(
                    url_diagnostics.as_ref(),
                    DiagnosticRecord::new(
                        DiagnosticCategory::BrowserMetadata,
                        None,
                        format!(
                            "browser url observed: {}",
                            redacted_browser_url_for_diagnostics(url.as_str())
                        ),
                    )
                    .with_surface(url_surface_id),
                );
                (uri_sink)(HostEvent::SurfaceUrlChanged {
                    surface_id: url_surface_id,
                    url: url.to_string(),
                });
            }
            emit_browser_navigation_state(
                web_view,
                url_surface_id,
                navigation_devtools.get(),
                &navigation_sink,
                navigation_diagnostics.as_ref(),
            );
        });

        if let Some(inspector) = webview.inspector() {
            let navigation_webview = webview.clone();
            let navigation_sink = url_sink.clone();
            let navigation_diagnostics = diagnostics.clone();
            let navigation_devtools = devtools_open.clone();
            let inspector_surface_id = entry.surface_id;
            inspector.connect_closed(move |_| {
                navigation_devtools.set(false);
                emit_browser_navigation_state(
                    &navigation_webview,
                    inspector_surface_id,
                    false,
                    &navigation_sink,
                    navigation_diagnostics.as_ref(),
                );
            });
        }

        if visible_plan.is_some_and(|plan| plan.active) && interactive {
            webview.grab_focus();
        }

        emit_diagnostic(
            diagnostics.as_ref(),
            DiagnosticRecord::new(
                DiagnosticCategory::SurfaceLifecycle,
                Some(revision),
                "browser surface created",
            )
            .with_pane(entry.pane_id)
            .with_surface(entry.surface_id),
        );

        emit_browser_navigation_state(
            &webview,
            entry.surface_id,
            devtools_open.get(),
            &url_sink,
            diagnostics.as_ref(),
        );

        Ok(Self {
            shell,
            attention_ring,
            surface_id: entry.surface_id,
            workspace_id,
            pane_id,
            webview,
            focus_state,
            network_session,
            profile_mode: entry.profile_mode,
            url,
            active: visible_plan.is_some_and(|plan| plan.active),
            interactive: visible_plan.is_some() && interactive,
            visible: visible_plan.is_some(),
            devtools_open,
            last_load_state,
            event_sink: url_sink,
            diagnostics,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn sync(
        &mut self,
        scene: &Fixed,
        entry: &BrowserSurfaceCatalogEntry,
        visible_plan: Option<&PortalSurfacePlan>,
        theme_id: &str,
        revision: u64,
        interactive: bool,
        diagnostics: Option<&DiagnosticsSink>,
    ) -> Result<()> {
        self.workspace_id.set(entry.workspace_id);
        self.pane_id.set(entry.pane_id);
        self.profile_mode = entry.profile_mode;
        let visible = visible_plan.is_some();
        let effective_interactive = visible && interactive;
        self.shell.set_interactive(effective_interactive);
        self.webview.set_can_target(effective_interactive);
        match visible_plan {
            Some(plan) => {
                self.shell.show_at(scene, plan.frame);
                self.attention_ring.show_at(
                    scene,
                    plan.pane_frame,
                    plan.notification_ring,
                    theme_id,
                );
            }
            None => {
                self.shell.park_hidden(scene);
                self.attention_ring.park_hidden(scene);
            }
        }

        if self.url != entry.url {
            self.webview.load_uri(&entry.url);
            self.url = entry.url.clone();
        }
        if visible_plan.is_some_and(|plan| plan.active)
            && effective_interactive
            && (!self.active || !self.interactive || !self.visible)
        {
            self.webview.grab_focus();
        }
        if !visible || !effective_interactive {
            self.focus_state.set(false);
        }
        self.active = visible_plan.is_some_and(|plan| plan.active);
        self.interactive = effective_interactive;
        self.visible = visible;

        emit_diagnostic(
            diagnostics,
            DiagnosticRecord::new(
                DiagnosticCategory::SurfaceLifecycle,
                Some(revision),
                "browser surface updated",
            )
            .with_pane(entry.pane_id)
            .with_surface(entry.surface_id),
        );

        Ok(())
    }

    fn navigate(&mut self, url: &str) {
        if self.url != url {
            self.webview.load_uri(url);
            self.url = url.to_string();
        }
        self.emit_navigation_state();
    }

    fn go_back(&mut self) {
        if self.webview.can_go_back() {
            self.webview.go_back();
        }
        self.emit_navigation_state();
    }

    fn go_forward(&mut self) {
        if self.webview.can_go_forward() {
            self.webview.go_forward();
        }
        self.emit_navigation_state();
    }

    fn reload(&mut self) {
        self.webview.reload();
        self.emit_navigation_state();
    }

    fn toggle_devtools(&mut self) {
        let Some(inspector) = self.webview.inspector() else {
            return;
        };
        if self.devtools_open.get() {
            inspector.close();
        } else {
            if inspector.can_attach() {
                inspector.attach();
            }
            inspector.show();
            self.devtools_open.set(true);
        }
        self.emit_navigation_state();
    }

    fn emit_navigation_state(&self) {
        emit_browser_navigation_state(
            &self.webview,
            self.surface_id,
            self.devtools_open.get(),
            &self.event_sink,
            self.diagnostics.as_ref(),
        );
    }

    fn handle(&self) -> BrowserSurfaceHandle {
        BrowserSurfaceHandle {
            surface_id: self.surface_id,
            workspace_id: self.workspace_id.clone(),
            pane_id: self.pane_id.clone(),
            webview: self.webview.clone(),
            profile_mode: self.profile_mode,
            network_session: self.network_session.clone(),
            last_load_state: self.last_load_state.clone(),
        }
    }
}

fn apply_taskers_webkit_graphics_fallback(settings: &WebKitSettings) {
    if std::env::var_os(WEBKIT_DISABLE_DMABUF_RENDERER_ENV).is_some_and(|value| value != "0") {
        settings.set_hardware_acceleration_policy(HardwareAccelerationPolicy::Never);
    }
}

struct TerminalSurface {
    surface_id: SurfaceId,
    workspace_id: Rc<Cell<WorkspaceId>>,
    pane_id: Rc<Cell<PaneId>>,
    spec: TerminalMountSpec,
    shell: NativeSurfaceShell,
    attention_ring: AttentionRingOverlay,
    widget: Widget,
    focus_state: Rc<Cell<bool>>,
    active: bool,
    interactive: bool,
    visible: bool,
    width_px: i32,
    height_px: i32,
    resize_count: u64,
    last_resize_revision: Option<u64>,
    resize_frozen: bool,
}

impl TerminalSurface {
    #[allow(clippy::too_many_arguments)]
    fn new(
        scene: &Fixed,
        entry: &TerminalSurfaceCatalogEntry,
        visible_plan: Option<&PortalSurfacePlan>,
        theme_id: &str,
        revision: u64,
        interactive: bool,
        resize_preview_active: bool,
        event_sink: HostEventSink,
        diagnostics: Option<DiagnosticsSink>,
        host: &GhosttyHost,
    ) -> Result<Self> {
        let spec = entry.spec.clone();
        let descriptor = surface_descriptor_from(&spec);
        let widget = host
            .create_surface(&descriptor)
            .map_err(|error| anyhow!(error.to_string()))?;
        widget.set_hexpand(true);
        widget.set_vexpand(true);
        widget.set_halign(Align::Fill);
        widget.set_valign(Align::Fill);
        widget.set_focusable(true);
        let (shell_class, widget_class) = native_surface_classes(PaneKind::Terminal);
        widget.add_css_class("native-surface-widget");
        widget.add_css_class(widget_class);
        widget.add_css_class("terminal-output");
        let effective_interactive = visible_plan.is_some() && interactive;
        widget.set_can_target(effective_interactive);
        let shell = NativeSurfaceShell::new(&widget, shell_class, effective_interactive);
        let attention_ring = AttentionRingOverlay::new();
        let initial_width_px = visible_plan.map_or(0, |plan| plan.frame.width);
        let initial_height_px = visible_plan.map_or(0, |plan| plan.frame.height);
        let mut resize_frozen = false;
        if resize_preview_active && visible_plan.is_some() {
            freeze_terminal_widget(&widget, initial_width_px, initial_height_px);
            resize_frozen = true;
        }
        match visible_plan {
            Some(plan) => {
                shell.show_at(scene, plan.frame);
                attention_ring.show_at(scene, plan.pane_frame, plan.notification_ring, theme_id);
            }
            None => {
                shell.park_hidden(scene);
                attention_ring.park_hidden(scene);
            }
        }

        let workspace_id = Rc::new(Cell::new(entry.workspace_id));
        let pane_id = Rc::new(Cell::new(entry.pane_id));
        let focus_state = Rc::new(Cell::new(false));
        connect_ghostty_widget(
            &widget,
            pane_id.clone(),
            entry.surface_id,
            event_sink,
            diagnostics.clone(),
            focus_state.clone(),
        );

        if visible_plan.is_some_and(|plan| plan.active) && effective_interactive {
            let _ = host.focus_surface(&widget);
        }

        emit_diagnostic(
            diagnostics.as_ref(),
            DiagnosticRecord::new(
                DiagnosticCategory::SurfaceLifecycle,
                Some(revision),
                "terminal surface created",
            )
            .with_pane(entry.pane_id)
            .with_surface(entry.surface_id),
        );

        Ok(Self {
            surface_id: entry.surface_id,
            workspace_id,
            pane_id,
            spec,
            shell,
            attention_ring,
            widget,
            focus_state,
            active: visible_plan.is_some_and(|plan| plan.active),
            interactive: effective_interactive,
            visible: visible_plan.is_some(),
            width_px: initial_width_px,
            height_px: initial_height_px,
            resize_count: 0,
            last_resize_revision: None,
            resize_frozen,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn sync(
        &mut self,
        scene: &Fixed,
        entry: &TerminalSurfaceCatalogEntry,
        visible_plan: Option<&PortalSurfacePlan>,
        theme_id: &str,
        revision: u64,
        interactive: bool,
        resize_preview_active: bool,
        host: Option<&GhosttyHost>,
        diagnostics: Option<&DiagnosticsSink>,
    ) {
        self.workspace_id.set(entry.workspace_id);
        self.pane_id.set(entry.pane_id);
        self.spec = entry.spec.clone();
        let visible = visible_plan.is_some();
        let effective_interactive = visible && interactive;
        self.widget.set_can_target(effective_interactive);
        self.shell.set_interactive(effective_interactive);
        if resize_preview_active && visible {
            if !self.resize_frozen {
                freeze_terminal_widget(&self.widget, self.width_px, self.height_px);
                self.resize_frozen = true;
            }
        } else if self.resize_frozen {
            thaw_terminal_widget(&self.widget);
            self.resize_frozen = false;
        }
        match visible_plan {
            Some(plan) => {
                self.shell.show_at(scene, plan.frame);
                self.attention_ring.show_at(
                    scene,
                    plan.pane_frame,
                    plan.notification_ring,
                    theme_id,
                );
            }
            None => {
                self.shell.park_hidden(scene);
                self.attention_ring.park_hidden(scene);
            }
        }
        if visible_plan.is_some_and(|plan| plan.active)
            && effective_interactive
            && (!self.active || !self.interactive || !self.visible)
            && let Some(host) = host
        {
            let _ = host.focus_surface(&self.widget);
        }
        if !visible || !effective_interactive {
            self.focus_state.set(false);
        }
        self.active = visible_plan.is_some_and(|plan| plan.active);
        self.interactive = effective_interactive;
        self.visible = visible;
        let next_width_px = visible_plan.map_or(0, |plan| plan.frame.width);
        let next_height_px = visible_plan.map_or(0, |plan| plan.frame.height);
        if !resize_preview_active {
            if next_width_px != self.width_px || next_height_px != self.height_px {
                self.resize_count = self.resize_count.saturating_add(1);
                self.last_resize_revision = Some(revision);
            }
            self.width_px = next_width_px;
            self.height_px = next_height_px;
        }

        emit_diagnostic(
            diagnostics,
            DiagnosticRecord::new(
                DiagnosticCategory::SurfaceLifecycle,
                Some(revision),
                "terminal surface updated",
            )
            .with_pane(entry.pane_id)
            .with_surface(self.surface_id),
        );
    }

    fn is_focused(&self) -> bool {
        self.focus_state.get() || self.widget.has_focus()
    }
}

fn freeze_terminal_widget(widget: &Widget, width_px: i32, height_px: i32) {
    widget.set_hexpand(false);
    widget.set_vexpand(false);
    widget.set_halign(Align::Start);
    widget.set_valign(Align::Start);
    widget.set_size_request(width_px.max(1), height_px.max(1));
    widget.queue_allocate();
}

fn thaw_terminal_widget(widget: &Widget) {
    widget.set_hexpand(true);
    widget.set_vexpand(true);
    widget.set_halign(Align::Fill);
    widget.set_valign(Align::Fill);
    widget.set_size_request(-1, -1);
    widget.queue_allocate();
}

struct NativeSurfaceShell {
    widget: Widget,
}

impl NativeSurfaceShell {
    fn new(widget: &Widget, kind_class: &'static str, interactive: bool) -> Self {
        widget.set_hexpand(false);
        widget.set_vexpand(false);
        widget.set_halign(Align::Start);
        widget.set_valign(Align::Start);
        widget.set_overflow(Overflow::Hidden);
        widget.set_can_target(native_surface_shell_can_target(interactive));
        widget.add_css_class("native-surface-host");
        widget.add_css_class(kind_class);
        Self {
            widget: widget.clone(),
        }
    }

    fn position(&self, scene: &Fixed, frame: taskers_core::Frame) {
        position_widget_in_fixed(scene, &self.widget, frame);
    }

    fn show_at(&self, scene: &Fixed, frame: taskers_core::Frame) {
        self.widget.set_opacity(1.0);
        self.position(scene, frame);
    }

    fn park_hidden(&self, scene: &Fixed) {
        self.widget.set_opacity(0.0);
        self.position(scene, self.hidden_frame());
    }

    fn set_interactive(&self, interactive: bool) {
        self.widget
            .set_can_target(native_surface_shell_can_target(interactive));
    }

    fn detach(&self, scene: &Fixed) {
        detach_from_fixed(scene, &self.widget);
    }

    fn hidden_frame(&self) -> taskers_core::Frame {
        hidden_frame()
    }
}

struct AttentionRingOverlay {
    widget: DrawingArea,
    state: Rc<Cell<Option<taskers_core::AttentionRingState>>>,
    palette: Rc<RefCell<HostAttentionPalette>>,
}

impl AttentionRingOverlay {
    fn new() -> Self {
        let widget = DrawingArea::new();
        widget.set_hexpand(true);
        widget.set_vexpand(true);
        widget.set_halign(Align::Fill);
        widget.set_valign(Align::Fill);
        widget.set_can_target(false);
        widget.set_focusable(false);
        widget.set_visible(false);

        let state = Rc::new(Cell::new(None));
        let palette = Rc::new(RefCell::new(host_attention_palette("dark")));
        let draw_state = state.clone();
        let draw_palette = palette.clone();
        widget.set_draw_func(move |_, ctx, width, height| {
            let Some(state) = draw_state.get() else {
                return;
            };
            let width = width.max(1) as f64;
            let height = height.max(1) as f64;
            if width <= 4.0 || height <= 4.0 {
                return;
            }

            let paint = draw_palette.borrow().paint(state);
            let glow_inset = 3.0;
            draw_attention_ring_path(
                ctx,
                glow_inset,
                glow_inset,
                (width - glow_inset * 2.0).max(1.0),
                (height - glow_inset * 2.0).max(1.0),
                7.0,
            );
            apply_host_rgba(ctx, paint.glow);
            ctx.set_line_width(6.0);
            let _ = ctx.stroke();

            let stroke_inset = 2.0;
            draw_attention_ring_path(
                ctx,
                stroke_inset,
                stroke_inset,
                (width - stroke_inset * 2.0).max(1.0),
                (height - stroke_inset * 2.0).max(1.0),
                6.0,
            );
            apply_host_rgba(ctx, paint.stroke);
            ctx.set_line_width(2.5);
            let _ = ctx.stroke();
        });

        Self {
            widget,
            state,
            palette,
        }
    }

    fn show_at(
        &self,
        scene: &Fixed,
        frame: taskers_core::Frame,
        state: Option<taskers_core::AttentionRingState>,
        theme_id: &str,
    ) {
        self.state.set(state);
        *self.palette.borrow_mut() = host_attention_palette(theme_id);
        self.widget.set_visible(state.is_some());
        if state.is_some() {
            self.widget.set_opacity(1.0);
            position_widget_in_fixed(scene, self.widget.upcast_ref(), frame);
        } else {
            self.park_hidden(scene);
        }
        self.widget.queue_draw();
    }

    fn park_hidden(&self, scene: &Fixed) {
        self.widget.set_visible(false);
        self.widget.set_opacity(0.0);
        position_widget_in_fixed(scene, self.widget.upcast_ref(), hidden_frame());
    }

    fn detach(&self, scene: &Fixed) {
        detach_from_fixed(scene, self.widget.upcast_ref());
    }
}

#[derive(Clone, Copy)]
struct HostRgba {
    red: f64,
    green: f64,
    blue: f64,
    alpha: f64,
}

#[derive(Clone, Copy)]
struct HostRingPaint {
    stroke: HostRgba,
    glow: HostRgba,
}

#[derive(Clone, Copy)]
struct HostAttentionPalette {
    waiting: HostRingPaint,
    error: HostRingPaint,
    completed: HostRingPaint,
}

impl HostAttentionPalette {
    fn paint(self, state: taskers_core::AttentionRingState) -> HostRingPaint {
        match state {
            taskers_core::AttentionRingState::Waiting => self.waiting,
            taskers_core::AttentionRingState::Error => self.error,
            taskers_core::AttentionRingState::Completed => self.completed,
        }
    }
}

fn host_attention_palette(theme_id: &str) -> HostAttentionPalette {
    match theme_id {
        "catppuccin-mocha" => HostAttentionPalette {
            waiting: host_ring_paint(0x94, 0xe2, 0xd5),
            error: host_ring_paint(0xf3, 0x8b, 0xa8),
            completed: host_ring_paint(0xa6, 0xe3, 0xa1),
        },
        "tokyo-night" => HostAttentionPalette {
            waiting: host_ring_paint(0x7d, 0xcf, 0xff),
            error: host_ring_paint(0xf7, 0x76, 0x8e),
            completed: host_ring_paint(0x9e, 0xce, 0x6a),
        },
        "gruvbox-dark" => HostAttentionPalette {
            waiting: host_ring_paint(0x8e, 0xc0, 0x7c),
            error: host_ring_paint(0xfb, 0x49, 0x34),
            completed: host_ring_paint(0xb8, 0xbb, 0x26),
        },
        _ => HostAttentionPalette {
            waiting: host_ring_paint(0x60, 0xa5, 0xfa),
            error: host_ring_paint(0xf8, 0x71, 0x71),
            completed: host_ring_paint(0x34, 0xd3, 0x99),
        },
    }
}

fn host_ring_paint(red: u8, green: u8, blue: u8) -> HostRingPaint {
    let base = host_rgba(red, green, blue, 1.0);
    HostRingPaint {
        stroke: host_rgba(red, green, blue, 0.98),
        glow: HostRgba {
            red: base.red,
            green: base.green,
            blue: base.blue,
            alpha: 0.34,
        },
    }
}

fn host_rgba(red: u8, green: u8, blue: u8, alpha: f64) -> HostRgba {
    HostRgba {
        red: f64::from(red) / 255.0,
        green: f64::from(green) / 255.0,
        blue: f64::from(blue) / 255.0,
        alpha,
    }
}

fn apply_host_rgba(ctx: &gtk::cairo::Context, color: HostRgba) {
    ctx.set_source_rgba(color.red, color.green, color.blue, color.alpha);
}

fn draw_attention_ring_path(
    ctx: &gtk::cairo::Context,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    radius: f64,
) {
    let radius = radius.min(width / 2.0).min(height / 2.0).max(0.0);
    let right = x + width;
    let bottom = y + height;

    ctx.new_sub_path();
    ctx.arc(right - radius, y + radius, radius, -FRAC_PI_2, 0.0);
    ctx.arc(right - radius, bottom - radius, radius, 0.0, FRAC_PI_2);
    ctx.arc(x + radius, bottom - radius, radius, FRAC_PI_2, PI);
    ctx.arc(x + radius, y + radius, radius, PI, TAU - FRAC_PI_2);
    ctx.close_path();
}

fn install_native_surface_css(theme_id: &str) -> CssProvider {
    let provider = CssProvider::new();
    update_native_surface_css(&provider, theme_id);
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
    provider
}

fn update_native_surface_css(provider: &CssProvider, theme_id: &str) {
    provider.load_from_data(&native_surface_css(theme_id));
}

fn native_surface_classes(kind: PaneKind) -> (&'static str, &'static str) {
    match kind {
        PaneKind::Terminal => ("native-surface-terminal", "native-surface-terminal-widget"),
        PaneKind::Browser => ("native-surface-browser", "native-surface-browser-widget"),
    }
}

fn resize_handle_class(cursor: taskers_core::ResizeHandleCursor) -> &'static str {
    match cursor {
        taskers_core::ResizeHandleCursor::EastWest => "resize-handle-ew",
        taskers_core::ResizeHandleCursor::NorthSouth => "resize-handle-ns",
        taskers_core::ResizeHandleCursor::SouthEast => "resize-handle-se",
    }
}

fn resize_cursor_name(cursor: taskers_core::ResizeHandleCursor) -> &'static str {
    match cursor {
        taskers_core::ResizeHandleCursor::EastWest => "ew-resize",
        taskers_core::ResizeHandleCursor::NorthSouth => "ns-resize",
        taskers_core::ResizeHandleCursor::SouthEast => "nwse-resize",
    }
}

fn preview_for_drag(
    target: &taskers_core::ResizeHandleTarget,
    split_gap: i32,
    dx: f64,
    dy: f64,
) -> Option<taskers_core::ResizePreview> {
    match target {
        taskers_core::ResizeHandleTarget::WorkspaceColumnEdge {
            workspace_id,
            column_widths,
            leading_index,
        } => resize_track_push(
            column_widths,
            *leading_index,
            dx.round() as i32,
            MIN_WORKSPACE_WINDOW_WIDTH,
        )
        .map(
            |widths| taskers_core::ResizePreview::WorkspaceColumnWidths {
                workspace_id: *workspace_id,
                widths,
            },
        ),
        taskers_core::ResizeHandleTarget::WorkspaceColumnOuterEdge {
            workspace_id,
            column_widths,
            column_index,
            edge,
        } => {
            let delta = match edge {
                taskers_core::WorkspaceOuterEdge::Left => -(dx.round() as i32),
                taskers_core::WorkspaceOuterEdge::Right => dx.round() as i32,
            };
            resize_track_push(
                column_widths,
                *column_index,
                delta,
                MIN_WORKSPACE_WINDOW_WIDTH,
            )
            .map(
                |widths| taskers_core::ResizePreview::WorkspaceColumnWidths {
                    workspace_id: *workspace_id,
                    widths,
                },
            )
        }
        taskers_core::ResizeHandleTarget::WorkspaceWindowBottomEdge {
            workspace_id,
            window_heights,
            upper_index,
        } => resize_track_pair(
            window_heights,
            *upper_index,
            dy.round() as i32,
            MIN_WORKSPACE_WINDOW_HEIGHT,
        )
        .map(
            |heights| taskers_core::ResizePreview::WorkspaceWindowHeights {
                workspace_id: *workspace_id,
                heights,
            },
        ),
        taskers_core::ResizeHandleTarget::WorkspaceWindowCorner {
            workspace_id,
            column_widths,
            leading_index,
            window_heights,
            upper_index,
        } => {
            let next_column_widths = resize_track_push(
                column_widths,
                *leading_index,
                dx.round() as i32,
                MIN_WORKSPACE_WINDOW_WIDTH,
            );
            let next_window_heights = resize_track_pair(
                window_heights,
                *upper_index,
                dy.round() as i32,
                MIN_WORKSPACE_WINDOW_HEIGHT,
            );
            if next_column_widths.is_none() && next_window_heights.is_none() {
                None
            } else {
                Some(taskers_core::ResizePreview::WorkspaceWindowCorner {
                    workspace_id: *workspace_id,
                    column_widths: next_column_widths.unwrap_or_else(|| column_widths.clone()),
                    window_heights: next_window_heights.unwrap_or_else(|| window_heights.clone()),
                })
            }
        }
        taskers_core::ResizeHandleTarget::WorkspaceWindowSplit {
            workspace_id,
            workspace_window_id,
            path,
            axis,
            parent_frame,
            initial_ratio,
        } => split_ratio_preview(*axis, *parent_frame, *initial_ratio, split_gap, dx, dy).map(
            |ratio| taskers_core::ResizePreview::WorkspaceWindowSplitRatio {
                workspace_id: *workspace_id,
                workspace_window_id: *workspace_window_id,
                path: path.clone(),
                ratio,
            },
        ),
        taskers_core::ResizeHandleTarget::PaneTabSplit {
            workspace_id,
            pane_container_id,
            pane_tab_id,
            path,
            axis,
            parent_frame,
            initial_ratio,
        } => split_ratio_preview(*axis, *parent_frame, *initial_ratio, split_gap, dx, dy).map(
            |ratio| taskers_core::ResizePreview::PaneTabSplitRatio {
                workspace_id: *workspace_id,
                pane_container_id: *pane_container_id,
                pane_tab_id: *pane_tab_id,
                path: path.clone(),
                ratio,
            },
        ),
    }
}

fn resize_track_pair<Id: Copy>(
    tracks: &[(Id, i32)],
    leading_index: usize,
    delta: i32,
    min_extent: i32,
) -> Option<Vec<(Id, i32)>> {
    if delta == 0 || leading_index + 1 >= tracks.len() {
        return None;
    }

    let leading = tracks[leading_index].1;
    let trailing = tracks[leading_index + 1].1;
    let clamped_delta = delta.clamp(min_extent - leading, trailing - min_extent);
    if clamped_delta == 0 {
        return None;
    }

    let mut next = tracks.to_vec();
    next[leading_index].1 = leading + clamped_delta;
    next[leading_index + 1].1 = trailing - clamped_delta;
    Some(next)
}

fn resize_track_push<Id: Copy>(
    tracks: &[(Id, i32)],
    leading_index: usize,
    delta: i32,
    min_extent: i32,
) -> Option<Vec<(Id, i32)>> {
    if delta == 0 || leading_index >= tracks.len() {
        return None;
    }

    let leading = tracks[leading_index].1;
    let clamped_delta = delta.max(min_extent - leading);
    if clamped_delta == 0 {
        return None;
    }

    let mut next = tracks.to_vec();
    next[leading_index].1 = leading + clamped_delta;
    Some(next)
}

fn corrected_drag_delta(widget: &GtkBox, anchor: DragAnchor, dx: f64, dy: f64) -> (f64, f64) {
    let pointer_abs_x = widget.margin_start() + anchor.pointer_offset_x + dx.round() as i32;
    let pointer_abs_y = widget.margin_top() + anchor.pointer_offset_y + dy.round() as i32;
    (
        f64::from(pointer_abs_x - anchor.start_abs_x),
        f64::from(pointer_abs_y - anchor.start_abs_y),
    )
}

fn split_ratio_preview(
    axis: taskers_core::SplitAxis,
    parent_frame: taskers_core::Frame,
    initial_ratio: u16,
    split_gap: i32,
    dx: f64,
    dy: f64,
) -> Option<u16> {
    let delta = match axis {
        taskers_core::SplitAxis::Horizontal => dx.round() as i32,
        taskers_core::SplitAxis::Vertical => dy.round() as i32,
    };
    if delta == 0 {
        return None;
    }

    let usable = match axis {
        taskers_core::SplitAxis::Horizontal => parent_frame.width.saturating_sub(split_gap),
        taskers_core::SplitAxis::Vertical => parent_frame.height.saturating_sub(split_gap),
    }
    .max(1);
    let max_first = usable.saturating_sub(1).max(1);
    let initial_first = (((usable * i32::from(initial_ratio)) / 1000).max(1)).clamp(1, max_first);
    let next_first = (initial_first + delta).clamp(1, max_first);
    let next_ratio = ((f64::from(next_first) / f64::from(usable)) * 1000.0).round() as u16;
    let next_ratio = next_ratio.clamp(MIN_RESIZE_SPLIT_RATIO, MAX_RESIZE_SPLIT_RATIO);
    (next_ratio != initial_ratio).then_some(next_ratio)
}

fn native_surface_css(theme_id: &str) -> String {
    format!(
        r#"
.native-surface-host,
.native-surface-widget {{
  margin: 0;
  padding: 0;
  border-radius: 0;
  box-shadow: none;
}}

.native-surface-terminal,
.native-surface-terminal-widget,
.terminal-output {{
  background: {};
  padding-left: 0px;
  padding-right: 0px;
  padding-top: 0px;
  padding-bottom: 0px;
}}

.native-surface-browser,
.native-surface-browser-widget {{
  background: transparent;
}}

.resize-handle {{
  background: transparent;
  border: none;
  min-width: 1px;
  min-height: 1px;
}}

.resize-handle:hover {{
  background: rgba(255, 255, 255, 0.08);
}}

.resize-handle-active {{
  background: rgba(255, 255, 255, 0.16);
}}
"#,
        terminal_surface_background(theme_id)
    )
}

fn terminal_surface_background(theme_id: &str) -> &'static str {
    match theme_id {
        "catppuccin-mocha" => "#1e1e2e",
        "tokyo-night" => "#1a1b26",
        "gruvbox-dark" => "#282828",
        _ => "#0f1117",
    }
}

fn ghostty_focus_enter_event(pane_id: PaneId) -> HostEvent {
    HostEvent::PaneFocused { pane_id }
}

fn ghostty_title_changed_event(
    surface_id: SurfaceId,
    title: Option<glib::GString>,
) -> Option<HostEvent> {
    title.map(|title| HostEvent::SurfaceTitleChanged {
        surface_id,
        title: title.to_string(),
    })
}

fn ghostty_cwd_changed_event(
    surface_id: SurfaceId,
    cwd: Option<glib::GString>,
) -> Option<HostEvent> {
    cwd.map(|cwd| HostEvent::SurfaceCwdChanged {
        surface_id,
        cwd: cwd.to_string(),
    })
}

fn ghostty_child_exited_event(
    pane_id: PaneId,
    surface_id: SurfaceId,
    child_exited: bool,
) -> Option<HostEvent> {
    child_exited.then_some(HostEvent::SurfaceClosed {
        pane_id,
        surface_id,
    })
}

fn connect_ghostty_widget(
    widget: &Widget,
    pane_id: Rc<Cell<PaneId>>,
    surface_id: SurfaceId,
    event_sink: HostEventSink,
    diagnostics: Option<DiagnosticsSink>,
    focus_state: Rc<Cell<bool>>,
) {
    let focus_pane_id = pane_id.clone();
    let focus_sink = event_sink.clone();
    let focus_diagnostics = diagnostics.clone();
    let focus_enter_state = focus_state.clone();
    let focus = EventControllerFocus::new();
    focus.connect_enter(move |_| {
        let pane_id = focus_pane_id.get();
        focus_enter_state.set(true);
        // Native terminal clicks should focus the widget directly; we only
        // mirror that focus change into Taskers state here.
        emit_diagnostic(
            focus_diagnostics.as_ref(),
            DiagnosticRecord::new(
                DiagnosticCategory::HostEvent,
                None,
                "terminal focus event received",
            )
            .with_pane(pane_id)
            .with_surface(surface_id),
        );
        (focus_sink)(ghostty_focus_enter_event(pane_id));
    });
    let focus_leave_state = focus_state;
    focus.connect_leave(move |_| {
        focus_leave_state.set(false);
    });
    widget.add_controller(focus);

    let title_sink = event_sink.clone();
    let title_diagnostics = diagnostics.clone();
    widget.connect_notify_local(Some("title"), move |widget, _| {
        if let Some(title) = widget.property::<Option<glib::GString>>("title")
            && let Some(event) = ghostty_title_changed_event(surface_id, Some(title.clone()))
        {
            emit_diagnostic(
                title_diagnostics.as_ref(),
                DiagnosticRecord::new(
                    DiagnosticCategory::HostEvent,
                    None,
                    format!("terminal title observed: {title}"),
                )
                .with_surface(surface_id),
            );
            (title_sink)(event);
        }
    });

    let cwd_sink = event_sink.clone();
    let cwd_diagnostics = diagnostics.clone();
    widget.connect_notify_local(Some("pwd"), move |widget, _| {
        if let Some(cwd) = widget.property::<Option<glib::GString>>("pwd")
            && let Some(event) = ghostty_cwd_changed_event(surface_id, Some(cwd.clone()))
        {
            emit_diagnostic(
                cwd_diagnostics.as_ref(),
                DiagnosticRecord::new(
                    DiagnosticCategory::HostEvent,
                    None,
                    format!("terminal cwd observed: {cwd}"),
                )
                .with_surface(surface_id),
            );
            (cwd_sink)(event);
        }
    });

    let exit_pane_id = pane_id;
    let exit_sink = event_sink;
    let exit_diagnostics = diagnostics;
    widget.connect_notify_local(Some("child-exited"), move |widget, _| {
        if widget.property::<bool>("child-exited")
            && let Some(event) = ghostty_child_exited_event(exit_pane_id.get(), surface_id, true)
        {
            let pane_id = exit_pane_id.get();
            emit_diagnostic(
                exit_diagnostics.as_ref(),
                DiagnosticRecord::new(DiagnosticCategory::HostEvent, None, "terminal child exited")
                    .with_pane(pane_id)
                    .with_surface(surface_id),
            );
            (exit_sink)(event);
        }
    });
}

fn emit_browser_navigation_state(
    webview: &WebView,
    surface_id: SurfaceId,
    devtools_open: bool,
    event_sink: &HostEventSink,
    diagnostics: Option<&DiagnosticsSink>,
) {
    emit_diagnostic(
        diagnostics,
        DiagnosticRecord::new(
            DiagnosticCategory::BrowserMetadata,
            None,
            format!(
                "browser navigation state updated back={} forward={} devtools={}",
                webview.can_go_back(),
                webview.can_go_forward(),
                devtools_open
            ),
        )
        .with_surface(surface_id),
    );
    (event_sink)(HostEvent::BrowserNavigationStateChanged {
        surface_id,
        can_go_back: webview.can_go_back(),
        can_go_forward: webview.can_go_forward(),
        devtools_open,
    });
}

fn browser_load_state_from_webkit(load_event: LoadEvent) -> BrowserLoadState {
    match load_event {
        LoadEvent::Started => BrowserLoadState::Started,
        LoadEvent::Redirected => BrowserLoadState::Redirected,
        LoadEvent::Committed => BrowserLoadState::Committed,
        LoadEvent::Finished => BrowserLoadState::Finished,
        _ => BrowserLoadState::Finished,
    }
}

fn browser_command_surface_id(command: &BrowserControlCommand) -> SurfaceId {
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

fn terminal_debug_surface_id(command: &TerminalDebugCommand) -> SurfaceId {
    match command {
        TerminalDebugCommand::IsFocused { surface_id }
        | TerminalDebugCommand::ReadText { surface_id, .. }
        | TerminalDebugCommand::RenderStats { surface_id } => *surface_id,
    }
}

fn trim_terminal_tail(text: String, tail_lines: Option<usize>) -> String {
    let Some(limit) = tail_lines else {
        return text;
    };
    if limit == 0 {
        return String::new();
    }

    let lines = text.lines().collect::<Vec<_>>();
    let start = lines.len().saturating_sub(limit);
    lines[start..].join("\n")
}

fn build_persistent_browser_session() -> Result<NetworkSession> {
    let paths = taskers_paths::TaskersPaths::detect();
    let data_dir = paths.data_dir().join("browser").join("default");
    let cache_dir = paths.cache_dir().join("browser").join("default");
    ensure_private_dir(&data_dir)?;
    ensure_private_dir(&cache_dir)?;
    let data_dir = data_dir.to_string_lossy().into_owned();
    let cache_dir = cache_dir.to_string_lossy().into_owned();
    let session = NetworkSession::new(Some(&data_dir), Some(&cache_dir));
    session.set_persistent_credential_storage_enabled(false);
    Ok(session)
}

fn build_ephemeral_browser_session() -> NetworkSession {
    let session = NetworkSession::new_ephemeral();
    session.set_persistent_credential_storage_enabled(false);
    session
}

fn ensure_private_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;

        let mut permissions = fs::metadata(path)?.permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(path, permissions)?;
    }
    Ok(())
}

fn surface_descriptor_from(spec: &TerminalMountSpec) -> SurfaceDescriptor {
    SurfaceDescriptor {
        cols: spec.cols,
        rows: spec.rows,
        kind: PaneKind::Terminal,
        cwd: spec.cwd.clone(),
        title: Some(spec.title.clone()),
        url: None,
        browser_profile_mode: BrowserProfileMode::PersistentDefault,
        // The current Ghostty bridge is more stable when it controls shell
        // selection itself, so keep command overrides empty until that path is
        // proven across hosts.
        command_argv: Vec::new(),
        env: spec.env.clone(),
    }
}

fn sync_native_surface_scene(
    root: &Overlay,
    viewport: &Fixed,
    scene: &Fixed,
    portal: &SurfacePortalPlan,
    workspace: &WorkspaceViewSnapshot,
    visible: bool,
    interactive: bool,
) {
    viewport.set_visible(visible);
    viewport.set_can_target(interactive);
    scene.set_can_target(interactive);
    if !visible {
        return;
    }
    position_widget(root, viewport.upcast_ref(), portal.content);
    position_widget_in_fixed(
        viewport,
        scene.upcast_ref(),
        taskers_core::Frame::new(
            workspace.canvas_offset_x - workspace.viewport_x,
            workspace.canvas_offset_y - workspace.viewport_y,
            workspace.canvas_width,
            workspace.canvas_height,
        ),
    );
}

fn scene_plans_for_kind(
    portal: &SurfacePortalPlan,
    workspace: &WorkspaceViewSnapshot,
    kind: PaneKind,
) -> Vec<PortalSurfacePlan> {
    portal
        .panes
        .iter()
        .filter(|plan| {
            matches!(
                (&plan.mount, &kind),
                (SurfaceMountSpec::Browser(_), PaneKind::Browser)
                    | (SurfaceMountSpec::Terminal(_), PaneKind::Terminal)
            )
        })
        .map(|plan| plan_in_scene_coordinates(plan, workspace))
        .collect()
}

fn plan_in_scene_coordinates(
    plan: &PortalSurfacePlan,
    workspace: &WorkspaceViewSnapshot,
) -> PortalSurfacePlan {
    PortalSurfacePlan {
        frame: display_frame_to_scene(plan.frame, workspace),
        pane_frame: display_frame_to_scene(plan.pane_frame, workspace),
        ..plan.clone()
    }
}

fn display_frame_to_scene(
    frame: taskers_core::Frame,
    workspace: &WorkspaceViewSnapshot,
) -> taskers_core::Frame {
    taskers_core::Frame::new(
        frame.x - workspace.viewport_origin_x - workspace.canvas_offset_x + workspace.viewport_x,
        frame.y - workspace.viewport_origin_y - workspace.canvas_offset_y + workspace.viewport_y,
        frame.width,
        frame.height,
    )
}

fn position_widget(overlay: &Overlay, widget: &Widget, frame: taskers_core::Frame) {
    let clamped_margin_start = frame.x.clamp(0, i32::from(i16::MAX));
    let clamped_margin_top = frame.y.clamp(0, i32::from(i16::MAX));
    let width = frame.width.max(1);
    let height = frame.height.max(1);
    let needs_resize = widget.width_request() != width || widget.height_request() != height;
    let needs_reposition =
        widget.margin_start() != clamped_margin_start || widget.margin_top() != clamped_margin_top;
    if needs_resize {
        widget.set_size_request(width, height);
    }
    widget.set_hexpand(false);
    widget.set_vexpand(false);
    widget.set_halign(Align::Start);
    widget.set_valign(Align::Start);
    if needs_reposition {
        widget.set_margin_start(clamped_margin_start);
        widget.set_margin_top(clamped_margin_top);
    }
    if widget.parent().is_some() {
        if needs_resize || needs_reposition {
            widget.queue_allocate();
        }
    } else {
        overlay.add_overlay(widget);
        overlay.set_measure_overlay(widget, false);
        overlay.set_clip_overlay(widget, true);
    }
}

fn position_widget_in_fixed(fixed: &Fixed, widget: &Widget, frame: taskers_core::Frame) {
    let width = frame.width.max(1);
    let height = frame.height.max(1);
    let needs_resize = widget.width_request() != width || widget.height_request() != height;
    if needs_resize {
        widget.set_size_request(width, height);
    }
    widget.set_hexpand(false);
    widget.set_vexpand(false);
    widget.set_halign(Align::Start);
    widget.set_valign(Align::Start);
    if widget.parent().is_some() {
        fixed.move_(widget, frame.x as f64, frame.y as f64);
        if needs_resize {
            widget.queue_allocate();
        }
    } else {
        fixed.put(widget, frame.x as f64, frame.y as f64);
    }
}

fn detach_from_overlay(overlay: &Overlay, widget: &Widget) {
    if widget.parent().is_some() {
        overlay.remove_overlay(widget);
    }
}

fn detach_from_fixed(fixed: &Fixed, widget: &Widget) {
    if widget.parent().is_some() {
        fixed.remove(widget);
    }
}

fn native_surfaces_interactive(
    section: ShellSection,
    drag_mode: ShellDragMode,
    overview_mode: bool,
) -> bool {
    matches!(section, ShellSection::Workspace) && drag_mode == ShellDragMode::None && !overview_mode
}

fn native_surface_shell_can_target(interactive: bool) -> bool {
    interactive
}

fn native_surfaces_visible(section: ShellSection, drag_mode: ShellDragMode) -> bool {
    matches!(section, ShellSection::Workspace) && drag_mode == ShellDragMode::None
}

fn ensure_visible_workspace(
    visible_workspace_id: WorkspaceId,
    requested_workspace_id: WorkspaceId,
) -> Result<(), ControlError> {
    if requested_workspace_id != visible_workspace_id {
        return Err(ControlError::not_supported(format!(
            "workspace {requested_workspace_id} is not the currently visible workspace {visible_workspace_id}; switch to it before capturing"
        )));
    }
    Ok(())
}

fn capture_widget_to_png(
    widget: &Widget,
    viewport: Option<taskers_core::Frame>,
    path: Option<String>,
    target: ScreenshotTargetResult,
) -> Result<ScreenshotResult, ControlError> {
    let texture = capture_widget_texture_with_retry(widget, viewport)?;
    let output_path = resolve_screenshot_output_path(path)?;
    texture
        .save_to_png(&output_path)
        .map_err(|error| ControlError::internal(error.to_string()))?;
    Ok(ScreenshotResult {
        path: output_path.display().to_string(),
        width: texture.width(),
        height: texture.height(),
        target,
    })
}

fn capture_widget_texture_with_retry(
    widget: &Widget,
    viewport: Option<taskers_core::Frame>,
) -> Result<gdk::Texture, ControlError> {
    with_capture_retries(|| {
        settle_capture_main_loop();
        snapshot_widget_texture_once(widget, viewport)
    })
}

fn settle_capture_main_loop() {
    let context = glib::MainContext::default();
    for _ in 0..2 {
        while context.pending() {
            let _ = context.iteration(false);
        }
        let _ = context.iteration(false);
    }
}

fn with_capture_retries<T>(
    mut attempt: impl FnMut() -> Result<T, ControlError>,
) -> Result<T, ControlError> {
    const CAPTURE_RETRY_ATTEMPTS: usize = 5;

    let mut last_timeout = None;
    for _ in 0..CAPTURE_RETRY_ATTEMPTS {
        match attempt() {
            Ok(value) => return Ok(value),
            Err(error) if error.code == ControlErrorCode::Timeout => {
                last_timeout = Some(error);
            }
            Err(error) => return Err(error),
        }
    }

    Err(last_timeout.unwrap_or_else(|| {
        ControlError::timeout("screenshot target did not stabilize before capture")
    }))
}

fn snapshot_widget_texture_once(
    widget: &Widget,
    viewport: Option<taskers_core::Frame>,
) -> Result<gdk::Texture, ControlError> {
    let width = widget.width();
    let height = widget.height();
    if !widget.is_visible() {
        return Err(ControlError::timeout(
            "screenshot target is not visible for capture",
        ));
    }
    if width <= 0 || height <= 0 {
        return Err(ControlError::timeout(
            "screenshot target is not yet allocated for capture",
        ));
    }
    let paintable = WidgetPaintable::new(Some(widget));
    let snapshot = Snapshot::new();
    paintable.snapshot(&snapshot, f64::from(width), f64::from(height));
    let node = snapshot
        .to_node()
        .ok_or_else(|| ControlError::internal("failed to build GTK render node for screenshot"))?;
    let native = widget.native().ok_or_else(|| {
        ControlError::not_supported("screenshot capture requires a realized GTK native surface")
    })?;
    let renderer = native.renderer().or_else(|| {
        native
            .surface()
            .as_ref()
            .and_then(gsk::Renderer::for_surface)
    });
    let renderer = renderer
        .ok_or_else(|| ControlError::not_supported("screenshot capture requires a GTK renderer"))?;
    let rect = viewport
        .map(|frame| clamp_frame_to_widget(frame, width, height))
        .transpose()?
        .map(|frame| {
            graphene::Rect::new(
                frame.x as f32,
                frame.y as f32,
                frame.width as f32,
                frame.height as f32,
            )
        });
    Ok(renderer.render_texture(node, rect.as_ref()))
}

fn clamp_frame_to_widget(
    frame: taskers_core::Frame,
    widget_width: i32,
    widget_height: i32,
) -> Result<taskers_core::Frame, ControlError> {
    let x = frame.x.clamp(0, widget_width.saturating_sub(1));
    let y = frame.y.clamp(0, widget_height.saturating_sub(1));
    let right = frame.right().clamp(x + 1, widget_width.max(x + 1));
    let bottom = frame.bottom().clamp(y + 1, widget_height.max(y + 1));
    let width = right - x;
    let height = bottom - y;
    if width <= 1 || height <= 1 {
        return Err(ControlError::timeout(
            "screenshot target viewport is too small to capture reliably",
        ));
    }
    Ok(taskers_core::Frame::new(x, y, width, height))
}

pub(crate) fn resolve_screenshot_output_path(
    path: Option<String>,
) -> Result<PathBuf, ControlError> {
    match path {
        Some(path) => {
            let trimmed = path.trim();
            if trimmed.is_empty() {
                return Err(ControlError::invalid_params(
                    "screenshot path must not be empty",
                ));
            }
            let output = PathBuf::from(trimmed);
            if let Some(parent) = output.parent()
                && !parent.as_os_str().is_empty()
            {
                fs::create_dir_all(parent).map_err(|error| {
                    ControlError::internal(format!(
                        "failed to create screenshot directory {}: {error}",
                        parent.display()
                    ))
                })?;
            }
            Ok(output)
        }
        None => {
            Ok(std::env::temp_dir()
                .join(format!("taskers-screenshot-{}.png", current_timestamp_ms())))
        }
    }
}

fn workspace_pan_delta(dx: f64, dy: f64) -> Option<(i32, i32)> {
    if !dx.is_finite() || !dy.is_finite() {
        return None;
    }
    if dx.abs() < 1.0 || dx.abs() < dy.abs() {
        return None;
    }
    Some((dx.round() as i32, 0))
}

pub fn browser_plans(portal: &SurfacePortalPlan) -> Vec<PortalSurfacePlan> {
    portal
        .panes
        .iter()
        .filter(|plan| matches!(plan.mount, SurfaceMountSpec::Browser(_)))
        .filter_map(|plan| clip_to_content(plan, &portal.content))
        .collect()
}

pub fn terminal_plans(portal: &SurfacePortalPlan) -> Vec<PortalSurfacePlan> {
    portal
        .panes
        .iter()
        .filter(|plan| matches!(plan.mount, SurfaceMountSpec::Terminal(_)))
        .filter_map(|plan| clip_to_content(plan, &portal.content))
        .collect()
}

fn clip_to_content(
    plan: &PortalSurfacePlan,
    content: &taskers_core::Frame,
) -> Option<PortalSurfacePlan> {
    let clipped_frame = clip_frame_to_content(plan.frame, *content)?;
    let clipped_pane_frame = clip_frame_to_content(plan.pane_frame, *content)?;
    if clipped_surface_is_too_small(plan.frame, clipped_frame) {
        return None;
    }

    Some(PortalSurfacePlan {
        frame: clipped_frame,
        pane_frame: clipped_pane_frame,
        ..plan.clone()
    })
}

fn clip_frame_to_content(
    frame: taskers_core::Frame,
    content: taskers_core::Frame,
) -> Option<taskers_core::Frame> {
    let f = &frame;
    let cx = content.x;
    let cy = content.y;
    let cr = content.x + content.width;
    let cb = content.y + content.height;

    let clipped_x = f.x.max(cx);
    let clipped_y = f.y.max(cy);
    let clipped_r = (f.x + f.width).min(cr);
    let clipped_b = (f.y + f.height).min(cb);

    let clipped_w = clipped_r - clipped_x;
    let clipped_h = clipped_b - clipped_y;

    if clipped_w <= 0 || clipped_h <= 0 {
        return None;
    }

    Some(taskers_core::Frame::new(
        clipped_x, clipped_y, clipped_w, clipped_h,
    ))
}

fn clipped_surface_is_too_small(
    original: taskers_core::Frame,
    clipped: taskers_core::Frame,
) -> bool {
    let clipped_horizontally = clipped.x != original.x || clipped.width != original.width;
    let clipped_vertically = clipped.y != original.y || clipped.height != original.height;

    (clipped_horizontally && clipped.width < MIN_CLIPPED_NATIVE_SURFACE_WIDTH_PX)
        || (clipped_vertically && clipped.height < MIN_CLIPPED_NATIVE_SURFACE_HEIGHT_PX)
}

fn emit_diagnostic(sink: Option<&DiagnosticsSink>, record: DiagnosticRecord) {
    if let Some(sink) = sink {
        sink(record);
    }
}

fn current_timestamp_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

fn hidden_frame() -> taskers_core::Frame {
    taskers_core::Frame::new(100_000, 100_000, 1, 1)
}

fn native_surface_visible_plan(
    visible_plan: Option<&PortalSurfacePlan>,
    _resize_preview_active: bool,
) -> Option<&PortalSurfacePlan> {
    visible_plan
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, sync::Mutex};

    use gtk::prelude::WidgetExt;

    use super::{
        HardwareAccelerationPolicy, TerminalSurfaceCreateDecision, WebKitSettings, browser_plans,
        build_native_surface_scene_layers, clamp_frame_to_widget, ghostty_child_exited_event,
        ghostty_cwd_changed_event, ghostty_focus_enter_event, ghostty_title_changed_event,
        host_attention_palette, native_surface_classes, native_surface_css,
        native_surface_shell_can_target, native_surface_visible_plan, native_surfaces_interactive,
        native_surfaces_visible, preview_for_drag, redacted_browser_url_for_diagnostics,
        resolve_screenshot_output_path, terminal_plans, terminal_surface_create_decision,
        trim_terminal_tail, with_capture_retries, workspace_pan_delta,
    };
    use taskers_control::{ControlError, ControlErrorCode};
    use taskers_domain::{MIN_WORKSPACE_WINDOW_HEIGHT, MIN_WORKSPACE_WINDOW_WIDTH, PaneKind};
    use taskers_shell_core::{
        AttentionRingState, BootstrapModel, Frame, HostEvent, PaneContainerId, PaneId, PaneTabId,
        PortalSurfacePlan, ResizeHandleTarget, ResizePreview, SharedCore, ShellDragMode,
        ShellSection, SplitAxis, SurfaceId, SurfaceMountSpec, TerminalMountSpec, WorkspaceColumnId,
        WorkspaceOuterEdge, WorkspaceWindowId,
    };

    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    #[test]
    fn partitions_portal_plans_by_surface_kind() {
        let core = SharedCore::bootstrap(BootstrapModel::default());
        let snapshot = core.snapshot();

        let browsers = browser_plans(&snapshot.portal);
        let terminals = terminal_plans(&snapshot.portal);

        assert_eq!(browsers.len(), 1);
        assert_eq!(terminals.len(), 1);
        assert!(matches!(browsers[0].mount, SurfaceMountSpec::Browser(_)));
        assert!(matches!(terminals[0].mount, SurfaceMountSpec::Terminal(_)));
    }

    #[test]
    fn workspace_pan_delta_prefers_deliberate_horizontal_motion() {
        assert_eq!(workspace_pan_delta(64.4, 4.0), Some((64, 0)));
        assert_eq!(workspace_pan_delta(0.4, 0.0), None);
        assert_eq!(workspace_pan_delta(6.0, 18.0), None);
        assert_eq!(workspace_pan_delta(f64::NAN, 0.0), None);
    }

    #[test]
    fn native_surfaces_disable_pointer_targeting_during_shell_drags() {
        assert!(native_surfaces_interactive(
            ShellSection::Workspace,
            ShellDragMode::None,
            false
        ));
        assert!(!native_surfaces_interactive(
            ShellSection::Workspace,
            ShellDragMode::None,
            true
        ));
        assert!(!native_surfaces_interactive(
            ShellSection::Workspace,
            ShellDragMode::Window,
            false
        ));
        assert!(!native_surfaces_interactive(
            ShellSection::Workspace,
            ShellDragMode::WindowTab,
            false
        ));
        assert!(!native_surfaces_interactive(
            ShellSection::Workspace,
            ShellDragMode::PaneTab,
            false
        ));
        assert!(!native_surfaces_interactive(
            ShellSection::Workspace,
            ShellDragMode::Surface,
            false
        ));
        assert!(!native_surfaces_interactive(
            ShellSection::Settings,
            ShellDragMode::None,
            false
        ));
    }

    #[test]
    fn native_surfaces_hide_during_shell_drags() {
        assert!(native_surfaces_visible(
            ShellSection::Workspace,
            ShellDragMode::None
        ));
        assert!(!native_surfaces_visible(
            ShellSection::Workspace,
            ShellDragMode::Window
        ));
        assert!(!native_surfaces_visible(
            ShellSection::Workspace,
            ShellDragMode::WindowTab
        ));
        assert!(!native_surfaces_visible(
            ShellSection::Workspace,
            ShellDragMode::PaneTab
        ));
        assert!(!native_surfaces_visible(
            ShellSection::Workspace,
            ShellDragMode::Surface
        ));
        assert!(!native_surfaces_visible(
            ShellSection::Settings,
            ShellDragMode::None
        ));
    }

    #[test]
    fn resize_preview_keeps_native_surface_plans_available() {
        let plan = PortalSurfacePlan {
            pane_id: PaneId::new(),
            surface_id: SurfaceId::new(),
            pane_frame: Frame::new(0, 0, 320, 240),
            frame: Frame::new(10, 20, 300, 200),
            mount: SurfaceMountSpec::Terminal(TerminalMountSpec {
                title: "Terminal".into(),
                cwd: None,
                cols: 80,
                rows: 24,
                command_argv: Vec::new(),
                env: BTreeMap::new(),
            }),
            active: true,
            notification_ring: None,
        };

        assert_eq!(
            native_surface_visible_plan(Some(&plan), false).map(|plan| plan.frame),
            Some(plan.frame)
        );
        assert_eq!(
            native_surface_visible_plan(Some(&plan), true).map(|plan| plan.frame),
            Some(plan.frame)
        );
        assert!(native_surface_visible_plan(None, true).is_none());
    }

    #[test]
    fn native_surface_classes_distinguish_terminal_and_browser_hosts() {
        assert_eq!(
            native_surface_classes(PaneKind::Terminal),
            ("native-surface-terminal", "native-surface-terminal-widget")
        );
        assert_eq!(
            native_surface_classes(PaneKind::Browser),
            ("native-surface-browser", "native-surface-browser-widget")
        );
    }

    #[test]
    fn native_surface_scene_layers_do_not_steal_shell_click_targets() {
        let _ = gtk::init();
        let (viewport, scene) = build_native_surface_scene_layers();
        assert!(!viewport.can_target());
        assert!(!scene.can_target());
        assert!(!viewport.is_focusable());
        assert!(!scene.is_focusable());
    }

    #[test]
    fn native_surface_shell_becomes_targetable_only_when_interactive() {
        assert!(native_surface_shell_can_target(true));
        assert!(!native_surface_shell_can_target(false));
    }

    #[test]
    fn native_surface_css_tracks_selected_theme_terminal_background() {
        let dark = native_surface_css("dark");
        let gruvbox = native_surface_css("gruvbox-dark");
        assert!(dark.contains(".native-surface-terminal"));
        assert!(dark.contains(".native-surface-terminal-widget"));
        assert!(dark.contains(".terminal-output"));
        assert!(dark.contains("background: #0f1117;"));
        assert!(dark.contains("padding-left: 0px;"));
        assert!(dark.contains("padding-right: 0px;"));
        assert!(dark.contains("padding-top: 0px;"));
        assert!(dark.contains("padding-bottom: 0px;"));
        assert!(gruvbox.contains("background: #282828;"));
    }

    #[test]
    fn webkit_graphics_fallback_follows_dmabuf_env_flag() {
        let _lock = ENV_MUTEX.lock().expect("env mutex");
        let settings = WebKitSettings::builder().build();

        unsafe { std::env::remove_var(super::WEBKIT_DISABLE_DMABUF_RENDERER_ENV) };
        super::apply_taskers_webkit_graphics_fallback(&settings);
        assert_eq!(
            settings.hardware_acceleration_policy(),
            HardwareAccelerationPolicy::Always
        );

        unsafe { std::env::set_var(super::WEBKIT_DISABLE_DMABUF_RENDERER_ENV, "1") };
        super::apply_taskers_webkit_graphics_fallback(&settings);
        assert_eq!(
            settings.hardware_acceleration_policy(),
            HardwareAccelerationPolicy::Never
        );
        unsafe { std::env::remove_var(super::WEBKIT_DISABLE_DMABUF_RENDERER_ENV) };
    }

    #[test]
    fn terminal_surface_creation_defers_until_bridge_quiesces() {
        assert_eq!(
            terminal_surface_create_decision(false, 1, 2),
            TerminalSurfaceCreateDecision::DeferUntilBridgeQuiesces
        );
        assert_eq!(
            terminal_surface_create_decision(true, 1, 1),
            TerminalSurfaceCreateDecision::DeferUntilBridgeQuiesces
        );
    }

    #[test]
    fn terminal_surface_creation_skips_when_surface_budget_is_exhausted() {
        assert_eq!(
            terminal_surface_create_decision(false, super::MAX_CONCURRENT_GHOSTTY_SURFACES, 3),
            TerminalSurfaceCreateDecision::SkipAtSurfaceBudget
        );
    }

    #[test]
    fn terminal_surface_creation_allows_safe_capacity() {
        assert_eq!(
            terminal_surface_create_decision(false, 2, 2),
            TerminalSurfaceCreateDecision::CreateNow
        );
    }

    #[test]
    fn ghostty_widget_focus_enter_emits_pane_focused_once() {
        let pane_id = PaneId::new();

        assert_eq!(
            ghostty_focus_enter_event(pane_id),
            HostEvent::PaneFocused { pane_id }
        );
    }

    #[test]
    fn ghostty_widget_notify_propagates_title_pwd_child_exited() {
        let pane_id = PaneId::new();
        let surface_id = SurfaceId::new();

        assert_eq!(
            ghostty_title_changed_event(surface_id, Some("Taskers Terminal".into())),
            Some(HostEvent::SurfaceTitleChanged {
                surface_id,
                title: "Taskers Terminal".into(),
            })
        );
        assert_eq!(
            ghostty_cwd_changed_event(surface_id, Some("/tmp/taskers".into())),
            Some(HostEvent::SurfaceCwdChanged {
                surface_id,
                cwd: "/tmp/taskers".into(),
            })
        );
        assert_eq!(
            ghostty_child_exited_event(pane_id, surface_id, true),
            Some(HostEvent::SurfaceClosed {
                pane_id,
                surface_id,
            })
        );
        assert_eq!(ghostty_title_changed_event(surface_id, None), None);
        assert_eq!(ghostty_cwd_changed_event(surface_id, None), None);
        assert_eq!(ghostty_child_exited_event(pane_id, surface_id, false), None);
    }

    #[test]
    fn trim_terminal_tail_keeps_requested_suffix() {
        let text = "one\ntwo\nthree\nfour".to_string();
        assert_eq!(trim_terminal_tail(text.clone(), None), text);
        assert_eq!(trim_terminal_tail(text.clone(), Some(2)), "three\nfour");
        assert_eq!(trim_terminal_tail(text, Some(10)), "one\ntwo\nthree\nfour");
    }

    #[test]
    fn host_attention_palette_tracks_selected_theme_ring_colors() {
        let dark = host_attention_palette("dark");
        let gruvbox = host_attention_palette("gruvbox-dark");

        let dark_waiting = dark.paint(AttentionRingState::Waiting);
        let gruvbox_waiting = gruvbox.paint(AttentionRingState::Waiting);
        let dark_error = dark.paint(AttentionRingState::Error);

        assert!(dark_waiting.stroke.blue > dark_waiting.stroke.red);
        assert!(dark_error.stroke.red > dark_error.stroke.green);
        assert_ne!(dark_waiting.stroke.green, gruvbox_waiting.stroke.green);
    }

    #[test]
    fn browser_url_diagnostics_strip_query_and_path_details() {
        assert_eq!(
            redacted_browser_url_for_diagnostics(
                "https://example.com/callback?token=secret#fragment"
            ),
            "https://example.com/..."
        );
        assert_eq!(
            redacted_browser_url_for_diagnostics("about:blank"),
            "about:blank"
        );
    }

    #[test]
    fn drops_horizontally_clipped_sliver_surfaces() {
        let core = SharedCore::bootstrap(BootstrapModel::default());
        let snapshot = core.snapshot();
        let plan = snapshot.portal.panes[0].clone();

        let clipped = super::clip_to_content(
            &PortalSurfacePlan {
                frame: Frame::new(0, 0, 720, plan.frame.height),
                pane_frame: Frame::new(0, 0, 720, plan.pane_frame.height),
                ..plan
            },
            &Frame::new(680, 0, 200, 1200),
        );

        assert!(
            clipped.is_none(),
            "expected narrow clipped sliver to be skipped"
        );
    }

    #[test]
    fn keeps_reasonably_wide_clipped_surfaces() {
        let core = SharedCore::bootstrap(BootstrapModel::default());
        let snapshot = core.snapshot();
        let plan = snapshot.portal.panes[0].clone();

        let clipped = super::clip_to_content(
            &PortalSurfacePlan {
                frame: Frame::new(0, 0, 720, plan.frame.height),
                pane_frame: Frame::new(0, 0, 720, plan.pane_frame.height),
                ..plan
            },
            &Frame::new(360, 0, 360, 1200),
        );

        assert!(
            clipped.is_some(),
            "expected substantial clipped width to remain renderable"
        );
    }

    #[test]
    fn keeps_moderately_clipped_surfaces() {
        let core = SharedCore::bootstrap(BootstrapModel::default());
        let snapshot = core.snapshot();
        let plan = snapshot.portal.panes[0].clone();

        let clipped = super::clip_to_content(
            &PortalSurfacePlan {
                frame: Frame::new(0, 0, 720, plan.frame.height),
                pane_frame: Frame::new(0, 0, 720, plan.pane_frame.height),
                ..plan
            },
            &Frame::new(640, 0, 200, 1200),
        );

        assert!(
            clipped.is_some(),
            "expected moderately clipped edge slice to stay renderable"
        );
    }

    #[test]
    fn keeps_moderately_wide_clipped_surfaces() {
        let core = SharedCore::bootstrap(BootstrapModel::default());
        let snapshot = core.snapshot();
        let plan = snapshot.portal.panes[0].clone();

        let clipped = super::clip_to_content(
            &PortalSurfacePlan {
                frame: Frame::new(0, 0, 720, plan.frame.height),
                pane_frame: Frame::new(0, 0, 720, plan.pane_frame.height),
                ..plan
            },
            &Frame::new(600, 0, 160, 1200),
        );

        assert!(
            clipped.is_some(),
            "expected moderately clipped edge surface to remain renderable"
        );
    }

    #[test]
    fn preview_for_drag_clamps_workspace_window_dimensions() {
        let workspace_id = taskers_shell_core::WorkspaceId::new();
        let workspace_column_id = WorkspaceColumnId::new();
        let neighbor_column_id = WorkspaceColumnId::new();
        let workspace_window_id = WorkspaceWindowId::new();
        let lower_window_id = WorkspaceWindowId::new();
        let preview = preview_for_drag(
            &ResizeHandleTarget::WorkspaceWindowCorner {
                workspace_id,
                column_widths: vec![
                    (workspace_column_id, MIN_WORKSPACE_WINDOW_WIDTH + 120),
                    (neighbor_column_id, MIN_WORKSPACE_WINDOW_WIDTH + 240),
                ],
                leading_index: 0,
                window_heights: vec![
                    (workspace_window_id, MIN_WORKSPACE_WINDOW_HEIGHT + 90),
                    (lower_window_id, MIN_WORKSPACE_WINDOW_HEIGHT + 170),
                ],
                upper_index: 0,
            },
            2,
            -480.0,
            -320.0,
        )
        .expect("corner preview");

        assert_eq!(
            preview,
            ResizePreview::WorkspaceWindowCorner {
                workspace_id,
                column_widths: vec![
                    (workspace_column_id, MIN_WORKSPACE_WINDOW_WIDTH),
                    (neighbor_column_id, MIN_WORKSPACE_WINDOW_WIDTH + 240),
                ],
                window_heights: vec![
                    (workspace_window_id, MIN_WORKSPACE_WINDOW_HEIGHT),
                    (lower_window_id, MIN_WORKSPACE_WINDOW_HEIGHT + 260),
                ],
            }
        );
    }

    #[test]
    fn preview_for_drag_can_grow_workspace_column_without_shrinking_neighbor() {
        let workspace_id = taskers_shell_core::WorkspaceId::new();
        let workspace_column_id = WorkspaceColumnId::new();
        let neighbor_column_id = WorkspaceColumnId::new();

        let preview = preview_for_drag(
            &ResizeHandleTarget::WorkspaceColumnEdge {
                workspace_id,
                column_widths: vec![
                    (workspace_column_id, MIN_WORKSPACE_WINDOW_WIDTH),
                    (neighbor_column_id, MIN_WORKSPACE_WINDOW_WIDTH),
                ],
                leading_index: 0,
            },
            2,
            240.0,
            0.0,
        )
        .expect("column preview");

        assert_eq!(
            preview,
            ResizePreview::WorkspaceColumnWidths {
                workspace_id,
                widths: vec![
                    (workspace_column_id, MIN_WORKSPACE_WINDOW_WIDTH + 240),
                    (neighbor_column_id, MIN_WORKSPACE_WINDOW_WIDTH),
                ],
            }
        );
    }

    #[test]
    fn preview_for_drag_resizes_workspace_outer_edges() {
        let workspace_id = taskers_shell_core::WorkspaceId::new();
        let workspace_column_id = WorkspaceColumnId::new();

        let right_preview = preview_for_drag(
            &ResizeHandleTarget::WorkspaceColumnOuterEdge {
                workspace_id,
                column_widths: vec![(workspace_column_id, MIN_WORKSPACE_WINDOW_WIDTH)],
                column_index: 0,
                edge: WorkspaceOuterEdge::Right,
            },
            2,
            180.0,
            0.0,
        )
        .expect("right outer edge preview");
        assert_eq!(
            right_preview,
            ResizePreview::WorkspaceColumnWidths {
                workspace_id,
                widths: vec![(workspace_column_id, MIN_WORKSPACE_WINDOW_WIDTH + 180)],
            }
        );

        let left_preview = preview_for_drag(
            &ResizeHandleTarget::WorkspaceColumnOuterEdge {
                workspace_id,
                column_widths: vec![(workspace_column_id, MIN_WORKSPACE_WINDOW_WIDTH + 180)],
                column_index: 0,
                edge: WorkspaceOuterEdge::Left,
            },
            2,
            180.0,
            0.0,
        )
        .expect("left outer edge preview");
        assert_eq!(
            left_preview,
            ResizePreview::WorkspaceColumnWidths {
                workspace_id,
                widths: vec![(workspace_column_id, MIN_WORKSPACE_WINDOW_WIDTH)],
            }
        );
    }

    #[test]
    fn preview_for_drag_generates_pane_split_ratio_updates() {
        let workspace_id = taskers_shell_core::WorkspaceId::new();
        let pane_container_id = PaneContainerId::new();
        let pane_tab_id = PaneTabId::new();
        let preview = preview_for_drag(
            &ResizeHandleTarget::PaneTabSplit {
                workspace_id,
                pane_container_id,
                pane_tab_id,
                path: vec![false, true],
                axis: SplitAxis::Horizontal,
                parent_frame: Frame::new(0, 0, 1000, 600),
                initial_ratio: 500,
            },
            2,
            120.0,
            0.0,
        )
        .expect("split preview");

        let ResizePreview::PaneTabSplitRatio { ratio, .. } = preview else {
            panic!("expected pane split preview");
        };
        assert!(ratio > 500);
    }

    #[test]
    fn screenshot_capture_rejects_tiny_viewports() {
        let error = clamp_frame_to_widget(Frame::new(0, 0, 1, 1), 120, 80)
            .expect_err("tiny viewport should fail closed");
        assert_eq!(error.code, ControlErrorCode::Timeout);
    }

    #[test]
    fn screenshot_output_path_rejects_empty_override() {
        let error = resolve_screenshot_output_path(Some("   ".into()))
            .expect_err("empty screenshot path should fail");
        assert_eq!(error.code, ControlErrorCode::InvalidParams);
    }

    #[test]
    fn screenshot_capture_times_out_after_retry_exhaustion() {
        let error = with_capture_retries::<()>(|| {
            Err(ControlError::timeout(
                "screenshot target did not stabilize before capture",
            ))
        })
        .expect_err("retry exhaustion should return timeout");
        assert_eq!(error.code, ControlErrorCode::Timeout);
    }
}
