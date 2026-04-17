const gtk = @import("gtk");
const bridge = @import("ghostty_gtk_bridge.zig");

pub const std_options = bridge.std_options;

pub export fn taskers_ghostty_host_new(options: ?*const bridge.HostOptions) ?*bridge.Host {
    return bridge.ghostty_gtk_host_new(options);
}

pub export fn taskers_ghostty_host_free(host: ?*bridge.Host) void {
    bridge.ghostty_gtk_host_free(host);
}

pub export fn taskers_ghostty_host_version() [*:0]const u8 {
    return bridge.ghostty_gtk_host_version();
}

pub export fn taskers_ghostty_host_build_id() [*:0]const u8 {
    return bridge.ghostty_gtk_host_build_id();
}

pub export fn taskers_ghostty_host_begin_shutdown(host: ?*bridge.Host) void {
    bridge.ghostty_gtk_host_begin_shutdown(host);
}

pub export fn taskers_ghostty_host_surface_count(host: ?*bridge.Host) usize {
    return bridge.ghostty_gtk_host_surface_count(host);
}

pub export fn taskers_ghostty_host_tick(host: ?*bridge.Host) c_int {
    return bridge.ghostty_gtk_host_tick(host);
}

pub export fn taskers_ghostty_surface_new(
    host: ?*bridge.Host,
    options: ?*const bridge.SurfaceOptions,
) ?*gtk.Widget {
    return bridge.ghostty_gtk_surface_new(host, options);
}

pub export fn taskers_ghostty_surface_destroy(widget: ?*gtk.Widget) void {
    bridge.ghostty_gtk_surface_destroy(widget);
}

pub export fn taskers_ghostty_surface_grab_focus(widget: ?*gtk.Widget) c_int {
    return bridge.ghostty_gtk_surface_grab_focus(widget);
}

pub export fn taskers_ghostty_surface_has_selection(widget: ?*gtk.Widget) c_int {
    return bridge.ghostty_gtk_surface_has_selection(widget);
}

pub export fn taskers_ghostty_surface_send_text(
    widget: ?*gtk.Widget,
    ptr: ?[*]const u8,
    len: usize,
) c_int {
    return bridge.ghostty_gtk_surface_send_text(widget, ptr, len);
}

pub export fn taskers_ghostty_surface_read_all_text(
    widget: ?*gtk.Widget,
    result: ?*bridge.Text,
) c_int {
    return bridge.ghostty_gtk_surface_read_all_text(widget, result);
}

pub export fn taskers_ghostty_surface_free_text(text: ?*bridge.Text) void {
    bridge.ghostty_gtk_surface_free_text(text);
}
