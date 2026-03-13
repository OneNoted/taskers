const std = @import("std");
const gtk = @import("gtk");

const CoreApp = @import("App.zig");
const GtkRuntimeApp = @import("apprt/gtk/App.zig");
const Surface = @import("apprt/gtk/class/surface.zig").Surface;
const state = &@import("global.zig").state;

pub const std_options = @import("main_ghostty.zig").std_options;

var initialized = false;

pub const Host = struct {
    core_app: *CoreApp,
    rt_app: GtkRuntimeApp,
};

pub const SurfaceOptions = extern struct {
    working_directory: ?[*:0]const u8 = null,
    title: ?[*:0]const u8 = null,
};

fn ensureInitialized() !void {
    if (initialized) return;
    try state.init();
    initialized = true;
}

pub export fn taskers_ghostty_host_new() ?*Host {
    ensureInitialized() catch |err| {
        std.log.err("failed to initialize Ghostty state err={}", .{err});
        return null;
    };

    const alloc = state.alloc;
    const core_app = CoreApp.create(alloc) catch |err| {
        std.log.err("failed to create Ghostty core app err={}", .{err});
        return null;
    };
    errdefer core_app.destroy();

    const host = alloc.create(Host) catch |err| {
        std.log.err("failed to allocate bridge host err={}", .{err});
        return null;
    };
    errdefer alloc.destroy(host);

    host.* = .{
        .core_app = core_app,
        .rt_app = undefined,
    };

    host.rt_app.init(core_app, .{}) catch |err| {
        std.log.err("failed to initialize Ghostty GTK runtime err={}", .{err});
        core_app.destroy();
        alloc.destroy(host);
        return null;
    };
    return host;
}

pub export fn taskers_ghostty_host_free(host: ?*Host) void {
    const ptr = host orelse return;
    const alloc = state.alloc;
    ptr.rt_app.terminate();
    ptr.core_app.destroy();
    alloc.destroy(ptr);
}

pub export fn taskers_ghostty_host_tick(host: ?*Host) c_int {
    const ptr = host orelse return 0;
    ptr.core_app.tick(&ptr.rt_app) catch |err| {
        std.log.warn("ghostty tick failed err={}", .{err});
        return 0;
    };
    return 1;
}

pub export fn taskers_ghostty_surface_new(
    host: ?*Host,
    options: ?*const SurfaceOptions,
) ?*gtk.Widget {
    const ptr = host orelse return null;
    const opts = options orelse &SurfaceOptions{};

    const surface = Surface.newForApp(ptr.rt_app.app, .{
        .working_directory = if (opts.working_directory) |value| std.mem.span(value) else null,
        .title = if (opts.title) |value| std.mem.span(value) else null,
    });

    _ = surface.refSink();
    return surface.as(gtk.Widget);
}

pub export fn taskers_ghostty_surface_grab_focus(widget: ?*gtk.Widget) c_int {
    const ptr = widget orelse return 0;
    const surface: *Surface = @ptrCast(@alignCast(ptr));
    surface.grabFocus();
    return 1;
}
