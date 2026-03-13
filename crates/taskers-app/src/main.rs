mod app_state;
mod pane_runtime;
mod session_store;

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    future::pending,
    path::PathBuf,
    rc::Rc,
    thread,
    time::Duration,
};

use adw::prelude::*;
use app_state::AppState;
use clap::Parser;
use gtk::{
    Align, Box as GtkBox, Button, CssProvider, Entry, Label, Orientation, Paned, PolicyType,
    STYLE_PROVIDER_PRIORITY_APPLICATION, ScrolledWindow, TextView, Widget, WrapMode, gdk, glib,
};
use pane_runtime::PaneRuntimeSnapshot;
use taskers_control::{
    ControlCommand, InMemoryController, bind_socket, default_socket_path, serve,
};
use taskers_domain::{
    AppModel, AttentionState, LayoutNode, PaneMetadataPatch, PaneRecord, SignalEvent, SignalKind,
    Workspace,
};
use taskers_ghostty::{
    BackendChoice, BackendProbe, DefaultBackend, GhosttyHost, SurfaceDescriptor, TerminalBackend,
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
}

struct StartupContext {
    app_state: AppState,
    backend_choice: BackendChoice,
    ghostty_host: Option<GhosttyHost>,
    startup_toast: Option<String>,
}

struct UiHandle {
    app_state: AppState,
    backend_choice: BackendChoice,
    window: adw::ApplicationWindow,
    overlay: adw::ToastOverlay,
    ghostty_host: Option<GhosttyHost>,
    ghostty_surfaces: RefCell<HashMap<taskers_domain::PaneId, Widget>>,
    shell: RefCell<Option<ShellWidgets>>,
    pane_cards: RefCell<HashMap<taskers_domain::PaneId, PaneCardWidgets>>,
    layout_state: RefCell<LayoutRenderState>,
    last_rendered: RefCell<Option<AppModel>>,
}

#[derive(Clone)]
struct ShellWidgets {
    root: Paned,
    sidebar_list: GtkBox,
    toolbar_label: Label,
    split_vertical_button: Button,
    split_horizontal_button: Button,
    layout_host: GtkBox,
}

#[derive(Clone)]
struct PaneCardWidgets {
    root: GtkBox,
    title: Label,
    status_dot: Label,
    terminal_host: GtkBox,
}

#[derive(Clone, Default, PartialEq, Eq)]
struct LayoutRenderState {
    workspace_id: Option<taskers_domain::WorkspaceId>,
    layout: Option<LayoutNode>,
}

impl UiHandle {
    fn new(
        app_state: AppState,
        backend_choice: BackendChoice,
        window: adw::ApplicationWindow,
        overlay: adw::ToastOverlay,
        ghostty_host: Option<GhosttyHost>,
    ) -> Rc<Self> {
        Rc::new(Self {
            app_state,
            backend_choice,
            window,
            overlay,
            ghostty_host,
            ghostty_surfaces: RefCell::new(HashMap::new()),
            shell: RefCell::new(None),
            pane_cards: RefCell::new(HashMap::new()),
            layout_state: RefCell::new(LayoutRenderState::default()),
            last_rendered: RefCell::new(None),
        })
    }

    fn refresh(self: &Rc<Self>, force: bool) {
        let model = self.app_state.snapshot_model();
        if let Err(error) = self.app_state.runtime().sync_model(&model) {
            self.toast(&error.to_string());
        }
        if !force
            && self
                .last_rendered
                .borrow()
                .as_ref()
                .is_some_and(|existing| existing == &model)
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

    fn send_input(self: &Rc<Self>, pane_id: taskers_domain::PaneId, input: String) {
        if let Err(error) = self.app_state.runtime().send_input(pane_id, &input) {
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

        if let Some(widget) = self.ghostty_surfaces.borrow().get(&pane.id) {
            detach_widget(widget);
            return Some(widget.clone());
        }

        let host = self.ghostty_host.as_ref()?;
        let widget = match host.create_surface(&SurfaceDescriptor {
            cols: 120,
            rows: 40,
            cwd: pane.metadata.cwd.clone(),
            title: pane.metadata.title.clone(),
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
        connect_ghostty_widget(self, workspace_id, pane.id, &widget);
        self.ghostty_surfaces
            .borrow_mut()
            .insert(pane.id, widget.clone());
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
        let live: HashSet<taskers_domain::PaneId> = model
            .workspaces
            .values()
            .flat_map(|ws| ws.panes.keys().copied())
            .collect();
        self.pane_cards.borrow_mut().retain(|id, _| live.contains(id));
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
        update_toolbar(&shell, model);
        update_layout(self, &shell, model);
    }

    fn pane_card(
        self: &Rc<Self>,
        workspace_id: taskers_domain::WorkspaceId,
        pane: &PaneRecord,
    ) -> PaneCardWidgets {
        if let Some(card) = self.pane_cards.borrow().get(&pane.id) {
            return card.clone();
        }

        let root = GtkBox::new(Orientation::Vertical, 0);
        root.add_css_class("pane-card");

        let header = GtkBox::new(Orientation::Horizontal, 6);
        header.add_css_class("pane-header");
        header.set_margin_start(6);
        header.set_margin_end(4);
        header.set_margin_top(2);
        header.set_margin_bottom(2);

        let title = Label::new(Some("Unnamed terminal pane"));
        title.add_css_class("pane-title");
        title.set_xalign(0.0);
        title.set_hexpand(true);
        header.append(&title);

        let status_dot = Label::new(Some("\u{25cf}"));
        status_dot.add_css_class("status-dot");
        status_dot.add_css_class(&attention_dot_class(pane.attention));
        status_dot.set_tooltip_text(Some(pane.attention.label()));
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

        root.append(&header);

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
            root,
            title,
            status_dot,
            terminal_host,
        };

        initialize_terminal_body(self, workspace_id, pane, &card);
        self.pane_cards.borrow_mut().insert(pane.id, card.clone());
        card
    }

    fn sync_pane_card(
        self: &Rc<Self>,
        workspace_id: taskers_domain::WorkspaceId,
        active_pane: taskers_domain::PaneId,
        pane: &PaneRecord,
    ) {
        let card = self.pane_card(workspace_id, pane);
        let snapshot = self.app_state.runtime().snapshot(pane.id);

        let display_title = pane
            .metadata
            .title
            .as_deref()
            .unwrap_or("Unnamed terminal pane");
        card.title.set_text(display_title);
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
        card.status_dot
            .add_css_class(&attention_dot_class(pane.attention));
        card.status_dot
            .set_tooltip_text(Some(pane.attention.label()));
    }
}

fn main() -> gtk::glib::ExitCode {
    let cli = Cli::parse();
    let socket_path = cli.socket.unwrap_or_else(default_socket_path);
    let session_path = cli
        .session
        .unwrap_or_else(session_store::default_session_path);
    let probe = DefaultBackend::probe(BackendChoice::Auto);
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
    let (backend_choice, _backend_note, ghostty_host, startup_toast) =
        initialize_terminal_backend(&probe);
    let app_state = match AppState::new(initial_model, session_path, backend_choice) {
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
        ghostty_host,
        startup_toast,
    };

    let app = adw::Application::builder()
        .application_id("dev.taskers.app")
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
        window.clone(),
        overlay,
        startup.ghostty_host,
    );
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
}

fn initialize_terminal_backend(
    probe: &BackendProbe,
) -> (BackendChoice, String, Option<GhosttyHost>, Option<String>) {
    if probe.selected != BackendChoice::Ghostty {
        return (BackendChoice::Mock, probe.notes.clone(), None, None);
    }

    match GhosttyHost::new() {
        Ok(host) => (
            BackendChoice::Ghostty,
            probe.notes.clone(),
            Some(host),
            None,
        ),
        Err(error) => {
            let note = format!(
                "{} Falling back to placeholder terminal surfaces.",
                error
            );
            let toast = format!("Ghostty backend unavailable: {error}");
            (BackendChoice::Mock, note, None, Some(toast))
        }
    }
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
    sidebar_scroll.set_size_request(180, -1);
    shell.set_start_child(Some(&sidebar_scroll));
    shell.set_resize_start_child(false);

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

    let split_vertical_button = Button::with_label("Split V");
    split_vertical_button.add_css_class("toolbar-action");
    split_vertical_button.set_tooltip_text(Some("Split pane vertically"));
    let split_vertical_ui = Rc::clone(ui);
    split_vertical_button.connect_clicked(move |_| {
        let model = split_vertical_ui.app_state.snapshot_model();
        if let Some(workspace) = model.active_workspace() {
            split_vertical_ui.dispatch(ControlCommand::SplitPane {
                workspace_id: workspace.id,
                pane_id: Some(workspace.active_pane),
                axis: taskers_domain::SplitAxis::Vertical,
            });
        }
    });
    toolbar.append(&split_vertical_button);

    let split_horizontal_button = Button::with_label("Split H");
    split_horizontal_button.add_css_class("toolbar-action");
    split_horizontal_button.set_tooltip_text(Some("Split pane horizontally"));
    let split_horizontal_ui = Rc::clone(ui);
    split_horizontal_button.connect_clicked(move |_| {
        let model = split_horizontal_ui.app_state.snapshot_model();
        if let Some(workspace) = model.active_workspace() {
            split_horizontal_ui.dispatch(ControlCommand::SplitPane {
                workspace_id: workspace.id,
                pane_id: Some(workspace.active_pane),
                axis: taskers_domain::SplitAxis::Horizontal,
            });
        }
    });
    toolbar.append(&split_horizontal_button);

    main_column.append(&toolbar);

    let layout_host = GtkBox::new(Orientation::Vertical, 0);
    layout_host.set_hexpand(true);
    layout_host.set_vexpand(true);
    main_column.append(&layout_host);

    shell.set_end_child(Some(&main_column));

    ShellWidgets {
        root: shell,
        sidebar_list,
        toolbar_label,
        split_vertical_button,
        split_horizontal_button,
        layout_host,
    }
}

fn update_sidebar(ui: &Rc<UiHandle>, shell: &ShellWidgets, model: &AppModel) {
    clear_box(&shell.sidebar_list);

    if let Ok(summaries) = model.workspace_summaries(model.active_window) {
        for summary in summaries {
            let outer = GtkBox::new(Orientation::Horizontal, 0);

            let button = Button::new();
            button.add_css_class("flat");
            button.add_css_class("workspace-button");
            button.set_hexpand(true);

            let row = GtkBox::new(Orientation::Horizontal, 6);
            row.add_css_class("workspace-item");
            row.set_margin_start(6);
            row.set_margin_end(4);
            row.set_margin_top(3);
            row.set_margin_bottom(3);

            if model.active_workspace_id() == Some(summary.workspace_id) {
                row.add_css_class("workspace-item-active");
            }

            let dot = Label::new(Some("\u{25cf}"));
            dot.add_css_class("status-dot");
            dot.add_css_class(&attention_dot_class(summary.highest_attention));
            row.append(&dot);

            let label = Label::new(Some(&summary.label));
            label.add_css_class("workspace-label");
            label.set_xalign(0.0);
            label.set_hexpand(true);
            row.append(&label);

            let pane_count: usize = summary.counts_by_attention.values().sum();
            if pane_count > 1 {
                let count_label = Label::new(Some(&format!("{pane_count}")));
                count_label.add_css_class("pane-count");
                row.append(&count_label);
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

            // Right-click context menu
            let menu_ui = Rc::clone(ui);
            let menu_ws_id = summary.workspace_id;
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

fn update_toolbar(shell: &ShellWidgets, model: &AppModel) {
    if let Some(workspace) = model.active_workspace() {
        shell.toolbar_label.set_text(&workspace.label);
        shell.split_vertical_button.set_sensitive(true);
        shell.split_horizontal_button.set_sensitive(true);
    } else {
        shell.toolbar_label.set_text("No workspace");
        shell.split_vertical_button.set_sensitive(false);
        shell.split_horizontal_button.set_sensitive(false);
    }
}

fn update_layout(ui: &Rc<UiHandle>, shell: &ShellWidgets, model: &AppModel) {
    let next_state = LayoutRenderState {
        workspace_id: model.active_workspace_id(),
        layout: model
            .active_workspace()
            .map(|workspace| workspace.layout.clone()),
    };
    let needs_rebuild = *ui.layout_state.borrow() != next_state;

    if needs_rebuild {
        // Build new layout FIRST — build_layout_widget detaches surviving pane
        // cards from the old Paned tree individually.  Dead pane cards stay in
        // the old tree; their Ghostty surfaces remain referenced in the cache
        // (cleanup runs later) so finalization of the old tree does not destroy
        // live GL resources.
        let new_content: Widget = if let Some(workspace) = model.active_workspace() {
            build_layout_widget(ui, workspace, &workspace.layout)
        } else {
            let empty = Label::new(Some("No workspace selected"));
            empty.add_css_class("empty-state");
            empty.set_xalign(0.5);
            empty.set_yalign(0.5);
            empty.set_hexpand(true);
            empty.set_vexpand(true);
            empty.upcast()
        };
        clear_box(&shell.layout_host);
        shell.layout_host.append(&new_content);
        *ui.layout_state.borrow_mut() = next_state;
    }

    if let Some(workspace) = model.active_workspace() {
        for pane in workspace.panes.values() {
            ui.sync_pane_card(workspace.id, workspace.active_pane, pane);
        }
    }
}

fn build_layout_widget(
    ui: &Rc<UiHandle>,
    workspace: &Workspace,
    node: &LayoutNode,
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
            first,
            second,
            ..
        } => {
            let paned = Paned::builder()
                .orientation(match axis {
                    taskers_domain::SplitAxis::Horizontal => Orientation::Horizontal,
                    taskers_domain::SplitAxis::Vertical => Orientation::Vertical,
                })
                .wide_handle(false)
                .build();
            paned.set_start_child(Some(&build_layout_widget(ui, workspace, first)));
            paned.set_end_child(Some(&build_layout_widget(ui, workspace, second)));
            paned.upcast()
        }
    }
}

fn initialize_terminal_body(
    ui: &Rc<UiHandle>,
    workspace_id: taskers_domain::WorkspaceId,
    pane: &PaneRecord,
    card: &PaneCardWidgets,
) {
    if let Some(widget) = ui.terminal_widget(workspace_id, pane) {
        card.terminal_host.append(&widget);
        return;
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
        return;
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
        .snapshot(pane.id)
        .map(|snapshot| snapshot.output)
        .unwrap_or_default();
    buffer.set_text(&initial_output);
    scroller.set_child(Some(&terminal_output));
    bind_output_updates(ui, pane.id, terminal_output);
    root.append(&scroller);

    let entry = Entry::new();
    entry.add_css_class("terminal-entry");
    entry.set_placeholder_text(Some("Type shell input and press Enter"));
    let input_ui = Rc::clone(ui);
    let pane_id = pane.id;
    entry.connect_activate(move |entry| {
        let text = entry.text();
        if text.is_empty() {
            return;
        }

        input_ui.send_input(pane_id, format!("{text}\n"));
        entry.set_text("");
    });
    root.append(&entry);

    card.terminal_host.append(&root);
}

fn clear_box(container: &GtkBox) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
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

fn bind_output_updates(ui: &Rc<UiHandle>, pane_id: taskers_domain::PaneId, text_view: TextView) {
    let runtime = ui.app_state.runtime();
    let buffer = text_view.buffer();
    let last_seen = Rc::new(RefCell::new(String::new()));
    let last_seen_for_timer = Rc::clone(&last_seen);

    gtk::glib::timeout_add_local(Duration::from_millis(150), move || {
        let Some(snapshot) = runtime.snapshot(pane_id) else {
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
    widget: &Widget,
) {
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
            .and_then(|pane| pane.metadata.title.clone());
        if current == title {
            return;
        }
        title_ui.dispatch(ControlCommand::UpdatePaneMetadata {
            pane_id,
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
            .and_then(|pane| pane.metadata.cwd.clone());
        if current == cwd {
            return;
        }
        pwd_ui.dispatch(ControlCommand::UpdatePaneMetadata {
            pane_id,
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

    let bell_ui = Rc::clone(ui);
    widget.connect_notify_local(Some("bell-ringing"), move |widget, _| {
        if widget.property::<bool>("bell-ringing") {
            bell_ui.dispatch(ControlCommand::EmitSignal {
                workspace_id,
                pane_id,
                event: SignalEvent::new(
                    "ghostty",
                    SignalKind::Notification,
                    Some("Terminal requested attention".into()),
                ),
            });
        }
    });

    let exit_ui = Rc::clone(ui);
    widget.connect_notify_local(Some("child-exited"), move |widget, _| {
        if widget.property::<bool>("child-exited") {
            exit_ui.dispatch(ControlCommand::ClosePane {
                workspace_id,
                pane_id,
            });
        }
    });
}

fn detach_widget(widget: &Widget) {
    if widget.parent().is_none() {
        return;
    }

    unsafe {
        gtk::ffi::gtk_widget_unparent(widget.as_ptr());
    }
}

fn format_pane_meta(pane: &PaneRecord, snapshot: Option<&PaneRuntimeSnapshot>) -> String {
    let cwd = pane.metadata.cwd.as_deref().unwrap_or("cwd unknown");
    let branch = pane
        .metadata
        .git_branch
        .as_deref()
        .unwrap_or("no branch");
    let agent = pane.metadata.agent_kind.as_deref().unwrap_or("shell");
    let ports = if pane.metadata.ports.is_empty() {
        "no ports".into()
    } else {
        pane.metadata
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

    format!(
        "{agent}  \u{2022}  {cwd}  \u{2022}  {branch}  \u{2022}  {ports}  \u{2022}  {process}"
    )
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
        }

        /* ── Sidebar ── */

        .workspace-sidebar {
            background: #09090b;
            border-right: 1px solid rgba(255,255,255,0.06);
        }

        .sidebar-heading {
            font-weight: 600;
            font-size: 0.8rem;
            color: #71717a;
        }

        .workspace-add {
            background: rgba(99,102,241,0.12);
            color: #a5b4fc;
            border-radius: 6px;
            min-width: 24px;
            min-height: 24px;
            padding: 0;
            font-size: 1rem;
        }

        .workspace-add:hover {
            background: rgba(99,102,241,0.22);
        }

        .workspace-button {
            padding: 0;
        }

        .workspace-button:hover .workspace-item {
            background: rgba(255,255,255,0.04);
        }

        .workspace-item {
            padding: 6px;
            border-radius: 4px;
            border-left: 2px solid transparent;
        }

        .workspace-item-active {
            background: rgba(99,102,241,0.08);
            border-left: 2px solid #6366f1;
        }

        .workspace-label {
            font-weight: 500;
            color: #d4d4d8;
            font-size: 0.82rem;
        }

        .pane-count {
            background: rgba(255,255,255,0.06);
            color: #52525b;
            font-size: 0.7rem;
            border-radius: 3px;
            padding: 1px 5px;
            min-height: 0;
        }

        .workspace-close {
            background: transparent;
            color: #3f3f46;
            border-radius: 4px;
            min-width: 22px;
            min-height: 22px;
            padding: 0;
            font-size: 0.85rem;
        }

        .workspace-close:hover {
            background: rgba(239,68,68,0.15);
            color: #ef4444;
        }

        /* ── Toolbar ── */

        .toolbar {
            border-bottom: 1px solid rgba(255,255,255,0.06);
        }

        .toolbar-label {
            font-weight: 600;
            font-size: 0.9rem;
            color: #fafafa;
        }

        .toolbar-action {
            background: rgba(255,255,255,0.06);
            color: #71717a;
            border-radius: 4px;
            padding: 2px 8px;
            font-size: 0.75rem;
        }

        .toolbar-action:hover {
            background: rgba(255,255,255,0.10);
            color: #a1a1aa;
        }

        /* ── Pane cards ── */

        .pane-card {
            background: transparent;
        }

        .pane-header {
            background: rgba(255,255,255,0.02);
            border-bottom: 1px solid rgba(255,255,255,0.04);
        }

        .pane-card-active .pane-header {
            background: rgba(99,102,241,0.06);
            border-bottom: 1px solid rgba(99,102,241,0.15);
        }

        .pane-title {
            font-weight: 500;
            color: #a1a1aa;
            font-size: 0.78rem;
        }

        .pane-card-active .pane-title {
            color: #e4e4e7;
        }

        .pane-close {
            background: transparent;
            color: #3f3f46;
            border-radius: 3px;
            min-width: 20px;
            min-height: 20px;
            padding: 0;
            font-size: 0.8rem;
        }

        .pane-close:hover {
            background: rgba(239,68,68,0.15);
            color: #ef4444;
        }

        .pane-meta {
            color: #52525b;
            font-size: 0.75rem;
        }

        /* ── Status dots ── */

        .status-dot {
            font-size: 0.5rem;
        }

        .status-dot-normal { color: #3f3f46; }
        .status-dot-busy { color: #6366f1; }
        .status-dot-completed { color: #22c55e; }
        .status-dot-waiting { color: #f59e0b; }
        .status-dot-error { color: #ef4444; }

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

        /* ── Popover menus ── */

        popover > contents {
            background: #18181b;
            border: 1px solid rgba(255,255,255,0.08);
            border-radius: 6px;
        }

        popover .destructive-action {
            color: #ef4444;
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
