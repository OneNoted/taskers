use dioxus::LaunchBuilder;
use dioxus_desktop::{
    Config, WindowBuilder,
    tao::{
        dpi::LogicalSize,
        event::{Event, WindowEvent},
    },
};
use taskers_core::{PixelSize, SharedCore};

fn main() {
    let core = SharedCore::demo();
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

                    if let Err(error) = taskers_host::attach_window(window.clone()) {
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
