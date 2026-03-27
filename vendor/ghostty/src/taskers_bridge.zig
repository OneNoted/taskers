const std = @import("std");
const gtk = @import("gtk");
const terminal = @import("terminal/main.zig");

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

pub const Text = extern struct {
    text: ?[*:0]const u8 = null,
    text_len: usize = 0,
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
    const surface = Surface.newForApp(ptr.rt_app.app, .{
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

pub export fn taskers_ghostty_surface_has_selection(widget: ?*gtk.Widget) c_int {
    const ptr = widget orelse return 0;
    const surface: *Surface = @ptrCast(@alignCast(ptr));
    const core = surface.core() orelse return 0;
    return if (core.hasSelection()) 1 else 0;
}

pub export fn taskers_ghostty_surface_read_all_text(
    widget: ?*gtk.Widget,
    result: ?*Text,
) c_int {
    const ptr = widget orelse return 0;
    const text = result orelse return 0;
    const surface: *Surface = @ptrCast(@alignCast(ptr));
    const core = surface.core() orelse return 0;
    const screen = core.io.terminal.screens.active;
    const br = screen.pages.getBottomRight(.screen) orelse {
        text.* = .{};
        return 1;
    };
    const selection = terminal.Selection.init(
        screen.pages.getTopLeft(.screen),
        br,
        true,
    );

    var dumped = core.dumpText(state.alloc, selection) catch |err| {
        std.log.warn("failed to read Ghostty surface text err={}", .{err});
        return 0;
    };
    errdefer dumped.deinit(state.alloc);

    text.* = .{
        .text = dumped.text.ptr,
        .text_len = dumped.text.len,
    };
    return 1;
}

pub export fn taskers_ghostty_surface_free_text(text: ?*Text) void {
    const ptr = text orelse return;
    if (ptr.text) |value| {
        state.alloc.free(value[0..ptr.text_len :0]);
    }
    ptr.* = .{};
}

fn taskersSurfaceConfig(app: anytype, ptr: *const Host, opts: *const SurfaceOptions) !*Config {
    const alloc = state.alloc;
    const base = app.getConfig();
    defer base.unref();

    var cloned = try base.get().clone(alloc);
    defer cloned.deinit();

    try applyTaskersEmbeddedSurfaceInvariants(alloc, &cloned, ptr.command_argv);
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

fn applyTaskersEmbeddedSurfaceInvariants(
    _: std.mem.Allocator,
    config: *configpkg.Config,
    command_argv: []const [:0]const u8,
) !void {
    const alloc = config.arenaAlloc();

    // Embedded panes inherit the user's loaded Ghostty config and only pin
    // the handful of settings Taskers must own for layout and shell startup.
    config.command = if (command_argv.len == 0) null else command: {
        const direct = configpkg.Command{ .direct = command_argv };
        break :command try direct.clone(alloc);
    };
    config.@"shell-integration" = .none;
    config.@"shell-integration-features" = .{};
    config.@"linux-cgroup" = .never;
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

test "taskers embedded config preserves user settings beyond required invariants" {
    const testing = std.testing;
    var config = try configpkg.Config.default(testing.allocator);
    defer config.deinit();

    config.@"font-size" = 19;
    config.command = .{ .shell = try testing.allocator.dupeZ(u8, "echo from-user-config") };
    config.@"shell-integration" = .zsh;
    config.@"shell-integration-features" = .{
        .cursor = false,
        .sudo = true,
        .title = false,
        .@"ssh-env" = true,
        .@"ssh-terminfo" = true,
        .path = false,
    };
    config.@"linux-cgroup" = .always;
    config.@"window-padding-x" = .{ .top_left = 7, .bottom_right = 9 };
    config.@"window-padding-y" = .{ .top_left = 11, .bottom_right = 13 };
    config.@"window-padding-balance" = true;

    const command_argv = [_][:0]const u8{ "/opt/taskers-shell-wrapper.sh", "-i" };
    try applyTaskersEmbeddedSurfaceInvariants(testing.allocator, &config, command_argv[0..]);

    try testing.expectEqual(@as(f32, 19), config.@"font-size");
    try testing.expectEqual(configpkg.Config.ShellIntegration.none, config.@"shell-integration");
    try testing.expectEqual(configpkg.ShellIntegrationFeatures{}, config.@"shell-integration-features");
    try testing.expectEqual(configpkg.Config.LinuxCgroup.never, config.@"linux-cgroup");
    try testing.expectEqual(@as(u32, 7), config.@"window-padding-x".top_left);
    try testing.expectEqual(@as(u32, 9), config.@"window-padding-x".bottom_right);
    try testing.expectEqual(@as(u32, 11), config.@"window-padding-y".top_left);
    try testing.expectEqual(@as(u32, 13), config.@"window-padding-y".bottom_right);
    try testing.expect(config.@"window-padding-balance");

    const command = config.command orelse return error.TestUnexpectedResult;
    switch (command) {
        .direct => |argv| {
            try testing.expectEqual(@as(usize, 2), argv.len);
            try testing.expectEqualStrings("/opt/taskers-shell-wrapper.sh", argv[0]);
            try testing.expectEqualStrings("-i", argv[1]);
        },
        else => return error.TestUnexpectedResult,
    }
}

test "taskers embedded config clears user command when taskers does not provide one" {
    const testing = std.testing;
    var config = try configpkg.Config.default(testing.allocator);
    defer config.deinit();

    config.@"font-size" = 17;
    config.command = .{ .shell = try testing.allocator.dupeZ(u8, "echo from-user-config") };

    try applyTaskersEmbeddedSurfaceInvariants(testing.allocator, &config, &.{});

    try testing.expectEqual(@as(f32, 17), config.@"font-size");
    try testing.expect(config.command == null);
}
