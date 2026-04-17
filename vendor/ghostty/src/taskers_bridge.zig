const bridge = @import("ghostty_gtk_bridge.zig");

pub const std_options = bridge.std_options;

comptime {
    @export(&bridge.ghostty_gtk_host_new, .{ .name = "taskers_ghostty_host_new" });
    @export(&bridge.ghostty_gtk_host_free, .{ .name = "taskers_ghostty_host_free" });
    @export(&bridge.ghostty_gtk_host_version, .{ .name = "taskers_ghostty_host_version" });
    @export(&bridge.ghostty_gtk_host_build_id, .{ .name = "taskers_ghostty_host_build_id" });
    @export(&bridge.ghostty_gtk_host_begin_shutdown, .{ .name = "taskers_ghostty_host_begin_shutdown" });
    @export(&bridge.ghostty_gtk_host_surface_count, .{ .name = "taskers_ghostty_host_surface_count" });
    @export(&bridge.ghostty_gtk_host_tick, .{ .name = "taskers_ghostty_host_tick" });
    @export(&bridge.ghostty_gtk_surface_new, .{ .name = "taskers_ghostty_surface_new" });
    @export(&bridge.ghostty_gtk_surface_destroy, .{ .name = "taskers_ghostty_surface_destroy" });
    @export(&bridge.ghostty_gtk_surface_grab_focus, .{ .name = "taskers_ghostty_surface_grab_focus" });
    @export(&bridge.ghostty_gtk_surface_has_selection, .{ .name = "taskers_ghostty_surface_has_selection" });
    @export(&bridge.ghostty_gtk_surface_send_text, .{ .name = "taskers_ghostty_surface_send_text" });
    @export(&bridge.ghostty_gtk_surface_read_all_text, .{ .name = "taskers_ghostty_surface_read_all_text" });
    @export(&bridge.ghostty_gtk_surface_free_text, .{ .name = "taskers_ghostty_surface_free_text" });
}
