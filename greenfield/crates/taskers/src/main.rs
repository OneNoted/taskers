use dioxus::LaunchBuilder;
use dioxus_desktop::{
    Config, WindowBuilder,
    tao::{
        dpi::LogicalSize,
        event::{Event, WindowEvent},
    },
};
use std::collections::BTreeMap;
use std::sync::Arc;
use taskers_core::{
    BootstrapModel, PixelSize, RuntimeCapability, RuntimeStatus, SharedCore, TerminalDefaults,
};
use taskers_paths::default_ghostty_runtime_dir;
use taskers_runtime::{ShellLaunchSpec, install_shell_integration, scrub_inherited_terminal_env};

fn main() {
    scrub_inherited_terminal_env();

    let (terminal_defaults, runtime_status) = bootstrap_runtime();
    let core = SharedCore::bootstrap(BootstrapModel {
        runtime_status,
        terminal_defaults,
    });
    let core_for_window = core.clone();
    let core_for_events = core.clone();

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

                    if let Err(error) = taskers_host::attach_window(window.clone(), event_sink) {
                        eprintln!("taskers host attach failed: {error}");
                    }
                    if let Err(error) = taskers_host::sync_snapshot(&core_for_window.snapshot()) {
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
                        if let Err(error) = taskers_host::sync_snapshot(&core_for_events.snapshot())
                        {
                            eprintln!("taskers host resize sync failed: {error}");
                        }
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
