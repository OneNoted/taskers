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
    command_argv: []const [:0]u8,
    env_entries: []const [:0]u8,
};

pub const HostOptions = extern struct {
    command_argv: ?[*]const [*:0]const u8 = null,
    command_argc: usize = 0,
    env_entries: ?[*]const [*:0]const u8 = null,
    env_count: usize = 0,
};

pub const SurfaceOptions = extern struct {
    working_directory: ?[*:0]const u8 = null,
    title: ?[*:0]const u8 = null,
    env_entries: ?[*]const [*:0]const u8 = null,
    env_count: usize = 0,
};

fn ensureInitialized() !void {
    if (initialized) return;
    try state.init();
    initialized = true;
}

pub export fn taskers_ghostty_host_new(options: ?*const HostOptions) ?*Host {
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

    const opts = options orelse &HostOptions{};
    const command_argv = duplicateStringList(alloc, opts.command_argv, opts.command_argc) catch |err| {
        std.log.err("failed to copy Ghostty host command args err={}", .{err});
        core_app.destroy();
        alloc.destroy(host);
        return null;
    };
    errdefer freeStringList(alloc, command_argv);

    const env_entries = duplicateStringList(alloc, opts.env_entries, opts.env_count) catch |err| {
        std.log.err("failed to copy Ghostty host env entries err={}", .{err});
        freeStringList(alloc, command_argv);
        core_app.destroy();
        alloc.destroy(host);
        return null;
    };
    errdefer freeStringList(alloc, env_entries);

    host.* = .{
        .core_app = core_app,
        .rt_app = undefined,
        .command_argv = command_argv,
        .env_entries = env_entries,
    };

    host.rt_app.init(core_app, .{}) catch |err| {
        std.log.err("failed to initialize Ghostty GTK runtime err={}", .{err});
        freeStringList(alloc, env_entries);
        freeStringList(alloc, command_argv);
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
    freeStringList(alloc, ptr.env_entries);
    freeStringList(alloc, ptr.command_argv);
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
    const command = command: {
        if (ptr.command_argv.len == 0) break :command null;
        break :command configpkg.Command{ .direct = ptr.command_argv };
    };

    const surface = Surface.newForApp(ptr.rt_app.app, .{
        .command = command,
        .working_directory = if (opts.working_directory) |value| std.mem.span(value) else null,
        .title = if (opts.title) |value| std.mem.span(value) else null,
    });
    const config = taskersSurfaceConfig(ptr.rt_app.app, ptr, opts) catch |err| {
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

fn taskersSurfaceConfig(app: anytype, ptr: *const Host, opts: *const SurfaceOptions) !*Config {
    const alloc = state.alloc;
    const base = app.getConfig();
    defer base.unref();

    var cloned = try base.get().clone(alloc);
    defer cloned.deinit();

    cloned.command = null;
    cloned.@"shell-integration" = .none;
    cloned.@"shell-integration-features" = .{};
    cloned.@"linux-cgroup" = .never;
    for (ptr.env_entries) |entry| {
        try cloned.env.parseCLI(alloc, entry);
    }
    if (opts.env_entries) |entries| {
        for (0..opts.env_count) |index| {
            try cloned.env.parseCLI(alloc, std.mem.span(entries[index]));
        }
    }

    return try Config.new(alloc, &cloned);
}

fn duplicateStringList(
    alloc: std.mem.Allocator,
    entries_ptr: ?[*]const [*:0]const u8,
    count: usize,
) ![]const [:0]u8 {
    var entries = try alloc.alloc([:0]u8, count);
    errdefer {
        for (entries) |entry| alloc.free(entry);
        alloc.free(entries);
    }

    if (entries_ptr) |source| {
        for (0..count) |index| {
            entries[index] = try alloc.dupeZ(u8, std.mem.span(source[index]));
        }
    }

    return entries;
}

fn freeStringList(alloc: std.mem.Allocator, entries: []const [:0]u8) void {
    for (entries) |entry| alloc.free(entry);
    alloc.free(entries);
}
