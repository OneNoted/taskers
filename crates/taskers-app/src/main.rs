mod app_state;
mod crash_reporter;
mod pane_runtime;
mod session_store;
mod settings_store;
mod terminal_transitions;

use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    future::pending,
    path::PathBuf,
    process::{Command, Stdio},
    rc::Rc,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use adw::prelude::*;
use app_state::AppState;
use clap::Parser;
use crash_reporter::CrashReporter;
use gtk::{
    Align, Box as GtkBox, Button, CssProvider, DrawingArea, Entry, Fixed, Label, Orientation,
    Overlay, Paned, PolicyType, STYLE_PROVIDER_PRIORITY_APPLICATION, ScrolledWindow, Separator,
    TextView, Widget, WrapMode, gdk, glib,
};
use pane_runtime::PaneRuntimeSnapshot;
use serde_json::json;
use settings_store::{AppConfig, ShortcutAction};
use svgtypes::{SimplePathSegment, SimplifyingPathParser};
use taskers_control::{
    ControlCommand, InMemoryController, bind_socket, default_socket_path, serve,
};
use taskers_domain::{
    ActivityItem, AppModel, AttentionState, DEFAULT_WORKSPACE_WINDOW_GAP,
    DEFAULT_WORKSPACE_WINDOW_HEIGHT, DEFAULT_WORKSPACE_WINDOW_WIDTH, Direction,
    KEYBOARD_RESIZE_STEP, LayoutNode, MIN_WORKSPACE_WINDOW_HEIGHT, PaneKind, PaneMetadata,
    PaneMetadataPatch, PaneRecord, SignalEvent, SignalKind, SurfaceId, SurfaceRecord, WindowFrame,
    Workspace, WorkspaceAgentState, WorkspaceViewport, WorkspaceWindowId,
};
use taskers_ghostty::{
    BackendChoice, BackendProbe, DefaultBackend, GhosttyHost, SurfaceDescriptor, TerminalBackend,
    ensure_runtime_installed,
};
use taskers_runtime::{
    ShellLaunchSpec, default_shell_program, install_shell_integration, validate_shell_program,
};
use terminal_transitions::{
    PaneSceneSnapshot, PresentedTransitionRect, TERMINAL_MOTION_SPEC, TransitionItemId,
    TransitionItemKind, TransitionPhase, WorkspaceSceneSnapshot, WorkspaceWindowSnapshot,
    derive_pane_frames, plan_workspace_transition, retarget_transition_plan,
};

#[derive(Debug, Clone, Parser)]
#[command(name = "taskers")]
#[command(about = "GTK shell for the taskers terminal workspace")]
struct Cli {
    #[arg(long)]
    socket: Option<PathBuf>,
    #[arg(long)]
    session: Option<PathBuf>,
    #[arg(long, default_value_t = false)]
    demo: bool,
    #[arg(long, default_value_t = false, conflicts_with = "raw_shell")]
    clean_shell: bool,
    #[arg(long, default_value_t = false, conflicts_with = "clean_shell")]
    raw_shell: bool,
    #[arg(long, hide = true, default_value_t = false)]
    internal_ghostty_probe: bool,
}

struct StartupContext {
    app_state: AppState,
    backend_choice: BackendChoice,
    config_path: PathBuf,
    app_config: AppConfig,
    crash_reporter: CrashReporter,
    ghostty_host: Option<GhosttyHost>,
    shell_launch: ShellLaunchSpec,
    startup_toast: Option<String>,
}

struct UiHandle {
    app_state: AppState,
    backend_choice: BackendChoice,
    application: adw::Application,
    window: adw::ApplicationWindow,
    overlay: adw::ToastOverlay,
    crash_reporter: CrashReporter,
    ghostty_host: Option<GhosttyHost>,
    shell_launch: ShellLaunchSpec,
    ghostty_surfaces: RefCell<HashMap<SurfaceId, Widget>>,
    shell: RefCell<Option<ShellWidgets>>,
    pane_cards: RefCell<HashMap<taskers_domain::PaneId, PaneCardWidgets>>,
    settings: RefCell<AppConfig>,
    config_path: PathBuf,
    layout_state: RefCell<LayoutRenderState>,
    last_rendered: RefCell<Option<AppModel>>,
    suppress_viewport_events: RefCell<bool>,
    pending_viewport: RefCell<Option<(taskers_domain::WorkspaceId, WorkspaceViewport)>>,
    pending_viewport_source: RefCell<Option<glib::SourceId>>,
    pending_focus_source: RefCell<Option<glib::SourceId>>,
    desktop_notifications: RefCell<HashSet<String>>,
    overview_mode: Cell<bool>,
    workspace_transition_state: RefCell<WorkspaceTransitionState>,
}

#[derive(Clone)]
struct ShellWidgets {
    root: Paned,
    sidebar_list: GtkBox,
    workspace_name_label: Label,
    activity_list: GtkBox,
    activity_empty: Label,
    layout_scroll: ScrolledWindow,
    layout_host: Fixed,
    workspace_stage: WorkspaceStageWidgets,
}

#[derive(Clone)]
struct PaneCardWidgets {
    root: GtkBox,
    agent_icon: AgentIconWidget,
    title: Label,
    status_dot: Label,
    surface_tabs: SurfaceTabStripWidgets,
    terminal_host: GtkBox,
    focus_target: Widget,
}

#[derive(Clone)]
struct WorkspaceStageWidgets {
    root: Overlay,
    ghost_layer: Fixed,
}

#[derive(Default)]
struct WorkspaceTransitionState {
    presented: HashMap<TransitionItemId, PresentedTransitionRect>,
    motion: Option<WorkspaceTransitionMotionState>,
    tick_running: bool,
    target_canvas_width: i32,
    target_canvas_height: i32,
}

#[derive(Clone)]
struct WorkspaceTransitionMotionState {
    start_time: i64,
    items: Vec<WorkspaceTransitionMotionItem>,
}

#[derive(Clone)]
struct WorkspaceTransitionMotionItem {
    id: TransitionItemId,
    widget: Widget,
    start_rect: PresentedTransitionRect,
    end_rect: PresentedTransitionRect,
    duration_us: i64,
    curve: terminal_transitions::MotionCurve,
    start_opacity: f64,
}

#[derive(Clone, Default)]
struct WorkspaceSceneVisuals {
    windows: HashMap<WorkspaceWindowId, WindowGhostVisual>,
}

#[derive(Clone, Copy)]
struct WindowGhostVisual {
    active: bool,
    attention: AttentionState,
}

#[derive(Clone)]
struct SurfaceTabStripWidgets {
    root: Fixed,
    add_button: Button,
    tabs: Rc<RefCell<HashMap<SurfaceId, SurfaceTabWidgets>>>,
    state: Rc<RefCell<SurfaceTabStripState>>,
}

#[derive(Clone)]
struct SurfaceTabWidgets {
    root: GtkBox,
    dot: Label,
    agent_icon: AgentIconWidget,
    title: Label,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum SurfaceTabItemKey {
    Surface(SurfaceId),
    AddButton,
}

#[derive(Clone, Debug, PartialEq)]
struct SurfaceTabStripLayout {
    items: Vec<SurfaceTabLayoutItem>,
    add_button: SurfaceTabAuxLayoutItem,
    height: i32,
}

#[derive(Clone, Debug, PartialEq)]
struct SurfaceTabLayoutItem {
    surface_id: SurfaceId,
    x: f64,
    width: i32,
    height: i32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct SurfaceTabAuxLayoutItem {
    x: f64,
    width: i32,
    height: i32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct PresentedSurfaceTabItem {
    x: f64,
    opacity: f64,
    width: i32,
    height: i32,
}

#[derive(Clone)]
struct SurfaceTabMotionState {
    start_time: i64,
    duration_us: i64,
    start_items: HashMap<SurfaceTabItemKey, PresentedSurfaceTabItem>,
    target_items: HashMap<SurfaceTabItemKey, PresentedSurfaceTabItem>,
}

#[derive(Clone)]
struct SurfaceTabExitAnimation {
    root: GtkBox,
    start_time: i64,
    duration_us: i64,
    start_item: PresentedSurfaceTabItem,
    end_item: PresentedSurfaceTabItem,
}

#[derive(Clone)]
struct SurfaceTabDragState {
    surface_id: SurfaceId,
    start_x: f64,
    current_dx: f64,
    preview_order: Vec<SurfaceId>,
    threshold_crossed: bool,
}

#[derive(Default)]
struct SurfaceTabStripState {
    model_order: Vec<SurfaceId>,
    layout: Option<SurfaceTabStripLayout>,
    presented: HashMap<SurfaceTabItemKey, PresentedSurfaceTabItem>,
    motion: Option<SurfaceTabMotionState>,
    exiting: Vec<SurfaceTabExitAnimation>,
    drag: Option<SurfaceTabDragState>,
    tick_running: bool,
    suppress_click_surface: Option<SurfaceId>,
    next_animation_duration_us: Option<i64>,
}

#[derive(Clone, PartialEq, Eq)]
enum LayoutRenderKey {
    WorkspaceWindows {
        windows: Vec<WorkspaceWindowRenderKey>,
    },
}

#[derive(Clone, Default, PartialEq, Eq)]
struct LayoutRenderState {
    workspace_id: Option<taskers_domain::WorkspaceId>,
    viewport_width: i32,
    viewport_height: i32,
    overview_mode: bool,
    layout: Option<LayoutRenderKey>,
}

#[derive(Clone, PartialEq, Eq)]
struct WorkspaceWindowRenderKey {
    window_id: WorkspaceWindowId,
    frame: WindowFrame,
    layout: LayoutNode,
}

#[derive(Clone, Copy)]
struct CanvasMetrics {
    offset_x: i32,
    offset_y: i32,
    width: i32,
    height: i32,
}

const WORKSPACE_CANVAS_PADDING: i32 = 2;
const SURFACE_TAB_GAP: i32 = 4;
const SURFACE_TAB_MIN_WIDTH: i32 = 72;
const SURFACE_TAB_MAX_WIDTH: i32 = 220;

#[derive(Clone, Copy)]
struct WorkspaceRenderContext {
    viewport_height: i32,
    overview_mode: bool,
    overview_scale: f64,
}

#[derive(Clone)]
struct AgentIconWidget {
    root: DrawingArea,
    state: Rc<RefCell<AgentIconState>>,
}

#[derive(Clone, Copy, Default)]
struct AgentIconState {
    kind: Option<&'static str>,
}

#[derive(Clone, Copy)]
struct AgentIconSpec {
    view_box_width: f64,
    view_box_height: f64,
    paths: &'static [AgentIconPathSpec],
}

#[derive(Clone, Copy)]
struct AgentIconPathSpec {
    data: &'static str,
    fill: AgentIconFill,
}

#[derive(Clone, Copy)]
enum AgentIconFill {
    CurrentColor,
    Fixed(AgentIconColor),
}

#[derive(Clone, Copy)]
struct AgentIconColor {
    red: f64,
    green: f64,
    blue: f64,
    alpha: f64,
}

thread_local! {
    static AGENT_ICON_PATHS: RefCell<HashMap<&'static str, Rc<Vec<SimplePathSegment>>>> =
        RefCell::new(HashMap::new());
}

impl UiHandle {
    fn new(
        app_state: AppState,
        backend_choice: BackendChoice,
        config_path: PathBuf,
        app_config: AppConfig,
        application: adw::Application,
        window: adw::ApplicationWindow,
        overlay: adw::ToastOverlay,
        crash_reporter: CrashReporter,
        ghostty_host: Option<GhosttyHost>,
        shell_launch: ShellLaunchSpec,
    ) -> Rc<Self> {
        Rc::new(Self {
            app_state,
            backend_choice,
            application,
            window,
            overlay,
            crash_reporter,
            ghostty_host,
            shell_launch,
            ghostty_surfaces: RefCell::new(HashMap::new()),
            shell: RefCell::new(None),
            pane_cards: RefCell::new(HashMap::new()),
            settings: RefCell::new(app_config),
            config_path,
            layout_state: RefCell::new(LayoutRenderState::default()),
            last_rendered: RefCell::new(None),
            suppress_viewport_events: RefCell::new(false),
            pending_viewport: RefCell::new(None),
            pending_viewport_source: RefCell::new(None),
            pending_focus_source: RefCell::new(None),
            desktop_notifications: RefCell::new(HashSet::new()),
            overview_mode: Cell::new(false),
            workspace_transition_state: RefCell::new(WorkspaceTransitionState::default()),
        })
    }

    fn refresh(self: &Rc<Self>, force: bool) {
        let model = self.app_state.snapshot_model();
        if let Err(error) = self.app_state.runtime().sync_model(&model) {
            self.toast(&error.to_string());
        }
        let next_layout_state = compute_layout_render_state(self.as_ref(), &model);
        if !force
            && self
                .last_rendered
                .borrow()
                .as_ref()
                .is_some_and(|existing| existing == &model)
            && *self.layout_state.borrow() == next_layout_state
        {
            return;
        }

        if let Err(error) = self.app_state.persist_model(&model) {
            self.toast(&error.to_string());
        }

        self.ensure_shell();
        self.render_model(&model);
        // Clean up stale caches AFTER layout rebuild so that Ghostty surfaces
        // for closed panes stay alive during the Paned tree teardown, avoiding
        // shared GL context corruption.
        self.cleanup_stale_panes(&model);
        self.write_ui_integrity_snapshot(&model);
        *self.last_rendered.borrow_mut() = Some(model);
    }

    fn dispatch(self: &Rc<Self>, command: ControlCommand) {
        let before = self.app_state.snapshot_model();
        if let Err(error) = self.app_state.dispatch(command) {
            self.toast(&error.to_string());
            return;
        }
        let after = self.app_state.snapshot_model();
        self.refresh(before != after);
    }

    fn toast(&self, message: &str) {
        self.overlay.add_toast(adw::Toast::new(message));
    }

    fn shortcut_spec(&self, action: ShortcutAction) -> Option<(gdk::Key, gdk::ModifierType)> {
        let accelerator = self
            .settings
            .borrow()
            .keybindings
            .accelerator(action)
            .to_string();
        gtk::accelerator_parse(&accelerator)
    }

    fn shortcut_label(&self, action: ShortcutAction) -> String {
        self.shortcut_spec(action)
            .map(|(key, modifiers)| gtk::accelerator_get_label(key, modifiers).to_string())
            .unwrap_or_else(|| {
                self.settings
                    .borrow()
                    .keybindings
                    .accelerator(action)
                    .to_string()
            })
    }

    fn shortcut_matches(
        &self,
        action: ShortcutAction,
        key: gdk::Key,
        state: gdk::ModifierType,
    ) -> bool {
        self.shortcut_spec(action)
            .is_some_and(|(expected_key, expected_modifiers)| {
                key == expected_key && normalize_shortcut_modifiers(state) == expected_modifiers
            })
    }

    fn set_shortcut(
        self: &Rc<Self>,
        action: ShortcutAction,
        accelerator: String,
    ) -> Result<String, String> {
        let Some((key, modifiers)) = gtk::accelerator_parse(&accelerator) else {
            return Err("shortcut is not a valid GTK accelerator".into());
        };

        if normalize_shortcut_modifiers(modifiers).is_empty() {
            return Err("shortcut must include at least one modifier".into());
        }
        if reserved_direction_shortcut(key, modifiers) {
            return Err("shortcut conflicts with the built-in directional bindings".into());
        }

        for other_action in ShortcutAction::ALL {
            if other_action == action {
                continue;
            }
            if self
                .shortcut_spec(other_action)
                .is_some_and(|(other_key, other_modifiers)| {
                    other_key == key && other_modifiers == normalize_shortcut_modifiers(modifiers)
                })
            {
                return Err(format!(
                    "shortcut is already assigned to {}",
                    other_action.label()
                ));
            }
        }

        let next_label =
            gtk::accelerator_get_label(key, normalize_shortcut_modifiers(modifiers)).to_string();
        let mut next_settings = self.settings.borrow().clone();
        next_settings.keybindings.set_accelerator(
            action,
            gtk::accelerator_name(key, normalize_shortcut_modifiers(modifiers)).to_string(),
        );
        settings_store::save_config(&self.config_path, &next_settings)
            .map_err(|error| format!("failed to save settings: {error}"))?;
        *self.settings.borrow_mut() = next_settings;
        Ok(next_label)
    }

    fn save_settings(&self, next_settings: AppConfig) -> Result<(), String> {
        settings_store::save_config(&self.config_path, &next_settings)
            .map_err(|error| format!("failed to save settings: {error}"))?;
        *self.settings.borrow_mut() = next_settings;
        Ok(())
    }

    fn set_shell_program(self: &Rc<Self>, shell_program: Option<String>) -> Result<(), String> {
        let normalized = shell_program.and_then(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        });
        validate_shell_program(normalized.as_deref())
            .map_err(|error| format!("invalid shell program: {error}"))?;

        let mut next_settings = self.settings.borrow().clone();
        next_settings.shell.program = normalized;
        self.save_settings(next_settings)
    }

    fn present_shortcut_capture_dialog(self: &Rc<Self>, action: ShortcutAction, label: &Label) {
        let dialog = gtk::Dialog::with_buttons(
            Some(action.label()),
            Some(&self.window),
            gtk::DialogFlags::MODAL,
            &[("Cancel", gtk::ResponseType::Cancel)],
        );
        dialog.set_default_size(420, -1);
        dialog.connect_response(|dialog, _| dialog.close());

        let content = dialog.content_area();
        content.set_margin_start(18);
        content.set_margin_end(18);
        content.set_margin_top(18);
        content.set_margin_bottom(18);
        content.set_spacing(12);

        let prompt = Label::new(Some("Press the new shortcut now."));
        prompt.set_xalign(0.0);
        prompt.add_css_class("dialog-heading");
        content.append(&prompt);

        let detail = Label::new(Some(
            "Esc cancels. Built-in directional chords stay reserved so navigation remains predictable.",
        ));
        detail.set_xalign(0.0);
        detail.set_wrap(true);
        content.append(&detail);

        let current = Label::new(Some(&format!("Current: {}", self.shortcut_label(action))));
        current.set_xalign(0.0);
        current.add_css_class("dim-label");
        content.append(&current);

        let capture_ui = Rc::clone(self);
        let captured_label = label.clone();
        let capture_dialog = dialog.clone();
        let controller = gtk::EventControllerKey::new();
        controller.set_propagation_phase(gtk::PropagationPhase::Capture);
        controller.connect_key_pressed(move |_, key, _, state| {
            if key == gdk::Key::Escape {
                capture_dialog.close();
                return glib::Propagation::Stop;
            }
            if is_modifier_key(key) {
                return glib::Propagation::Stop;
            }

            let modifiers = normalize_shortcut_modifiers(state);
            if modifiers.is_empty() || !gtk::accelerator_valid(key, modifiers) {
                capture_ui.toast("Shortcut must use a valid modified key chord.");
                return glib::Propagation::Stop;
            }

            match capture_ui.set_shortcut(action, gtk::accelerator_name(key, modifiers).to_string())
            {
                Ok(next_label) => {
                    captured_label.set_text(&next_label);
                    capture_dialog.close();
                }
                Err(error) => capture_ui.toast(&error),
            }

            glib::Propagation::Stop
        });
        dialog.add_controller(controller);
        dialog.present();
    }

    fn present_settings_dialog(self: &Rc<Self>) {
        let dialog = gtk::Dialog::with_buttons(
            Some("Settings"),
            Some(&self.window),
            gtk::DialogFlags::MODAL,
            &[("Close", gtk::ResponseType::Close)],
        );
        dialog.set_default_size(560, -1);
        dialog.connect_response(|dialog, _| dialog.close());

        let content = dialog.content_area();
        content.set_margin_start(18);
        content.set_margin_end(18);
        content.set_margin_top(18);
        content.set_margin_bottom(18);
        content.set_spacing(14);

        // ── Animations toggle ──
        let anim_row = GtkBox::new(Orientation::Horizontal, 12);
        let anim_details = GtkBox::new(Orientation::Vertical, 4);
        anim_details.set_hexpand(true);

        let anim_title = Label::new(Some("Animations"));
        anim_title.set_xalign(0.0);
        anim_title.add_css_class("pane-title");
        anim_details.append(&anim_title);

        let anim_detail = Label::new(Some(
            "Animate terminal lifecycle changes, including pane/window create-delete transitions and surface tab open-close motion.",
        ));
        anim_detail.set_xalign(0.0);
        anim_detail.set_wrap(true);
        anim_detail.add_css_class("dim-label");
        anim_details.append(&anim_detail);
        anim_row.append(&anim_details);

        let anim_switch = gtk::Switch::new();
        anim_switch.set_active(self.settings.borrow().animations_enabled);
        anim_switch.set_valign(Align::Center);
        let anim_ui = Rc::clone(self);
        anim_switch.connect_state_set(move |_, active| {
            let mut next_settings = anim_ui.settings.borrow().clone();
            next_settings.animations_enabled = active;
            if let Err(error) = settings_store::save_config(&anim_ui.config_path, &next_settings) {
                anim_ui.toast(&format!("Failed to save settings: {error}"));
            }
            *anim_ui.settings.borrow_mut() = next_settings;
            glib::Propagation::Proceed
        });
        anim_row.append(&anim_switch);
        content.append(&anim_row);

        let sep = Separator::new(Orientation::Horizontal);
        sep.add_css_class("context-separator");
        content.append(&sep);

        let shell_row = GtkBox::new(Orientation::Horizontal, 12);
        let shell_details = GtkBox::new(Orientation::Vertical, 4);
        shell_details.set_hexpand(true);

        let shell_title = Label::new(Some("Shell program"));
        shell_title.set_xalign(0.0);
        shell_title.add_css_class("pane-title");
        shell_details.append(&shell_title);

        let system_shell = default_shell_program();
        let shell_detail = Label::new(Some(&format!(
            "Optional shell override for new panes. Leave empty to use the system login shell (currently {}). Relaunch Taskers after changing this.",
            system_shell.display()
        )));
        shell_detail.set_xalign(0.0);
        shell_detail.set_wrap(true);
        shell_detail.add_css_class("dim-label");
        shell_details.append(&shell_detail);
        shell_row.append(&shell_details);

        let shell_entry = Entry::new();
        shell_entry.set_hexpand(true);
        shell_entry.set_width_chars(24);
        shell_entry.set_placeholder_text(Some("System default login shell"));
        if let Some(program) = self.settings.borrow().shell.program.as_deref() {
            shell_entry.set_text(program);
        }
        let activate_ui = Rc::clone(self);
        shell_entry.connect_activate(move |entry| {
            let text = entry.text().to_string();
            if let Err(error) = activate_ui.set_shell_program(Some(text.clone())) {
                activate_ui.toast(&error);
                return;
            }
            let normalized = text.trim().to_string();
            entry.set_text(&normalized);
            activate_ui.toast("Shell setting saved. Relaunch Taskers to apply.");
        });
        let focus_ui = Rc::clone(self);
        shell_entry.connect_notify_local(Some("has-focus"), move |entry, _| {
            if entry.has_focus() {
                return;
            }

            let text = entry.text().to_string();
            if let Err(error) = focus_ui.set_shell_program(Some(text.clone())) {
                focus_ui.toast(&error);
                return;
            }
            entry.set_text(text.trim());
        });
        shell_row.append(&shell_entry);

        let reset_shell = Button::with_label("Use system");
        let reset_ui = Rc::clone(self);
        let reset_entry = shell_entry.clone();
        reset_shell.connect_clicked(move |_| {
            if let Err(error) = reset_ui.set_shell_program(None) {
                reset_ui.toast(&error);
                return;
            }
            reset_entry.set_text("");
            reset_ui.toast("Shell setting cleared. Relaunch Taskers to apply.");
        });
        shell_row.append(&reset_shell);
        content.append(&shell_row);

        let sep = Separator::new(Orientation::Horizontal);
        sep.add_css_class("context-separator");
        content.append(&sep);

        // ── Keyboard shortcuts ──
        let intro = Label::new(Some(
            "Keyboard shortcuts. Directional navigation and resize chords stay fixed.",
        ));
        intro.set_wrap(true);
        intro.set_xalign(0.0);
        content.append(&intro);

        for action in ShortcutAction::ALL {
            let row = GtkBox::new(Orientation::Horizontal, 12);

            let details = GtkBox::new(Orientation::Vertical, 4);
            details.set_hexpand(true);

            let title = Label::new(Some(action.label()));
            title.set_xalign(0.0);
            title.add_css_class("pane-title");
            details.append(&title);

            let detail = Label::new(Some(action.detail()));
            detail.set_xalign(0.0);
            detail.set_wrap(true);
            detail.add_css_class("dim-label");
            details.append(&detail);

            row.append(&details);

            let shortcut_label = Label::new(Some(&self.shortcut_label(action)));
            shortcut_label.set_width_chars(14);
            shortcut_label.set_xalign(1.0);
            shortcut_label.add_css_class("monospace");
            row.append(&shortcut_label);

            let change_button = Button::with_label("Change");
            let change_ui = Rc::clone(self);
            let change_label = shortcut_label.clone();
            change_button.connect_clicked(move |_| {
                change_ui.present_shortcut_capture_dialog(action, &change_label);
            });
            row.append(&change_button);

            let reset_button = Button::with_label("Reset");
            let reset_ui = Rc::clone(self);
            let reset_label = shortcut_label.clone();
            reset_button.connect_clicked(move |_| {
                match reset_ui.set_shortcut(action, action.default_accelerator().to_string()) {
                    Ok(next_label) => reset_label.set_text(&next_label),
                    Err(error) => reset_ui.toast(&error),
                }
            });
            row.append(&reset_button);

            content.append(&row);
        }

        dialog.present();
    }

    fn send_input(self: &Rc<Self>, surface_id: SurfaceId, input: String) {
        if let Err(error) = self.app_state.runtime().send_input(surface_id, &input) {
            self.toast(&error.to_string());
        }
    }

    fn tick_terminal_host(&self) {
        if let Some(host) = &self.ghostty_host {
            let _ = host.tick();
        }
    }

    fn terminal_widget(
        self: &Rc<Self>,
        workspace_id: taskers_domain::WorkspaceId,
        pane: &PaneRecord,
    ) -> Option<Widget> {
        if self.backend_choice != BackendChoice::Ghostty {
            return None;
        }

        let surface = pane.active_surface()?;

        if let Some(widget) = self.ghostty_surfaces.borrow().get(&surface.id) {
            detach_widget(widget);
            return Some(widget.clone());
        }

        let host = self.ghostty_host.as_ref()?;
        let mut env = self.shell_launch.env.clone();
        env.insert("TASKERS_PANE_ID".into(), pane.id.to_string());
        env.insert("TASKERS_WORKSPACE_ID".into(), workspace_id.to_string());
        env.insert("TASKERS_SURFACE_ID".into(), surface.id.to_string());
        let widget = match host.create_surface(&SurfaceDescriptor {
            cols: 120,
            rows: 40,
            cwd: surface.metadata.cwd.clone(),
            title: surface.metadata.title.clone(),
            command_argv: self
                .shell_launch
                .program_and_args()
                .into_iter()
                .collect::<Vec<_>>(),
            env,
        }) {
            Ok(widget) => widget,
            Err(error) => {
                self.toast(&error.to_string());
                return None;
            }
        };

        widget.set_hexpand(true);
        widget.set_vexpand(true);
        widget.add_css_class("terminal-output");
        connect_ghostty_widget(self, workspace_id, pane.id, surface.id, &widget);
        self.ghostty_surfaces
            .borrow_mut()
            .insert(surface.id, widget.clone());
        Some(widget)
    }

    fn present_window(&self) {
        self.window.present();
    }

    fn ensure_shell(self: &Rc<Self>) {
        if self.shell.borrow().is_some() {
            return;
        }

        let shell = build_shell_scaffold(self);
        self.overlay.set_child(Some(&shell.root));
        *self.shell.borrow_mut() = Some(shell);
    }

    fn cleanup_stale_panes(&self, model: &AppModel) {
        let live: HashSet<SurfaceId> = model
            .workspaces
            .values()
            .flat_map(|ws| {
                ws.panes
                    .values()
                    .flat_map(|pane| pane.surface_ids())
                    .collect::<Vec<_>>()
            })
            .collect();
        self.pane_cards.borrow_mut().retain(|id, _| {
            model
                .workspaces
                .values()
                .any(|workspace| workspace.panes.contains_key(id))
        });
        self.ghostty_surfaces
            .borrow_mut()
            .retain(|id, _| live.contains(id));
    }

    fn render_model(self: &Rc<Self>, model: &AppModel) {
        let shell = self
            .shell
            .borrow()
            .as_ref()
            .cloned()
            .expect("shell scaffold should exist before render");
        update_sidebar(self, &shell, model);
        update_toolbar(&shell, model, self.overview_mode.get());
        update_activity_panel(self, &shell, model);
        update_layout(self, &shell, model);
        self.sync_desktop_notifications(model);
        if model.active_workspace().is_some() && !self.overview_mode.get() {
            self.queue_focus_active_pane_input(model);
        }
    }

    fn sync_desktop_notifications(&self, model: &AppModel) {
        let items = model.activity_items();
        let active_keys = items
            .iter()
            .map(activity_notification_key)
            .collect::<HashSet<_>>();

        let mut delivered = self.desktop_notifications.borrow_mut();
        let stale = delivered
            .iter()
            .filter(|key| !active_keys.contains(*key))
            .cloned()
            .collect::<Vec<_>>();
        for key in stale {
            self.application.withdraw_notification(&key);
            delivered.remove(&key);
        }

        for item in items {
            let key = activity_notification_key(&item);
            if delivered.contains(&key) {
                continue;
            }

            let (workspace_label, pane_title) = model
                .workspaces
                .get(&item.workspace_id)
                .map(|workspace| {
                    let title = workspace
                        .panes
                        .get(&item.pane_id)
                        .and_then(|pane| {
                            pane.surfaces
                                .get(&item.surface_id)
                                .or_else(|| pane.active_surface())
                                .map(display_surface_title)
                        })
                        .unwrap_or_else(|| "Terminal pane".into());
                    (workspace.label.clone(), title)
                })
                .unwrap_or_else(|| ("Workspace".into(), "Terminal pane".into()));

            let notification_title =
                format!("{} in {}", activity_kind_label(&item.kind), workspace_label);
            let notification = gtk::gio::Notification::new(&notification_title);
            notification.set_body(Some(&format!("{pane_title}: {}", item.message)));
            notification.set_priority(match item.state {
                AttentionState::Error => gtk::gio::NotificationPriority::Urgent,
                AttentionState::WaitingInput => gtk::gio::NotificationPriority::High,
                _ => gtk::gio::NotificationPriority::Normal,
            });
            self.application
                .send_notification(Some(&key), &notification);
            delivered.insert(key);
        }
    }

    fn queue_viewport_persist(
        self: &Rc<Self>,
        workspace_id: taskers_domain::WorkspaceId,
        viewport: WorkspaceViewport,
    ) {
        if self.overview_mode.get() {
            return;
        }
        *self.pending_viewport.borrow_mut() = Some((workspace_id, viewport));
        if let Some(source) = self.pending_viewport_source.borrow_mut().take() {
            source.remove();
        }

        let save_ui = Rc::clone(self);
        let source = glib::timeout_add_local_once(Duration::from_millis(150), move || {
            *save_ui.pending_viewport_source.borrow_mut() = None;
            let Some((workspace_id, viewport)) = save_ui.pending_viewport.borrow_mut().take()
            else {
                return;
            };

            let current_viewport = save_ui
                .app_state
                .snapshot_model()
                .workspaces
                .get(&workspace_id)
                .map(|workspace| workspace.viewport.clone());
            if current_viewport.as_ref() == Some(&viewport) {
                return;
            }

            save_ui.dispatch(ControlCommand::SetWorkspaceViewport {
                workspace_id,
                viewport,
            });
        });
        *self.pending_viewport_source.borrow_mut() = Some(source);
    }

    fn persist_viewport_now(
        self: &Rc<Self>,
        workspace_id: taskers_domain::WorkspaceId,
        viewport: WorkspaceViewport,
    ) {
        let current_viewport = self
            .app_state
            .snapshot_model()
            .workspaces
            .get(&workspace_id)
            .map(|workspace| workspace.viewport.clone());
        if current_viewport.as_ref() == Some(&viewport) {
            return;
        }

        self.dispatch(ControlCommand::SetWorkspaceViewport {
            workspace_id,
            viewport,
        });
    }

    fn queue_active_workspace_viewport_persist(self: &Rc<Self>) {
        if *self.suppress_viewport_events.borrow() || self.overview_mode.get() {
            return;
        }

        let Some(shell) = self.shell.borrow().as_ref().cloned() else {
            return;
        };
        let model = self.app_state.snapshot_model();
        let Some(workspace) = model.active_workspace() else {
            return;
        };
        self.queue_viewport_persist(workspace.id, current_workspace_viewport(&shell));
    }

    fn set_workspace_viewport(&self, shell: &ShellWidgets, viewport: &WorkspaceViewport) {
        let h_adjustment = shell.layout_scroll.hadjustment();
        let v_adjustment = shell.layout_scroll.vadjustment();
        let max_x = (h_adjustment.upper() - h_adjustment.page_size()).max(h_adjustment.lower());
        let max_y = (v_adjustment.upper() - v_adjustment.page_size()).max(v_adjustment.lower());

        *self.suppress_viewport_events.borrow_mut() = true;
        h_adjustment.set_value(f64::from(viewport.x).clamp(h_adjustment.lower(), max_x));
        v_adjustment.set_value(f64::from(viewport.y).clamp(v_adjustment.lower(), max_y));
        *self.suppress_viewport_events.borrow_mut() = false;
    }

    fn reveal_active_window(&self, shell: &ShellWidgets, workspace: &Workspace) {
        let Some(active_window) = workspace.active_window_record() else {
            return;
        };
        let render_context = workspace_render_context(
            self,
            Some(shell),
            workspace,
            self.overview_mode.get(),
            workspace_viewport_width(self, Some(shell)),
            workspace_viewport_height(self, Some(shell)),
        );
        let metrics = workspace_canvas_metrics(workspace, render_context);
        let active_frame = display_window_frame(active_window.frame, render_context);

        let h_adjustment = shell.layout_scroll.hadjustment();
        let v_adjustment = shell.layout_scroll.vadjustment();
        let h_page = h_adjustment.page_size();
        let v_page = v_adjustment.page_size();
        if h_page <= 0.0 || v_page <= 0.0 {
            return;
        }

        let left = f64::from(active_frame.x + metrics.offset_x);
        let top = f64::from(active_frame.y + metrics.offset_y);
        let right = left + f64::from(active_frame.width);
        let bottom = top + f64::from(active_frame.height);

        let current_x = h_adjustment.value();
        let current_y = v_adjustment.value();
        let max_x = (h_adjustment.upper() - h_page).max(h_adjustment.lower());
        let max_y = (v_adjustment.upper() - v_page).max(v_adjustment.lower());

        let next_x = if left < current_x {
            left
        } else if right > current_x + h_page {
            right - h_page
        } else {
            current_x
        }
        .clamp(h_adjustment.lower(), max_x);

        let next_y = if top < current_y {
            top
        } else if bottom > current_y + v_page {
            bottom - v_page
        } else {
            current_y
        }
        .clamp(v_adjustment.lower(), max_y);

        *self.suppress_viewport_events.borrow_mut() = true;
        h_adjustment.set_value(next_x);
        v_adjustment.set_value(next_y);
        *self.suppress_viewport_events.borrow_mut() = false;
    }

    fn write_ui_integrity_snapshot(&self, model: &AppModel) {
        let path = self.crash_reporter.ui_integrity_path().to_path_buf();
        let live_layout_host = self
            .shell
            .borrow()
            .as_ref()
            .map(|shell| shell.layout_host.clone().upcast::<Widget>());

        let (cached_pane_card_ids, attached_pane_card_ids) = {
            let pane_cards = self.pane_cards.borrow();
            (
                sorted_id_strings(pane_cards.keys().copied()),
                sorted_id_strings(pane_cards.iter().filter_map(|(id, card)| {
                    live_layout_host.as_ref().and_then(|layout_host| {
                        widget_is_descendant_of(card.root.upcast_ref(), layout_host).then_some(*id)
                    })
                })),
            )
        };

        let (cached_ghostty_surface_ids, attached_ghostty_surface_ids) = {
            let ghostty_surfaces = self.ghostty_surfaces.borrow();
            (
                sorted_id_strings(ghostty_surfaces.keys().copied()),
                sorted_id_strings(ghostty_surfaces.iter().filter_map(|(id, widget)| {
                    live_layout_host.as_ref().and_then(|layout_host| {
                        widget_is_descendant_of(widget, layout_host).then_some(*id)
                    })
                })),
            )
        };

        let (
            layout_host_child_count,
            layout_root_widget_type,
            viewport_page_x,
            viewport_page_y,
            viewport_upper_x,
            viewport_upper_y,
        ) = {
            let shell = self.shell.borrow();
            shell
                .as_ref()
                .map_or((0, None, 0.0, 0.0, 0.0, 0.0), |shell| {
                    (
                        count_widget_children(shell.layout_host.upcast_ref()),
                        shell
                            .layout_host
                            .first_child()
                            .map(|child| child.type_().name().to_string()),
                        shell.layout_scroll.hadjustment().page_size(),
                        shell.layout_scroll.vadjustment().page_size(),
                        shell.layout_scroll.hadjustment().upper(),
                        shell.layout_scroll.vadjustment().upper(),
                    )
                })
        };
        let display_viewport_height = self
            .shell
            .borrow()
            .as_ref()
            .map(|shell| workspace_viewport_height(self, Some(shell)))
            .unwrap_or(DEFAULT_WORKSPACE_WINDOW_HEIGHT);
        let display_viewport_width = self
            .shell
            .borrow()
            .as_ref()
            .map(|shell| workspace_viewport_width(self, Some(shell)))
            .unwrap_or(DEFAULT_WORKSPACE_WINDOW_WIDTH);
        let focused_widget_type = gtk::prelude::GtkWindowExt::focus(&self.window)
            .map(|widget| widget.type_().name().to_string());
        let active_pane_focus_widget_type = model.active_workspace().and_then(|workspace| {
            self.pane_cards
                .borrow()
                .get(&workspace.active_pane)
                .map(|card| card.focus_target.type_().name().to_string())
        });
        let active_pane_focus_has_focus = model.active_workspace().is_some_and(|workspace| {
            self.pane_cards
                .borrow()
                .get(&workspace.active_pane)
                .is_some_and(|card| widget_contains_window_focus(&self.window, &card.focus_target))
        });
        let active_pane_card_has_focus = model.active_workspace().is_some_and(|workspace| {
            self.pane_cards
                .borrow()
                .get(&workspace.active_pane)
                .is_some_and(|card| {
                    widget_contains_window_focus(&self.window, card.root.upcast_ref())
                })
        });

        let all_live_pane_ids = sorted_id_strings(
            model
                .workspaces
                .values()
                .flat_map(|workspace| workspace.panes.keys().copied()),
        );
        let (
            active_workspace_pane_ids,
            active_layout_pane_ids,
            active_workspace_label,
            active_window_id,
            workspace_window_ids,
            workspace_windows,
        ) = model.active_workspace().map_or_else(
            || {
                (
                    Vec::new(),
                    Vec::new(),
                    None,
                    None::<String>,
                    Vec::<String>::new(),
                    Vec::<serde_json::Value>::new(),
                )
            },
            |workspace| {
                let render_context = workspace_render_context(
                    self,
                    self.shell.borrow().as_ref(),
                    workspace,
                    self.overview_mode.get(),
                    display_viewport_width,
                    display_viewport_height,
                );
                (
                    sorted_id_strings(workspace.panes.keys().copied()),
                    workspace
                        .windows
                        .values()
                        .flat_map(|window| window.layout.leaves())
                        .map(|pane_id| pane_id.to_string())
                        .collect(),
                    Some(workspace.label.clone()),
                    Some(workspace.active_window.to_string()),
                    sorted_id_strings(workspace.windows.keys().copied()),
                    workspace
                        .windows
                        .values()
                        .map(|window| {
                            let display_frame = display_window_frame(window.frame, render_context);
                            json!({
                                "id": window.id.to_string(),
                                "x": window.frame.x,
                                "y": window.frame.y,
                                "width": window.frame.width,
                                "height": window.frame.height,
                                "display_x": display_frame.x,
                                "display_y": display_frame.y,
                                "display_width": display_frame.width,
                                "display_height": display_frame.height,
                                "active_pane": window.active_pane.to_string(),
                                "leaf_pane_ids": window
                                    .layout
                                    .leaves()
                                    .into_iter()
                                    .map(|pane_id| pane_id.to_string())
                                    .collect::<Vec<_>>(),
                            })
                        })
                        .collect(),
                )
            },
        );
        let viewport = self
            .shell
            .borrow()
            .as_ref()
            .map(current_workspace_viewport)
            .unwrap_or_default();

        let payload = json!({
            "active_workspace_id": model.active_workspace_id().map(|id| id.to_string()),
            "active_workspace_label": active_workspace_label,
            "active_workspace_window_id": active_window_id,
            "active_workspace_window_ids": workspace_window_ids,
            "active_workspace_windows": workspace_windows,
            "all_live_pane_ids": all_live_pane_ids,
            "active_workspace_pane_ids": active_workspace_pane_ids,
            "active_layout_pane_ids": active_layout_pane_ids,
            "cached_pane_card_ids": cached_pane_card_ids,
            "attached_pane_card_ids": attached_pane_card_ids,
            "cached_ghostty_surface_ids": cached_ghostty_surface_ids,
            "attached_ghostty_surface_ids": attached_ghostty_surface_ids,
            "layout_host_child_count": layout_host_child_count,
            "layout_root_widget_type": layout_root_widget_type,
            "viewport_x": viewport.x,
            "viewport_y": viewport.y,
            "viewport_page_x": viewport_page_x,
            "viewport_page_y": viewport_page_y,
            "viewport_upper_x": viewport_upper_x,
            "viewport_upper_y": viewport_upper_y,
            "focused_widget_type": focused_widget_type,
            "active_pane_focus_widget_type": active_pane_focus_widget_type,
            "active_pane_focus_has_focus": active_pane_focus_has_focus,
            "active_pane_card_has_focus": active_pane_card_has_focus,
            "overview_mode": self.overview_mode.get(),
        });

        if let Some(parent) = path.parent()
            && let Err(error) = std::fs::create_dir_all(parent)
        {
            eprintln!(
                "failed to create taskers UI integrity directory {}: {error}",
                parent.display()
            );
            return;
        }

        let tmp_path = path.with_extension("tmp");
        let encoded = match serde_json::to_vec_pretty(&payload) {
            Ok(encoded) => encoded,
            Err(error) => {
                eprintln!("failed to encode taskers UI integrity snapshot: {error}");
                return;
            }
        };

        if let Err(error) = std::fs::write(&tmp_path, encoded) {
            eprintln!(
                "failed to write taskers UI integrity snapshot to {}: {error}",
                tmp_path.display()
            );
            return;
        }

        if let Err(error) = std::fs::rename(&tmp_path, &path) {
            let _ = std::fs::remove_file(&tmp_path);
            eprintln!(
                "failed to publish taskers UI integrity snapshot to {}: {error}",
                path.display()
            );
        }
    }

    fn pane_card(
        self: &Rc<Self>,
        workspace_id: taskers_domain::WorkspaceId,
        pane: &PaneRecord,
    ) -> PaneCardWidgets {
        if let Some(card) = self.pane_cards.borrow().get(&pane.id) {
            configure_pane_card_layout(&card);
            return card.clone();
        }

        let root = GtkBox::new(Orientation::Vertical, 0);
        root.add_css_class("pane-card");

        let header = GtkBox::new(Orientation::Horizontal, 4);
        header.add_css_class("pane-header");
        header.set_margin_start(6);
        header.set_margin_end(4);
        header.set_margin_top(1);
        header.set_margin_bottom(1);

        let agent_icon = build_agent_icon(pane.active_surface().and_then(surface_agent_kind), 14);
        agent_icon.add_css_class("pane-agent-icon");
        header.append(agent_icon.widget());

        let title = Label::new(Some("Unnamed terminal pane"));
        title.add_css_class("pane-title");
        title.set_xalign(0.0);
        title.set_hexpand(true);
        header.append(&title);

        let status_dot = Label::new(Some("\u{25cf}"));
        status_dot.add_css_class("status-dot");
        let pane_attention = pane.active_attention();
        status_dot.add_css_class(&attention_dot_class(pane_attention));
        status_dot.set_tooltip_text(Some(pane_attention.label()));
        header.append(&status_dot);

        let new_window_right_btn = Button::with_label("\u{2192}");
        new_window_right_btn.add_css_class("pane-action");
        new_window_right_btn.add_css_class("pane-window-action");
        new_window_right_btn.set_tooltip_text(Some("New window right"));
        let nwr_ui = Rc::clone(self);
        let nwr_pane_id = pane.id;
        new_window_right_btn.connect_clicked(move |_| {
            create_workspace_window_from_pane(&nwr_ui, workspace_id, nwr_pane_id, Direction::Right);
        });
        header.append(&new_window_right_btn);

        let new_window_down_btn = Button::with_label("\u{2193}");
        new_window_down_btn.add_css_class("pane-action");
        new_window_down_btn.add_css_class("pane-window-action");
        new_window_down_btn.set_tooltip_text(Some("New window below"));
        let nwd_ui = Rc::clone(self);
        let nwd_pane_id = pane.id;
        new_window_down_btn.connect_clicked(move |_| {
            create_workspace_window_from_pane(&nwd_ui, workspace_id, nwd_pane_id, Direction::Down);
        });
        header.append(&new_window_down_btn);

        let split_right_btn = Button::with_label("\u{25eb}");
        split_right_btn.add_css_class("pane-action");
        split_right_btn.add_css_class("pane-split-action");
        split_right_btn.set_tooltip_text(Some("Split right"));
        let sr_ui = Rc::clone(self);
        let sr_pane_id = pane.id;
        split_right_btn.connect_clicked(move |_| {
            sr_ui.dispatch(ControlCommand::SplitPane {
                workspace_id,
                pane_id: Some(sr_pane_id),
                axis: taskers_domain::SplitAxis::Horizontal,
            });
        });
        header.append(&split_right_btn);

        let split_down_btn = Button::with_label("\u{2501}");
        split_down_btn.add_css_class("pane-action");
        split_down_btn.add_css_class("pane-split-action");
        split_down_btn.set_tooltip_text(Some("Split down"));
        let sd_ui = Rc::clone(self);
        let sd_pane_id = pane.id;
        split_down_btn.connect_clicked(move |_| {
            sd_ui.dispatch(ControlCommand::SplitPane {
                workspace_id,
                pane_id: Some(sd_pane_id),
                axis: taskers_domain::SplitAxis::Vertical,
            });
        });
        header.append(&split_down_btn);

        let close_button = Button::with_label("\u{00d7}");
        close_button.add_css_class("pane-close");
        close_button.add_css_class("pane-close-action");
        close_button.set_tooltip_text(Some("Close pane"));
        let close_ui = Rc::clone(self);
        let close_pane_id = pane.id;
        close_button.connect_clicked(move |_| {
            close_ui.dispatch(ControlCommand::ClosePane {
                workspace_id,
                pane_id: close_pane_id,
            });
        });
        header.append(&close_button);

        // Right-click context menu on pane header
        let header_for_ctx = header.clone();
        let ctx_click = gtk::GestureClick::new();
        ctx_click.set_button(3);
        let ctx_ui = Rc::clone(self);
        let ctx_pane_id = pane.id;
        ctx_click.connect_pressed(move |_, _, _, _| {
            let popover = gtk::Popover::new();
            popover.set_parent(&header_for_ctx);

            let content = GtkBox::new(Orientation::Vertical, 2);
            content.set_margin_start(4);
            content.set_margin_end(4);
            content.set_margin_top(4);
            content.set_margin_bottom(4);

            let new_right = Button::with_label("\u{2192} New Window Right");
            new_right.add_css_class("flat");
            new_right.add_css_class("context-item");
            let nr_ui = Rc::clone(&ctx_ui);
            let nr_pop = popover.clone();
            new_right.connect_clicked(move |_| {
                nr_pop.popdown();
                create_workspace_window_from_pane(
                    &nr_ui,
                    workspace_id,
                    ctx_pane_id,
                    Direction::Right,
                );
            });
            content.append(&new_right);

            let new_below = Button::with_label("\u{2193} New Window Below");
            new_below.add_css_class("flat");
            new_below.add_css_class("context-item");
            let nb_ui = Rc::clone(&ctx_ui);
            let nb_pop = popover.clone();
            new_below.connect_clicked(move |_| {
                nb_pop.popdown();
                create_workspace_window_from_pane(
                    &nb_ui,
                    workspace_id,
                    ctx_pane_id,
                    Direction::Down,
                );
            });
            content.append(&new_below);

            let new_window_sep = Separator::new(Orientation::Horizontal);
            new_window_sep.add_css_class("context-separator");
            content.append(&new_window_sep);

            let split_right = Button::with_label("\u{25eb} Split Right");
            split_right.add_css_class("flat");
            split_right.add_css_class("context-item");
            let sr_ui = Rc::clone(&ctx_ui);
            let sr_pop = popover.clone();
            split_right.connect_clicked(move |_| {
                sr_pop.popdown();
                sr_ui.dispatch(ControlCommand::SplitPane {
                    workspace_id,
                    pane_id: Some(ctx_pane_id),
                    axis: taskers_domain::SplitAxis::Horizontal,
                });
            });
            content.append(&split_right);

            let split_down = Button::with_label("\u{2501} Split Down");
            split_down.add_css_class("flat");
            split_down.add_css_class("context-item");
            let sd_ui = Rc::clone(&ctx_ui);
            let sd_pop = popover.clone();
            split_down.connect_clicked(move |_| {
                sd_pop.popdown();
                sd_ui.dispatch(ControlCommand::SplitPane {
                    workspace_id,
                    pane_id: Some(ctx_pane_id),
                    axis: taskers_domain::SplitAxis::Vertical,
                });
            });
            content.append(&split_down);

            let sep = Separator::new(Orientation::Horizontal);
            sep.add_css_class("context-separator");
            content.append(&sep);

            let close = Button::with_label("Close Pane");
            close.add_css_class("flat");
            close.add_css_class("destructive-action");
            let cl_ui = Rc::clone(&ctx_ui);
            let cl_pop = popover.clone();
            close.connect_clicked(move |_| {
                cl_pop.popdown();
                cl_ui.dispatch(ControlCommand::ClosePane {
                    workspace_id,
                    pane_id: ctx_pane_id,
                });
            });
            content.append(&close);

            popover.set_child(Some(&content));
            let pop_cleanup = popover.clone();
            popover.connect_closed(move |_| {
                pop_cleanup.unparent();
            });
            popover.popup();
        });
        header.add_controller(ctx_click);

        root.append(&header);

        let surface_tabs = build_surface_tab_strip(self, workspace_id, pane.id);
        root.append(&surface_tabs.root);

        let terminal_host = GtkBox::new(Orientation::Vertical, 0);
        terminal_host.set_hexpand(true);
        terminal_host.set_vexpand(true);
        root.append(&terminal_host);

        let click = gtk::GestureClick::new();
        let focus_ui = Rc::clone(self);
        let pane_id = pane.id;
        click.connect_pressed(move |_, _, _, _| {
            focus_ui.dispatch(ControlCommand::FocusPane {
                workspace_id,
                pane_id,
            });
        });
        root.add_controller(click);

        let card = PaneCardWidgets {
            focus_target: root.clone().upcast(),
            root,
            agent_icon,
            title,
            status_dot,
            surface_tabs,
            terminal_host,
        };

        configure_pane_card_layout(&card);
        let card = PaneCardWidgets { ..card };
        self.pane_cards.borrow_mut().insert(pane.id, card.clone());
        sync_surface_tabs(self, workspace_id, pane, &card);
        refresh_terminal_body(self, workspace_id, pane, &card);
        card
    }

    fn sync_pane_card(
        self: &Rc<Self>,
        workspace_id: taskers_domain::WorkspaceId,
        active_pane: taskers_domain::PaneId,
        pane: &PaneRecord,
    ) {
        let card = self.pane_card(workspace_id, pane);
        let snapshot = pane
            .active_surface()
            .and_then(|surface| self.app_state.runtime().snapshot(surface.id));

        let display_title = pane
            .active_surface()
            .map(display_surface_title)
            .unwrap_or_else(|| "Unnamed terminal pane".into());
        configure_agent_icon(
            &card.agent_icon,
            pane.active_surface().and_then(surface_agent_kind),
            14,
        );
        card.title.set_text(&display_title);
        card.title
            .set_tooltip_text(Some(&format_pane_meta(pane, snapshot.as_ref())));

        if pane.id == active_pane {
            card.root.add_css_class("pane-card-active");
        } else {
            card.root.remove_css_class("pane-card-active");
        }

        for cls in &[
            "status-dot-normal",
            "status-dot-busy",
            "status-dot-completed",
            "status-dot-waiting",
            "status-dot-error",
        ] {
            card.status_dot.remove_css_class(cls);
        }
        let pane_attention = pane.active_attention();
        for cls in &[
            "pane-card-state-busy",
            "pane-card-state-completed",
            "pane-card-state-waiting",
            "pane-card-state-error",
        ] {
            card.root.remove_css_class(cls);
        }
        if pane_attention != AttentionState::Normal {
            card.root.add_css_class(&format!(
                "pane-card-state-{}",
                attention_state_slug(pane_attention)
            ));
        }
        card.status_dot
            .add_css_class(&attention_dot_class(pane_attention));
        card.status_dot
            .set_tooltip_text(Some(pane_attention.label()));
        sync_surface_tabs(self, workspace_id, pane, &card);
        refresh_terminal_body(self, workspace_id, pane, &card);
    }

    fn try_focus_pane_input(
        &self,
        workspace_id: taskers_domain::WorkspaceId,
        pane_id: taskers_domain::PaneId,
    ) -> bool {
        let snapshot = self.app_state.snapshot_model();
        let Some(active_workspace) = snapshot.active_workspace() else {
            return false;
        };
        if active_workspace.id != workspace_id || active_workspace.active_pane != pane_id {
            return false;
        }

        let Some(card) = self.pane_cards.borrow().get(&pane_id).cloned() else {
            return false;
        };

        let target = if card.focus_target.parent().is_some() {
            card.focus_target
        } else {
            card.root.upcast()
        };
        target.set_focusable(true);
        gtk::prelude::RootExt::set_focus(&self.window, Some(&target));

        let focused_surface_id = snapshot
            .active_workspace()
            .and_then(|workspace| workspace.panes.get(&pane_id))
            .and_then(|pane| pane.active_surface().map(|surface| surface.id));
        let focused = if let Some(surface) = focused_surface_id
            .and_then(|surface_id| self.ghostty_surfaces.borrow().get(&surface_id).cloned())
        {
            self.ghostty_host
                .as_ref()
                .is_some_and(|host| host.focus_surface(&surface).is_ok())
        } else {
            false
        };
        if !focused {
            let _ = target.grab_focus();
        }

        widget_contains_window_focus(&self.window, &target)
    }

    fn queue_focus_active_pane_input(self: &Rc<Self>, model: &AppModel) {
        let Some(workspace) = model.active_workspace() else {
            return;
        };
        let expected_workspace_id = workspace.id;
        let expected_pane_id = workspace.active_pane;

        if self.try_focus_pane_input(expected_workspace_id, expected_pane_id) {
            if let Some(source) = self.pending_focus_source.borrow_mut().take() {
                source.remove();
            }
            return;
        }

        if let Some(source) = self.pending_focus_source.borrow_mut().take() {
            source.remove();
        }

        let focus_ui = Rc::clone(self);
        let attempts_remaining = Rc::new(Cell::new(50));
        let attempts_for_tick = Rc::clone(&attempts_remaining);
        let source = glib::timeout_add_local(Duration::from_millis(10), move || {
            if focus_ui.try_focus_pane_input(expected_workspace_id, expected_pane_id) {
                *focus_ui.pending_focus_source.borrow_mut() = None;
                return glib::ControlFlow::Break;
            }

            let remaining = attempts_for_tick.get() - 1;
            attempts_for_tick.set(remaining);
            if remaining <= 0 {
                *focus_ui.pending_focus_source.borrow_mut() = None;
                return glib::ControlFlow::Break;
            }

            glib::ControlFlow::Continue
        });
        *self.pending_focus_source.borrow_mut() = Some(source);
    }

    fn toggle_overview(self: &Rc<Self>) {
        let next = !self.overview_mode.get();
        self.overview_mode.set(next);
        self.refresh(true);

        let sync_ui = Rc::clone(self);
        glib::timeout_add_local_once(Duration::from_millis(30), move || {
            let Some(shell) = sync_ui.shell.borrow().as_ref().cloned() else {
                return;
            };
            let model = sync_ui.app_state.snapshot_model();
            let Some(workspace) = model.active_workspace() else {
                return;
            };

            if sync_ui.overview_mode.get() {
                sync_ui.set_workspace_viewport(&shell, &WorkspaceViewport::default());
            } else {
                sync_ui.set_workspace_viewport(&shell, &workspace.viewport);
                sync_ui.reveal_active_window(&shell, workspace);
                sync_ui.queue_focus_active_pane_input(&model);
            }
        });
    }
}

fn main() -> gtk::glib::ExitCode {
    let cli = Cli::parse();
    if cli.internal_ghostty_probe {
        return run_internal_ghostty_probe();
    }
    let shell_mode_non_unique = cli.clean_shell || cli.raw_shell;
    let run_non_unique = shell_mode_non_unique
        || cli.socket.is_some()
        || cli.session.is_some()
        || std::env::var_os("TASKERS_NON_UNIQUE").is_some();
    let socket_path = cli.socket.unwrap_or_else(default_socket_path);
    let session_path = cli
        .session
        .unwrap_or_else(session_store::default_session_path);
    let config_path = settings_store::default_config_path();
    let crash_reporter = CrashReporter::for_session(&session_path, &config_path);
    crash_reporter.install_panic_hook();
    let recovered_crash_report = match crash_reporter.recover_previous_run() {
        Ok(path) => path,
        Err(error) => {
            eprintln!("failed to recover previous taskers crash report: {error}");
            None
        }
    };
    if let Err(error) = crash_reporter.mark_launch() {
        eprintln!("failed to write taskers run marker: {error}");
    }
    let ghostty_runtime_toast = match ensure_runtime_installed() {
        Ok(Some(runtime)) => Some(format!(
            "Installed Ghostty runtime assets to {}",
            runtime.runtime_dir.display()
        )),
        Ok(None) => None,
        Err(error) => Some(format!("Ghostty runtime bootstrap unavailable: {error}")),
    };
    let probe = DefaultBackend::probe(BackendChoice::Auto);
    if cli.clean_shell {
        unsafe {
            std::env::set_var("TASKERS_SHELL_PROFILE", "clean");
        }
    }
    if cli.raw_shell {
        unsafe {
            std::env::set_var("TASKERS_SHELL_PROFILE", "clean");
            std::env::set_var("TASKERS_DISABLE_SHELL_INTEGRATION", "1");
        }
    }
    let initial_model = match session_store::load_or_bootstrap(&session_path, cli.demo) {
        Ok(model) => model,
        Err(error) => {
            eprintln!(
                "failed to load session from {}: {error}",
                session_path.display()
            );
            if cli.demo {
                AppModel::demo()
            } else {
                AppModel::new("Workspace 1")
            }
        }
    };
    let app_config = match settings_store::load_or_default(&config_path) {
        Ok(config) => config,
        Err(error) => {
            eprintln!(
                "failed to load settings from {}: {error}",
                config_path.display()
            );
            AppConfig::default()
        }
    };
    let (mut shell_launch, shell_integration_toast) =
        match install_shell_integration(app_config.shell.program.as_deref()) {
            Ok(integration) => (integration.launch_spec(), None),
            Err(error) => (
                ShellLaunchSpec::fallback(),
                Some(format!("Shell integration unavailable: {error}")),
            ),
        };
    shell_launch
        .env
        .insert("TASKERS_SOCKET".into(), socket_path.display().to_string());
    let (backend_choice, _backend_note, ghostty_host, backend_toast) =
        initialize_terminal_backend(&probe);
    let runtime_toast = merge_startup_toasts(
        merge_startup_toasts(
            merge_startup_toasts(ghostty_runtime_toast, shell_integration_toast),
            cli.clean_shell
                .then(|| "Using clean shell startup".to_string()),
        ),
        merge_startup_toasts(
            cli.raw_shell.then(|| "Using raw shell startup".to_string()),
            backend_toast,
        ),
    );
    let startup_toast = merge_startup_toasts(
        recovered_crash_report
            .map(|path| format!("Recovered an unclean shutdown report at {}", path.display())),
        runtime_toast,
    );
    let app_state = match AppState::new(
        initial_model,
        session_path,
        backend_choice,
        shell_launch.clone(),
    ) {
        Ok(state) => state,
        Err(error) => {
            let _ = crash_reporter.mark_clean_shutdown();
            eprintln!("failed to initialize app state: {error}");
            return gtk::glib::ExitCode::FAILURE;
        }
    };
    let _server_note = spawn_control_server(app_state.controller(), socket_path);

    let startup = StartupContext {
        app_state,
        backend_choice,
        config_path,
        app_config,
        crash_reporter,
        ghostty_host,
        shell_launch,
        startup_toast,
    };

    let app = adw::Application::builder()
        .application_id("dev.taskers.app")
        .flags(if run_non_unique {
            gtk::gio::ApplicationFlags::NON_UNIQUE
        } else {
            gtk::gio::ApplicationFlags::empty()
        })
        .build();

    let startup = Rc::new(RefCell::new(Some(startup)));
    let hold_guard = Rc::new(RefCell::new(None));
    let startup_for_build = Rc::clone(&startup);
    let hold_guard_for_startup = Rc::clone(&hold_guard);
    app.connect_startup(move |app| {
        install_css();
        *hold_guard_for_startup.borrow_mut() = Some(app.hold());
        if let Some(startup) = startup_for_build.borrow_mut().take() {
            build_ui(app, startup, Rc::clone(&hold_guard_for_startup));
        }
    });
    app.connect_activate(move |app| {
        if let Some(window) = app.active_window() {
            window.present();
        }
    });

    app.run_with_args(&["taskers"])
}

fn build_ui(
    app: &adw::Application,
    startup: StartupContext,
    hold_guard: Rc<RefCell<Option<gtk::gio::ApplicationHoldGuard>>>,
) {
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("taskers")
        .default_width(1440)
        .default_height(960)
        .build();
    window.connect_close_request(move |_| {
        drop(hold_guard.borrow_mut().take());
        glib::Propagation::Proceed
    });

    let header = adw::HeaderBar::new();
    let title = adw::WindowTitle::builder().title("taskers").build();
    header.set_title_widget(Some(&title));

    let overlay = adw::ToastOverlay::new();
    overlay.set_vexpand(true);
    let root = GtkBox::new(Orientation::Vertical, 0);
    root.append(&header);
    root.append(&overlay);
    window.set_content(Some(&root));

    let crash_reporter_for_shutdown = startup.crash_reporter.clone();
    app.connect_shutdown(move |_| {
        if let Err(error) = crash_reporter_for_shutdown.mark_clean_shutdown() {
            eprintln!("failed to clear taskers run marker: {error}");
        }
    });

    let ui = UiHandle::new(
        startup.app_state,
        startup.backend_choice,
        startup.config_path,
        startup.app_config,
        app.clone(),
        window.clone(),
        overlay,
        startup.crash_reporter.clone(),
        startup.ghostty_host,
        startup.shell_launch,
    );
    connect_navigation_shortcuts(&ui);
    ui.refresh(true);
    if let Some(message) = startup.startup_toast {
        ui.toast(&message);
    }

    let poll_ui = Rc::clone(&ui);
    gtk::glib::timeout_add_local(Duration::from_millis(300), move || {
        poll_ui.refresh(false);
        gtk::glib::ControlFlow::Continue
    });

    let tick_ui = Rc::clone(&ui);
    glib::timeout_add_local(Duration::from_millis(16), move || {
        tick_ui.tick_terminal_host();
        glib::ControlFlow::Continue
    });

    ui.present_window();
    let reveal_ui = Rc::clone(&ui);
    glib::timeout_add_local_once(Duration::from_millis(80), move || {
        reveal_ui.refresh(false);
        let model = reveal_ui.app_state.snapshot_model();
        reveal_ui.queue_focus_active_pane_input(&model);
    });
}

fn initialize_terminal_backend(
    probe: &BackendProbe,
) -> (BackendChoice, String, Option<GhosttyHost>, Option<String>) {
    if probe.selected != BackendChoice::Ghostty {
        return (BackendChoice::Mock, probe.notes.clone(), None, None);
    }

    if let Err(error) = probe_ghostty_backend_process() {
        let note = format!(
            "{} Ghostty self-probe failed; using placeholder terminal surfaces.",
            probe.notes
        );
        let toast = format!("Ghostty backend unavailable: {error}");
        return (BackendChoice::Mock, note, None, Some(toast));
    }

    match GhosttyHost::new() {
        Ok(host) => (
            BackendChoice::Ghostty,
            probe.notes.clone(),
            Some(host),
            None,
        ),
        Err(error) => {
            let note = format!("{} Falling back to placeholder terminal surfaces.", error);
            let toast = format!("Ghostty backend unavailable: {error}");
            (BackendChoice::Mock, note, None, Some(toast))
        }
    }
}

fn run_internal_ghostty_probe() -> gtk::glib::ExitCode {
    match GhosttyHost::new() {
        Ok(host) => {
            let _ = host.tick();
            thread::sleep(Duration::from_millis(250));
            gtk::glib::ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("ghostty self-probe failed: {error}");
            gtk::glib::ExitCode::FAILURE
        }
    }
}

fn probe_ghostty_backend_process() -> Result<(), String> {
    let current_exe = std::env::current_exe()
        .map_err(|error| format!("failed to resolve current executable: {error}"))?;
    let mut child = Command::new(current_exe)
        .arg("--internal-ghostty-probe")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("failed to launch Ghostty self-probe: {error}"))?;

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if status.success() {
                    return Ok(());
                }
                return Err(describe_exit_status(status));
            }
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(50)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Ghostty self-probe timed out".into());
            }
            Err(error) => return Err(format!("failed to wait for Ghostty self-probe: {error}")),
        }
    }
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

fn merge_startup_toasts(first: Option<String>, second: Option<String>) -> Option<String> {
    match (first, second) {
        (Some(first), Some(second)) => Some(format!("{first}\n{second}")),
        (Some(first), None) => Some(first),
        (None, Some(second)) => Some(second),
        (None, None) => None,
    }
}

fn normalize_shortcut_modifiers(state: gdk::ModifierType) -> gdk::ModifierType {
    state
        & (gdk::ModifierType::SHIFT_MASK
            | gdk::ModifierType::CONTROL_MASK
            | gdk::ModifierType::ALT_MASK
            | gdk::ModifierType::SUPER_MASK
            | gdk::ModifierType::META_MASK)
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

fn directional_shortcut_key(key: gdk::Key) -> bool {
    matches!(
        key,
        gdk::Key::Left
            | gdk::Key::Right
            | gdk::Key::Up
            | gdk::Key::Down
            | gdk::Key::h
            | gdk::Key::H
            | gdk::Key::j
            | gdk::Key::J
            | gdk::Key::k
            | gdk::Key::K
            | gdk::Key::l
            | gdk::Key::L
    )
}

fn reserved_direction_shortcut(key: gdk::Key, modifiers: gdk::ModifierType) -> bool {
    if !directional_shortcut_key(key) {
        return false;
    }

    let normalized = normalize_shortcut_modifiers(modifiers);
    let base = gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::ALT_MASK;

    normalized == base
        || normalized == (base | gdk::ModifierType::SHIFT_MASK)
        || normalized == (base | gdk::ModifierType::SUPER_MASK)
        || normalized == (base | gdk::ModifierType::SUPER_MASK | gdk::ModifierType::SHIFT_MASK)
        || normalized == (base | gdk::ModifierType::META_MASK)
        || normalized == (base | gdk::ModifierType::META_MASK | gdk::ModifierType::SHIFT_MASK)
}

fn connect_navigation_shortcuts(ui: &Rc<UiHandle>) {
    let controller = gtk::EventControllerKey::new();
    controller.set_propagation_phase(gtk::PropagationPhase::Capture);
    let shortcuts_ui = Rc::clone(ui);
    controller.connect_key_pressed(move |_, key, _, state| {
        if shortcuts_ui.shortcut_matches(ShortcutAction::ToggleOverview, key, state) {
            shortcuts_ui.toggle_overview();
            return glib::Propagation::Stop;
        }

        let model = shortcuts_ui.app_state.snapshot_model();
        let Some(workspace) = model.active_workspace() else {
            return glib::Propagation::Proceed;
        };

        if shortcuts_ui.shortcut_matches(ShortcutAction::NewTerminal, key, state) {
            shortcuts_ui.dispatch(ControlCommand::CreateWorkspaceWindow {
                workspace_id: workspace.id,
                direction: Direction::Right,
            });
            return glib::Propagation::Stop;
        }

        if shortcuts_ui.shortcut_matches(ShortcutAction::CloseTerminal, key, state) {
            shortcuts_ui.dispatch(ControlCommand::ClosePane {
                workspace_id: workspace.id,
                pane_id: workspace.active_pane,
            });
            return glib::Propagation::Stop;
        }

        let normalized = normalize_shortcut_modifiers(state);
        let alt_pressed = normalized.contains(gdk::ModifierType::ALT_MASK);
        let control_pressed = normalized.contains(gdk::ModifierType::CONTROL_MASK);
        if !(alt_pressed && control_pressed) {
            return glib::Propagation::Proceed;
        }

        let shift_pressed = normalized.contains(gdk::ModifierType::SHIFT_MASK);
        let super_pressed = normalized.contains(gdk::ModifierType::SUPER_MASK)
            || normalized.contains(gdk::ModifierType::META_MASK);

        let direction = match key {
            gdk::Key::Left | gdk::Key::h | gdk::Key::H => Some(Direction::Left),
            gdk::Key::Right | gdk::Key::l | gdk::Key::L => Some(Direction::Right),
            gdk::Key::Up | gdk::Key::k | gdk::Key::K => Some(Direction::Up),
            gdk::Key::Down | gdk::Key::j | gdk::Key::J => Some(Direction::Down),
            _ => None,
        };
        let Some(direction) = direction else {
            return glib::Propagation::Proceed;
        };

        if super_pressed && shift_pressed {
            shortcuts_ui.dispatch(ControlCommand::ResizeActivePaneSplit {
                workspace_id: workspace.id,
                direction,
                amount: KEYBOARD_RESIZE_STEP,
            });
        } else if super_pressed {
            shortcuts_ui.dispatch(ControlCommand::ResizeActiveWindow {
                workspace_id: workspace.id,
                direction,
                amount: KEYBOARD_RESIZE_STEP,
            });
        } else if shift_pressed {
            shortcuts_ui.dispatch(ControlCommand::CreateWorkspaceWindow {
                workspace_id: workspace.id,
                direction,
            });
        } else {
            shortcuts_ui.dispatch(ControlCommand::FocusPaneDirection {
                workspace_id: workspace.id,
                direction,
            });
        }

        glib::Propagation::Stop
    });
    ui.window.add_controller(controller);
}

fn build_shell_scaffold(ui: &Rc<UiHandle>) -> ShellWidgets {
    let shell = Paned::builder()
        .orientation(Orientation::Horizontal)
        .wide_handle(false)
        .build();

    // --- Sidebar ---
    let sidebar = GtkBox::new(Orientation::Vertical, 2);
    sidebar.add_css_class("workspace-sidebar");
    sidebar.set_margin_start(8);
    sidebar.set_margin_end(8);
    sidebar.set_margin_top(8);
    sidebar.set_margin_bottom(8);

    let sidebar_header = GtkBox::new(Orientation::Horizontal, 8);
    sidebar_header.set_margin_bottom(6);

    let workspaces_label = Label::new(Some("Workspaces"));
    workspaces_label.add_css_class("sidebar-heading");
    workspaces_label.set_xalign(0.0);
    workspaces_label.set_hexpand(true);
    sidebar_header.append(&workspaces_label);

    let add_workspace = Button::with_label("+");
    add_workspace.add_css_class("workspace-add");
    add_workspace.set_tooltip_text(Some("New workspace"));
    let add_ui = Rc::clone(ui);
    add_workspace.connect_clicked(move |_| {
        let model = add_ui.app_state.snapshot_model();
        let label = format!("Workspace {}", model.workspaces.len() + 1);
        add_ui.dispatch(ControlCommand::CreateWorkspace { label });
    });
    sidebar_header.append(&add_workspace);
    sidebar.append(&sidebar_header);

    let sidebar_list = GtkBox::new(Orientation::Vertical, 2);
    sidebar.append(&sidebar_list);

    let sidebar_scroll = ScrolledWindow::new();
    sidebar_scroll.set_policy(PolicyType::Never, PolicyType::Automatic);
    sidebar_scroll.set_child(Some(&sidebar));
    sidebar_scroll.set_size_request(224, -1);
    shell.set_start_child(Some(&sidebar_scroll));
    shell.set_resize_start_child(false);

    // --- Main content split ---
    let content_split = Paned::builder()
        .orientation(Orientation::Horizontal)
        .wide_handle(false)
        .build();

    // --- Main column ---
    let main_column = GtkBox::new(Orientation::Vertical, 0);

    // --- Workspace header (slim replacement for old toolbar) ---
    let workspace_header = GtkBox::new(Orientation::Horizontal, 8);
    workspace_header.add_css_class("workspace-header");
    workspace_header.set_size_request(-1, 32);
    workspace_header.set_margin_start(10);
    workspace_header.set_margin_end(10);
    workspace_header.set_valign(Align::Center);

    let workspace_name_label = Label::new(Some(""));
    workspace_name_label.add_css_class("workspace-header-label");
    workspace_name_label.set_xalign(0.0);
    workspace_name_label.set_hexpand(true);
    workspace_header.append(&workspace_name_label);

    // New-window popover button
    let new_window_btn = Button::with_label("+");
    new_window_btn.add_css_class("workspace-header-action");
    new_window_btn.set_tooltip_text(Some("New window"));
    let nw_parent = new_window_btn.clone();
    let nw_ui = Rc::clone(ui);
    new_window_btn.connect_clicked(move |_| {
        let popover = gtk::Popover::new();
        popover.set_parent(&nw_parent);

        let content = GtkBox::new(Orientation::Vertical, 2);
        content.set_margin_start(4);
        content.set_margin_end(4);
        content.set_margin_top(4);
        content.set_margin_bottom(4);

        let new_right = Button::with_label("\u{25eb}");
        new_right.add_css_class("flat");
        new_right.add_css_class("context-item");
        new_right.set_tooltip_text(Some("New window right"));
        let nr_ui = Rc::clone(&nw_ui);
        let nr_pop = popover.clone();
        new_right.connect_clicked(move |_| {
            nr_pop.popdown();
            let model = nr_ui.app_state.snapshot_model();
            if let Some(workspace) = model.active_workspace() {
                nr_ui.dispatch(ControlCommand::CreateWorkspaceWindow {
                    workspace_id: workspace.id,
                    direction: Direction::Right,
                });
            }
        });
        content.append(&new_right);

        let new_below = Button::with_label("\u{2193}");
        new_below.add_css_class("flat");
        new_below.add_css_class("context-item");
        new_below.set_tooltip_text(Some("New window below"));
        let nb_ui = Rc::clone(&nw_ui);
        let nb_pop = popover.clone();
        new_below.connect_clicked(move |_| {
            nb_pop.popdown();
            let model = nb_ui.app_state.snapshot_model();
            if let Some(workspace) = model.active_workspace() {
                nb_ui.dispatch(ControlCommand::CreateWorkspaceWindow {
                    workspace_id: workspace.id,
                    direction: Direction::Down,
                });
            }
        });
        content.append(&new_below);

        popover.set_child(Some(&content));
        let pop_cleanup = popover.clone();
        popover.connect_closed(move |_| {
            pop_cleanup.unparent();
        });
        popover.popup();
    });
    workspace_header.append(&new_window_btn);

    // Settings button
    let settings_button = Button::with_label("\u{2699}");
    settings_button.add_css_class("workspace-header-action");
    settings_button.set_tooltip_text(Some("Settings"));
    let settings_ui = Rc::clone(ui);
    settings_button.connect_clicked(move |_| {
        settings_ui.present_settings_dialog();
    });
    workspace_header.append(&settings_button);

    main_column.append(&workspace_header);

    let layout_host = Fixed::new();
    layout_host.set_hexpand(true);
    layout_host.set_vexpand(true);
    layout_host.set_halign(Align::Start);
    layout_host.set_valign(Align::Start);

    let workspace_stage_root = Overlay::new();
    workspace_stage_root.set_halign(Align::Start);
    workspace_stage_root.set_valign(Align::Start);
    workspace_stage_root.set_hexpand(false);
    workspace_stage_root.set_vexpand(false);

    let ghost_layer = Fixed::new();
    ghost_layer.set_halign(Align::Start);
    ghost_layer.set_valign(Align::Start);
    ghost_layer.set_hexpand(false);
    ghost_layer.set_vexpand(false);
    ghost_layer.set_can_target(false);
    workspace_stage_root.add_overlay(&ghost_layer);

    let workspace_stage = WorkspaceStageWidgets {
        root: workspace_stage_root,
        ghost_layer,
    };

    let layout_scroll = ScrolledWindow::new();
    layout_scroll.set_hexpand(true);
    layout_scroll.set_vexpand(true);
    layout_scroll.set_policy(PolicyType::Automatic, PolicyType::Automatic);
    layout_scroll.set_child(Some(&layout_host));

    let horizontal_ui = Rc::clone(ui);
    layout_scroll.hadjustment().connect_value_changed(move |_| {
        horizontal_ui.queue_active_workspace_viewport_persist();
    });
    let vertical_ui = Rc::clone(ui);
    layout_scroll.vadjustment().connect_value_changed(move |_| {
        vertical_ui.queue_active_workspace_viewport_persist();
    });

    main_column.append(&layout_scroll);

    content_split.set_start_child(Some(&main_column));
    content_split.set_resize_start_child(true);
    content_split.set_shrink_start_child(false);

    // --- Attention column ---
    let attention_panel = GtkBox::new(Orientation::Vertical, 6);
    attention_panel.add_css_class("attention-panel");
    attention_panel.set_margin_start(8);
    attention_panel.set_margin_end(8);
    attention_panel.set_margin_top(8);
    attention_panel.set_margin_bottom(8);

    let attention_header = GtkBox::new(Orientation::Horizontal, 8);
    let attention_label = Label::new(Some("Attention"));
    attention_label.add_css_class("sidebar-heading");
    attention_label.set_xalign(0.0);
    attention_label.set_hexpand(true);
    attention_header.append(&attention_label);
    attention_panel.append(&attention_header);

    let activity_empty = Label::new(Some("No unread items."));
    activity_empty.add_css_class("empty-state");
    activity_empty.set_wrap(true);
    activity_empty.set_xalign(0.0);
    attention_panel.append(&activity_empty);

    let activity_list = GtkBox::new(Orientation::Vertical, 0);
    activity_list.set_vexpand(true);
    attention_panel.append(&activity_list);

    let attention_scroll = ScrolledWindow::new();
    attention_scroll.set_policy(PolicyType::Never, PolicyType::Automatic);
    attention_scroll.set_size_request(264, -1);
    attention_scroll.set_child(Some(&attention_panel));
    content_split.set_end_child(Some(&attention_scroll));
    content_split.set_resize_end_child(false);
    content_split.set_shrink_end_child(false);

    shell.set_end_child(Some(&content_split));

    ShellWidgets {
        root: shell,
        sidebar_list,
        workspace_name_label,
        activity_list,
        activity_empty,
        layout_scroll,
        layout_host,
        workspace_stage,
    }
}

fn update_sidebar(ui: &Rc<UiHandle>, shell: &ShellWidgets, model: &AppModel) {
    clear_box(&shell.sidebar_list);

    if let Ok(summaries) = model.workspace_summaries(model.active_window) {
        for summary in summaries {
            let outer = GtkBox::new(Orientation::Horizontal, 4);
            outer.add_css_class("workspace-row");

            let button = Button::new();
            button.add_css_class("flat");
            button.add_css_class("workspace-button");
            button.set_hexpand(true);

            let row = GtkBox::new(Orientation::Vertical, 4);
            row.add_css_class("workspace-item");
            if summary.display_attention != AttentionState::Normal {
                row.add_css_class("workspace-item-has-attention");
                row.add_css_class(&format!(
                    "workspace-item-state-{}",
                    attention_state_slug(summary.display_attention)
                ));
            }
            if summary.unread_count > 0 {
                row.add_css_class("workspace-item-has-unread");
            }
            row.set_margin_start(4);
            row.set_margin_end(0);
            row.set_margin_top(2);
            row.set_margin_bottom(2);

            if model.active_workspace_id() == Some(summary.workspace_id) {
                row.add_css_class("workspace-item-active");
            }

            let heading = GtkBox::new(Orientation::Horizontal, 8);
            heading.set_hexpand(true);
            heading.append(&build_workspace_status_widget(&summary));

            let agent_icon = build_agent_icon(
                model
                    .workspaces
                    .get(&summary.workspace_id)
                    .and_then(workspace_agent_kind),
                12,
            );
            agent_icon.add_css_class("workspace-agent-icon");
            heading.append(agent_icon.widget());

            let label = Label::new(Some(&summary.label));
            label.add_css_class("workspace-label");
            label.set_xalign(0.0);
            label.set_hexpand(true);
            label.set_ellipsize(gtk::pango::EllipsizeMode::End);
            heading.append(&label);
            row.append(&heading);

            if let Some(preview_text) = workspace_preview_text(&summary) {
                let preview = Label::new(Some(&preview_text));
                preview.add_css_class("workspace-preview");
                preview.set_xalign(0.0);
                preview.set_hexpand(true);
                preview.set_ellipsize(gtk::pango::EllipsizeMode::End);
                row.append(&preview);
            }

            if let Some(workspace) = model.workspaces.get(&summary.workspace_id)
                && let Some(meta_text) = workspace_metadata_line(workspace)
            {
                let meta = Label::new(Some(&meta_text));
                meta.add_css_class("workspace-meta");
                meta.set_xalign(0.0);
                meta.set_hexpand(true);
                meta.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
                row.append(&meta);
            }

            button.set_child(Some(&row));

            let switch_ui = Rc::clone(ui);
            let workspace_id = summary.workspace_id;
            button.connect_clicked(move |_| {
                switch_ui.dispatch(ControlCommand::SwitchWorkspace {
                    window_id: None,
                    workspace_id,
                });
            });
            outer.append(&button);

            let close_btn = Button::with_label("\u{00d7}");
            close_btn.add_css_class("workspace-close");
            close_btn.set_tooltip_text(Some("Delete workspace"));
            close_btn.set_valign(Align::Center);
            let close_ui = Rc::clone(ui);
            let close_ws_id = summary.workspace_id;
            close_btn.connect_clicked(move |_| {
                close_ui.dispatch(ControlCommand::CloseWorkspace {
                    workspace_id: close_ws_id,
                });
            });
            outer.append(&close_btn);

            // Double-click to rename workspace
            let dbl_click = gtk::GestureClick::new();
            dbl_click.set_button(1);
            let dbl_ui = Rc::clone(ui);
            let dbl_ws_id = summary.workspace_id;
            let dbl_label = summary.label.clone();
            let dbl_parent = button.clone();
            dbl_click.connect_pressed(move |gesture, n_press, _, _| {
                if n_press != 2 {
                    return;
                }
                gesture.set_state(gtk::EventSequenceState::Claimed);
                begin_inline_rename(&dbl_ui, &dbl_parent, dbl_ws_id, &dbl_label);
            });
            button.add_controller(dbl_click);

            // Right-click context menu
            let menu_ui = Rc::clone(ui);
            let menu_ws_id = summary.workspace_id;
            let menu_label = summary.label.clone();
            let menu_parent = button.clone();
            let right_click = gtk::GestureClick::new();
            right_click.set_button(3);
            right_click.connect_pressed(move |_, _, _, _| {
                let popover = gtk::Popover::new();
                popover.set_parent(&menu_parent);

                let content = GtkBox::new(Orientation::Vertical, 2);
                content.set_margin_start(4);
                content.set_margin_end(4);
                content.set_margin_top(4);
                content.set_margin_bottom(4);

                let rename = Button::with_label("Rename workspace");
                rename.add_css_class("flat");
                rename.add_css_class("context-item");
                let ren_ui = Rc::clone(&menu_ui);
                let ren_ws = menu_ws_id;
                let ren_label = menu_label.clone();
                let ren_parent = menu_parent.clone();
                let ren_pop = popover.clone();
                rename.connect_clicked(move |_| {
                    ren_pop.popdown();
                    begin_inline_rename(&ren_ui, &ren_parent, ren_ws, &ren_label);
                });
                content.append(&rename);

                let sep = Separator::new(Orientation::Horizontal);
                sep.add_css_class("context-separator");
                content.append(&sep);

                let delete = Button::with_label("Delete workspace");
                delete.add_css_class("flat");
                delete.add_css_class("destructive-action");
                let del_ui = Rc::clone(&menu_ui);
                let del_ws = menu_ws_id;
                let pop = popover.clone();
                delete.connect_clicked(move |_| {
                    pop.popdown();
                    del_ui.dispatch(ControlCommand::CloseWorkspace {
                        workspace_id: del_ws,
                    });
                });
                content.append(&delete);

                popover.set_child(Some(&content));
                let pop_cleanup = popover.clone();
                popover.connect_closed(move |_| {
                    pop_cleanup.unparent();
                });
                popover.popup();
            });
            button.add_controller(right_click);

            shell.sidebar_list.append(&outer);
        }
    }
}

fn build_workspace_status_widget(summary: &taskers_domain::WorkspaceSummary) -> Widget {
    let status_text = if summary.unread_count > 0 {
        summary.unread_count.min(9).to_string()
    } else {
        "\u{25cf}".into()
    };
    let badge = Label::new(Some(&status_text));
    badge.add_css_class("workspace-status-badge");
    badge.add_css_class(&format!(
        "workspace-status-badge-state-{}",
        attention_state_slug(summary.display_attention)
    ));
    if summary.unread_count == 0 {
        badge.add_css_class("workspace-status-badge-dot");
    }
    if summary.display_attention == AttentionState::Normal {
        badge.add_css_class("workspace-status-badge-idle");
    }
    badge.set_valign(Align::Start);
    badge.set_tooltip_text(Some(&format_workspace_status(summary)));
    badge.upcast()
}

fn workspace_preview_text(summary: &taskers_domain::WorkspaceSummary) -> Option<String> {
    if let Some(message) = summary.latest_notification.as_deref() {
        let preview = compact_preview(message);
        if !preview.is_empty() {
            return Some(preview);
        }
    }

    if let Some(agent_summary) = workspace_agent_subtitle(summary) {
        return Some(agent_summary);
    }

    (summary.display_attention != AttentionState::Normal)
        .then(|| format_workspace_attention(summary))
}

fn workspace_metadata_line(workspace: &Workspace) -> Option<String> {
    let metadata = workspace_display_metadata(workspace)?;
    let mut parts = Vec::new();

    if let Some(branch) = metadata
        .git_branch
        .as_deref()
        .map(str::trim)
        .filter(|branch| !branch.is_empty())
    {
        parts.push(branch.to_string());
    }

    if let Some(cwd) = metadata
        .cwd
        .as_deref()
        .map(str::trim)
        .filter(|cwd| !cwd.is_empty())
    {
        parts.push(compact_path(cwd));
    }

    if !metadata.ports.is_empty() {
        parts.push(format_ports(&metadata.ports));
    }

    if parts.is_empty() {
        metadata
            .repo_name
            .as_deref()
            .map(str::trim)
            .filter(|repo_name| !repo_name.is_empty())
            .map(str::to_owned)
    } else {
        Some(parts.join("  •  "))
    }
}

fn workspace_display_metadata(workspace: &Workspace) -> Option<&PaneMetadata> {
    workspace
        .panes
        .get(&workspace.active_pane)
        .and_then(PaneRecord::active_metadata)
        .filter(|metadata| metadata_has_display_context(metadata))
        .or_else(|| {
            workspace
                .panes
                .values()
                .filter_map(PaneRecord::active_metadata)
                .find(|metadata| metadata_has_display_context(metadata))
        })
}

fn workspace_agent_subtitle(summary: &taskers_domain::WorkspaceSummary) -> Option<String> {
    if summary.agent_summaries.is_empty() {
        return None;
    }

    let waiting = summary
        .agent_summaries
        .iter()
        .filter(|agent| agent.state == WorkspaceAgentState::Waiting)
        .count();
    let working = summary
        .agent_summaries
        .iter()
        .filter(|agent| agent.state == WorkspaceAgentState::Working)
        .count();
    let inactive = summary
        .agent_summaries
        .iter()
        .filter(|agent| agent.state == WorkspaceAgentState::Inactive)
        .count();

    let mut parts = Vec::new();
    if waiting > 0 {
        parts.push(format!("{waiting} waiting"));
    }
    if working > 0 {
        parts.push(format!("{working} working"));
    }
    if inactive > 0 {
        parts.push(format!("{inactive} inactive"));
    }

    (!parts.is_empty()).then(|| parts.join(", "))
}

fn format_workspace_attention(summary: &taskers_domain::WorkspaceSummary) -> String {
    let count = summary
        .counts_by_attention
        .get(&summary.display_attention)
        .copied()
        .filter(|count| *count > 0)
        .unwrap_or(summary.unread_count.max(1));
    let noun = if count == 1 { "tab" } else { "tabs" };

    match summary.display_attention {
        AttentionState::Normal => "Idle".into(),
        AttentionState::Busy => format!("{count} {noun} busy"),
        AttentionState::Completed => format!("{count} {noun} completed"),
        AttentionState::WaitingInput => format!("{count} {noun} waiting"),
        AttentionState::Error => format!("{count} {noun} errored"),
    }
}

fn format_workspace_status(summary: &taskers_domain::WorkspaceSummary) -> String {
    if summary.unread_count > 0 {
        let noun = if summary.unread_count == 1 {
            "unread item"
        } else {
            "unread items"
        };
        format!("{} {noun}", summary.unread_count)
    } else if let Some(agent_summary) = workspace_agent_subtitle(summary) {
        agent_summary
    } else {
        summary.display_attention.label().to_string()
    }
}

fn update_toolbar(shell: &ShellWidgets, model: &AppModel, overview_mode: bool) {
    if let Some(workspace) = model.active_workspace() {
        let label = if overview_mode {
            format!("{} \u{00b7} Overview", workspace.label)
        } else {
            workspace.label.clone()
        };
        shell.workspace_name_label.set_text(&label);
    } else {
        shell.workspace_name_label.set_text("");
    }
}

fn update_activity_panel(ui: &Rc<UiHandle>, shell: &ShellWidgets, model: &AppModel) {
    clear_box(&shell.activity_list);

    let items = model.activity_items();
    shell.activity_empty.set_visible(items.is_empty());

    for item in items {
        shell
            .activity_list
            .append(&build_activity_row(ui, model, &item));
    }
}

fn build_activity_row(ui: &Rc<UiHandle>, model: &AppModel, item: &ActivityItem) -> Widget {
    let outer = GtkBox::new(Orientation::Horizontal, 6);

    let button = Button::new();
    button.add_css_class("flat");
    button.add_css_class("activity-item-button");
    button.set_focusable(false);
    button.set_hexpand(true);

    let row = GtkBox::new(Orientation::Vertical, 2);
    row.add_css_class("activity-item");
    row.add_css_class(&format!(
        "activity-item-state-{}",
        attention_state_slug(item.state)
    ));
    row.set_margin_start(8);
    row.set_margin_end(6);
    row.set_margin_top(5);
    row.set_margin_bottom(5);

    let heading = GtkBox::new(Orientation::Horizontal, 6);
    let dot = Label::new(Some("\u{25cf}"));
    dot.add_css_class("status-dot");
    dot.add_css_class(&attention_dot_class(item.state));
    heading.append(&dot);

    let agent_icon = build_agent_icon(
        activity_surface(model, item).and_then(surface_agent_kind),
        13,
    );
    agent_icon.add_css_class("activity-agent-icon");
    heading.append(agent_icon.widget());

    let title = model
        .workspaces
        .get(&item.workspace_id)
        .and_then(|workspace| workspace.panes.get(&item.pane_id))
        .and_then(|pane| {
            pane.surfaces
                .get(&item.surface_id)
                .or_else(|| pane.active_surface())
                .map(display_surface_title)
        })
        .unwrap_or_else(|| "Terminal pane".into());
    let title_label = Label::new(Some(&title));
    title_label.add_css_class("pane-title");
    title_label.set_xalign(0.0);
    title_label.set_hexpand(true);
    title_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    heading.append(&title_label);

    let time_label = Label::new(Some(&item.created_at.time().to_string()));
    time_label.add_css_class("activity-time");
    heading.append(&time_label);
    row.append(&heading);

    let meta_label = Label::new(Some(&activity_context_line(model, item)));
    meta_label.add_css_class("activity-meta");
    meta_label.set_xalign(0.0);
    meta_label.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    row.append(&meta_label);

    let message = Label::new(Some(&compact_preview(&item.message)));
    message.add_css_class("activity-preview");
    message.set_xalign(0.0);
    message.set_ellipsize(gtk::pango::EllipsizeMode::End);
    row.append(&message);
    button.set_tooltip_text(Some(&item.message));

    button.set_child(Some(&row));

    let click_ui = Rc::clone(ui);
    let workspace_id = item.workspace_id;
    let workspace_window_id = item.workspace_window_id;
    let pane_id = item.pane_id;
    let surface_id = item.surface_id;
    button.connect_clicked(move |_| {
        focus_activity_target(
            &click_ui,
            workspace_id,
            workspace_window_id,
            pane_id,
            surface_id,
        );
    });

    outer.append(&button);

    let done_button = Button::with_label("Clear");
    done_button.add_css_class("activity-action");
    done_button.set_valign(Align::Center);
    done_button.set_tooltip_text(Some("Mark this item addressed"));
    let done_ui = Rc::clone(ui);
    let done_workspace_id = item.workspace_id;
    let done_pane_id = item.pane_id;
    let done_surface_id = item.surface_id;
    done_button.connect_clicked(move |_| {
        done_ui.dispatch(ControlCommand::MarkSurfaceCompleted {
            workspace_id: done_workspace_id,
            pane_id: done_pane_id,
            surface_id: done_surface_id,
        });
    });
    outer.append(&done_button);

    outer.upcast()
}

fn activity_context_line(model: &AppModel, item: &ActivityItem) -> String {
    let workspace_label = model
        .workspaces
        .get(&item.workspace_id)
        .map(|workspace| workspace.label.clone())
        .unwrap_or_else(|| "Workspace".into());

    let mut parts = vec![workspace_label];
    if let Some(metadata) = activity_metadata(model, item) {
        if let Some(branch) = metadata
            .git_branch
            .as_deref()
            .map(str::trim)
            .filter(|branch| !branch.is_empty())
        {
            parts.push(branch.to_string());
        }

        if let Some(cwd) = metadata
            .cwd
            .as_deref()
            .map(str::trim)
            .filter(|cwd| !cwd.is_empty())
        {
            parts.push(compact_path(cwd));
        }
    }
    parts.push(activity_kind_label(&item.kind).to_string());
    parts.join("  •  ")
}

fn activity_surface<'a>(model: &'a AppModel, item: &ActivityItem) -> Option<&'a SurfaceRecord> {
    model
        .workspaces
        .get(&item.workspace_id)
        .and_then(|workspace| workspace.panes.get(&item.pane_id))
        .and_then(|pane| {
            pane.surfaces
                .get(&item.surface_id)
                .or_else(|| pane.active_surface())
        })
}

fn activity_metadata<'a>(model: &'a AppModel, item: &ActivityItem) -> Option<&'a PaneMetadata> {
    activity_surface(model, item).map(|surface| &surface.metadata)
}

fn focus_activity_target(
    ui: &Rc<UiHandle>,
    workspace_id: taskers_domain::WorkspaceId,
    workspace_window_id: Option<WorkspaceWindowId>,
    pane_id: taskers_domain::PaneId,
    surface_id: SurfaceId,
) {
    let model = ui.app_state.snapshot_model();
    if model.active_workspace_id() != Some(workspace_id) {
        ui.dispatch(ControlCommand::SwitchWorkspace {
            window_id: None,
            workspace_id,
        });
    }

    focus_workspace_surface(ui, workspace_id, workspace_window_id, pane_id, surface_id);
}

fn focus_workspace_surface(
    ui: &Rc<UiHandle>,
    workspace_id: taskers_domain::WorkspaceId,
    workspace_window_id: Option<WorkspaceWindowId>,
    pane_id: taskers_domain::PaneId,
    surface_id: SurfaceId,
) {
    let model = ui.app_state.snapshot_model();
    let Some(workspace) = model.workspaces.get(&workspace_id) else {
        return;
    };

    if let Some(workspace_window_id) =
        workspace_window_id.filter(|window_id| workspace.windows.contains_key(window_id))
    {
        ui.dispatch(ControlCommand::FocusWorkspaceWindow {
            workspace_id,
            workspace_window_id,
        });
    }

    let Some(pane) = workspace.panes.get(&pane_id) else {
        return;
    };

    if pane.surfaces.contains_key(&surface_id) {
        ui.dispatch(ControlCommand::FocusSurface {
            workspace_id,
            pane_id,
            surface_id,
        });
    } else {
        ui.dispatch(ControlCommand::FocusPane {
            workspace_id,
            pane_id,
        });
    }
}

fn begin_inline_rename(
    ui: &Rc<UiHandle>,
    button: &Button,
    workspace_id: taskers_domain::WorkspaceId,
    current_label: &str,
) {
    let entry = Entry::new();
    entry.set_text(current_label);
    entry.add_css_class("workspace-rename-entry");
    button.set_child(Some(&entry));
    entry.grab_focus();
    entry.select_region(0, -1);

    let commit_ui = Rc::clone(ui);
    let committed = Rc::new(Cell::new(false));
    let committed_for_activate = Rc::clone(&committed);
    entry.connect_activate(move |entry| {
        if committed_for_activate.get() {
            return;
        }
        committed_for_activate.set(true);
        let new_label = entry.text().to_string();
        if !new_label.is_empty() {
            commit_ui.dispatch(ControlCommand::RenameWorkspace {
                workspace_id,
                label: new_label,
            });
        } else {
            commit_ui.refresh(true);
        }
    });

    let focus_ui = Rc::clone(ui);
    let committed_for_focus = Rc::clone(&committed);
    entry.connect_notify_local(Some("has-focus"), move |entry, _| {
        if !entry.has_focus() && !committed_for_focus.get() {
            committed_for_focus.set(true);
            let new_label = entry.text().to_string();
            if !new_label.is_empty() {
                focus_ui.dispatch(ControlCommand::RenameWorkspace {
                    workspace_id,
                    label: new_label,
                });
            } else {
                focus_ui.refresh(true);
            }
        }
    });

    let esc_ui = Rc::clone(ui);
    let committed_for_esc = Rc::clone(&committed);
    let key_controller = gtk::EventControllerKey::new();
    key_controller.connect_key_pressed(move |_, key, _, _| {
        if key == gdk::Key::Escape {
            committed_for_esc.set(true);
            esc_ui.refresh(true);
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    entry.add_controller(key_controller);
}

fn layout_render_key(
    workspace: &Workspace,
    render_context: WorkspaceRenderContext,
) -> LayoutRenderKey {
    // active_window is intentionally excluded so that focus switches
    // don't trigger a full canvas rebuild. Active window styling is synced
    // separately in update_layout().
    LayoutRenderKey::WorkspaceWindows {
        windows: workspace
            .windows
            .values()
            .map(|window| WorkspaceWindowRenderKey {
                window_id: window.id,
                frame: display_window_frame(window.frame, render_context),
                layout: window.layout.clone(),
            })
            .collect(),
    }
}

fn compute_layout_render_state(ui: &UiHandle, model: &AppModel) -> LayoutRenderState {
    let shell = ui.shell.borrow().as_ref().cloned();
    let viewport_width = workspace_viewport_width(ui, shell.as_ref());
    let viewport_height = workspace_viewport_height(ui, shell.as_ref());
    let overview_mode = ui.overview_mode.get();

    LayoutRenderState {
        workspace_id: model.active_workspace_id(),
        viewport_width,
        viewport_height,
        overview_mode,
        layout: model.active_workspace().map(|workspace| {
            let render_context = workspace_render_context(
                ui,
                shell.as_ref(),
                workspace,
                overview_mode,
                viewport_width,
                viewport_height,
            );
            layout_render_key(workspace, render_context)
        }),
    }
}

fn update_layout(ui: &Rc<UiHandle>, shell: &ShellWidgets, model: &AppModel) {
    let previous_model = ui.last_rendered.borrow().clone();
    let next_state = compute_layout_render_state(ui.as_ref(), model);
    let previous_layout_state = ui.layout_state.borrow().clone();
    let needs_rebuild = previous_layout_state != next_state;
    let overview_mode = ui.overview_mode.get();

    if needs_rebuild {
        if let Some(workspace) = model.active_workspace() {
            ensure_workspace_stage(shell);
            let new_canvas = build_workspace_canvas_widget(ui, shell, workspace);
            shell.workspace_stage.root.set_child(Some(&new_canvas));

            let next_scene = build_workspace_scene_snapshot(ui.as_ref(), shell, workspace);
            let next_visuals = build_workspace_scene_visuals(workspace);
            let previous_workspace = previous_model
                .as_ref()
                .and_then(AppModel::active_workspace)
                .filter(|candidate| candidate.id == workspace.id);
            let should_animate_transition = ui.settings.borrow().animations_enabled
                && !overview_mode
                && !previous_layout_state.overview_mode
                && previous_workspace
                    .map(|previous| has_terminal_lifecycle_change(previous, workspace))
                    .unwrap_or(false);

            if should_animate_transition {
                let previous_scene = previous_workspace
                    .map(|workspace| build_workspace_scene_snapshot(ui.as_ref(), shell, workspace));
                let previous_visuals = previous_workspace
                    .map(build_workspace_scene_visuals)
                    .unwrap_or_default();
                let mut plan = plan_workspace_transition(
                    previous_scene.as_ref(),
                    &next_scene,
                    TERMINAL_MOTION_SPEC,
                );
                let presented = ui.workspace_transition_state.borrow().presented.clone();
                retarget_transition_plan(&mut plan, &presented);
                if plan.items.is_empty() {
                    reset_workspace_transition(
                        ui.as_ref(),
                        shell,
                        (next_scene.canvas_width, next_scene.canvas_height),
                    );
                } else {
                    start_workspace_transition(
                        ui,
                        shell,
                        plan,
                        (next_scene.canvas_width, next_scene.canvas_height),
                        &previous_visuals,
                        &next_visuals,
                    );
                }
            } else {
                reset_workspace_transition(
                    ui.as_ref(),
                    shell,
                    (next_scene.canvas_width, next_scene.canvas_height),
                );
            }
        } else {
            reset_workspace_transition(ui.as_ref(), shell, (1, 1));
            if shell.workspace_stage.root.parent().is_some() {
                shell.layout_host.remove(&shell.workspace_stage.root);
            }
            shell.layout_host.set_size_request(-1, -1);
            let empty = Label::new(Some("No workspace selected"));
            empty.add_css_class("empty-state");
            empty.set_xalign(0.5);
            empty.set_yalign(0.5);
            empty.set_hexpand(true);
            empty.set_vexpand(true);
            clear_fixed(&shell.layout_host);
            shell.layout_host.put(&empty, 0.0, 0.0);
        }
        *ui.layout_state.borrow_mut() = next_state;
    }

    if let Some(workspace) = model.active_workspace() {
        // Sync active window CSS class without a full rebuild.
        let active_name = format!("ww-{}", workspace.active_window);
        sync_active_window_class(&shell.workspace_stage, &active_name);

        for pane in workspace.panes.values() {
            ui.sync_pane_card(workspace.id, workspace.active_pane, pane);
        }

        let previous_workspace_id = previous_model
            .as_ref()
            .and_then(AppModel::active_workspace_id);
        let previous_active_window = previous_model
            .as_ref()
            .and_then(AppModel::active_workspace)
            .map(|workspace| workspace.active_window);
        let previous_active_pane = previous_model
            .as_ref()
            .and_then(AppModel::active_workspace)
            .map(|workspace| workspace.active_pane);
        let should_restore_viewport = previous_workspace_id != Some(workspace.id);
        let should_focus_input = needs_rebuild
            || previous_workspace_id != Some(workspace.id)
            || previous_active_window != Some(workspace.active_window)
            || previous_active_pane != Some(workspace.active_pane);
        let should_reveal = needs_rebuild
            || previous_workspace_id != Some(workspace.id)
            || previous_active_window != Some(workspace.active_window);
        if should_focus_input {
            ui.queue_focus_active_pane_input(model);
        }
        if overview_mode {
            ui.set_workspace_viewport(shell, &WorkspaceViewport::default());
        }
        if should_restore_viewport || should_reveal {
            let sync_ui = Rc::clone(ui);
            let shell = shell.clone();
            let workspace_id = workspace.id;
            let viewport = workspace.viewport.clone();
            glib::timeout_add_local_once(Duration::from_millis(30), move || {
                let model = sync_ui.app_state.snapshot_model();
                let Some(active_workspace) = model
                    .active_workspace()
                    .filter(|workspace| workspace.id == workspace_id)
                else {
                    return;
                };
                if sync_ui.overview_mode.get() {
                    sync_ui.set_workspace_viewport(&shell, &WorkspaceViewport::default());
                    return;
                }
                if should_restore_viewport {
                    sync_ui.set_workspace_viewport(&shell, &viewport);
                }
                if should_reveal {
                    sync_ui.reveal_active_window(&shell, active_workspace);
                    sync_ui.persist_viewport_now(workspace_id, current_workspace_viewport(&shell));
                }
            });
        }
    } else {
        ui.set_workspace_viewport(shell, &WorkspaceViewport::default());
    }
}

fn ensure_workspace_stage(shell: &ShellWidgets) {
    if shell.workspace_stage.root.parent().is_none() {
        clear_fixed(&shell.layout_host);
        shell.layout_host.put(&shell.workspace_stage.root, 0.0, 0.0);
    }
}

fn set_workspace_stage_size(shell: &ShellWidgets, width: i32, height: i32) {
    let width = width.max(1);
    let height = height.max(1);
    shell.workspace_stage.root.set_size_request(width, height);
    shell
        .workspace_stage
        .ghost_layer
        .set_size_request(width, height);
    shell.layout_host.set_size_request(width, height);
}

fn reset_workspace_transition(ui: &UiHandle, shell: &ShellWidgets, canvas_size: (i32, i32)) {
    clear_fixed(&shell.workspace_stage.ghost_layer);
    let mut state = ui.workspace_transition_state.borrow_mut();
    state.motion = None;
    state.presented.clear();
    state.target_canvas_width = canvas_size.0.max(1);
    state.target_canvas_height = canvas_size.1.max(1);
    drop(state);
    if shell.workspace_stage.root.parent().is_some() {
        set_workspace_stage_size(shell, canvas_size.0, canvas_size.1);
    }
}

fn build_workspace_scene_snapshot(
    ui: &UiHandle,
    shell: &ShellWidgets,
    workspace: &Workspace,
) -> WorkspaceSceneSnapshot {
    let render_context = workspace_render_context(
        ui,
        Some(shell),
        workspace,
        ui.overview_mode.get(),
        workspace_viewport_width(ui, Some(shell)),
        workspace_viewport_height(ui, Some(shell)),
    );
    let metrics = workspace_canvas_metrics(workspace, render_context);
    let windows = workspace
        .windows
        .values()
        .map(|window| {
            let display_frame = display_window_frame(window.frame, render_context);
            WorkspaceWindowSnapshot {
                id: window.id,
                rect: WindowFrame {
                    x: display_frame.x + metrics.offset_x,
                    y: display_frame.y + metrics.offset_y,
                    width: display_frame.width,
                    height: display_frame.height,
                },
            }
        })
        .collect::<Vec<_>>();
    let panes = workspace
        .windows
        .values()
        .flat_map(|window| {
            let display_frame = display_window_frame(window.frame, render_context);
            derive_pane_frames(display_frame, &window.layout)
                .into_iter()
                .map(move |(pane_id, pane_rect)| PaneSceneSnapshot {
                    id: pane_id,
                    window_id: window.id,
                    rect: WindowFrame {
                        x: pane_rect.x + metrics.offset_x,
                        y: pane_rect.y + metrics.offset_y,
                        width: pane_rect.width,
                        height: pane_rect.height,
                    },
                })
        })
        .collect::<Vec<_>>();

    WorkspaceSceneSnapshot {
        canvas_width: metrics.width,
        canvas_height: metrics.height,
        windows,
        panes,
    }
}

fn build_workspace_scene_visuals(workspace: &Workspace) -> WorkspaceSceneVisuals {
    let windows = workspace
        .windows
        .values()
        .map(|window| {
            (
                window.id,
                WindowGhostVisual {
                    active: window.id == workspace.active_window,
                    attention: workspace_window_attention(workspace, window),
                },
            )
        })
        .collect::<HashMap<_, _>>();

    WorkspaceSceneVisuals { windows }
}

fn has_terminal_lifecycle_change(previous: &Workspace, next: &Workspace) -> bool {
    previous.windows.keys().copied().collect::<HashSet<_>>()
        != next.windows.keys().copied().collect::<HashSet<_>>()
}

fn start_workspace_transition(
    ui: &Rc<UiHandle>,
    shell: &ShellWidgets,
    plan: terminal_transitions::TransitionPlan,
    target_canvas_size: (i32, i32),
    previous_visuals: &WorkspaceSceneVisuals,
    next_visuals: &WorkspaceSceneVisuals,
) {
    clear_fixed(&shell.workspace_stage.ghost_layer);
    set_workspace_stage_size(shell, plan.canvas_width, plan.canvas_height);

    let mut items = Vec::new();
    for item in plan.items {
        let widget = build_workspace_transition_widget(&item, previous_visuals, next_visuals);
        let start_rect = presented_transition_rect(item.start_rect);
        let end_rect = presented_transition_rect(item.end_rect);
        let spec = workspace_transition_spec(item.kind);
        shell
            .workspace_stage
            .ghost_layer
            .put(&widget, start_rect.x, start_rect.y);
        apply_workspace_transition_widget_frame(
            &shell.workspace_stage.ghost_layer,
            &widget,
            start_rect,
            spec.ghost_start_opacity,
        );
        items.push(WorkspaceTransitionMotionItem {
            id: item.id,
            widget,
            start_rect,
            end_rect,
            duration_us: spec.timing.duration_us,
            curve: spec.timing.curve,
            start_opacity: spec.ghost_start_opacity,
        });
    }

    let mut state = ui.workspace_transition_state.borrow_mut();
    state.target_canvas_width = target_canvas_size.0.max(1);
    state.target_canvas_height = target_canvas_size.1.max(1);
    state.presented = items
        .iter()
        .map(|item| (item.id, item.start_rect))
        .collect();
    state.motion = Some(WorkspaceTransitionMotionState {
        start_time: glib::monotonic_time(),
        items,
    });
    drop(state);

    start_workspace_transition_tick(ui, shell);
}

fn workspace_transition_spec(
    kind: TransitionItemKind,
) -> terminal_transitions::LifecycleMotionSpec {
    match kind {
        TransitionItemKind::Window => TERMINAL_MOTION_SPEC.window,
    }
}

fn build_workspace_transition_widget(
    item: &terminal_transitions::TransitionItem,
    previous_visuals: &WorkspaceSceneVisuals,
    next_visuals: &WorkspaceSceneVisuals,
) -> Widget {
    match item.id {
        TransitionItemId::Window(window_id) => {
            let visual = match item.phase {
                TransitionPhase::Exit => previous_visuals.windows.get(&window_id).copied(),
                _ => next_visuals
                    .windows
                    .get(&window_id)
                    .copied()
                    .or_else(|| previous_visuals.windows.get(&window_id).copied()),
            }
            .unwrap_or(WindowGhostVisual {
                active: false,
                attention: AttentionState::Normal,
            });
            build_workspace_window_ghost(visual)
        }
    }
}

fn build_workspace_window_ghost(visual: WindowGhostVisual) -> Widget {
    let root = GtkBox::new(Orientation::Vertical, 0);
    root.add_css_class("workspace-window");
    root.add_css_class("workspace-window-ghost");
    if visual.attention != AttentionState::Normal {
        root.add_css_class(&format!(
            "workspace-window-state-{}",
            attention_state_slug(visual.attention)
        ));
    }
    if visual.active {
        root.add_css_class("workspace-window-active");
    }

    let chrome = GtkBox::new(Orientation::Vertical, 0);
    chrome.add_css_class("workspace-window-ghost-chrome");
    let header = GtkBox::new(Orientation::Horizontal, 4);
    header.add_css_class("pane-header");
    header.add_css_class("workspace-window-ghost-header");
    let title = Label::new(Some("Terminal"));
    title.add_css_class("pane-title");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    header.append(&title);
    let dot = Label::new(Some("\u{25cf}"));
    dot.add_css_class("status-dot");
    dot.add_css_class(&attention_dot_class(visual.attention));
    header.append(&dot);
    chrome.append(&header);

    let strip = GtkBox::new(Orientation::Horizontal, 4);
    strip.add_css_class("surface-tabs");
    strip.add_css_class("workspace-window-ghost-strip");
    let tab = GtkBox::new(Orientation::Horizontal, 0);
    tab.add_css_class("surface-tab");
    tab.add_css_class("workspace-window-ghost-tab");
    tab.set_size_request(132, 22);
    strip.append(&tab);
    chrome.append(&strip);

    let body = GtkBox::new(Orientation::Vertical, 0);
    body.add_css_class("workspace-window-ghost-body");
    body.set_hexpand(true);
    body.set_vexpand(true);
    chrome.append(&body);

    root.append(&chrome);
    root.upcast()
}

fn start_workspace_transition_tick(ui: &Rc<UiHandle>, shell: &ShellWidgets) {
    let mut state = ui.workspace_transition_state.borrow_mut();
    if state.tick_running {
        return;
    }
    state.tick_running = true;
    drop(state);

    let ui = Rc::clone(ui);
    let shell = shell.clone();
    let tick_root = shell.workspace_stage.root.clone();
    tick_root.add_tick_callback(move |_, clock| {
        if advance_workspace_transition_tick(&ui, &shell, clock.frame_time()) {
            glib::ControlFlow::Continue
        } else {
            ui.workspace_transition_state.borrow_mut().tick_running = false;
            glib::ControlFlow::Break
        }
    });
}

fn advance_workspace_transition_tick(ui: &UiHandle, shell: &ShellWidgets, now: i64) -> bool {
    let (frames, keep_running, target_canvas_size) = {
        let mut state = ui.workspace_transition_state.borrow_mut();
        let Some(motion) = state.motion.clone() else {
            return false;
        };

        state.presented.clear();
        let mut frames = Vec::new();
        let mut keep_running = false;
        for item in &motion.items {
            let progress =
                ((now - motion.start_time) as f64 / item.duration_us as f64).clamp(0.0, 1.0);
            let eased = item.curve.sample(progress);
            let rect = lerp_presented_transition_rect(item.start_rect, item.end_rect, eased);
            let opacity = item.start_opacity * (1.0 - eased);
            state.presented.insert(item.id, rect);
            frames.push((item.widget.clone(), rect, opacity));
            if progress < 1.0 {
                keep_running = true;
            }
        }
        if !keep_running {
            state.motion = None;
            state.presented.clear();
        }
        (
            frames,
            keep_running,
            (state.target_canvas_width, state.target_canvas_height),
        )
    };

    for (widget, rect, opacity) in &frames {
        apply_workspace_transition_widget_frame(
            &shell.workspace_stage.ghost_layer,
            widget,
            *rect,
            *opacity,
        );
    }

    if !keep_running {
        clear_fixed(&shell.workspace_stage.ghost_layer);
        set_workspace_stage_size(shell, target_canvas_size.0, target_canvas_size.1);
    }

    keep_running
}

fn presented_transition_rect(frame: WindowFrame) -> PresentedTransitionRect {
    PresentedTransitionRect {
        x: f64::from(frame.x),
        y: f64::from(frame.y),
        width: f64::from(frame.width),
        height: f64::from(frame.height),
    }
}

fn lerp_presented_transition_rect(
    start: PresentedTransitionRect,
    end: PresentedTransitionRect,
    t: f64,
) -> PresentedTransitionRect {
    PresentedTransitionRect {
        x: start.x + ((end.x - start.x) * t),
        y: start.y + ((end.y - start.y) * t),
        width: start.width + ((end.width - start.width) * t),
        height: start.height + ((end.height - start.height) * t),
    }
}

fn apply_workspace_transition_widget_frame(
    layer: &Fixed,
    widget: &Widget,
    rect: PresentedTransitionRect,
    opacity: f64,
) {
    widget.set_size_request(
        rect.width.round().max(1.0) as i32,
        rect.height.round().max(1.0) as i32,
    );
    layer.move_(widget, rect.x, rect.y);
    widget.set_opacity(opacity.clamp(0.0, 1.0));
}

/// Walk the layout host tree to toggle `.workspace-window-active` on the
/// correct window widget, identified by its widget name.
fn sync_active_window_class(workspace_stage: &WorkspaceStageWidgets, active_name: &str) {
    let Some(canvas) = workspace_stage.root.child() else {
        return;
    };
    let mut child = canvas.first_child();
    while let Some(widget) = child {
        // Each child of the canvas is an Overlay; the workspace-window box
        // is the Overlay's child.
        if let Some(inner) = widget.first_child() {
            if inner.widget_name().as_str() == active_name {
                inner.add_css_class("workspace-window-active");
            } else {
                inner.remove_css_class("workspace-window-active");
            }
        }
        child = widget.next_sibling();
    }
}

fn build_workspace_canvas_widget(
    ui: &Rc<UiHandle>,
    shell: &ShellWidgets,
    workspace: &Workspace,
) -> gtk::Widget {
    let canvas = Fixed::new();
    canvas.set_halign(Align::Start);
    canvas.set_valign(Align::Start);
    canvas.set_hexpand(false);
    canvas.set_vexpand(false);

    let render_context = workspace_render_context(
        ui.as_ref(),
        Some(shell),
        workspace,
        ui.overview_mode.get(),
        workspace_viewport_width(ui.as_ref(), Some(shell)),
        workspace_viewport_height(ui.as_ref(), Some(shell)),
    );
    let metrics = workspace_canvas_metrics(workspace, render_context);
    canvas.set_size_request(metrics.width, metrics.height);

    for window in workspace.windows.values() {
        let display_frame = display_window_frame(window.frame, render_context);
        let window_widget = build_workspace_window_widget(ui, workspace, window, display_frame);
        let final_x = f64::from(display_frame.x + metrics.offset_x);
        let final_y = f64::from(display_frame.y + metrics.offset_y);
        canvas.put(&window_widget, final_x, final_y);
    }

    canvas.upcast()
}

fn build_workspace_window_widget(
    ui: &Rc<UiHandle>,
    workspace: &Workspace,
    window: &taskers_domain::WorkspaceWindowRecord,
    display_frame: WindowFrame,
) -> gtk::Widget {
    let overlay = Overlay::new();
    overlay.set_size_request(display_frame.width, display_frame.height);
    overlay.set_halign(Align::Fill);
    overlay.set_valign(Align::Fill);

    let root = GtkBox::new(Orientation::Vertical, 0);
    root.add_css_class("workspace-window");
    root.set_widget_name(&format!("ww-{}", window.id));
    let window_attention = workspace_window_attention(workspace, window);
    if window_attention != AttentionState::Normal {
        root.add_css_class(&format!(
            "workspace-window-state-{}",
            attention_state_slug(window_attention)
        ));
    }
    if window.id == workspace.active_window {
        root.add_css_class("workspace-window-active");
    }
    root.set_size_request(display_frame.width, display_frame.height);
    root.set_hexpand(true);
    root.set_vexpand(true);

    // Focus click on the window root (no separate header bar)
    let focus_ui = Rc::clone(ui);
    let workspace_id = workspace.id;
    let window_id = window.id;
    let focus_click = gtk::GestureClick::new();
    focus_click.connect_pressed(move |_, _, _, _| {
        focus_ui.dispatch(ControlCommand::FocusWorkspaceWindow {
            workspace_id,
            workspace_window_id: window_id,
        });
    });
    root.add_controller(focus_click);

    // Right-click context menu on workspace window
    let root_for_ctx = root.clone();
    let wctx_click = gtk::GestureClick::new();
    wctx_click.set_button(3);
    let wctx_ui = Rc::clone(ui);
    let wctx_ws_id = workspace.id;
    let wctx_win_id = window.id;
    let wctx_is_active = window.id == workspace.active_window;
    wctx_click.connect_pressed(move |_, _, _, _| {
        let popover = gtk::Popover::new();
        popover.set_parent(&root_for_ctx);

        let content = GtkBox::new(Orientation::Vertical, 2);
        content.set_margin_start(4);
        content.set_margin_end(4);
        content.set_margin_top(4);
        content.set_margin_bottom(4);

        let new_right = Button::with_label("\u{25eb} New Window Right");
        new_right.add_css_class("flat");
        new_right.add_css_class("context-item");
        let nr_ui = Rc::clone(&wctx_ui);
        let nr_pop = popover.clone();
        new_right.connect_clicked(move |_| {
            nr_pop.popdown();
            nr_ui.dispatch(ControlCommand::CreateWorkspaceWindow {
                workspace_id: wctx_ws_id,
                direction: Direction::Right,
            });
        });
        content.append(&new_right);

        let new_below = Button::with_label("\u{2193} New Window Below");
        new_below.add_css_class("flat");
        new_below.add_css_class("context-item");
        let nb_ui = Rc::clone(&wctx_ui);
        let nb_pop = popover.clone();
        new_below.connect_clicked(move |_| {
            nb_pop.popdown();
            nb_ui.dispatch(ControlCommand::CreateWorkspaceWindow {
                workspace_id: wctx_ws_id,
                direction: Direction::Down,
            });
        });
        content.append(&new_below);

        let sep = Separator::new(Orientation::Horizontal);
        sep.add_css_class("context-separator");
        content.append(&sep);

        let focus_btn = Button::with_label("Focus Window");
        focus_btn.add_css_class("flat");
        focus_btn.add_css_class("context-item");
        focus_btn.set_sensitive(!wctx_is_active);
        let f_ui = Rc::clone(&wctx_ui);
        let f_pop = popover.clone();
        focus_btn.connect_clicked(move |_| {
            f_pop.popdown();
            f_ui.dispatch(ControlCommand::FocusWorkspaceWindow {
                workspace_id: wctx_ws_id,
                workspace_window_id: wctx_win_id,
            });
        });
        content.append(&focus_btn);

        popover.set_child(Some(&content));
        let pop_cleanup = popover.clone();
        popover.connect_closed(move |_| {
            pop_cleanup.unparent();
        });
        popover.popup();
    });
    root.add_controller(wctx_click);

    let body = build_split_layout_widget(
        ui,
        workspace,
        window.id,
        &window.layout,
        display_frame,
        Vec::new(),
    );
    body.set_hexpand(true);
    body.set_vexpand(true);
    root.append(&body);

    overlay.set_child(Some(&root));
    if !ui.overview_mode.get() {
        attach_workspace_window_resize_handles(
            ui,
            &overlay,
            workspace.id,
            window.id,
            display_frame,
        );
    }
    overlay.upcast()
}

fn build_split_layout_widget(
    ui: &Rc<UiHandle>,
    workspace: &Workspace,
    workspace_window_id: WorkspaceWindowId,
    node: &LayoutNode,
    rect: WindowFrame,
    path: Vec<bool>,
) -> gtk::Widget {
    match node {
        LayoutNode::Leaf { pane_id } => {
            let pane = workspace
                .panes
                .get(pane_id)
                .expect("layout pane should exist in workspace");
            ui.sync_pane_card(workspace.id, workspace.active_pane, pane);
            let card = ui.pane_card(workspace.id, pane);
            detach_widget(card.root.upcast_ref());
            card.root.upcast()
        }
        LayoutNode::Split {
            axis,
            ratio,
            first,
            second,
        } => {
            let (first_rect, second_rect) = split_layout_rects(rect, *axis, *ratio);
            let paned = Paned::builder()
                .orientation(match axis {
                    taskers_domain::SplitAxis::Horizontal => Orientation::Horizontal,
                    taskers_domain::SplitAxis::Vertical => Orientation::Vertical,
                })
                .wide_handle(false)
                .build();
            // Seed the Paned with its final divider position up front so split
            // creation does not visibly "settle" on the next main-loop turn.
            paned.set_position(split_position_for_extent(
                match axis {
                    taskers_domain::SplitAxis::Horizontal => rect.width,
                    taskers_domain::SplitAxis::Vertical => rect.height,
                },
                *ratio,
            ));
            let mut first_path = path.clone();
            first_path.push(false);
            paned.set_start_child(Some(&build_split_layout_widget(
                ui,
                workspace,
                workspace_window_id,
                first,
                first_rect,
                first_path,
            )));
            let mut second_path = path.clone();
            second_path.push(true);
            paned.set_end_child(Some(&build_split_layout_widget(
                ui,
                workspace,
                workspace_window_id,
                second,
                second_rect,
                second_path,
            )));
            bind_split_ratio_updates(ui, workspace.id, workspace_window_id, &paned, *axis, path);
            paned.upcast()
        }
    }
}

fn attach_workspace_window_resize_handles(
    ui: &Rc<UiHandle>,
    overlay: &Overlay,
    workspace_id: taskers_domain::WorkspaceId,
    workspace_window_id: WorkspaceWindowId,
    display_frame: WindowFrame,
) {
    overlay.add_overlay(&build_workspace_window_resize_handle(
        ui,
        workspace_id,
        workspace_window_id,
        display_frame,
        ResizeHandleEdge::Right,
    ));
    overlay.add_overlay(&build_workspace_window_resize_handle(
        ui,
        workspace_id,
        workspace_window_id,
        display_frame,
        ResizeHandleEdge::Bottom,
    ));
}

fn build_workspace_window_resize_handle(
    ui: &Rc<UiHandle>,
    workspace_id: taskers_domain::WorkspaceId,
    workspace_window_id: WorkspaceWindowId,
    display_frame: WindowFrame,
    edge: ResizeHandleEdge,
) -> Widget {
    let handle = GtkBox::new(Orientation::Vertical, 0);
    handle.add_css_class("workspace-window-resize-handle");
    match edge {
        ResizeHandleEdge::Right => {
            handle.add_css_class("workspace-window-resize-handle-right");
            handle.set_halign(Align::End);
            handle.set_valign(Align::Fill);
            handle.set_vexpand(true);
            handle.set_size_request(8, -1);
        }
        ResizeHandleEdge::Bottom => {
            handle.add_css_class("workspace-window-resize-handle-bottom");
            handle.set_halign(Align::Fill);
            handle.set_valign(Align::End);
            handle.set_hexpand(true);
            handle.set_size_request(-1, 8);
        }
    }

    let start_frame = Rc::new(Cell::new(display_frame));
    let current_frame = Rc::new(Cell::new(display_frame));
    let handle_widget = handle.clone();
    let drag_ui = Rc::clone(ui);
    let drag_begin_ui = Rc::clone(ui);
    let drag = gtk::GestureDrag::new();
    let start_frame_for_begin = Rc::clone(&start_frame);
    let current_frame_for_begin = Rc::clone(&current_frame);
    drag.connect_drag_begin(move |_, _, _| {
        let raw_frame = drag_begin_ui
            .app_state
            .snapshot_model()
            .workspaces
            .get(&workspace_id)
            .and_then(|workspace| workspace.windows.get(&workspace_window_id))
            .map(|window| window.frame)
            .unwrap_or(display_frame);
        let latest_frame = display_window_frame(
            raw_frame,
            workspace_render_context(
                drag_begin_ui.as_ref(),
                drag_begin_ui.shell.borrow().as_ref(),
                drag_begin_ui
                    .app_state
                    .snapshot_model()
                    .workspaces
                    .get(&workspace_id)
                    .expect("workspace should exist while resizing"),
                drag_begin_ui.overview_mode.get(),
                workspace_viewport_width(
                    drag_begin_ui.as_ref(),
                    drag_begin_ui.shell.borrow().as_ref(),
                ),
                workspace_viewport_height(
                    drag_begin_ui.as_ref(),
                    drag_begin_ui.shell.borrow().as_ref(),
                ),
            ),
        );
        start_frame_for_begin.set(latest_frame);
        current_frame_for_begin.set(latest_frame);
        drag_begin_ui.dispatch(ControlCommand::FocusWorkspaceWindow {
            workspace_id,
            workspace_window_id,
        });
    });
    let current_frame_for_update = Rc::clone(&current_frame);
    drag.connect_drag_update(move |_, dx, dy| {
        let mut next = start_frame.get();
        match edge {
            ResizeHandleEdge::Right => {
                next.width = (next.width + dx.round() as i32).max(720);
            }
            ResizeHandleEdge::Bottom => {
                next.height = (next.height + dy.round() as i32).max(MIN_WORKSPACE_WINDOW_HEIGHT);
            }
        }
        current_frame_for_update.set(next);
        if let Some(overlay) = handle_widget.parent().and_downcast::<Overlay>() {
            overlay.set_size_request(next.width, next.height);
            if let Some(child) = overlay.child() {
                child.set_size_request(next.width, next.height);
            }
        }
    });
    drag.connect_drag_end(move |_, _, _| {
        drag_ui.dispatch(ControlCommand::SetWorkspaceWindowFrame {
            workspace_id,
            workspace_window_id,
            frame: current_frame.get(),
        });
    });
    handle.add_controller(drag);

    handle.upcast()
}

fn bind_split_ratio_updates(
    ui: &Rc<UiHandle>,
    workspace_id: taskers_domain::WorkspaceId,
    workspace_window_id: WorkspaceWindowId,
    paned: &Paned,
    axis: taskers_domain::SplitAxis,
    path: Vec<bool>,
) {
    let path = Arc::new(path);
    let pending_source = Arc::new(Mutex::new(None::<glib::SourceId>));
    let app_state = ui.app_state.clone();
    let pending_for_notify = Arc::clone(&pending_source);
    let path_for_notify = Arc::clone(&path);
    paned.connect_position_notify(move |paned| {
        let previous_source = {
            let mut pending = pending_for_notify
                .lock()
                .expect("split ratio source mutex poisoned");
            pending.take()
        };
        if let Some(source) = previous_source {
            source.remove();
        }

        let extent = paned_extent(paned, axis);
        if extent <= 0 {
            return;
        }
        let ratio = (((paned.position() as f64) / f64::from(extent)) * 1000.0)
            .round()
            .clamp(0.0, 1000.0) as u16;

        let app_state = app_state.clone();
        let path = Arc::clone(&path_for_notify);
        let pending_for_timeout = Arc::clone(&pending_for_notify);
        let source = glib::timeout_add_once(Duration::from_millis(120), move || {
            if let Ok(mut pending) = pending_for_timeout.lock() {
                pending.take();
            }

            if let Err(error) = app_state.dispatch(ControlCommand::SetWindowSplitRatio {
                workspace_id,
                workspace_window_id,
                path: path.as_ref().clone(),
                ratio,
            }) {
                eprintln!("failed to update window split ratio: {error}");
            }
        });
        let mut pending = pending_for_notify
            .lock()
            .expect("split ratio source mutex poisoned");
        if pending.is_none() {
            *pending = Some(source);
        } else {
            drop(pending);
            source.remove();
        }
    });
}

fn split_position_for_extent(extent: i32, ratio: u16) -> i32 {
    ((extent * i32::from(ratio)) / 1000).max(1)
}

fn split_layout_rects(
    rect: WindowFrame,
    axis: taskers_domain::SplitAxis,
    ratio: u16,
) -> (WindowFrame, WindowFrame) {
    match axis {
        taskers_domain::SplitAxis::Horizontal => {
            let first_width = split_position_for_extent(rect.width, ratio);
            (
                WindowFrame {
                    width: first_width,
                    ..rect
                },
                WindowFrame {
                    x: rect.x + first_width,
                    width: rect.width - first_width,
                    ..rect
                },
            )
        }
        taskers_domain::SplitAxis::Vertical => {
            let first_height = split_position_for_extent(rect.height, ratio);
            (
                WindowFrame {
                    height: first_height,
                    ..rect
                },
                WindowFrame {
                    y: rect.y + first_height,
                    height: rect.height - first_height,
                    ..rect
                },
            )
        }
    }
}

fn initialize_terminal_body(
    ui: &Rc<UiHandle>,
    workspace_id: taskers_domain::WorkspaceId,
    pane: &PaneRecord,
    card: &PaneCardWidgets,
) -> Widget {
    if let Some(widget) = ui.terminal_widget(workspace_id, pane) {
        widget.set_focusable(true);
        card.terminal_host.append(&widget);
        return widget;
    }

    if ui.backend_choice == BackendChoice::Ghostty {
        let unavailable = GtkBox::new(Orientation::Vertical, 8);
        unavailable.set_hexpand(true);
        unavailable.set_vexpand(true);
        unavailable.add_css_class("terminal-output");

        let title = Label::new(Some("Ghostty surface unavailable for this pane."));
        title.set_wrap(true);
        title.set_xalign(0.0);
        unavailable.append(&title);

        let detail = Label::new(Some(
            "Restart the app or switch back to the mock backend while the bridge is being debugged.",
        ));
        detail.add_css_class("pane-meta");
        detail.set_wrap(true);
        detail.set_xalign(0.0);
        unavailable.append(&detail);

        card.terminal_host.append(&unavailable);
        return unavailable.upcast();
    }

    let root = GtkBox::new(Orientation::Vertical, 0);
    root.set_hexpand(true);
    root.set_vexpand(true);

    let scroller = ScrolledWindow::new();
    scroller.set_vexpand(true);
    scroller.set_hexpand(true);
    scroller.set_policy(PolicyType::Automatic, PolicyType::Automatic);

    let terminal_output = TextView::new();
    terminal_output.add_css_class("terminal-output");
    terminal_output.set_editable(false);
    terminal_output.set_cursor_visible(false);
    terminal_output.set_monospace(true);
    terminal_output.set_wrap_mode(WrapMode::WordChar);
    terminal_output.set_vexpand(true);
    let buffer = terminal_output.buffer();
    let initial_output = ui
        .app_state
        .runtime()
        .snapshot(
            pane.active_surface()
                .map(|surface| surface.id)
                .unwrap_or_else(SurfaceId::new),
        )
        .map(|snapshot| snapshot.output)
        .unwrap_or_default();
    buffer.set_text(&initial_output);
    scroller.set_child(Some(&terminal_output));
    if let Some(surface_id) = pane.active_surface().map(|surface| surface.id) {
        bind_output_updates(ui, surface_id, terminal_output);
    }
    root.append(&scroller);

    let entry = Entry::new();
    entry.add_css_class("terminal-entry");
    entry.set_placeholder_text(Some("Type shell input and press Enter"));
    let input_ui = Rc::clone(ui);
    let surface_id = pane
        .active_surface()
        .map(|surface| surface.id)
        .unwrap_or_else(SurfaceId::new);
    entry.connect_activate(move |entry| {
        let text = entry.text();
        if text.is_empty() {
            return;
        }

        input_ui.send_input(surface_id, format!("{text}\n"));
        entry.set_text("");
    });
    root.append(&entry);

    card.terminal_host.append(&root);
    entry.upcast()
}

fn refresh_terminal_body(
    ui: &Rc<UiHandle>,
    workspace_id: taskers_domain::WorkspaceId,
    pane: &PaneRecord,
    card: &PaneCardWidgets,
) {
    clear_box(&card.terminal_host);
    let _ = initialize_terminal_body(ui, workspace_id, pane, card);
}

fn create_workspace_window_from_pane(
    ui: &Rc<UiHandle>,
    workspace_id: taskers_domain::WorkspaceId,
    pane_id: taskers_domain::PaneId,
    direction: Direction,
) {
    let should_focus_pane = ui
        .app_state
        .snapshot_model()
        .workspaces
        .get(&workspace_id)
        .is_some_and(|workspace| {
            workspace.active_pane != pane_id
                || workspace
                    .window_for_pane(pane_id)
                    .is_some_and(|window_id| window_id != workspace.active_window)
        });

    if should_focus_pane {
        ui.dispatch(ControlCommand::FocusPane {
            workspace_id,
            pane_id,
        });
    }

    ui.dispatch(ControlCommand::CreateWorkspaceWindow {
        workspace_id,
        direction,
    });
}

fn sync_surface_tabs(
    ui: &Rc<UiHandle>,
    workspace_id: taskers_domain::WorkspaceId,
    pane: &PaneRecord,
    card: &PaneCardWidgets,
) {
    let desired_surface_ids = pane.surface_ids().collect::<HashSet<_>>();
    let model_order = pane.surface_ids().collect::<Vec<_>>();
    let animations_enabled = ui.settings.borrow().animations_enabled;
    let mut tabs = card.surface_tabs.tabs.borrow_mut();
    for surface in pane.surfaces.values() {
        let tab = tabs.entry(surface.id).or_insert_with(|| {
            build_surface_tab(ui, &card.surface_tabs, workspace_id, pane.id, surface.id)
        });
        if tab.root.parent().is_none() {
            card.surface_tabs.root.put(&tab.root, 0.0, 0.0);
        }
        configure_surface_tab(tab, surface, pane.active_surface);
    }

    let stale_surface_ids = tabs
        .keys()
        .copied()
        .filter(|surface_id| !desired_surface_ids.contains(surface_id))
        .collect::<Vec<_>>();
    let mut removed_tabs = Vec::new();
    for surface_id in stale_surface_ids {
        if let Some(tab) = tabs.remove(&surface_id) {
            removed_tabs.push((surface_id, tab));
        }
    }
    drop(tabs);

    {
        let mut state = card.surface_tabs.state.borrow_mut();
        state.model_order = model_order.clone();
        if state
            .drag
            .as_ref()
            .is_some_and(|drag| !desired_surface_ids.contains(&drag.surface_id))
        {
            state.drag = None;
        }
    }

    for (surface_id, tab) in removed_tabs {
        remove_surface_tab(&card.surface_tabs, surface_id, tab, animations_enabled);
    }

    let display_order = {
        let state = card.surface_tabs.state.borrow();
        state
            .drag
            .as_ref()
            .map(|drag| {
                let mut order = drag
                    .preview_order
                    .iter()
                    .copied()
                    .filter(|surface_id| desired_surface_ids.contains(surface_id))
                    .collect::<Vec<_>>();
                for surface_id in &model_order {
                    if !order.contains(surface_id) {
                        order.push(*surface_id);
                    }
                }
                order
            })
            .unwrap_or_else(|| model_order.clone())
    };

    let layout = compute_surface_tab_strip_layout(&card.surface_tabs, &display_order);
    set_surface_tab_layout(&card.surface_tabs, layout, animations_enabled);
    apply_surface_tab_widgets(&card.surface_tabs);
}

fn build_surface_tab_strip(
    ui: &Rc<UiHandle>,
    workspace_id: taskers_domain::WorkspaceId,
    pane_id: taskers_domain::PaneId,
) -> SurfaceTabStripWidgets {
    let root = Fixed::new();
    root.add_css_class("surface-tabs");
    root.set_hexpand(true);

    let add_button = Button::with_label("+");
    add_button.add_css_class("flat");
    add_button.add_css_class("surface-tab-add");
    let add_ui = Rc::clone(ui);
    add_button.connect_clicked(move |_| {
        add_ui.dispatch(ControlCommand::CreateSurface {
            workspace_id,
            pane_id,
            kind: PaneKind::Terminal,
        });
    });
    root.put(&add_button, 0.0, 0.0);

    SurfaceTabStripWidgets {
        root,
        add_button,
        tabs: Rc::new(RefCell::new(HashMap::new())),
        state: Rc::new(RefCell::new(SurfaceTabStripState::default())),
    }
}

fn build_surface_tab(
    ui: &Rc<UiHandle>,
    strip: &SurfaceTabStripWidgets,
    workspace_id: taskers_domain::WorkspaceId,
    pane_id: taskers_domain::PaneId,
    surface_id: SurfaceId,
) -> SurfaceTabWidgets {
    let root = GtkBox::new(Orientation::Horizontal, 4);
    root.add_css_class("surface-tab");

    let dot = Label::new(Some("\u{25cf}"));
    dot.add_css_class("status-dot");
    root.append(&dot);

    let label = Button::new();
    label.add_css_class("flat");
    label.add_css_class("surface-tab-label");
    label.set_hexpand(true);
    let label_content = GtkBox::new(Orientation::Horizontal, 4);
    label_content.set_hexpand(true);
    let agent_icon = build_agent_icon(None, 12);
    agent_icon.add_css_class("surface-tab-agent-icon");
    label_content.append(agent_icon.widget());
    let title = Label::new(None);
    title.add_css_class("surface-tab-title");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    label_content.append(&title);
    label.set_child(Some(&label_content));
    let focus_ui = Rc::clone(ui);
    let focus_state = Rc::clone(&strip.state);
    label.connect_clicked(move |_| {
        let mut strip_state = focus_state.borrow_mut();
        if strip_state.suppress_click_surface == Some(surface_id) {
            strip_state.suppress_click_surface = None;
            return;
        }
        drop(strip_state);
        focus_ui.dispatch(ControlCommand::FocusSurface {
            workspace_id,
            pane_id,
            surface_id,
        });
    });
    let drag_ui = Rc::clone(ui);
    let drag_strip = strip.clone();
    let drag = gtk::GestureDrag::new();
    drag.connect_drag_begin(move |_, _, _| {
        begin_surface_tab_drag(&drag_strip, surface_id);
    });
    let drag_strip = strip.clone();
    let drag_ui_for_update = Rc::clone(&drag_ui);
    drag.connect_drag_update(move |_, dx, _| {
        update_surface_tab_drag(&drag_ui_for_update, &drag_strip, surface_id, dx);
    });
    let drag_strip = strip.clone();
    drag.connect_drag_end(move |_, _, _| {
        end_surface_tab_drag(&drag_ui, &drag_strip, workspace_id, pane_id, surface_id);
    });
    label.add_controller(drag);
    root.append(&label);

    let close = Button::with_label("\u{00d7}");
    close.add_css_class("flat");
    close.add_css_class("surface-tab-close");
    let close_ui = Rc::clone(ui);
    close.connect_clicked(move |_| {
        close_ui.dispatch(ControlCommand::CloseSurface {
            workspace_id,
            pane_id,
            surface_id,
        });
    });
    root.append(&close);

    SurfaceTabWidgets {
        root,
        dot,
        agent_icon,
        title,
    }
}

fn configure_surface_tab(
    tab: &SurfaceTabWidgets,
    surface: &SurfaceRecord,
    active_surface_id: SurfaceId,
) {
    for cls in &[
        "surface-tab-has-attention",
        "surface-tab-active",
        "surface-tab-state-busy",
        "surface-tab-state-completed",
        "surface-tab-state-waiting",
        "surface-tab-state-error",
    ] {
        tab.root.remove_css_class(cls);
    }
    for cls in &[
        "status-dot-normal",
        "status-dot-busy",
        "status-dot-completed",
        "status-dot-waiting",
        "status-dot-error",
    ] {
        tab.dot.remove_css_class(cls);
    }

    if surface.attention != AttentionState::Normal {
        tab.root.add_css_class("surface-tab-has-attention");
        tab.root.add_css_class(&format!(
            "surface-tab-state-{}",
            attention_state_slug(surface.attention)
        ));
    }
    if surface.id == active_surface_id {
        tab.root.add_css_class("surface-tab-active");
    }

    tab.dot
        .add_css_class(&attention_dot_class(surface.attention));
    tab.dot.set_tooltip_text(Some(surface.attention.label()));
    configure_agent_icon(&tab.agent_icon, surface_agent_kind(surface), 12);
    tab.title.set_text(&display_surface_title(surface));
}

fn remove_surface_tab(
    strip: &SurfaceTabStripWidgets,
    surface_id: SurfaceId,
    tab: SurfaceTabWidgets,
    animations_enabled: bool,
) {
    let key = SurfaceTabItemKey::Surface(surface_id);
    let mut state = strip.state.borrow_mut();
    if !animations_enabled {
        state.presented.remove(&key);
        if tab.root.parent().is_some() {
            strip.root.remove(&tab.root);
        }
        return;
    }

    if let Some(start_item) = state.presented.remove(&key) {
        tab.root.add_css_class("surface-tab-exiting");
        state.exiting.push(SurfaceTabExitAnimation {
            root: tab.root,
            start_time: glib::monotonic_time(),
            duration_us: TERMINAL_MOTION_SPEC.tab.structural.duration_us,
            start_item,
            end_item: PresentedSurfaceTabItem {
                x: start_item.x - TERMINAL_MOTION_SPEC.tab.exit_offset_px,
                opacity: 0.0,
                width: (start_item.width - TERMINAL_MOTION_SPEC.tab.size_delta_px)
                    .max(SURFACE_TAB_MIN_WIDTH),
                height: start_item.height,
            },
        });
    } else if tab.root.parent().is_some() {
        strip.root.remove(&tab.root);
    }
}

fn compute_surface_tab_strip_layout(
    strip: &SurfaceTabStripWidgets,
    order: &[SurfaceId],
) -> SurfaceTabStripLayout {
    let tabs = strip.tabs.borrow();
    let mut widths = Vec::new();
    let mut heights = Vec::new();
    for surface_id in order {
        let Some(tab) = tabs.get(surface_id) else {
            continue;
        };
        let (_, natural_width, _, _) = tab.root.measure(Orientation::Horizontal, -1);
        let (_, natural_height, _, _) = tab.root.measure(Orientation::Vertical, -1);
        widths.push(natural_width.clamp(SURFACE_TAB_MIN_WIDTH, SURFACE_TAB_MAX_WIDTH));
        heights.push(natural_height);
    }
    drop(tabs);

    let (_, add_width, _, _) = strip.add_button.measure(Orientation::Horizontal, -1);
    let (_, add_height, _, _) = strip.add_button.measure(Orientation::Vertical, -1);
    let gap_count = if widths.is_empty() {
        0
    } else {
        widths.len() as i32
    };
    shrink_surface_tab_widths(
        &mut widths,
        add_width,
        gap_count,
        strip.root.allocated_width(),
    );

    let mut x = 0.0;
    let mut items = Vec::new();
    let mut max_height = add_height;
    for (index, surface_id) in order.iter().copied().enumerate() {
        let Some(width) = widths.get(index).copied() else {
            continue;
        };
        let height = heights.get(index).copied().unwrap_or(add_height);
        max_height = max_height.max(height);
        items.push(SurfaceTabLayoutItem {
            surface_id,
            x,
            width,
            height,
        });
        x += f64::from(width + SURFACE_TAB_GAP);
    }

    SurfaceTabStripLayout {
        items,
        add_button: SurfaceTabAuxLayoutItem {
            x,
            width: add_width,
            height: add_height,
        },
        height: max_height,
    }
}

fn shrink_surface_tab_widths(
    widths: &mut [i32],
    add_width: i32,
    gap_count: i32,
    available_width: i32,
) {
    if widths.is_empty() || available_width <= 0 {
        return;
    }

    let gap_total = gap_count * SURFACE_TAB_GAP;
    let mut total_width = widths.iter().sum::<i32>() + add_width + gap_total;
    if total_width <= available_width {
        return;
    }

    let min_total = widths.len() as i32 * SURFACE_TAB_MIN_WIDTH + add_width + gap_total;
    if available_width <= min_total {
        widths.fill(SURFACE_TAB_MIN_WIDTH);
        return;
    }

    while total_width > available_width {
        let mut reduced_any = false;
        for width in widths.iter_mut() {
            if total_width <= available_width {
                break;
            }
            if *width > SURFACE_TAB_MIN_WIDTH {
                *width -= 1;
                total_width -= 1;
                reduced_any = true;
            }
        }
        if !reduced_any {
            break;
        }
    }
}

fn set_surface_tab_layout(
    strip: &SurfaceTabStripWidgets,
    layout: SurfaceTabStripLayout,
    animations_enabled: bool,
) {
    let mut state = strip.state.borrow_mut();
    let target_items = surface_tab_target_items(&layout, state.drag.as_ref());
    let duration_us = state
        .next_animation_duration_us
        .take()
        .unwrap_or(TERMINAL_MOTION_SPEC.tab.structural.duration_us);
    state.layout = Some(layout);

    if !animations_enabled {
        for exit in state.exiting.drain(..) {
            if exit.root.parent().is_some() {
                strip.root.remove(&exit.root);
            }
        }
        state.motion = None;
        state.presented = target_items;
        return;
    }

    if state.presented.is_empty() {
        state.motion = None;
        state.presented = target_items;
        return;
    }

    let start_items = target_items
        .iter()
        .map(|(key, target)| {
            let start = state
                .presented
                .get(key)
                .copied()
                .unwrap_or_else(|| surface_tab_enter_item(*key, *target));
            (*key, start)
        })
        .collect::<HashMap<_, _>>();

    if start_items == target_items {
        state.motion = None;
        state.presented = target_items;
        return;
    }

    state.presented = start_items.clone();
    state.motion = Some(SurfaceTabMotionState {
        start_time: glib::monotonic_time(),
        duration_us,
        start_items,
        target_items,
    });
    drop(state);
    start_surface_tab_tick(strip);
}

fn surface_tab_target_items(
    layout: &SurfaceTabStripLayout,
    drag: Option<&SurfaceTabDragState>,
) -> HashMap<SurfaceTabItemKey, PresentedSurfaceTabItem> {
    let mut items = layout
        .items
        .iter()
        .map(|item| {
            (
                SurfaceTabItemKey::Surface(item.surface_id),
                PresentedSurfaceTabItem {
                    x: item.x,
                    opacity: 1.0,
                    width: item.width,
                    height: item.height,
                },
            )
        })
        .collect::<HashMap<_, _>>();
    items.insert(
        SurfaceTabItemKey::AddButton,
        PresentedSurfaceTabItem {
            x: layout.add_button.x,
            opacity: 1.0,
            width: layout.add_button.width,
            height: layout.add_button.height,
        },
    );

    if let Some(drag) = drag.filter(|drag| drag.threshold_crossed) {
        if let Some(item) = items.get_mut(&SurfaceTabItemKey::Surface(drag.surface_id)) {
            item.x = drag.start_x + drag.current_dx;
        }
    }

    items
}

fn surface_tab_enter_item(
    key: SurfaceTabItemKey,
    target: PresentedSurfaceTabItem,
) -> PresentedSurfaceTabItem {
    match key {
        SurfaceTabItemKey::Surface(_) => PresentedSurfaceTabItem {
            x: target.x + TERMINAL_MOTION_SPEC.tab.enter_offset_px,
            opacity: 0.0,
            width: (target.width - TERMINAL_MOTION_SPEC.tab.size_delta_px)
                .max(SURFACE_TAB_MIN_WIDTH),
            height: target.height,
        },
        SurfaceTabItemKey::AddButton => target,
    }
}

fn start_surface_tab_tick(strip: &SurfaceTabStripWidgets) {
    let mut state = strip.state.borrow_mut();
    if state.tick_running {
        return;
    }
    state.tick_running = true;
    drop(state);

    let strip = strip.clone();
    let root = strip.root.clone();
    root.add_tick_callback(move |_, clock| {
        if advance_surface_tab_tick(&strip, clock.frame_time()) {
            glib::ControlFlow::Continue
        } else {
            strip.state.borrow_mut().tick_running = false;
            glib::ControlFlow::Break
        }
    });
}

fn advance_surface_tab_tick(strip: &SurfaceTabStripWidgets, now: i64) -> bool {
    let (presented, exiting, height, completed_exits, keep_running) = {
        let mut state = strip.state.borrow_mut();

        if let Some(motion) = state.motion.clone() {
            let progress =
                ((now - motion.start_time) as f64 / motion.duration_us as f64).clamp(0.0, 1.0);
            let eased = TERMINAL_MOTION_SPEC.tab.structural.curve.sample(progress);
            state.presented = motion
                .target_items
                .iter()
                .map(|(key, target)| {
                    let start = motion.start_items.get(key).copied().unwrap_or(*target);
                    (*key, lerp_surface_tab_item(start, *target, eased))
                })
                .collect();
            if progress >= 1.0 {
                state.presented = motion.target_items;
                state.motion = None;
            }
        }

        if let Some(drag_surface_id) = state
            .drag
            .as_ref()
            .filter(|drag| drag.threshold_crossed)
            .map(|drag| (drag.surface_id, drag.start_x + drag.current_dx))
        {
            if let Some(item) = state
                .presented
                .get_mut(&SurfaceTabItemKey::Surface(drag_surface_id.0))
            {
                item.x = drag_surface_id.1;
                item.opacity = 1.0;
            }
        }

        let mut exiting = Vec::new();
        let mut completed_exits = Vec::new();
        state.exiting.retain(|exit| {
            let progress =
                ((now - exit.start_time) as f64 / exit.duration_us as f64).clamp(0.0, 1.0);
            let eased = TERMINAL_MOTION_SPEC.tab.structural.curve.sample(progress);
            let item = lerp_surface_tab_item(exit.start_item, exit.end_item, eased);
            if progress >= 1.0 {
                completed_exits.push(exit.root.clone());
                false
            } else {
                exiting.push((exit.root.clone(), item));
                true
            }
        });

        let height = state
            .layout
            .as_ref()
            .map(|layout| layout.height)
            .unwrap_or(0);
        let keep_running = state.motion.is_some() || !state.exiting.is_empty();
        (
            state.presented.clone(),
            exiting,
            height,
            completed_exits,
            keep_running,
        )
    };

    apply_surface_tab_widget_frames(strip, &presented, &exiting, height);
    for exit_root in completed_exits {
        if exit_root.parent().is_some() {
            strip.root.remove(&exit_root);
        }
    }

    keep_running
}

fn apply_surface_tab_widgets(strip: &SurfaceTabStripWidgets) {
    let (presented, exiting, height) = {
        let state = strip.state.borrow();
        let exiting = state
            .exiting
            .iter()
            .map(|exit| (exit.root.clone(), exit.start_item))
            .collect::<Vec<_>>();
        let mut presented = state.presented.clone();
        if let Some(drag) = state.drag.as_ref().filter(|drag| drag.threshold_crossed) {
            if let Some(item) = presented.get_mut(&SurfaceTabItemKey::Surface(drag.surface_id)) {
                item.x = drag.start_x + drag.current_dx;
                item.opacity = 1.0;
            }
        }
        (
            presented,
            exiting,
            state
                .layout
                .as_ref()
                .map(|layout| layout.height)
                .unwrap_or(0),
        )
    };
    apply_surface_tab_widget_frames(strip, &presented, &exiting, height);
}

fn apply_surface_tab_widget_frames(
    strip: &SurfaceTabStripWidgets,
    presented: &HashMap<SurfaceTabItemKey, PresentedSurfaceTabItem>,
    exiting: &[(GtkBox, PresentedSurfaceTabItem)],
    height: i32,
) {
    let tabs = strip.tabs.borrow();
    for (surface_id, tab) in tabs.iter() {
        let Some(item) = presented.get(&SurfaceTabItemKey::Surface(*surface_id)) else {
            continue;
        };
        if tab.root.parent().is_none() {
            strip.root.put(&tab.root, item.x, 0.0);
        }
        tab.root
            .set_size_request(item.width, item.height.max(height));
        strip.root.move_(&tab.root, item.x, 0.0);
        tab.root.set_opacity(item.opacity);
    }

    if let Some(item) = presented.get(&SurfaceTabItemKey::AddButton) {
        if strip.add_button.parent().is_none() {
            strip.root.put(&strip.add_button, item.x, 0.0);
        }
        strip
            .add_button
            .set_size_request(item.width, item.height.max(height));
        strip.root.move_(&strip.add_button, item.x, 0.0);
        strip.add_button.set_opacity(item.opacity);
    }

    for (exit_root, item) in exiting {
        if exit_root.parent().is_none() {
            strip.root.put(exit_root, item.x, 0.0);
        }
        exit_root.set_size_request(item.width, item.height.max(height));
        strip.root.move_(exit_root, item.x, 0.0);
        exit_root.set_opacity(item.opacity);
    }

    strip.root.set_size_request(-1, height.max(1));
}

fn begin_surface_tab_drag(strip: &SurfaceTabStripWidgets, surface_id: SurfaceId) {
    let mut state = strip.state.borrow_mut();
    let Some(presented) = state
        .presented
        .get(&SurfaceTabItemKey::Surface(surface_id))
        .copied()
    else {
        return;
    };
    let preview_order = state
        .layout
        .as_ref()
        .map(|layout| layout.items.iter().map(|item| item.surface_id).collect())
        .unwrap_or_else(|| state.model_order.clone());
    state.drag = Some(SurfaceTabDragState {
        surface_id,
        start_x: presented.x,
        current_dx: 0.0,
        preview_order,
        threshold_crossed: false,
    });
    drop(state);

    if let Some(tab) = strip.tabs.borrow().get(&surface_id).cloned() {
        tab.root.add_css_class("surface-tab-dragging");
        raise_surface_tab_widget(strip, &tab.root, presented);
    }
}

fn update_surface_tab_drag(
    ui: &Rc<UiHandle>,
    strip: &SurfaceTabStripWidgets,
    surface_id: SurfaceId,
    dx: f64,
) {
    let mut next_order = None;
    let (should_suppress_click, threshold_crossed, layout, current_center, current_preview) = {
        let mut state = strip.state.borrow_mut();
        let current_width = surface_tab_item_width(&state, surface_id);
        let state_layout = state.layout.clone();
        let Some(drag) = state.drag.as_mut() else {
            return;
        };
        if drag.surface_id != surface_id {
            return;
        }
        drag.current_dx = dx;
        let mut should_suppress_click = false;
        if !drag.threshold_crossed && dx.abs() >= TERMINAL_MOTION_SPEC.tab.drag_threshold_px {
            drag.threshold_crossed = true;
            should_suppress_click = true;
        }
        (
            should_suppress_click,
            drag.threshold_crossed,
            state_layout,
            drag.start_x + drag.current_dx + current_width / 2.0,
            drag.preview_order.clone(),
        )
    };

    if should_suppress_click {
        strip.state.borrow_mut().suppress_click_surface = Some(surface_id);
    }
    if !threshold_crossed {
        apply_surface_tab_widgets(strip);
        return;
    }

    if let Some(layout) = layout {
        let preview =
            surface_tab_preview_order(&layout, &current_preview, surface_id, current_center);
        if preview != current_preview {
            if let Some(drag) = strip.state.borrow_mut().drag.as_mut() {
                drag.preview_order = preview.clone();
            }
            next_order = Some(preview);
        }
    }

    if let Some(order) = next_order {
        let layout = compute_surface_tab_strip_layout(strip, &order);
        set_surface_tab_layout(strip, layout, ui.settings.borrow().animations_enabled);
    }
    apply_surface_tab_widgets(strip);
}

fn end_surface_tab_drag(
    ui: &Rc<UiHandle>,
    strip: &SurfaceTabStripWidgets,
    workspace_id: taskers_domain::WorkspaceId,
    pane_id: taskers_domain::PaneId,
    surface_id: SurfaceId,
) {
    let (threshold_crossed, preview_index, current_index) = {
        let mut state = strip.state.borrow_mut();
        let Some(drag) = state.drag.take() else {
            return;
        };
        state.next_animation_duration_us = drag
            .threshold_crossed
            .then_some(TERMINAL_MOTION_SPEC.tab.drag_snap.duration_us);
        (
            drag.threshold_crossed,
            drag.preview_order.iter().position(|id| *id == surface_id),
            state.model_order.iter().position(|id| *id == surface_id),
        )
    };

    if let Some(tab) = strip.tabs.borrow().get(&surface_id).cloned() {
        tab.root.remove_css_class("surface-tab-dragging");
    }

    if !threshold_crossed {
        apply_surface_tab_widgets(strip);
        return;
    }

    if preview_index != current_index {
        ui.dispatch(ControlCommand::MoveSurface {
            workspace_id,
            pane_id,
            surface_id,
            to_index: preview_index.unwrap_or_default(),
        });
        return;
    }

    ui.refresh(true);
}

fn surface_tab_item_width(state: &SurfaceTabStripState, surface_id: SurfaceId) -> f64 {
    state
        .presented
        .get(&SurfaceTabItemKey::Surface(surface_id))
        .map(|item| f64::from(item.width))
        .or_else(|| {
            state.layout.as_ref().and_then(|layout| {
                layout
                    .items
                    .iter()
                    .find(|item| item.surface_id == surface_id)
                    .map(|item| f64::from(item.width))
            })
        })
        .unwrap_or(f64::from(SURFACE_TAB_MIN_WIDTH))
}

fn surface_tab_preview_order(
    layout: &SurfaceTabStripLayout,
    current_order: &[SurfaceId],
    surface_id: SurfaceId,
    current_center: f64,
) -> Vec<SurfaceId> {
    let mut order = current_order
        .iter()
        .copied()
        .filter(|candidate| *candidate != surface_id)
        .collect::<Vec<_>>();
    let insert_index = order
        .iter()
        .position(|candidate| {
            layout
                .items
                .iter()
                .find(|item| item.surface_id == *candidate)
                .map(|item| current_center < item.x + (f64::from(item.width) / 2.0))
                .unwrap_or(false)
        })
        .unwrap_or(order.len());
    order.insert(insert_index, surface_id);
    order
}

fn raise_surface_tab_widget(
    strip: &SurfaceTabStripWidgets,
    widget: &GtkBox,
    presented: PresentedSurfaceTabItem,
) {
    if widget.parent().is_some() {
        strip.root.remove(widget);
    }
    strip.root.put(widget, presented.x, 0.0);
}

fn lerp_surface_tab_item(
    start: PresentedSurfaceTabItem,
    end: PresentedSurfaceTabItem,
    progress: f64,
) -> PresentedSurfaceTabItem {
    PresentedSurfaceTabItem {
        x: start.x + ((end.x - start.x) * progress),
        opacity: start.opacity + ((end.opacity - start.opacity) * progress),
        width: lerp_i32(start.width, end.width, progress),
        height: lerp_i32(start.height, end.height, progress),
    }
}

fn lerp_i32(start: i32, end: i32, progress: f64) -> i32 {
    (f64::from(start) + (f64::from(end - start) * progress)).round() as i32
}

fn clear_box(container: &GtkBox) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

fn clear_fixed(container: &Fixed) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

fn activity_notification_key(item: &ActivityItem) -> String {
    format!(
        "{}:{}:{}:{}:{}",
        item.workspace_id, item.pane_id, item.surface_id, item.created_at, item.message
    )
}

fn activity_kind_label(kind: &SignalKind) -> &'static str {
    match kind {
        SignalKind::Metadata => "Updated",
        SignalKind::Started => "Started",
        SignalKind::Progress => "Working",
        SignalKind::Completed => "Completed",
        SignalKind::WaitingInput => "Waiting",
        SignalKind::Error => "Error",
        SignalKind::Notification => "Notification",
    }
}

fn widget_contains_window_focus(window: &adw::ApplicationWindow, target: &Widget) -> bool {
    let mut current = gtk::prelude::GtkWindowExt::focus(window);
    while let Some(widget) = current {
        if widget == *target {
            return true;
        }
        current = widget.parent();
    }
    false
}

fn widget_is_descendant_of(widget: &Widget, ancestor: &Widget) -> bool {
    let mut current = Some(widget.clone());
    while let Some(node) = current {
        if node == *ancestor {
            return true;
        }
        current = node.parent();
    }
    false
}

fn display_surface_title(surface: &SurfaceRecord) -> String {
    if let Some(title) = surface
        .metadata
        .title
        .as_deref()
        .map(str::trim)
        .filter(|title| !title.is_empty())
    {
        return title.to_string();
    }

    if let Some(agent) = surface_agent_kind(surface) {
        return humanize_agent_kind(agent);
    }

    match surface.kind {
        PaneKind::Terminal => "Terminal".into(),
        PaneKind::Browser => "Browser".into(),
    }
}

const AGENT_ICON_CLASSES: [&str; 4] = [
    "agent-icon",
    "agent-icon-codex",
    "agent-icon-claude",
    "agent-icon-opencode",
];

const CODEX_ICON_PATH: &str = "M239.184 106.203a64.716 64.716 0 0 0-5.576-53.103C219.452 28.459 191 15.784 163.213 21.74A65.586 65.586 0 0 0 52.096 45.22a64.716 64.716 0 0 0-43.23 31.36c-14.31 24.602-11.061 55.634 8.033 76.74a64.665 64.665 0 0 0 5.525 53.102c14.174 24.65 42.644 37.324 70.446 31.36a64.72 64.72 0 0 0 48.754 21.744c28.481.025 53.714-18.361 62.414-45.481a64.767 64.767 0 0 0 43.229-31.36c14.137-24.558 10.875-55.423-8.083-76.483Zm-97.56 136.338a48.397 48.397 0 0 1-31.105-11.255l1.535-.87 51.67-29.825a8.595 8.595 0 0 0 4.247-7.367v-72.85l21.845 12.636c.218.111.37.32.409.563v60.367c-.056 26.818-21.783 48.545-48.601 48.601Zm-104.466-44.61a48.345 48.345 0 0 1-5.781-32.589l1.534.921 51.722 29.826a8.339 8.339 0 0 0 8.441 0l63.181-36.425v25.221a.87.87 0 0 1-.358.665l-52.335 30.184c-23.257 13.398-52.97 5.431-66.404-17.803ZM23.549 85.38a48.499 48.499 0 0 1 25.58-21.333v61.39a8.288 8.288 0 0 0 4.195 7.316l62.874 36.272-21.845 12.636a.819.819 0 0 1-.767 0L41.353 151.53c-23.211-13.454-31.171-43.144-17.804-66.405v.256Zm179.466 41.695-63.08-36.63L161.73 77.86a.819.819 0 0 1 .768 0l52.233 30.184a48.6 48.6 0 0 1-7.316 87.635v-61.391a8.544 8.544 0 0 0-4.4-7.213Zm21.742-32.69-1.535-.922-51.619-30.081a8.39 8.39 0 0 0-8.492 0L99.98 99.808V74.587a.716.716 0 0 1 .307-.665l52.233-30.133a48.652 48.652 0 0 1 72.236 50.391v.205ZM88.061 139.097l-21.845-12.585a.87.87 0 0 1-.41-.614V65.685a48.652 48.652 0 0 1 79.757-37.346l-1.535.87-51.67 29.825a8.595 8.595 0 0 0-4.246 7.367l-.051 72.697Zm11.868-25.58 28.138-16.217 28.188 16.218v32.434l-28.086 16.218-28.188-16.218-.052-32.434Z";

const CLAUDE_CODE_ICON_PATH: &str = "m50.228 170.321 50.357-28.257.843-2.463-.843-1.361h-2.462l-8.426-.518-28.775-.778-24.952-1.037-24.175-1.296-6.092-1.297L0 125.796l.583-3.759 5.12-3.434 7.324.648 16.202 1.101 24.304 1.685 17.629 1.037 26.118 2.722h4.148l.583-1.685-1.426-1.037-1.101-1.037-25.147-17.045-27.22-18.017-14.258-10.37-7.713-5.25-3.888-4.925-1.685-10.758 7-7.713 9.397.649 2.398.648 9.527 7.323 20.35 15.75L94.817 91.9l3.889 3.24 1.555-1.102.195-.777-1.75-2.917-14.453-26.118-15.425-26.572-6.87-11.018-1.814-6.61c-.648-2.723-1.102-4.991-1.102-7.778l7.972-10.823L71.42 0 82.05 1.426l4.472 3.888 6.61 15.101 10.694 23.786 16.591 32.34 4.861 9.592 2.592 8.879.973 2.722h1.685v-1.556l1.36-18.211 2.528-22.36 2.463-28.776.843-8.1 4.018-9.722 7.971-5.25 6.222 2.981 5.12 7.324-.713 4.73-3.046 19.768-5.962 30.98-3.889 20.739h2.268l2.593-2.593 10.499-13.934 17.628-22.036 7.778-8.749 9.073-9.657 5.833-4.601h11.018l8.1 12.055-3.628 12.443-11.342 14.388-9.398 12.184-13.48 18.147-8.426 14.518.778 1.166 2.01-.194 30.46-6.481 16.462-2.982 19.637-3.37 8.88 4.148.971 4.213-3.5 8.62-20.998 5.184-24.628 4.926-36.682 8.685-.454.324.519.648 16.526 1.555 7.065.389h17.304l32.21 2.398 8.426 5.574 5.055 6.805-.843 5.184-12.962 6.611-17.498-4.148-40.83-9.721-14-3.5h-1.944v1.167l11.666 11.406 21.387 19.314 26.767 24.887 1.36 6.157-3.434 4.86-3.63-.518-23.526-17.693-9.073-7.972-20.545-17.304h-1.36v1.814l4.73 6.935 25.017 37.59 1.296 11.536-1.814 3.76-6.481 2.268-7.13-1.297-14.647-20.544-15.1-23.138-12.185-20.739-1.49.843-7.194 77.448-3.37 3.953-7.778 2.981-6.48-4.925-3.436-7.972 3.435-15.749 4.148-20.544 3.37-16.333 3.046-20.285 1.815-6.74-.13-.454-1.49.194-15.295 20.999-23.267 31.433-18.406 19.702-4.407 1.75-7.648-3.954.713-7.064 4.277-6.286 25.47-32.405 15.36-20.092 9.917-11.6-.065-1.686h-.583L44.07 198.125l-12.055 1.555-5.185-4.86.648-7.972 2.463-2.593 20.35-13.999-.064.065Z";

const OPENCODE_ICON_FRAME_PATH: &str = "M24 8H8V32H24V8ZM32 40H0V0H32V40Z";
const OPENCODE_ICON_CORE_PATH: &str = "M24 32H8V16H24V32Z";

const CLAUDE_CODE_ORANGE: AgentIconColor = AgentIconColor {
    red: 0.850_980_392_156_862_7,
    green: 0.466_666_666_666_666_7,
    blue: 0.341_176_470_588_235_3,
    alpha: 1.0,
};

const OPENCODE_CORE_GREY: AgentIconColor = AgentIconColor {
    red: 0.737_254_901_960_784_4,
    green: 0.733_333_333_333_333_3,
    blue: 0.733_333_333_333_333_3,
    alpha: 1.0,
};

const CODEX_ICON_SPEC: AgentIconSpec = AgentIconSpec {
    view_box_width: 256.0,
    view_box_height: 260.0,
    paths: &[AgentIconPathSpec {
        data: CODEX_ICON_PATH,
        fill: AgentIconFill::CurrentColor,
    }],
};

const CLAUDE_CODE_ICON_SPEC: AgentIconSpec = AgentIconSpec {
    view_box_width: 256.0,
    view_box_height: 257.0,
    paths: &[AgentIconPathSpec {
        data: CLAUDE_CODE_ICON_PATH,
        fill: AgentIconFill::Fixed(CLAUDE_CODE_ORANGE),
    }],
};

const OPENCODE_ICON_SPEC: AgentIconSpec = AgentIconSpec {
    view_box_width: 32.0,
    view_box_height: 40.0,
    paths: &[
        AgentIconPathSpec {
            data: OPENCODE_ICON_FRAME_PATH,
            fill: AgentIconFill::CurrentColor,
        },
        AgentIconPathSpec {
            data: OPENCODE_ICON_CORE_PATH,
            fill: AgentIconFill::Fixed(OPENCODE_CORE_GREY),
        },
    ],
};

impl AgentIconWidget {
    fn new(agent_kind: Option<&str>, size: i32) -> Self {
        let root = DrawingArea::new();
        root.set_halign(Align::Center);
        root.set_valign(Align::Center);

        let state = Rc::new(RefCell::new(AgentIconState::default()));
        let draw_state = Rc::clone(&state);
        root.set_draw_func(move |area, cr, width, height| {
            let Some(agent_kind) = draw_state.borrow().kind else {
                return;
            };
            render_agent_icon(area, cr, width, height, agent_kind);
        });

        let icon = Self { root, state };
        configure_agent_icon(&icon, agent_kind, size);
        icon
    }

    fn widget(&self) -> &DrawingArea {
        &self.root
    }

    fn add_css_class(&self, class_name: &str) {
        self.root.add_css_class(class_name);
    }
}

fn build_agent_icon(agent_kind: Option<&str>, size: i32) -> AgentIconWidget {
    AgentIconWidget::new(agent_kind, size)
}

fn configure_agent_icon(icon: &AgentIconWidget, agent_kind: Option<&str>, size: i32) {
    icon.root.set_content_width(size);
    icon.root.set_content_height(size);
    icon.root.set_size_request(size, size);
    for class_name in AGENT_ICON_CLASSES {
        icon.root.remove_css_class(class_name);
    }

    let Some(agent_kind) =
        agent_kind.and_then(|agent_kind| normalized_agent_kind(Some(agent_kind)))
    else {
        icon.state.borrow_mut().kind = None;
        icon.root.set_tooltip_text(None);
        icon.root.set_visible(false);
        icon.root.queue_draw();
        return;
    };

    if agent_icon_spec(agent_kind).is_none() {
        icon.state.borrow_mut().kind = None;
        icon.root.set_tooltip_text(None);
        icon.root.set_visible(false);
        icon.root.queue_draw();
        return;
    }

    icon.state.borrow_mut().kind = Some(agent_kind);
    icon.root.add_css_class("agent-icon");
    icon.root.add_css_class(agent_icon_class(agent_kind));
    icon.root
        .set_tooltip_text(Some(&humanize_agent_kind(agent_kind)));
    icon.root.set_visible(true);
    icon.root.queue_draw();
}

fn agent_icon_class(agent_kind: &str) -> &'static str {
    match agent_kind {
        "codex" => "agent-icon-codex",
        "claude" => "agent-icon-claude",
        "opencode" => "agent-icon-opencode",
        _ => "agent-icon",
    }
}

fn agent_icon_spec(agent_kind: &str) -> Option<&'static AgentIconSpec> {
    match agent_kind {
        "codex" => Some(&CODEX_ICON_SPEC),
        "claude" => Some(&CLAUDE_CODE_ICON_SPEC),
        "opencode" => Some(&OPENCODE_ICON_SPEC),
        _ => None,
    }
}

fn render_agent_icon(
    area: &DrawingArea,
    cr: &gtk::cairo::Context,
    width: i32,
    height: i32,
    agent_kind: &'static str,
) {
    let Some(spec) = agent_icon_spec(agent_kind) else {
        return;
    };
    if width <= 0 || height <= 0 {
        return;
    }

    let scale = f64::min(
        f64::from(width) / spec.view_box_width,
        f64::from(height) / spec.view_box_height,
    );
    if !scale.is_finite() || scale <= 0.0 {
        return;
    }

    let offset_x = (f64::from(width) - (spec.view_box_width * scale)) / 2.0;
    let offset_y = (f64::from(height) - (spec.view_box_height * scale)) / 2.0;

    let _ = cr.save();
    cr.set_antialias(gtk::cairo::Antialias::Best);
    cr.translate(offset_x, offset_y);
    cr.scale(scale, scale);

    for path in spec.paths {
        let Some(commands) = agent_icon_commands(path.data) else {
            continue;
        };
        cr.new_path();
        append_agent_icon_path(cr, commands.as_ref());
        apply_agent_icon_fill(area, cr, path.fill);
        let _ = cr.fill();
    }

    let _ = cr.restore();
}

fn agent_icon_commands(path_data: &'static str) -> Option<Rc<Vec<SimplePathSegment>>> {
    AGENT_ICON_PATHS.with(|cache| {
        if let Some(commands) = cache.borrow().get(path_data).cloned() {
            return Some(commands);
        }

        let commands = Rc::new(
            SimplifyingPathParser::from(path_data)
                .map(|segment| segment.ok())
                .collect::<Option<Vec<_>>>()?,
        );
        cache.borrow_mut().insert(path_data, Rc::clone(&commands));
        Some(commands)
    })
}

fn append_agent_icon_path(cr: &gtk::cairo::Context, commands: &[SimplePathSegment]) {
    let mut current = (0.0, 0.0);
    let mut subpath_start = (0.0, 0.0);

    for command in commands {
        match *command {
            SimplePathSegment::MoveTo { x, y } => {
                cr.move_to(x, y);
                current = (x, y);
                subpath_start = (x, y);
            }
            SimplePathSegment::LineTo { x, y } => {
                cr.line_to(x, y);
                current = (x, y);
            }
            SimplePathSegment::CurveTo {
                x1,
                y1,
                x2,
                y2,
                x,
                y,
            } => {
                cr.curve_to(x1, y1, x2, y2, x, y);
                current = (x, y);
            }
            SimplePathSegment::Quadratic { x1, y1, x, y } => {
                let cubic_1_x = current.0 + ((2.0 / 3.0) * (x1 - current.0));
                let cubic_1_y = current.1 + ((2.0 / 3.0) * (y1 - current.1));
                let cubic_2_x = x + ((2.0 / 3.0) * (x1 - x));
                let cubic_2_y = y + ((2.0 / 3.0) * (y1 - y));
                cr.curve_to(cubic_1_x, cubic_1_y, cubic_2_x, cubic_2_y, x, y);
                current = (x, y);
            }
            SimplePathSegment::ClosePath => {
                cr.close_path();
                current = subpath_start;
            }
        }
    }
}

fn apply_agent_icon_fill(area: &DrawingArea, cr: &gtk::cairo::Context, fill: AgentIconFill) {
    let color = match fill {
        AgentIconFill::CurrentColor => {
            let color = area.style_context().color();
            AgentIconColor {
                red: f64::from(color.red()),
                green: f64::from(color.green()),
                blue: f64::from(color.blue()),
                alpha: f64::from(color.alpha()),
            }
        }
        AgentIconFill::Fixed(color) => color,
    };

    cr.set_source_rgba(color.red, color.green, color.blue, color.alpha);
}

fn normalized_agent_kind(agent_kind: Option<&str>) -> Option<&'static str> {
    let agent_kind = agent_kind?.trim();
    if agent_kind.is_empty() || agent_kind.eq_ignore_ascii_case("shell") {
        return None;
    }

    if agent_kind.eq_ignore_ascii_case("codex") || agent_kind.eq_ignore_ascii_case("openai") {
        Some("codex")
    } else if agent_kind.eq_ignore_ascii_case("claude")
        || agent_kind.eq_ignore_ascii_case("claude code")
        || agent_kind.eq_ignore_ascii_case("claude-code")
        || agent_kind.eq_ignore_ascii_case("anthropic")
    {
        Some("claude")
    } else if agent_kind.eq_ignore_ascii_case("opencode") {
        Some("opencode")
    } else if agent_kind.eq_ignore_ascii_case("aider") {
        Some("aider")
    } else {
        None
    }
}

fn infer_agent_kind(value: &str) -> Option<&'static str> {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized.contains("codex") {
        Some("codex")
    } else if normalized.contains("claude") {
        Some("claude")
    } else if normalized.contains("opencode") {
        Some("opencode")
    } else if normalized.contains("aider") {
        Some("aider")
    } else {
        None
    }
}

fn surface_agent_kind(surface: &SurfaceRecord) -> Option<&'static str> {
    normalized_agent_kind(surface.metadata.agent_kind.as_deref())
        .or_else(|| surface.metadata.title.as_deref().and_then(infer_agent_kind))
        .or_else(|| {
            surface
                .command
                .as_ref()
                .and_then(|command| command.first())
                .and_then(|command| infer_agent_kind(command))
        })
}

fn workspace_agent_kind(workspace: &Workspace) -> Option<&'static str> {
    workspace
        .panes
        .get(&workspace.active_pane)
        .and_then(PaneRecord::active_surface)
        .and_then(surface_agent_kind)
        .or_else(|| {
            workspace
                .panes
                .values()
                .filter_map(PaneRecord::active_surface)
                .find_map(surface_agent_kind)
        })
}

fn humanize_agent_kind(agent: &str) -> String {
    match agent {
        "codex" => "Codex".into(),
        "claude" => "Claude Code".into(),
        "opencode" => "OpenCode".into(),
        "aider" => "Aider".into(),
        other => {
            let mut chars = other.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect(),
                None => "Terminal".into(),
            }
        }
    }
}

fn configure_pane_card_layout(card: &PaneCardWidgets) {
    card.root.remove_css_class("pane-card-scrollable");
    card.root.set_halign(Align::Fill);
    card.root.set_valign(Align::Fill);
    card.root.set_hexpand(true);
    card.root.set_vexpand(true);
    card.root.set_size_request(-1, -1);
}

fn current_workspace_viewport(shell: &ShellWidgets) -> WorkspaceViewport {
    WorkspaceViewport {
        x: shell.layout_scroll.hadjustment().value().round() as i32,
        y: shell.layout_scroll.vadjustment().value().round() as i32,
    }
}

fn workspace_viewport_width(ui: &UiHandle, shell: Option<&ShellWidgets>) -> i32 {
    let Some(shell) = shell else {
        return DEFAULT_WORKSPACE_WINDOW_WIDTH;
    };

    let page_width = shell.layout_scroll.hadjustment().page_size().round() as i32;
    if page_width > WORKSPACE_CANVAS_PADDING * 2 {
        return (page_width - (WORKSPACE_CANVAS_PADDING * 2)).max(1);
    }

    let allocated_width = shell.layout_scroll.allocated_width();
    if allocated_width > WORKSPACE_CANVAS_PADDING * 2 {
        return (allocated_width - (WORKSPACE_CANVAS_PADDING * 2)).max(1);
    }

    let window_width = ui.window.allocated_width();
    if window_width > 320 {
        return (window_width - 260).max(1);
    }

    DEFAULT_WORKSPACE_WINDOW_WIDTH
}

fn workspace_viewport_height(ui: &UiHandle, shell: Option<&ShellWidgets>) -> i32 {
    let Some(shell) = shell else {
        return DEFAULT_WORKSPACE_WINDOW_HEIGHT;
    };

    let page_height = shell.layout_scroll.vadjustment().page_size().round() as i32;
    if page_height > WORKSPACE_CANVAS_PADDING * 2 {
        return (page_height - (WORKSPACE_CANVAS_PADDING * 2)).max(MIN_WORKSPACE_WINDOW_HEIGHT);
    }

    let allocated_height = shell.layout_scroll.allocated_height();
    if allocated_height > WORKSPACE_CANVAS_PADDING * 2 {
        return (allocated_height - (WORKSPACE_CANVAS_PADDING * 2))
            .max(MIN_WORKSPACE_WINDOW_HEIGHT);
    }

    let window_height = ui.window.allocated_height();
    if window_height > 180 {
        return (window_height - 140).max(MIN_WORKSPACE_WINDOW_HEIGHT);
    }

    DEFAULT_WORKSPACE_WINDOW_HEIGHT
}

fn expanded_window_frame(frame: WindowFrame, viewport_height: i32) -> WindowFrame {
    if frame.height != DEFAULT_WORKSPACE_WINDOW_HEIGHT {
        return frame;
    }

    let mut display = frame;
    display.height = viewport_height.max(MIN_WORKSPACE_WINDOW_HEIGHT);

    let default_stride = DEFAULT_WORKSPACE_WINDOW_HEIGHT + DEFAULT_WORKSPACE_WINDOW_GAP;
    if default_stride > 0 && frame.y % default_stride == 0 {
        let display_stride = display.height + DEFAULT_WORKSPACE_WINDOW_GAP;
        display.y = (frame.y / default_stride) * display_stride;
    }

    display
}

fn scale_window_frame(frame: WindowFrame, scale: f64) -> WindowFrame {
    if (scale - 1.0).abs() < f64::EPSILON {
        return frame;
    }

    WindowFrame {
        x: (f64::from(frame.x) * scale).round() as i32,
        y: (f64::from(frame.y) * scale).round() as i32,
        width: (f64::from(frame.width) * scale).round().max(1.0) as i32,
        height: (f64::from(frame.height) * scale).round().max(1.0) as i32,
    }
}

fn display_window_frame(frame: WindowFrame, render_context: WorkspaceRenderContext) -> WindowFrame {
    let expanded = expanded_window_frame(frame, render_context.viewport_height);
    if !render_context.overview_mode {
        return expanded;
    }

    scale_window_frame(expanded, render_context.overview_scale)
}

fn workspace_base_canvas_metrics(workspace: &Workspace, viewport_height: i32) -> CanvasMetrics {
    let display_frames: Vec<_> = workspace
        .windows
        .values()
        .map(|window| expanded_window_frame(window.frame, viewport_height))
        .collect();

    let min_x = display_frames
        .iter()
        .map(|frame| frame.x)
        .min()
        .unwrap_or(0);
    let min_y = display_frames
        .iter()
        .map(|frame| frame.y)
        .min()
        .unwrap_or(0);
    let offset_x = WORKSPACE_CANVAS_PADDING - min_x;
    let offset_y = WORKSPACE_CANVAS_PADDING - min_y;
    let width = display_frames
        .iter()
        .map(|frame| frame.right() + offset_x + WORKSPACE_CANVAS_PADDING)
        .max()
        .unwrap_or(WORKSPACE_CANVAS_PADDING * 2);
    let height = display_frames
        .iter()
        .map(|frame| frame.bottom() + offset_y + WORKSPACE_CANVAS_PADDING)
        .max()
        .unwrap_or(WORKSPACE_CANVAS_PADDING * 2);

    CanvasMetrics {
        offset_x,
        offset_y,
        width,
        height,
    }
}

fn workspace_render_context(
    _ui: &UiHandle,
    _shell: Option<&ShellWidgets>,
    workspace: &Workspace,
    overview_mode: bool,
    viewport_width: i32,
    viewport_height: i32,
) -> WorkspaceRenderContext {
    if !overview_mode {
        return WorkspaceRenderContext {
            viewport_height,
            overview_mode: false,
            overview_scale: 1.0,
        };
    }

    let base_metrics = workspace_base_canvas_metrics(workspace, viewport_height);
    let content_width = (base_metrics.width - (WORKSPACE_CANVAS_PADDING * 2)).max(1);
    let content_height = (base_metrics.height - (WORKSPACE_CANVAS_PADDING * 2)).max(1);
    let available_width = (viewport_width - (WORKSPACE_CANVAS_PADDING * 2)).max(1) as f64;
    let available_height = (viewport_height - (WORKSPACE_CANVAS_PADDING * 2)).max(1) as f64;
    let overview_scale = (available_width / f64::from(content_width))
        .min(available_height / f64::from(content_height))
        .clamp(0.05, 1.0);

    WorkspaceRenderContext {
        viewport_height,
        overview_mode: true,
        overview_scale,
    }
}

fn workspace_canvas_metrics(
    workspace: &Workspace,
    render_context: WorkspaceRenderContext,
) -> CanvasMetrics {
    let display_frames: Vec<_> = workspace
        .windows
        .values()
        .map(|window| display_window_frame(window.frame, render_context))
        .collect();

    let min_x = display_frames
        .iter()
        .map(|frame| frame.x)
        .min()
        .unwrap_or(0);
    let min_y = display_frames
        .iter()
        .map(|frame| frame.y)
        .min()
        .unwrap_or(0);
    let offset_x = WORKSPACE_CANVAS_PADDING - min_x;
    let offset_y = WORKSPACE_CANVAS_PADDING - min_y;
    let width = display_frames
        .iter()
        .map(|frame| frame.right() + offset_x + WORKSPACE_CANVAS_PADDING)
        .max()
        .unwrap_or(WORKSPACE_CANVAS_PADDING * 2);
    let height = display_frames
        .iter()
        .map(|frame| frame.bottom() + offset_y + WORKSPACE_CANVAS_PADDING)
        .max()
        .unwrap_or(WORKSPACE_CANVAS_PADDING * 2);

    CanvasMetrics {
        offset_x,
        offset_y,
        width,
        height,
    }
}

fn workspace_window_attention(
    workspace: &Workspace,
    window: &taskers_domain::WorkspaceWindowRecord,
) -> AttentionState {
    window
        .layout
        .leaves()
        .into_iter()
        .filter_map(|pane_id| workspace.panes.get(&pane_id))
        .map(PaneRecord::active_attention)
        .max_by_key(|attention| attention.rank())
        .unwrap_or(AttentionState::Normal)
}

fn paned_extent(paned: &Paned, axis: taskers_domain::SplitAxis) -> i32 {
    match axis {
        taskers_domain::SplitAxis::Horizontal => paned.allocated_width(),
        taskers_domain::SplitAxis::Vertical => paned.allocated_height(),
    }
}

#[derive(Clone, Copy)]
enum ResizeHandleEdge {
    Right,
    Bottom,
}

fn count_widget_children(widget: &Widget) -> usize {
    let mut count = 0;
    let mut child = widget.first_child();

    while let Some(current) = child {
        count += 1;
        child = current.next_sibling();
    }

    count
}

fn attention_dot_class(state: AttentionState) -> String {
    match state {
        AttentionState::Normal => "status-dot-normal".into(),
        AttentionState::Busy => "status-dot-busy".into(),
        AttentionState::Completed => "status-dot-completed".into(),
        AttentionState::WaitingInput => "status-dot-waiting".into(),
        AttentionState::Error => "status-dot-error".into(),
    }
}

fn attention_state_slug(state: AttentionState) -> &'static str {
    match state {
        AttentionState::Normal => "normal",
        AttentionState::Busy => "busy",
        AttentionState::Completed => "completed",
        AttentionState::WaitingInput => "waiting",
        AttentionState::Error => "error",
    }
}

fn bind_output_updates(ui: &Rc<UiHandle>, surface_id: SurfaceId, text_view: TextView) {
    let runtime = ui.app_state.runtime();
    let buffer = text_view.buffer();
    let last_seen = Rc::new(RefCell::new(String::new()));
    let last_seen_for_timer = Rc::clone(&last_seen);

    gtk::glib::timeout_add_local(Duration::from_millis(150), move || {
        let Some(snapshot) = runtime.snapshot(surface_id) else {
            return gtk::glib::ControlFlow::Continue;
        };

        if *last_seen_for_timer.borrow() != snapshot.output {
            buffer.set_text(&snapshot.output);
            *last_seen_for_timer.borrow_mut() = snapshot.output;
            let mut end = buffer.end_iter();
            text_view.scroll_to_iter(&mut end, 0.0, false, 0.0, 1.0);
        }

        gtk::glib::ControlFlow::Continue
    });
}

fn connect_ghostty_widget(
    ui: &Rc<UiHandle>,
    workspace_id: taskers_domain::WorkspaceId,
    pane_id: taskers_domain::PaneId,
    surface_id: SurfaceId,
    widget: &Widget,
) {
    widget.set_focusable(true);
    let focus_ui = Rc::clone(ui);
    let focus_click = gtk::GestureClick::new();
    focus_click.connect_pressed(move |_, _, _, _| {
        focus_ui.dispatch(ControlCommand::FocusPane {
            workspace_id,
            pane_id,
        });
    });
    widget.add_controller(focus_click);

    let title_ui = Rc::clone(ui);
    widget.connect_notify_local(Some("title"), move |widget, _| {
        let title = widget
            .property::<Option<glib::GString>>("title")
            .map(|value| value.to_string());
        let current = title_ui
            .app_state
            .snapshot_model()
            .workspaces
            .values()
            .find_map(|workspace| workspace.panes.get(&pane_id))
            .and_then(|pane| pane.surfaces.get(&surface_id))
            .and_then(|surface| surface.metadata.title.clone());
        if current == title {
            return;
        }
        let deferred_ui = Rc::clone(&title_ui);
        glib::idle_add_local_once(move || {
            deferred_ui.dispatch(ControlCommand::UpdateSurfaceMetadata {
                surface_id,
                patch: PaneMetadataPatch {
                    title,
                    cwd: None,
                    repo_name: None,
                    git_branch: None,
                    ports: None,
                    agent_kind: None,
                },
            });
        });
    });

    let pwd_ui = Rc::clone(ui);
    widget.connect_notify_local(Some("pwd"), move |widget, _| {
        let cwd = widget
            .property::<Option<glib::GString>>("pwd")
            .map(|value| value.to_string());
        let current = pwd_ui
            .app_state
            .snapshot_model()
            .workspaces
            .values()
            .find_map(|workspace| workspace.panes.get(&pane_id))
            .and_then(|pane| pane.surfaces.get(&surface_id))
            .and_then(|surface| surface.metadata.cwd.clone());
        if current == cwd {
            return;
        }
        let deferred_ui = Rc::clone(&pwd_ui);
        glib::idle_add_local_once(move || {
            deferred_ui.dispatch(ControlCommand::UpdateSurfaceMetadata {
                surface_id,
                patch: PaneMetadataPatch {
                    title: None,
                    cwd,
                    repo_name: None,
                    git_branch: None,
                    ports: None,
                    agent_kind: None,
                },
            });
        });
    });

    let bell_ui = Rc::clone(ui);
    widget.connect_notify_local(Some("bell-ringing"), move |widget, _| {
        if widget.property::<bool>("bell-ringing") {
            let model = bell_ui.app_state.snapshot_model();
            let active_pane_has_focus = model.active_workspace().is_some_and(|workspace| {
                workspace.id == workspace_id
                    && workspace.active_pane == pane_id
                    && bell_ui.window.is_active()
                    && widget_contains_window_focus(&bell_ui.window, widget)
            });
            if active_pane_has_focus {
                return;
            }

            let deferred_ui = Rc::clone(&bell_ui);
            glib::idle_add_local_once(move || {
                deferred_ui.dispatch(ControlCommand::EmitSignal {
                    workspace_id,
                    pane_id,
                    surface_id: Some(surface_id),
                    event: SignalEvent::new(
                        "ghostty",
                        SignalKind::Notification,
                        Some("Terminal requested attention".into()),
                    ),
                });
            });
        }
    });

    let exit_ui = Rc::clone(ui);
    widget.connect_notify_local(Some("child-exited"), move |widget, _| {
        if widget.property::<bool>("child-exited") {
            let deferred_ui = Rc::clone(&exit_ui);
            glib::idle_add_local_once(move || {
                deferred_ui.dispatch(ControlCommand::CloseSurface {
                    workspace_id,
                    pane_id,
                    surface_id,
                });
            });
        }
    });
}

fn detach_widget(widget: &Widget) {
    let Some(parent) = widget.parent() else {
        return;
    };

    // Reparent through the actual container API. Calling gtk_widget_unparent()
    // directly from app code bypasses the parent's bookkeeping and can leave
    // reused pane widgets in a visually corrupted state after split/close
    // rebuilds.
    if let Ok(container) = parent.clone().downcast::<GtkBox>() {
        container.remove(widget);
        return;
    }

    if let Ok(paned) = parent.downcast::<Paned>() {
        if paned
            .start_child()
            .as_ref()
            .is_some_and(|child| child == widget)
        {
            paned.set_start_child(None::<&Widget>);
            return;
        }

        if paned
            .end_child()
            .as_ref()
            .is_some_and(|child| child == widget)
        {
            paned.set_end_child(None::<&Widget>);
            return;
        }
    }

    unreachable!("unsupported parent type for detachable widget");
}

fn format_pane_meta(pane: &PaneRecord, snapshot: Option<&PaneRuntimeSnapshot>) -> String {
    let metadata = pane.active_metadata();
    let cwd = metadata
        .and_then(|meta| meta.cwd.as_deref())
        .unwrap_or("cwd unknown");
    let branch = metadata
        .and_then(|meta| meta.git_branch.as_deref())
        .unwrap_or("no branch");
    let agent = metadata
        .and_then(|meta| meta.agent_kind.as_deref())
        .unwrap_or("shell");
    let ports = if metadata.is_none_or(|meta| meta.ports.is_empty()) {
        "no ports".into()
    } else {
        metadata
            .expect("metadata exists when ports are present")
            .ports
            .iter()
            .map(u16::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    };
    let process = snapshot
        .and_then(|snapshot| snapshot.process_id)
        .map(|process_id| format!("pid {process_id}"))
        .unwrap_or_else(|| "starting shell".into());

    format!("{agent}  \u{2022}  {cwd}  \u{2022}  {branch}  \u{2022}  {ports}  \u{2022}  {process}")
}

fn metadata_has_display_context(metadata: &PaneMetadata) -> bool {
    metadata
        .cwd
        .as_deref()
        .is_some_and(|cwd| !cwd.trim().is_empty())
        || metadata
            .git_branch
            .as_deref()
            .is_some_and(|branch| !branch.trim().is_empty())
        || metadata
            .repo_name
            .as_deref()
            .is_some_and(|repo_name| !repo_name.trim().is_empty())
        || metadata
            .agent_kind
            .as_deref()
            .is_some_and(|agent_kind| !agent_kind.trim().is_empty())
        || !metadata.ports.is_empty()
}

fn compact_preview(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn compact_path(path: &str) -> String {
    let home = std::env::var("HOME").ok();
    if let Some(home) = home {
        if path == home {
            return "~".into();
        }
        if let Some(suffix) = path.strip_prefix(&(home + "/")) {
            return format!("~/{suffix}");
        }
    }

    path.to_string()
}

fn format_ports(ports: &[u16]) -> String {
    match ports {
        [] => String::new(),
        [port] => format!(":{port}"),
        [first, rest @ ..] => format!(":{first} +{}", rest.len()),
    }
}

fn sorted_id_strings<I, T>(values: I) -> Vec<String>
where
    I: IntoIterator<Item = T>,
    T: ToString,
{
    let mut ids = values
        .into_iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>();
    ids.sort();
    ids
}

fn spawn_control_server(controller: InMemoryController, socket_path: PathBuf) -> String {
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
                    if let Err(error) = serve(listener, controller, pending::<()>()).await {
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

fn install_css() {
    let provider = CssProvider::new();
    provider.load_from_data(
        "
        /* ── Base ── */

        window {
            background: #0f1117;
            color: #e2e4ea;
        }

        headerbar {
            background: #0f1117;
            border-bottom: 1px solid rgba(255,255,255,0.07);
            box-shadow: none;
        }

        /* ── Paned separators ── */

        paned > separator {
            background: rgba(255,255,255,0.07);
            min-width: 1px;
            min-height: 1px;
            padding: 0;
        }

        /* ── Sidebar ── */

        .workspace-sidebar {
            background: #0d0f15;
            border-right: 1px solid rgba(255,255,255,0.07);
        }

        .sidebar-heading {
            font-weight: 600;
            font-size: 0.72rem;
            color: #5c6178;
            letter-spacing: 0.10em;
            text-transform: uppercase;
        }

        .workspace-add {
            background: transparent;
            color: #5c6178;
            border: 1px solid rgba(255,255,255,0.10);
            border-radius: 999px;
            min-width: 22px;
            min-height: 22px;
            padding: 0;
            font-size: 0.95rem;
            transition: background 180ms ease-in-out, color 180ms ease-in-out, border-color 180ms ease-in-out;
        }

        .workspace-add:hover {
            background: rgba(96,165,250,0.10);
            color: #e2e4ea;
            border-color: rgba(96,165,250,0.24);
        }

        .workspace-add:active {
            background: rgba(96,165,250,0.16);
        }

        .workspace-button {
            padding: 0;
        }

        .workspace-button:hover .workspace-item {
            background: rgba(255,255,255,0.04);
            border-color: rgba(255,255,255,0.10);
        }

        .workspace-item {
            padding: 7px 8px;
            border-radius: 8px;
            border: 1px solid transparent;
            transition: background 160ms ease-in-out, border-color 160ms ease-in-out;
        }

        .workspace-item-active {
            background: rgba(255,255,255,0.05);
            border-color: rgba(255,255,255,0.10);
        }

        .workspace-label {
            font-weight: 600;
            color: #f0f2f8;
            font-size: 0.80rem;
        }

        .workspace-agent-icon,
        .activity-agent-icon,
        .pane-agent-icon,
        .surface-tab-agent-icon {
            opacity: 0.96;
        }

        .agent-icon-codex {
            color: #f0f2f8;
        }

        .agent-icon-claude {
            color: #d97757;
        }

        .agent-icon-opencode {
            color: #c4cad4;
        }

        .workspace-preview {
            color: #b0b4c4;
            font-size: 0.72rem;
        }

        .workspace-meta {
            color: #5c6178;
            font-size: 0.68rem;
            letter-spacing: 0.01em;
        }

        .workspace-status-badge {
            background: rgba(124,138,255,0.14);
            color: #c7d2fe;
            border-radius: 999px;
            padding: 0 5px;
            min-width: 18px;
            min-height: 18px;
            font-size: 0.62rem;
            font-weight: 700;
            letter-spacing: 0.04em;
        }

        .workspace-status-badge-dot {
            background: transparent;
            min-width: 14px;
            min-height: 14px;
            padding: 0;
            font-size: 0.46rem;
        }

        .workspace-status-badge-idle {
            color: #3d4259;
        }

        .workspace-status-badge-state-busy {
            background: rgba(124,138,255,0.16);
            color: #c7d2fe;
        }

        .workspace-status-badge-state-completed {
            background: rgba(52,211,153,0.16);
            color: #a7f3d0;
        }

        .workspace-status-badge-state-waiting {
            background: rgba(96,165,250,0.18);
            color: #dbeafe;
        }

        .workspace-status-badge-state-error {
            background: rgba(248,113,113,0.16);
            color: #fecaca;
        }

        .workspace-item-has-attention {
            border-color: rgba(255,255,255,0.10);
        }

        .workspace-item-state-busy {
            background: rgba(124,138,255,0.05);
            border-color: rgba(124,138,255,0.16);
        }

        .workspace-item-state-completed {
            background: rgba(52,211,153,0.06);
            border-color: rgba(52,211,153,0.16);
        }

        .workspace-item-state-waiting {
            background: rgba(96,165,250,0.08);
            border-color: rgba(96,165,250,0.20);
        }

        .workspace-item-state-error {
            background: rgba(248,113,113,0.08);
            border-color: rgba(248,113,113,0.18);
        }

        .workspace-item-has-unread .workspace-label {
            color: #f0f2f8;
        }

        .workspace-item-active.workspace-item-state-busy {
            background: rgba(124,138,255,0.10);
            border-color: rgba(124,138,255,0.24);
        }

        .workspace-item-active.workspace-item-state-completed {
            background: rgba(52,211,153,0.09);
            border-color: rgba(52,211,153,0.22);
        }

        .workspace-item-active.workspace-item-state-waiting {
            background: rgba(96,165,250,0.12);
            border-color: rgba(96,165,250,0.30);
        }

        .workspace-item-active.workspace-item-state-error {
            background: rgba(248,113,113,0.10);
            border-color: rgba(248,113,113,0.24);
        }

        .workspace-close {
            background: transparent;
            color: #3d4259;
            border-radius: 4px;
            min-width: 22px;
            min-height: 22px;
            padding: 0;
            font-size: 0.85rem;
            transition: background 160ms ease-in-out, color 160ms ease-in-out;
        }

        .workspace-close:hover {
            background: rgba(248,113,113,0.15);
            color: #f87171;
        }

        .workspace-rename-entry {
            background: rgba(124,138,255,0.08);
            color: #e2e4ea;
            border: 1px solid rgba(124,138,255,0.30);
            border-radius: 4px;
            padding: 4px 6px;
            font-size: 0.82rem;
            font-weight: 500;
            min-height: 0;
        }

        .workspace-rename-entry:focus {
            border-color: rgba(124,138,255,0.55);
        }

        /* ── Workspace header ── */

        .workspace-header {
            border-bottom: 1px solid rgba(255,255,255,0.07);
            padding: 4px 0;
        }

        .workspace-header-label {
            font-weight: 600;
            font-size: 0.82rem;
            color: #f0f2f8;
        }

        .workspace-header-action {
            background: transparent;
            color: #3d4259;
            border-radius: 4px;
            min-width: 24px;
            min-height: 24px;
            padding: 0;
            font-size: 0.85rem;
            transition: background 180ms ease-in-out, color 180ms ease-in-out;
        }

        .workspace-header-action:hover {
            background: rgba(255,255,255,0.06);
            color: #8b8fa3;
        }

        /* ── Attention panel ── */

        .attention-panel {
            background: #0d0f15;
            border-left: 1px solid rgba(255,255,255,0.07);
        }

        .activity-item-button {
            padding: 0;
        }

        .activity-item {
            background: transparent;
            border-left: 2px solid transparent;
            transition: background 160ms ease-in-out, border-color 160ms ease-in-out;
        }

        .activity-item-button:hover .activity-item {
            background: rgba(255,255,255,0.03);
        }

        .activity-item-state-busy {
            border-left-color: rgba(124,138,255,0.55);
        }

        .activity-item-state-completed {
            border-left-color: rgba(52,211,153,0.55);
        }

        .activity-item-state-waiting {
            border-left-color: rgba(96,165,250,0.70);
        }

        .activity-item-state-error {
            border-left-color: rgba(248,113,113,0.65);
        }

        .activity-meta {
            color: #5c6178;
            font-size: 0.68rem;
        }

        .activity-preview {
            color: #b0b4c4;
            font-size: 0.74rem;
        }

        .activity-action {
            background: transparent;
            color: #5c6178;
            border: 1px solid rgba(255,255,255,0.10);
            border-radius: 999px;
            padding: 2px 8px;
            font-size: 0.68rem;
            font-weight: 600;
            min-height: 0;
            transition: background 160ms ease-in-out, color 160ms ease-in-out, border-color 160ms ease-in-out;
        }

        .activity-action:hover {
            background: rgba(96,165,250,0.10);
            color: #dbeafe;
            border-color: rgba(96,165,250,0.25);
        }

        .activity-time {
            color: #3d4259;
            font-size: 0.68rem;
        }

        /* ── Workspace windows ── */

        .workspace-window {
            background: #12141c;
            border: 1px solid rgba(255,255,255,0.07);
            border-radius: 6px;
        }

        .workspace-window-active {
            border-color: rgba(255,255,255,0.14);
        }

        .workspace-window-state-busy {
            border-color: rgba(124,138,255,0.22);
        }

        .workspace-window-state-completed {
            border-color: rgba(52,211,153,0.22);
        }

        .workspace-window-state-waiting {
            border-color: rgba(96,165,250,0.30);
        }

        .workspace-window-state-error {
            border-color: rgba(248,113,113,0.24);
        }

        .workspace-window-active.workspace-window-state-busy {
            border-color: rgba(124,138,255,0.38);
        }

        .workspace-window-active.workspace-window-state-completed {
            border-color: rgba(52,211,153,0.34);
        }

        .workspace-window-active.workspace-window-state-waiting {
            border-color: rgba(96,165,250,0.48);
        }

        .workspace-window-active.workspace-window-state-error {
            border-color: rgba(248,113,113,0.38);
        }

        .workspace-window-ghost {
            background: rgba(18,20,28,0.78);
            border-style: dashed;
            box-shadow: 0 12px 28px rgba(0,0,0,0.26);
        }

        .workspace-window-ghost-chrome {
            padding: 6px;
            spacing: 0;
        }

        .workspace-window-ghost-header {
            margin: 0;
            padding: 2px 6px;
        }

        .workspace-window-ghost-strip {
            margin: 4px 0 6px;
        }

        .workspace-window-ghost-tab {
            background: rgba(255,255,255,0.05);
            border-color: rgba(255,255,255,0.10);
        }

        .workspace-window-ghost-body {
            margin: 0 2px 2px;
            border-radius: 4px;
            background: rgba(255,255,255,0.03);
            border: 1px solid rgba(255,255,255,0.04);
        }

        .workspace-window-resize-handle {
            background: transparent;
            transition: background 160ms ease-in-out;
        }

        .workspace-window-resize-handle-right:hover,
        .workspace-window-resize-handle-bottom:hover {
            background: rgba(124,138,255,0.14);
        }

        /* ── Pane cards ── */

        .pane-card {
            background: transparent;
        }

        .pane-header {
            background: rgba(255,255,255,0.02);
            border-bottom: 1px solid rgba(255,255,255,0.05);
            padding: 2px 0;
            transition: background 160ms ease-in-out;
        }

        .pane-header:hover {
            background: rgba(255,255,255,0.04);
        }

        .pane-card-active .pane-header {
            background: rgba(124,138,255,0.06);
            border-bottom: 1px solid rgba(124,138,255,0.15);
        }

        .pane-card-active .pane-header:hover {
            background: rgba(124,138,255,0.10);
        }

        .pane-card-state-busy .pane-header {
            background: rgba(124,138,255,0.04);
            border-bottom-color: rgba(124,138,255,0.16);
        }

        .pane-card-state-completed .pane-header {
            background: rgba(52,211,153,0.04);
            border-bottom-color: rgba(52,211,153,0.16);
        }

        .pane-card-state-waiting .pane-header {
            background: rgba(96,165,250,0.06);
            border-bottom-color: rgba(96,165,250,0.18);
        }

        .pane-card-state-error .pane-header {
            background: rgba(248,113,113,0.05);
            border-bottom-color: rgba(248,113,113,0.16);
        }

        .pane-card-active.pane-card-state-waiting .pane-header {
            background: rgba(96,165,250,0.10);
            border-bottom-color: rgba(96,165,250,0.24);
        }

        .pane-title {
            font-weight: 500;
            color: #8b8fa3;
            font-size: 0.72rem;
        }

        .pane-card-active .pane-title {
            color: #e2e4ea;
        }

        .pane-close {
            background: transparent;
            color: #3d4259;
            border-radius: 3px;
            min-width: 18px;
            min-height: 18px;
            padding: 0;
            font-size: 0.75rem;
            transition: background 160ms ease-in-out, color 160ms ease-in-out;
        }

        .pane-close:hover {
            background: rgba(248,113,113,0.15);
            color: #f87171;
        }

        .pane-card-active .pane-close-action {
            background: rgba(248,113,113,0.10);
            color: #f9a8a8;
        }

        .pane-card-active .pane-close-action:hover {
            background: rgba(248,113,113,0.18);
            color: #fecaca;
        }

        .pane-action {
            background: transparent;
            color: #3d4259;
            border-radius: 3px;
            min-width: 18px;
            min-height: 18px;
            padding: 0;
            font-size: 0.75rem;
            opacity: 0;
            transition: opacity 180ms ease-in-out, background 160ms ease-in-out, color 160ms ease-in-out;
        }

        .pane-header:hover .pane-action {
            opacity: 0.7;
        }

        .pane-card-active .pane-action {
            opacity: 0.62;
        }

        .pane-card-active .pane-window-action {
            background: rgba(96,165,250,0.12);
            color: #7dd3fc;
        }

        .pane-card-active .pane-split-action {
            background: rgba(45,212,191,0.11);
            color: #5eead4;
        }

        .pane-action:hover {
            opacity: 1;
            background: rgba(124,138,255,0.12);
            color: #8b8fa3;
        }

        .pane-card-active .pane-window-action:hover {
            background: rgba(96,165,250,0.20);
            color: #d8f4ff;
        }

        .pane-card-active .pane-split-action:hover {
            background: rgba(45,212,191,0.18);
            color: #ccfbf1;
        }

        .pane-meta {
            color: #3d4259;
            font-size: 0.75rem;
        }

        .surface-tabs {
            margin: 4px 8px 6px;
            min-height: 24px;
        }

        .surface-tab {
            background: rgba(255,255,255,0.03);
            border: 1px solid rgba(255,255,255,0.07);
            border-radius: 6px;
            padding: 2px 6px;
            transition: background 160ms ease-in-out, border-color 160ms ease-in-out;
        }

        .surface-tab-active {
            background: rgba(124,138,255,0.14);
            border-color: rgba(124,138,255,0.35);
        }

        .surface-tab-has-attention.surface-tab-state-busy {
            background: rgba(124,138,255,0.08);
            border-color: rgba(124,138,255,0.22);
        }

        .surface-tab-has-attention.surface-tab-state-completed {
            background: rgba(52,211,153,0.08);
            border-color: rgba(52,211,153,0.22);
        }

        .surface-tab-has-attention.surface-tab-state-waiting {
            background: rgba(96,165,250,0.10);
            border-color: rgba(96,165,250,0.28);
        }

        .surface-tab-has-attention.surface-tab-state-error {
            background: rgba(248,113,113,0.10);
            border-color: rgba(248,113,113,0.28);
        }

        .surface-tab-dragging {
            background: rgba(255,255,255,0.08);
            border-color: rgba(255,255,255,0.18);
        }

        .surface-tab-exiting {
            border-color: rgba(255,255,255,0.04);
        }

        .surface-tab-label,
        .surface-tab-close,
        .surface-tab-add {
            min-height: 0;
            padding: 0;
        }

        .surface-tab-label {
            color: #8b8fa3;
            font-size: 0.74rem;
        }

        .surface-tab-active .surface-tab-label {
            color: #f0f2f8;
        }

        .surface-tab-title {
            color: #8b8fa3;
            font-size: 0.74rem;
        }

        .surface-tab-active .surface-tab-title {
            color: #f0f2f8;
        }

        .surface-tab-close,
        .surface-tab-add {
            color: #5c6178;
            border-radius: 4px;
            min-width: 18px;
            min-height: 18px;
        }

        .surface-tab-close:hover,
        .surface-tab-add:hover {
            background: rgba(255,255,255,0.06);
            color: #b0b4c4;
        }

        /* ── Status dots ── */

        .status-dot {
            font-size: 0.5rem;
        }

        .status-dot-normal { color: #3d4259; }
        .status-dot-busy { color: #7c8aff; }
        .status-dot-completed { color: #34d399; }
        .status-dot-waiting { color: #60a5fa; }
        .status-dot-error { color: #f87171; }

        /* ── Empty state ── */

        .empty-state {
            color: #3d4259;
            font-size: 0.85rem;
        }

        /* ── Terminal ── */

        .terminal-output,
        .terminal-entry {
            border-radius: 0;
            background: #0f1117;
            color: #e2e4ea;
            font-family: Monospace;
        }

        .terminal-output {
            padding: 4px;
        }

        .terminal-entry {
            padding: 6px 8px;
            border-top: 1px solid rgba(255,255,255,0.07);
        }

        .terminal-entry:focus {
            border-top: 1px solid rgba(124,138,255,0.4);
        }

        /* ── Popover / context menus ── */

        popover > contents {
            background: #1a1d28;
            border: 1px solid rgba(255,255,255,0.10);
            border-radius: 8px;
            padding: 4px;
            box-shadow: 0 8px 24px rgba(0,0,0,0.4);
        }

        .context-item {
            color: #b0b4c4;
            font-size: 0.8rem;
            padding: 6px 12px;
            border-radius: 4px;
            min-height: 0;
            transition: background 160ms ease-in-out;
        }

        .context-item:hover {
            background: rgba(124,138,255,0.12);
            color: #e2e4ea;
        }

        .context-separator {
            background: rgba(255,255,255,0.07);
            margin: 4px 8px;
            min-height: 1px;
        }

        popover .destructive-action {
            color: #f87171;
            font-size: 0.8rem;
            padding: 6px 12px;
            border-radius: 4px;
            min-height: 0;
            transition: background 160ms ease-in-out;
        }

        popover .destructive-action:hover {
            background: rgba(248,113,113,0.12);
        }
        ",
    );

    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}
