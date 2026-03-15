mod app_state;
mod pane_runtime;
mod session_store;
mod settings_store;

use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    future::pending,
    path::PathBuf,
    process::{Command, Stdio},
    rc::Rc,
    thread,
    time::{Duration, Instant},
};

use adw::prelude::*;
use app_state::AppState;
use clap::Parser;
use gtk::{
    Align, Box as GtkBox, Button, CssProvider, Entry, Fixed, Label, Orientation, Overlay, Paned,
    PolicyType, STYLE_PROVIDER_PRIORITY_APPLICATION, ScrolledWindow, Separator, TextView, Widget,
    WrapMode, gdk, glib,
};
use pane_runtime::PaneRuntimeSnapshot;
use serde_json::json;
use settings_store::{AppConfig, ShortcutAction};
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
}

#[derive(Clone)]
struct ShellWidgets {
    root: Paned,
    sidebar_list: GtkBox,
    toolbar_label: Label,
    btn_window_right: Button,
    btn_window_down: Button,
    btn_split_right: Button,
    btn_split_down: Button,
    activity_list: GtkBox,
    activity_empty: Label,
    layout_scroll: ScrolledWindow,
    layout_host: Fixed,
}

#[derive(Clone)]
struct PaneCardWidgets {
    root: GtkBox,
    agent_icon: Label,
    title: Label,
    status_dot: Label,
    surface_tabs: GtkBox,
    terminal_host: GtkBox,
    focus_target: Widget,
}

#[derive(Clone, PartialEq, Eq)]
enum LayoutRenderKey {
    WorkspaceWindows {
        active_window: WorkspaceWindowId,
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

#[derive(Clone, Copy)]
struct WorkspaceRenderContext {
    viewport_height: i32,
    overview_mode: bool,
    overview_scale: f64,
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
        ghostty_host: Option<GhosttyHost>,
        shell_launch: ShellLaunchSpec,
    ) -> Rc<Self> {
        Rc::new(Self {
            app_state,
            backend_choice,
            application,
            window,
            overlay,
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
            "Smooth fade transitions when creating windows, switching workspaces, and splitting panes.",
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
        let Some(path) = std::env::var_os("TASKERS_UI_INTEGRITY_PATH").map(PathBuf::from) else {
            return;
        };
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

        let agent_icon = build_agent_icon(
            pane.active_surface()
                .and_then(|surface| surface.metadata.agent_kind.as_deref()),
        );
        agent_icon.add_css_class("pane-agent-icon");
        header.append(&agent_icon);

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

        let close_button = Button::with_label("\u{00d7}");
        close_button.add_css_class("pane-close");
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

        let surface_tabs = GtkBox::new(Orientation::Horizontal, 4);
        surface_tabs.add_css_class("surface-tabs");
        root.append(&surface_tabs);

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
        animate_pane_slide_in(self, card.root.upcast_ref());
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
            pane.active_surface()
                .and_then(|surface| surface.metadata.agent_kind.as_deref()),
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
    let startup_toast = merge_startup_toasts(
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
    let app_state = match AppState::new(
        initial_model,
        session_path,
        backend_choice,
        shell_launch.clone(),
    ) {
        Ok(state) => state,
        Err(error) => {
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

    let ui = UiHandle::new(
        startup.app_state,
        startup.backend_choice,
        startup.config_path,
        startup.app_config,
        app.clone(),
        window.clone(),
        overlay,
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

    let toolbar = GtkBox::new(Orientation::Horizontal, 8);
    toolbar.add_css_class("toolbar");
    toolbar.set_size_request(-1, 36);
    toolbar.set_margin_start(10);
    toolbar.set_margin_end(10);
    toolbar.set_valign(Align::Center);

    let toolbar_label = Label::new(Some("taskers"));
    toolbar_label.add_css_class("toolbar-label");
    toolbar_label.set_xalign(0.0);
    toolbar_label.set_hexpand(true);
    toolbar.append(&toolbar_label);

    // --- Window group ---
    let window_group = GtkBox::new(Orientation::Horizontal, 4);
    window_group.add_css_class("toolbar-group");

    let btn_window_right = Button::with_label("\u{25eb} Window Right");
    btn_window_right.add_css_class("toolbar-action");
    btn_window_right.set_tooltip_text(Some("New window to the right (Ctrl+Alt+Shift+Right)"));
    let wr_ui = Rc::clone(ui);
    btn_window_right.connect_clicked(move |_| {
        let model = wr_ui.app_state.snapshot_model();
        if let Some(workspace) = model.active_workspace() {
            wr_ui.dispatch(ControlCommand::CreateWorkspaceWindow {
                workspace_id: workspace.id,
                direction: Direction::Right,
            });
        }
    });
    window_group.append(&btn_window_right);

    let btn_window_down = Button::with_label("\u{2193} Window Down");
    btn_window_down.add_css_class("toolbar-action");
    btn_window_down.set_tooltip_text(Some("New window below (Ctrl+Alt+Shift+Down)"));
    let wd_ui = Rc::clone(ui);
    btn_window_down.connect_clicked(move |_| {
        let model = wd_ui.app_state.snapshot_model();
        if let Some(workspace) = model.active_workspace() {
            wd_ui.dispatch(ControlCommand::CreateWorkspaceWindow {
                workspace_id: workspace.id,
                direction: Direction::Down,
            });
        }
    });
    window_group.append(&btn_window_down);
    toolbar.append(&window_group);

    let sep1 = Separator::new(Orientation::Vertical);
    sep1.add_css_class("toolbar-separator");
    toolbar.append(&sep1);

    // --- Pane split group ---
    let pane_group = GtkBox::new(Orientation::Horizontal, 4);
    pane_group.add_css_class("toolbar-group");

    let btn_split_right = Button::with_label("\u{25eb} Split Right");
    btn_split_right.add_css_class("toolbar-action");
    btn_split_right.set_tooltip_text(Some("Split active pane to the right"));
    let sr_ui = Rc::clone(ui);
    btn_split_right.connect_clicked(move |_| {
        let model = sr_ui.app_state.snapshot_model();
        if let Some(workspace) = model.active_workspace() {
            sr_ui.dispatch(ControlCommand::SplitPane {
                workspace_id: workspace.id,
                pane_id: Some(workspace.active_pane),
                axis: taskers_domain::SplitAxis::Horizontal,
            });
        }
    });
    pane_group.append(&btn_split_right);

    let btn_split_down = Button::with_label("\u{2501} Split Down");
    btn_split_down.add_css_class("toolbar-action");
    btn_split_down.set_tooltip_text(Some("Split active pane downward"));
    let sd_ui = Rc::clone(ui);
    btn_split_down.connect_clicked(move |_| {
        let model = sd_ui.app_state.snapshot_model();
        if let Some(workspace) = model.active_workspace() {
            sd_ui.dispatch(ControlCommand::SplitPane {
                workspace_id: workspace.id,
                pane_id: Some(workspace.active_pane),
                axis: taskers_domain::SplitAxis::Vertical,
            });
        }
    });
    pane_group.append(&btn_split_down);
    toolbar.append(&pane_group);

    let sep2 = Separator::new(Orientation::Vertical);
    sep2.add_css_class("toolbar-separator");
    toolbar.append(&sep2);

    // --- Settings ---
    let settings_button = Button::with_label("\u{2699} Settings");
    settings_button.add_css_class("toolbar-action");
    settings_button.add_css_class("toolbar-action-subtle");
    settings_button.set_tooltip_text(Some("Keyboard settings"));
    let settings_ui = Rc::clone(ui);
    settings_button.connect_clicked(move |_| {
        settings_ui.present_settings_dialog();
    });
    toolbar.append(&settings_button);

    main_column.append(&toolbar);

    let layout_host = Fixed::new();
    layout_host.set_hexpand(true);
    layout_host.set_vexpand(true);
    layout_host.set_halign(Align::Start);
    layout_host.set_valign(Align::Start);

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
        toolbar_label,
        btn_window_right,
        btn_window_down,
        btn_split_right,
        btn_split_down,
        activity_list,
        activity_empty,
        layout_scroll,
        layout_host,
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
                    .and_then(workspace_display_metadata)
                    .and_then(|metadata| metadata.agent_kind.as_deref())
                    .or_else(|| {
                        summary
                            .agent_summaries
                            .first()
                            .map(|agent| agent.agent_kind.as_str())
                    }),
            );
            agent_icon.add_css_class("workspace-agent-icon");
            heading.append(&agent_icon);

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
        shell.toolbar_label.set_text(&label);
        shell.btn_window_right.set_sensitive(true);
        shell.btn_window_down.set_sensitive(true);
        shell.btn_split_right.set_sensitive(true);
        shell.btn_split_down.set_sensitive(true);
    } else {
        shell.toolbar_label.set_text("No workspace");
        shell.btn_window_right.set_sensitive(false);
        shell.btn_window_down.set_sensitive(false);
        shell.btn_split_right.set_sensitive(false);
        shell.btn_split_down.set_sensitive(false);
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
        activity_metadata(model, item).and_then(|metadata| metadata.agent_kind.as_deref()),
    );
    agent_icon.add_css_class("activity-agent-icon");
    heading.append(&agent_icon);

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

fn activity_metadata<'a>(model: &'a AppModel, item: &ActivityItem) -> Option<&'a PaneMetadata> {
    model
        .workspaces
        .get(&item.workspace_id)
        .and_then(|workspace| workspace.panes.get(&item.pane_id))
        .and_then(|pane| {
            pane.surfaces
                .get(&item.surface_id)
                .or_else(|| pane.active_surface())
                .map(|surface| &surface.metadata)
        })
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

// ── Animation helpers ──
//
// Niri-style slide + fade using a manual frame timer (~60fps).
// This bypasses adw animation APIs which don't reliably fire on
// freshly-created widgets that haven't been fully realized yet.

const ANIM_SLIDE_PX: f64 = 80.0;
const ANIM_DURATION_MS: u64 = 350;
const ANIM_FRAME_MS: u64 = 16;

/// Ease-out cubic: starts fast, decelerates to a smooth stop.
fn ease_out_cubic(t: f64) -> f64 {
    let t1 = 1.0 - t;
    1.0 - t1 * t1 * t1
}

/// Animate a workspace window sliding into position on a Fixed canvas.
/// Widget starts at offset position with opacity 0 and slides to final
/// position over ANIM_DURATION_MS with an ease-out curve.
fn animate_window_slide_in(
    ui: &UiHandle,
    canvas: &Fixed,
    widget: &Widget,
    final_x: f64,
    final_y: f64,
    display_frame: WindowFrame,
) {
    if !ui.settings.borrow().animations_enabled || ui.backend_choice == BackendChoice::Ghostty {
        return;
    }

    // Determine slide direction from frame position.
    let (offset_x, offset_y) = if display_frame.x > 0 {
        (ANIM_SLIDE_PX, 0.0)
    } else if display_frame.y > 0 {
        (0.0, ANIM_SLIDE_PX)
    } else {
        (ANIM_SLIDE_PX, 0.0)
    };

    let start_x = final_x + offset_x;
    let start_y = final_y + offset_y;

    // Place at start and hide.
    canvas.move_(widget, start_x, start_y);
    widget.set_opacity(0.0);

    // Drive animation with a frame timer.
    let w = widget.clone();
    let c = canvas.clone();
    let start_time = Rc::new(Cell::new(None::<u64>));
    glib::timeout_add_local(Duration::from_millis(ANIM_FRAME_MS), move || {
        let now = glib::monotonic_time() as u64; // microseconds
        let started = start_time.get();
        let t0 = match started {
            Some(t0) => t0,
            None => {
                start_time.set(Some(now));
                now
            }
        };

        let elapsed_ms = (now.saturating_sub(t0)) / 1000;
        let progress = (elapsed_ms as f64 / ANIM_DURATION_MS as f64).min(1.0);
        let eased = ease_out_cubic(progress);

        let x = start_x + (final_x - start_x) * eased;
        let y = start_y + (final_y - start_y) * eased;
        c.move_(&w, x, y);
        w.set_opacity(eased);

        if progress >= 1.0 {
            w.set_opacity(1.0);
            c.move_(&w, final_x, final_y);
            glib::ControlFlow::Break
        } else {
            glib::ControlFlow::Continue
        }
    });
}

/// Animate a pane card sliding in from the right.
fn animate_pane_slide_in(ui: &UiHandle, widget: &Widget) {
    if !ui.settings.borrow().animations_enabled || ui.backend_choice == BackendChoice::Ghostty {
        return;
    }

    widget.set_opacity(0.0);
    widget.set_margin_start(40);

    let w = widget.clone();
    let start_time = Rc::new(Cell::new(None::<u64>));
    glib::timeout_add_local(Duration::from_millis(ANIM_FRAME_MS), move || {
        let now = glib::monotonic_time() as u64;
        let t0 = match start_time.get() {
            Some(t0) => t0,
            None => {
                start_time.set(Some(now));
                now
            }
        };

        let elapsed_ms = (now.saturating_sub(t0)) / 1000;
        let progress = (elapsed_ms as f64 / ANIM_DURATION_MS as f64).min(1.0);
        let eased = ease_out_cubic(progress);

        w.set_opacity(eased);
        w.set_margin_start(((1.0 - eased) * 40.0).round() as i32);

        if progress >= 1.0 {
            w.set_opacity(1.0);
            w.set_margin_start(0);
            glib::ControlFlow::Break
        } else {
            glib::ControlFlow::Continue
        }
    });
}

fn layout_render_key(
    workspace: &Workspace,
    render_context: WorkspaceRenderContext,
) -> LayoutRenderKey {
    LayoutRenderKey::WorkspaceWindows {
        active_window: workspace.active_window,
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
    let needs_rebuild = *ui.layout_state.borrow() != next_state;
    let overview_mode = ui.overview_mode.get();

    if needs_rebuild {
        let previous_window_ids: HashSet<WorkspaceWindowId> = previous_model
            .as_ref()
            .and_then(AppModel::active_workspace)
            .map(|ws| ws.windows.keys().copied().collect())
            .unwrap_or_default();
        let new_content: Widget = if let Some(workspace) = model.active_workspace() {
            build_workspace_canvas_widget(ui, shell, workspace, &previous_window_ids)
        } else {
            let empty = Label::new(Some("No workspace selected"));
            empty.add_css_class("empty-state");
            empty.set_xalign(0.5);
            empty.set_yalign(0.5);
            empty.set_hexpand(true);
            empty.set_vexpand(true);
            empty.upcast()
        };
        clear_fixed(&shell.layout_host);
        shell.layout_host.put(&new_content, 0.0, 0.0);
        *ui.layout_state.borrow_mut() = next_state;
    }

    if let Some(workspace) = model.active_workspace() {
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
            || previous_active_window != Some(workspace.active_window)
            || previous_active_pane != Some(workspace.active_pane);
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

fn build_workspace_canvas_widget(
    ui: &Rc<UiHandle>,
    shell: &ShellWidgets,
    workspace: &Workspace,
    previous_window_ids: &HashSet<WorkspaceWindowId>,
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
        if !previous_window_ids.contains(&window.id) {
            animate_window_slide_in(
                ui.as_ref(),
                &canvas,
                &window_widget,
                final_x,
                final_y,
                display_frame,
            );
        }
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

    let body = build_split_layout_widget(ui, workspace, window.id, &window.layout, Vec::new());
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
            let paned = Paned::builder()
                .orientation(match axis {
                    taskers_domain::SplitAxis::Horizontal => Orientation::Horizontal,
                    taskers_domain::SplitAxis::Vertical => Orientation::Vertical,
                })
                .wide_handle(false)
                .build();
            let mut first_path = path.clone();
            first_path.push(false);
            paned.set_start_child(Some(&build_split_layout_widget(
                ui,
                workspace,
                workspace_window_id,
                first,
                first_path,
            )));
            let mut second_path = path.clone();
            second_path.push(true);
            paned.set_end_child(Some(&build_split_layout_widget(
                ui,
                workspace,
                workspace_window_id,
                second,
                second_path,
            )));
            bind_split_ratio_updates(
                ui,
                workspace.id,
                workspace_window_id,
                &paned,
                *axis,
                path,
                *ratio,
            );
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
    ratio: u16,
) {
    let suppress = Rc::new(Cell::new(false));
    let path = Rc::new(path);
    let pending_source = Rc::new(RefCell::new(None::<glib::SourceId>));

    let sync_paned = paned.clone();
    let suppress_for_sync = Rc::clone(&suppress);
    glib::idle_add_local_once(move || {
        suppress_for_sync.set(true);
        let extent = paned_extent(&sync_paned, axis);
        if extent > 0 {
            sync_paned.set_position(((extent * i32::from(ratio)) / 1000).max(1));
        }
        suppress_for_sync.set(false);
    });

    let ratio_ui = Rc::clone(ui);
    let suppress_for_notify = Rc::clone(&suppress);
    let pending_for_notify = Rc::clone(&pending_source);
    paned.connect_notify_local(Some("position"), move |paned, _| {
        if suppress_for_notify.get() {
            return;
        }

        if let Some(source) = pending_for_notify.borrow_mut().take() {
            source.remove();
        }
        let extent = paned_extent(paned, axis);
        if extent <= 0 {
            return;
        }
        let ratio = (((paned.position() as f64) / f64::from(extent)) * 1000.0)
            .round()
            .clamp(0.0, 1000.0) as u16;
        let path = Rc::clone(&path);
        let ratio_ui = Rc::clone(&ratio_ui);
        let source = glib::timeout_add_local_once(Duration::from_millis(120), move || {
            ratio_ui.dispatch(ControlCommand::SetWindowSplitRatio {
                workspace_id,
                workspace_window_id,
                path: path.as_ref().clone(),
                ratio,
            });
        });
        *pending_for_notify.borrow_mut() = Some(source);
    });
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

fn sync_surface_tabs(
    ui: &Rc<UiHandle>,
    workspace_id: taskers_domain::WorkspaceId,
    pane: &PaneRecord,
    card: &PaneCardWidgets,
) {
    clear_box(&card.surface_tabs);

    for surface in pane.surfaces.values() {
        let tab = GtkBox::new(Orientation::Horizontal, 4);
        tab.add_css_class("surface-tab");
        if surface.attention != AttentionState::Normal {
            tab.add_css_class("surface-tab-has-attention");
            tab.add_css_class(&format!(
                "surface-tab-state-{}",
                attention_state_slug(surface.attention)
            ));
        }
        if surface.id == pane.active_surface {
            tab.add_css_class("surface-tab-active");
        }

        let dot = Label::new(Some("\u{25cf}"));
        dot.add_css_class("status-dot");
        dot.add_css_class(&attention_dot_class(surface.attention));
        dot.set_tooltip_text(Some(surface.attention.label()));
        tab.append(&dot);

        let label = Button::new();
        label.add_css_class("flat");
        label.add_css_class("surface-tab-label");
        let label_content = GtkBox::new(Orientation::Horizontal, 4);
        label_content.set_hexpand(true);
        let agent_icon = build_agent_icon(surface.metadata.agent_kind.as_deref());
        agent_icon.add_css_class("surface-tab-agent-icon");
        label_content.append(&agent_icon);
        let title = Label::new(Some(&display_surface_title(surface)));
        title.add_css_class("surface-tab-title");
        title.set_xalign(0.0);
        title.set_hexpand(true);
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label_content.append(&title);
        label.set_child(Some(&label_content));
        let focus_ui = Rc::clone(ui);
        let pane_id = pane.id;
        let surface_id = surface.id;
        label.connect_clicked(move |_| {
            focus_ui.dispatch(ControlCommand::FocusSurface {
                workspace_id,
                pane_id,
                surface_id,
            });
        });
        tab.append(&label);

        let close = Button::with_label("\u{00d7}");
        close.add_css_class("flat");
        close.add_css_class("surface-tab-close");
        let close_ui = Rc::clone(ui);
        let close_pane_id = pane.id;
        let close_surface_id = surface.id;
        close.connect_clicked(move |_| {
            close_ui.dispatch(ControlCommand::CloseSurface {
                workspace_id,
                pane_id: close_pane_id,
                surface_id: close_surface_id,
            });
        });
        tab.append(&close);

        card.surface_tabs.append(&tab);
    }

    let add = Button::with_label("+");
    add.add_css_class("flat");
    add.add_css_class("surface-tab-add");
    let add_ui = Rc::clone(ui);
    let pane_id = pane.id;
    add.connect_clicked(move |_| {
        add_ui.dispatch(ControlCommand::CreateSurface {
            workspace_id,
            pane_id,
            kind: PaneKind::Terminal,
        });
    });
    card.surface_tabs.append(&add);
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

    if let Some(agent) = surface.metadata.agent_kind.as_deref() {
        return humanize_agent_kind(agent);
    }

    match surface.kind {
        PaneKind::Terminal => "Terminal".into(),
        PaneKind::Browser => "Browser".into(),
    }
}

fn build_agent_icon(agent_kind: Option<&str>) -> Label {
    let icon = Label::new(None);
    icon.add_css_class("agent-icon");
    configure_agent_icon(&icon, agent_kind);
    icon
}

fn configure_agent_icon(icon: &Label, agent_kind: Option<&str>) {
    for class in &[
        "agent-icon-codex",
        "agent-icon-claude",
        "agent-icon-opencode",
        "agent-icon-aider",
        "agent-icon-generic",
    ] {
        icon.remove_css_class(class);
    }

    let Some(agent_kind) = normalized_agent_kind(agent_kind) else {
        icon.set_text("");
        icon.set_tooltip_text(None);
        icon.set_visible(false);
        return;
    };

    icon.set_text(agent_icon_text(agent_kind));
    icon.set_tooltip_text(Some(&humanize_agent_kind(agent_kind)));
    icon.add_css_class(&format!("agent-icon-{}", agent_icon_slug(agent_kind)));
    icon.set_visible(true);
}

fn normalized_agent_kind(agent_kind: Option<&str>) -> Option<&str> {
    agent_kind
        .map(str::trim)
        .filter(|agent_kind| !agent_kind.is_empty())
}

fn agent_icon_text(agent_kind: &str) -> &'static str {
    match agent_kind {
        "codex" => "CX",
        "claude" => "CL",
        "opencode" => "OC",
        "aider" => "AI",
        _ => "AG",
    }
}

fn agent_icon_slug(agent_kind: &str) -> &'static str {
    match agent_kind {
        "codex" => "codex",
        "claude" => "claude",
        "opencode" => "opencode",
        "aider" => "aider",
        _ => "generic",
    }
}

fn humanize_agent_kind(agent: &str) -> String {
    match agent {
        "codex" => "Codex".into(),
        "claude" => "Claude".into(),
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
        .unwrap_or(0)
        .min(0);
    let min_y = display_frames
        .iter()
        .map(|frame| frame.y)
        .min()
        .unwrap_or(0)
        .min(0);
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
        .unwrap_or(0)
        .min(0);
    let min_y = display_frames
        .iter()
        .map(|frame| frame.y)
        .min()
        .unwrap_or(0)
        .min(0);
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
            background: #09090b;
            color: #e4e4e7;
        }

        headerbar {
            background: #09090b;
            border-bottom: 1px solid rgba(255,255,255,0.06);
            box-shadow: none;
        }

        /* ── Paned separators ── */

        paned > separator {
            background: rgba(255,255,255,0.08);
            min-width: 1px;
            min-height: 1px;
            padding: 0;
        }

        /* ── Sidebar ── */

        .workspace-sidebar {
            background: #08090c;
            border-right: 1px solid rgba(255,255,255,0.06);
        }

        .sidebar-heading {
            font-weight: 600;
            font-size: 0.72rem;
            color: #71717a;
            letter-spacing: 0.10em;
            text-transform: uppercase;
        }

        .workspace-add {
            background: transparent;
            color: #71717a;
            border: 1px solid rgba(255,255,255,0.08);
            border-radius: 999px;
            min-width: 22px;
            min-height: 22px;
            padding: 0;
            font-size: 0.95rem;
            transition: background 150ms ease, color 150ms ease, border-color 150ms ease;
        }

        .workspace-add:hover {
            background: rgba(59,130,246,0.10);
            color: #dbeafe;
            border-color: rgba(59,130,246,0.24);
        }

        .workspace-add:active {
            background: rgba(59,130,246,0.16);
        }

        .workspace-button {
            padding: 0;
        }

        .workspace-button:hover .workspace-item {
            background: rgba(255,255,255,0.04);
            border-color: rgba(255,255,255,0.08);
        }

        .workspace-item {
            padding: 7px 8px;
            border-radius: 8px;
            border: 1px solid transparent;
            transition: background 120ms ease, border-color 120ms ease;
        }

        .workspace-item-active {
            background: rgba(255,255,255,0.05);
            border-color: rgba(255,255,255,0.10);
        }

        .workspace-label {
            font-weight: 600;
            color: #f4f4f5;
            font-size: 0.80rem;
        }

        .workspace-agent-icon {
            margin-top: 1px;
        }

        .workspace-preview {
            color: #d4d4d8;
            font-size: 0.72rem;
        }

        .workspace-meta {
            color: #71717a;
            font-size: 0.68rem;
            letter-spacing: 0.01em;
        }

        .workspace-status-badge {
            background: rgba(99,102,241,0.14);
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
            color: #52525b;
        }

        .workspace-status-badge-state-busy {
            background: rgba(99,102,241,0.16);
            color: #c7d2fe;
        }

        .workspace-status-badge-state-completed {
            background: rgba(34,197,94,0.16);
            color: #bbf7d0;
        }

        .workspace-status-badge-state-waiting {
            background: rgba(59,130,246,0.18);
            color: #dbeafe;
        }

        .workspace-status-badge-state-error {
            background: rgba(239,68,68,0.16);
            color: #fecaca;
        }

        .workspace-item-has-attention {
            border-color: rgba(255,255,255,0.08);
        }

        .workspace-item-state-busy {
            background: rgba(99,102,241,0.05);
            border-color: rgba(99,102,241,0.16);
        }

        .workspace-item-state-completed {
            background: rgba(34,197,94,0.06);
            border-color: rgba(34,197,94,0.16);
        }

        .workspace-item-state-waiting {
            background: rgba(59,130,246,0.08);
            border-color: rgba(59,130,246,0.20);
        }

        .workspace-item-state-error {
            background: rgba(239,68,68,0.08);
            border-color: rgba(239,68,68,0.18);
        }

        .workspace-item-has-unread .workspace-label {
            color: #fafafa;
        }

        .workspace-item-active.workspace-item-state-busy {
            background: rgba(99,102,241,0.10);
            border-color: rgba(99,102,241,0.24);
        }

        .workspace-item-active.workspace-item-state-completed {
            background: rgba(34,197,94,0.09);
            border-color: rgba(34,197,94,0.22);
        }

        .workspace-item-active.workspace-item-state-waiting {
            background: rgba(59,130,246,0.12);
            border-color: rgba(59,130,246,0.30);
        }

        .workspace-item-active.workspace-item-state-error {
            background: rgba(239,68,68,0.10);
            border-color: rgba(239,68,68,0.24);
        }

        .workspace-close {
            background: transparent;
            color: #3f3f46;
            border-radius: 4px;
            min-width: 22px;
            min-height: 22px;
            padding: 0;
            font-size: 0.85rem;
            transition: background 120ms ease, color 120ms ease;
        }

        .workspace-close:hover {
            background: rgba(239,68,68,0.15);
            color: #ef4444;
        }

        .workspace-rename-entry {
            background: rgba(99,102,241,0.08);
            color: #e4e4e7;
            border: 1px solid rgba(99,102,241,0.30);
            border-radius: 4px;
            padding: 4px 6px;
            font-size: 0.82rem;
            font-weight: 500;
            min-height: 0;
        }

        .workspace-rename-entry:focus {
            border-color: rgba(99,102,241,0.55);
        }

        /* ── Toolbar ── */

        .toolbar {
            border-bottom: 1px solid rgba(255,255,255,0.06);
            padding: 4px 0;
        }

        .toolbar-label {
            font-weight: 600;
            font-size: 0.9rem;
            color: #fafafa;
        }

        .toolbar-separator {
            background: rgba(255,255,255,0.08);
            margin: 4px 6px;
            min-width: 1px;
        }

        .toolbar-action {
            background: rgba(255,255,255,0.04);
            color: #a1a1aa;
            border: 1px solid rgba(255,255,255,0.06);
            border-radius: 6px;
            padding: 4px 10px;
            font-size: 0.75rem;
            font-weight: 500;
            transition: background 150ms ease, color 150ms ease, border-color 150ms ease;
        }

        .toolbar-action:hover {
            background: rgba(99,102,241,0.10);
            color: #c7d2fe;
            border-color: rgba(99,102,241,0.25);
        }

        .toolbar-action:active {
            background: rgba(99,102,241,0.18);
        }

        .toolbar-action-subtle {
            background: transparent;
            border-color: transparent;
        }

        .toolbar-action-subtle:hover {
            background: rgba(255,255,255,0.06);
            border-color: rgba(255,255,255,0.08);
            color: #d4d4d8;
        }

        /* ── Attention panel ── */

        .attention-panel {
            background: #08090c;
            border-left: 1px solid rgba(255,255,255,0.06);
        }

        .activity-item-button {
            padding: 0;
        }

        .activity-item {
            background: transparent;
            border-left: 2px solid transparent;
            transition: background 120ms ease, border-color 120ms ease;
        }

        .activity-item-button:hover .activity-item {
            background: rgba(255,255,255,0.03);
        }

        .activity-item-state-busy {
            border-left-color: rgba(99,102,241,0.55);
        }

        .activity-item-state-completed {
            border-left-color: rgba(34,197,94,0.55);
        }

        .activity-item-state-waiting {
            border-left-color: rgba(59,130,246,0.70);
        }

        .activity-item-state-error {
            border-left-color: rgba(239,68,68,0.65);
        }

        .activity-meta {
            color: #71717a;
            font-size: 0.68rem;
        }

        .activity-preview {
            color: #d4d4d8;
            font-size: 0.74rem;
        }

        .activity-action {
            background: transparent;
            color: #71717a;
            border: 1px solid rgba(255,255,255,0.08);
            border-radius: 999px;
            padding: 2px 8px;
            font-size: 0.68rem;
            font-weight: 600;
            min-height: 0;
            transition: background 120ms ease, color 120ms ease, border-color 120ms ease;
        }

        .activity-action:hover {
            background: rgba(59,130,246,0.10);
            color: #dbeafe;
            border-color: rgba(59,130,246,0.25);
        }

        .activity-time {
            color: #52525b;
            font-size: 0.68rem;
        }

        /* ── Workspace windows ── */

        .workspace-window {
            background: #0b0b0f;
            border: 1px solid rgba(255,255,255,0.06);
            border-radius: 0;
        }

        .workspace-window-active {
            border-color: rgba(255,255,255,0.14);
        }

        .workspace-window-state-busy {
            border-color: rgba(99,102,241,0.22);
        }

        .workspace-window-state-completed {
            border-color: rgba(34,197,94,0.22);
        }

        .workspace-window-state-waiting {
            border-color: rgba(59,130,246,0.30);
        }

        .workspace-window-state-error {
            border-color: rgba(239,68,68,0.24);
        }

        .workspace-window-active.workspace-window-state-busy {
            border-color: rgba(99,102,241,0.38);
        }

        .workspace-window-active.workspace-window-state-completed {
            border-color: rgba(34,197,94,0.34);
        }

        .workspace-window-active.workspace-window-state-waiting {
            border-color: rgba(59,130,246,0.48);
        }

        .workspace-window-active.workspace-window-state-error {
            border-color: rgba(239,68,68,0.38);
        }

        .workspace-window-resize-handle {
            background: transparent;
            transition: background 120ms ease;
        }

        .workspace-window-resize-handle-right:hover,
        .workspace-window-resize-handle-bottom:hover {
            background: rgba(99,102,241,0.14);
        }

        /* ── Pane cards ── */

        .pane-card {
            background: transparent;
        }

        .pane-header {
            background: rgba(255,255,255,0.02);
            border-bottom: 1px solid rgba(255,255,255,0.04);
            padding: 2px 0;
            transition: background 120ms ease;
        }

        .pane-header:hover {
            background: rgba(255,255,255,0.04);
        }

        .pane-card-active .pane-header {
            background: rgba(99,102,241,0.06);
            border-bottom: 1px solid rgba(99,102,241,0.15);
        }

        .pane-card-active .pane-header:hover {
            background: rgba(99,102,241,0.10);
        }

        .pane-card-state-busy .pane-header {
            background: rgba(99,102,241,0.04);
            border-bottom-color: rgba(99,102,241,0.16);
        }

        .pane-card-state-completed .pane-header {
            background: rgba(34,197,94,0.04);
            border-bottom-color: rgba(34,197,94,0.16);
        }

        .pane-card-state-waiting .pane-header {
            background: rgba(59,130,246,0.06);
            border-bottom-color: rgba(59,130,246,0.18);
        }

        .pane-card-state-error .pane-header {
            background: rgba(239,68,68,0.05);
            border-bottom-color: rgba(239,68,68,0.16);
        }

        .pane-card-active.pane-card-state-waiting .pane-header {
            background: rgba(59,130,246,0.10);
            border-bottom-color: rgba(59,130,246,0.24);
        }

        .pane-title {
            font-weight: 500;
            color: #a1a1aa;
            font-size: 0.72rem;
        }

        .pane-agent-icon {
            margin-right: 2px;
        }

        .pane-card-active .pane-title {
            color: #e4e4e7;
        }

        .pane-close {
            background: transparent;
            color: #3f3f46;
            border-radius: 3px;
            min-width: 18px;
            min-height: 18px;
            padding: 0;
            font-size: 0.75rem;
            transition: background 120ms ease, color 120ms ease;
        }

        .pane-close:hover {
            background: rgba(239,68,68,0.15);
            color: #ef4444;
        }

        .pane-meta {
            color: #52525b;
            font-size: 0.75rem;
        }

        .surface-tabs {
            margin: 4px 8px 6px;
        }

        .surface-tab {
            background: rgba(255,255,255,0.03);
            border: 1px solid rgba(255,255,255,0.06);
            border-radius: 8px;
            padding: 2px 6px;
            transition: background 120ms ease, border-color 120ms ease;
        }

        .surface-tab-active {
            background: rgba(99,102,241,0.14);
            border-color: rgba(99,102,241,0.35);
        }

        .surface-tab-has-attention.surface-tab-state-busy {
            background: rgba(99,102,241,0.08);
            border-color: rgba(99,102,241,0.22);
        }

        .surface-tab-has-attention.surface-tab-state-completed {
            background: rgba(34,197,94,0.08);
            border-color: rgba(34,197,94,0.22);
        }

        .surface-tab-has-attention.surface-tab-state-waiting {
            background: rgba(59,130,246,0.10);
            border-color: rgba(59,130,246,0.28);
        }

        .surface-tab-has-attention.surface-tab-state-error {
            background: rgba(239,68,68,0.10);
            border-color: rgba(239,68,68,0.28);
        }

        .surface-tab-label,
        .surface-tab-close,
        .surface-tab-add {
            min-height: 0;
            padding: 0;
        }

        .surface-tab-label {
            color: #a1a1aa;
            font-size: 0.74rem;
        }

        .surface-tab-active .surface-tab-label {
            color: #fafafa;
        }

        .surface-tab-title {
            color: #a1a1aa;
            font-size: 0.74rem;
        }

        .surface-tab-active .surface-tab-title {
            color: #fafafa;
        }

        .surface-tab-close,
        .surface-tab-add {
            color: #71717a;
            border-radius: 4px;
            min-width: 18px;
            min-height: 18px;
        }

        .surface-tab-close:hover,
        .surface-tab-add:hover {
            background: rgba(255,255,255,0.06);
            color: #d4d4d8;
        }

        /* ── Status dots ── */

        .status-dot {
            font-size: 0.5rem;
        }

        .status-dot-normal { color: #3f3f46; }
        .status-dot-busy { color: #6366f1; }
        .status-dot-completed { color: #22c55e; }
        .status-dot-waiting { color: #3b82f6; }
        .status-dot-error { color: #ef4444; }

        .agent-icon {
            border-radius: 999px;
            padding: 0 5px;
            min-width: 18px;
            min-height: 16px;
            font-size: 0.58rem;
            font-weight: 700;
            letter-spacing: 0.06em;
        }

        .agent-icon-codex {
            background: rgba(59,130,246,0.16);
            color: #dbeafe;
            border: 1px solid rgba(59,130,246,0.26);
        }

        .agent-icon-claude {
            background: rgba(245,158,11,0.16);
            color: #fef3c7;
            border: 1px solid rgba(245,158,11,0.26);
        }

        .agent-icon-opencode {
            background: rgba(16,185,129,0.16);
            color: #d1fae5;
            border: 1px solid rgba(16,185,129,0.26);
        }

        .agent-icon-aider {
            background: rgba(168,85,247,0.16);
            color: #f3e8ff;
            border: 1px solid rgba(168,85,247,0.26);
        }

        .agent-icon-generic {
            background: rgba(255,255,255,0.08);
            color: #e4e4e7;
            border: 1px solid rgba(255,255,255,0.12);
        }

        .activity-agent-icon,
        .surface-tab-agent-icon {
            min-width: 16px;
            min-height: 14px;
            padding: 0 4px;
            font-size: 0.54rem;
        }

        /* ── Empty state ── */

        .empty-state {
            color: #3f3f46;
            font-size: 0.85rem;
        }

        /* ── Terminal ── */

        .terminal-output,
        .terminal-entry {
            border-radius: 0;
            background: #09090b;
            color: #e4e4e7;
            font-family: Monospace;
        }

        .terminal-output {
            padding: 4px;
        }

        .terminal-entry {
            padding: 6px 8px;
            border-top: 1px solid rgba(255,255,255,0.06);
        }

        .terminal-entry:focus {
            border-top: 1px solid rgba(99,102,241,0.4);
        }

        /* ── Popover / context menus ── */

        popover > contents {
            background: #1a1a1e;
            border: 1px solid rgba(255,255,255,0.10);
            border-radius: 8px;
            padding: 4px;
            box-shadow: 0 8px 24px rgba(0,0,0,0.4);
        }

        .context-item {
            color: #d4d4d8;
            font-size: 0.8rem;
            padding: 6px 12px;
            border-radius: 4px;
            min-height: 0;
            transition: background 100ms ease;
        }

        .context-item:hover {
            background: rgba(99,102,241,0.12);
            color: #e4e4e7;
        }

        .context-separator {
            background: rgba(255,255,255,0.06);
            margin: 4px 8px;
            min-height: 1px;
        }

        popover .destructive-action {
            color: #ef4444;
            font-size: 0.8rem;
            padding: 6px 12px;
            border-radius: 4px;
            min-height: 0;
            transition: background 100ms ease;
        }

        popover .destructive-action:hover {
            background: rgba(239,68,68,0.12);
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
