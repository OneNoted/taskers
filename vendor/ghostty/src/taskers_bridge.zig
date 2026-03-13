const std = @import("std");
const gtk = @import("gtk");

const CoreApp = @import("App.zig");
const GtkRuntimeApp = @import("apprt/gtk/App.zig");
const Config = @import("apprt/gtk/class/config.zig").Config;
const Surface = @import("apprt/gtk/class/surface.zig").Surface;
const configpkg = @import("config.zig");
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
    command_argv: ?[*]const [*:0]const u8 = null,
    command_argc: usize = 0,
    env_entries: ?[*]const [*:0]const u8 = null,
    env_count: usize = 0,
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
    var arena = std.heap.ArenaAllocator.init(state.alloc);
    defer arena.deinit();
    const arena_alloc = arena.allocator();

    const command = command: {
        const argv = opts.command_argv orelse break :command null;
        if (opts.command_argc == 0) break :command null;

        const args = arena_alloc.alloc([:0]const u8, opts.command_argc) catch |err| {
            std.log.err("failed to allocate Ghostty command args err={}", .{err});
            return null;
        };
        for (0..opts.command_argc) |index| {
            args[index] = arena_alloc.dupeZ(u8, std.mem.span(argv[index])) catch |err| {
                std.log.err("failed to copy Ghostty command arg err={}", .{err});
                return null;
            };
        }

        break :command configpkg.Command{ .direct = args };
    };

    const surface = Surface.newForApp(ptr.rt_app.app, .{
        .command = command,
        .working_directory = if (opts.working_directory) |value| std.mem.span(value) else null,
        .title = if (opts.title) |value| std.mem.span(value) else null,
    });
    const config = taskersSurfaceConfig(ptr.rt_app.app, opts) catch |err| {
        std.log.err("failed to configure Taskers Ghostty surface err={}", .{err});
        return null;
    };
    defer config.unref();
    surface.setConfig(config);

    _ = surface.refSink();
    return surface.as(gtk.Widget);
}

pub export fn taskers_ghostty_surface_grab_focus(widget: ?*gtk.Widget) c_int {
    const ptr = widget orelse return 0;
    const surface: *Surface = @ptrCast(@alignCast(ptr));
    surface.grabFocus();
    return 1;
}

fn taskersSurfaceConfig(app: anytype, opts: *const SurfaceOptions) !*Config {
    const alloc = state.alloc;
    const base = app.getConfig();
    defer base.unref();

    var cloned = try base.get().clone(alloc);
    defer cloned.deinit();

    // Taskers should not inherit the user's standalone Ghostty shell command.
    cloned.command = null;
    cloned.@"shell-integration" = .none;
    cloned.@"shell-integration-features" = .{};
    cloned.@"linux-cgroup" = .never;
    if (opts.env_entries) |entries| {
        for (0..opts.env_count) |index| {
            try cloned.env.parseCLI(alloc, std.mem.span(entries[index]));
        }
    }

    return try Config.new(alloc, &cloned);
}
