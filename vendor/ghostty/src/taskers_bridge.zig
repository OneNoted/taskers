const std = @import("std");
const gtk = @import("gtk");
const terminal = @import("terminal/main.zig");

const CoreApp = @import("App.zig");
const GtkRuntimeApp = @import("apprt/gtk/App.zig");
const Config = @import("apprt/gtk/class/config.zig").Config;
const Surface = @import("apprt/gtk/class/surface.zig").Surface;
const configpkg = @import("config.zig");
const state = &@import("global.zig").state;
const build_info = @import("taskers_bridge_build_info.zig");

pub const std_options = @import("main_ghostty.zig").std_options;

var initialized = false;
const vendor_version = build_info.version;
const vendor_fingerprint = build_info.fingerprint;
const vendor_version_z = cString(vendor_version);
const bridge_build_id_z = cString(std.fmt.comptimePrint(
    "ghostty-{s}-{s}",
    .{ vendor_version, vendor_fingerprint },
));

pub const Host = struct {
    core_app: *CoreApp,
    rt_app: GtkRuntimeApp,
    command_argv: []const [:0]u8,
    env_entries: []const [:0]u8,
    base_config_path: ?[:0]u8,
    override_config_path: ?[:0]u8,
    config_diagnostics_logged: bool = false,
    shutting_down: bool = false,
};

pub const HostOptions = extern struct {
    command_argv: ?[*]const [*:0]const u8 = null,
    command_argc: usize = 0,
    env_entries: ?[*]const [*:0]const u8 = null,
    env_count: usize = 0,
    base_config_path: ?[*:0]const u8 = null,
    override_config_path: ?[*:0]const u8 = null,
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

    const base_config_path = duplicateOptionalString(alloc, opts.base_config_path) catch |err| {
        std.log.err("failed to copy Ghostty base config path err={}", .{err});
        freeStringList(alloc, env_entries);
        freeStringList(alloc, command_argv);
        core_app.destroy();
        alloc.destroy(host);
        return null;
    };
    errdefer freeOptionalString(alloc, base_config_path);

    const override_config_path = duplicateOptionalString(alloc, opts.override_config_path) catch |err| {
        std.log.err("failed to copy Ghostty override config path err={}", .{err});
        freeOptionalString(alloc, base_config_path);
        freeStringList(alloc, env_entries);
        freeStringList(alloc, command_argv);
        core_app.destroy();
        alloc.destroy(host);
        return null;
    };
    errdefer freeOptionalString(alloc, override_config_path);

    host.* = .{
        .core_app = core_app,
        .rt_app = undefined,
        .command_argv = command_argv,
        .env_entries = env_entries,
        .base_config_path = base_config_path,
        .override_config_path = override_config_path,
        .config_diagnostics_logged = false,
        .shutting_down = false,
    };
    std.log.info("Taskers embedded terminal base config={?s}", .{host.base_config_path});
    std.log.info("Taskers embedded terminal override config={?s}", .{host.override_config_path});

    host.rt_app.init(core_app, .{}) catch |err| {
        std.log.err("failed to initialize Ghostty GTK runtime err={}", .{err});
        freeOptionalString(alloc, override_config_path);
        freeOptionalString(alloc, base_config_path);
        freeStringList(alloc, env_entries);
        freeStringList(alloc, command_argv);
        core_app.destroy();
        alloc.destroy(host);
        return null;
    };
    logConfigDiagnostics(host);
    return host;
}

pub export fn ghostty_gtk_host_new(options: ?*const HostOptions) ?*Host {
    return taskers_ghostty_host_new(options);
}

pub export fn taskers_ghostty_host_free(host: ?*Host) void {
    const ptr = host orelse return;
    const alloc = state.alloc;
    ptr.shutting_down = true;
    ptr.rt_app.terminate();
    freeOptionalString(alloc, ptr.override_config_path);
    freeOptionalString(alloc, ptr.base_config_path);
    freeStringList(alloc, ptr.env_entries);
    freeStringList(alloc, ptr.command_argv);
    ptr.core_app.destroy();
    alloc.destroy(ptr);
}

pub export fn ghostty_gtk_host_free(host: ?*Host) void {
    taskers_ghostty_host_free(host);
}

pub export fn taskers_ghostty_host_version() [*:0]const u8 {
    return &vendor_version_z;
}

pub export fn ghostty_gtk_host_version() [*:0]const u8 {
    return taskers_ghostty_host_version();
}

pub export fn taskers_ghostty_host_build_id() [*:0]const u8 {
    return &bridge_build_id_z;
}

pub export fn ghostty_gtk_host_build_id() [*:0]const u8 {
    return taskers_ghostty_host_build_id();
}

pub export fn taskers_ghostty_host_begin_shutdown(host: ?*Host) void {
    const ptr = host orelse return;
    ptr.shutting_down = true;
}

pub export fn ghostty_gtk_host_begin_shutdown(host: ?*Host) void {
    taskers_ghostty_host_begin_shutdown(host);
}

pub export fn taskers_ghostty_host_surface_count(host: ?*Host) usize {
    const ptr = host orelse return 0;
    return ptr.core_app.surfaces.items.len;
}

pub export fn ghostty_gtk_host_surface_count(host: ?*Host) usize {
    return taskers_ghostty_host_surface_count(host);
}

pub export fn taskers_ghostty_host_tick(host: ?*Host) c_int {
    const ptr = host orelse return 0;
    if (ptr.shutting_down) return 1;
    ptr.core_app.tick(&ptr.rt_app) catch |err| {
        std.log.warn("ghostty tick failed err={}", .{err});
        return 0;
    };
    return 1;
}

pub export fn ghostty_gtk_host_tick(host: ?*Host) c_int {
    return taskers_ghostty_host_tick(host);
}

pub export fn taskers_ghostty_surface_new(
    host: ?*Host,
    options: ?*const SurfaceOptions,
) ?*gtk.Widget {
    const ptr = host orelse return null;
    if (ptr.shutting_down) return null;
    const opts = options orelse &SurfaceOptions{};
    const surface = Surface.newForApp(ptr.rt_app.app, .{
        .working_directory = if (opts.working_directory) |value| std.mem.span(value) else null,
        .title = if (opts.title) |value| std.mem.span(value) else null,
    });
    const config = taskersSurfaceConfig(ptr, opts) catch |err| {
        std.log.err("failed to configure Taskers Ghostty surface err={}", .{err});
        return null;
    };
    defer config.unref();
    surface.setConfig(config);

    _ = surface.refSink();
    return surface.as(gtk.Widget);
}

pub export fn ghostty_gtk_surface_new(
    host: ?*Host,
    options: ?*const SurfaceOptions,
) ?*gtk.Widget {
    return taskers_ghostty_surface_new(host, options);
}

pub export fn taskers_ghostty_surface_destroy(widget: ?*gtk.Widget) void {
    const ptr = widget orelse return;
    const surface: *Surface = @ptrCast(@alignCast(ptr));
    surface.close();
}

pub export fn ghostty_gtk_surface_destroy(widget: ?*gtk.Widget) void {
    taskers_ghostty_surface_destroy(widget);
}

pub export fn taskers_ghostty_surface_grab_focus(widget: ?*gtk.Widget) c_int {
    const ptr = widget orelse return 0;
    const surface: *Surface = @ptrCast(@alignCast(ptr));
    surface.grabFocus();
    return 1;
}

pub export fn ghostty_gtk_surface_grab_focus(widget: ?*gtk.Widget) c_int {
    return taskers_ghostty_surface_grab_focus(widget);
}

pub export fn taskers_ghostty_surface_has_selection(widget: ?*gtk.Widget) c_int {
    const ptr = widget orelse return 0;
    const surface: *Surface = @ptrCast(@alignCast(ptr));
    const core = surface.core() orelse return 0;
    return if (core.hasSelection()) 1 else 0;
}

pub export fn ghostty_gtk_surface_has_selection(widget: ?*gtk.Widget) c_int {
    return taskers_ghostty_surface_has_selection(widget);
}

pub export fn taskers_ghostty_surface_send_text(
    widget: ?*gtk.Widget,
    ptr: ?[*]const u8,
    len: usize,
) c_int {
    const widget_ptr = widget orelse return 0;
    const bytes = ptr orelse return 0;
    const surface: *Surface = @ptrCast(@alignCast(widget_ptr));
    const core = surface.core() orelse return 0;
    _ = core.textCallback(bytes[0..len]) catch return 0;
    return 1;
}

pub export fn ghostty_gtk_surface_send_text(
    widget: ?*gtk.Widget,
    ptr: ?[*]const u8,
    len: usize,
) c_int {
    return taskers_ghostty_surface_send_text(widget, ptr, len);
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

pub export fn ghostty_gtk_surface_read_all_text(
    widget: ?*gtk.Widget,
    result: ?*Text,
) c_int {
    return taskers_ghostty_surface_read_all_text(widget, result);
}

pub export fn taskers_ghostty_surface_free_text(text: ?*Text) void {
    const ptr = text orelse return;
    if (ptr.text) |value| {
        state.alloc.free(value[0..ptr.text_len :0]);
    }
    ptr.* = .{};
}

pub export fn ghostty_gtk_surface_free_text(text: ?*Text) void {
    taskers_ghostty_surface_free_text(text);
}

fn taskersSurfaceConfig(ptr: *Host, opts: *const SurfaceOptions) !*Config {
    const alloc = state.alloc;

    var cloned = try configpkg.Config.default(alloc);
    defer cloned.deinit();

    try loadOptionalConfigFile(&cloned, alloc, ptr.base_config_path);
    try loadOptionalConfigFile(&cloned, alloc, ptr.override_config_path);
    try cloned.finalize();

    try applyTaskersEmbeddedSurfaceInvariants(
        alloc,
        &cloned,
        ptr.command_argv,
        !ptr.config_diagnostics_logged,
    );
    for (ptr.env_entries) |entry| {
        try cloned.env.parseCLI(alloc, entry);
    }
    if (opts.env_entries) |entries| {
        for (0..opts.env_count) |index| {
            try cloned.env.parseCLI(alloc, std.mem.span(entries[index]));
        }
    }

    const config = try Config.new(alloc, &cloned);
    if (!ptr.config_diagnostics_logged and config.hasDiagnostics()) {
        logEffectiveConfigDiagnostics(config);
        ptr.config_diagnostics_logged = true;
    }
    return config;
}

fn applyTaskersEmbeddedSurfaceInvariants(
    _: std.mem.Allocator,
    config: *configpkg.Config,
    command_argv: []const [:0]const u8,
    log_conflicts: bool,
) !void {
    const alloc = config.arenaAlloc();

    if (log_conflicts and config.command != null) {
        std.log.warn("Taskers embedded config command override ignored in favor of Taskers-owned launch command", .{});
    }
    if (log_conflicts and config.@"shell-integration" != .none) {
        std.log.warn("Taskers embedded config shell-integration override ignored for embedded panes", .{});
    }
    if (log_conflicts and config.@"linux-cgroup" != .never) {
        std.log.warn("Taskers embedded config linux-cgroup override ignored for embedded panes", .{});
    }

    config.command = if (command_argv.len == 0) null else command: {
        const direct = configpkg.Command{ .direct = command_argv };
        break :command try direct.clone(alloc);
    };
    config.@"shell-integration" = .none;
    config.@"shell-integration-features" = .{};
    config.@"linux-cgroup" = .never;
}

fn logConfigDiagnostics(host: *const Host) void {
    const app_config: *Config = host.rt_app.app.getConfig();
    defer app_config.unref();
    if (!app_config.hasDiagnostics()) return;

    var buf: [4095:0]u8 = undefined;
    var writer: std.Io.Writer = .fixed(&buf);
    for (app_config.get()._diagnostics.items()) |diag| {
        writer.end = 0;
        diag.format(&writer) catch |err| {
            std.log.warn("failed to format Ghostty config diagnostic err={}", .{err});
            continue;
        };
        std.log.warn("ghostty config diagnostic: {s}", .{buf[0..writer.end]});
    }
}

fn logEffectiveConfigDiagnostics(config: *Config) void {
    if (!config.hasDiagnostics()) return;

    var buf: [4095:0]u8 = undefined;
    var writer: std.Io.Writer = .fixed(&buf);
    for (config.get()._diagnostics.items()) |diag| {
        writer.end = 0;
        diag.format(&writer) catch |err| {
            std.log.warn("failed to format Taskers embedded config diagnostic err={}", .{err});
            continue;
        };
        std.log.warn("Taskers embedded config diagnostic: {s}", .{buf[0..writer.end]});
    }
}

fn loadOptionalConfigFile(
    config: *configpkg.Config,
    alloc: std.mem.Allocator,
    path_opt: ?[:0]const u8,
) !void {
    const path = path_opt orelse return;
    std.fs.accessAbsolute(path, .{}) catch |err| switch (err) {
        error.FileNotFound => return,
        else => return err,
    };
    try config.loadFile(alloc, path);
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

fn duplicateOptionalString(alloc: std.mem.Allocator, value: ?[*:0]const u8) !?[:0]u8 {
    const raw = value orelse return null;
    return try alloc.dupeZ(u8, std.mem.span(raw));
}

fn freeOptionalString(alloc: std.mem.Allocator, value: ?[:0]const u8) void {
    if (value) |entry| alloc.free(entry);
}

fn cString(comptime value: []const u8) [value.len:0]u8 {
    var output: [value.len:0]u8 = undefined;
    @memcpy(output[0..value.len], value);
    output[value.len] = 0;
    return output;
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
    try applyTaskersEmbeddedSurfaceInvariants(
        testing.allocator,
        &config,
        command_argv[0..],
        false,
    );

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

    try applyTaskersEmbeddedSurfaceInvariants(testing.allocator, &config, &.{}, false);

    try testing.expectEqual(@as(f32, 17), config.@"font-size");
    try testing.expect(config.command == null);
}

test "taskers embedded config preserves non-invariant visual settings" {
    const testing = std.testing;
    var config = try configpkg.Config.default(testing.allocator);
    defer config.deinit();

    config.theme = .{ .light = "Catppuccin Latte", .dark = "Catppuccin Mocha" };
    config.background = .{ .r = 0x1E, .g = 0x1E, .b = 0x2E };
    config.@"background-opacity" = 0.5;
    config.@"window-padding-x" = .{ .top_left = 7, .bottom_right = 9 };
    config.@"window-padding-y" = .{ .top_left = 11, .bottom_right = 13 };
    config.@"window-padding-balance" = true;
    config.@"window-padding-color" = .extend;

    try applyTaskersEmbeddedSurfaceInvariants(testing.allocator, &config, &.{}, false);

    try testing.expectEqualStrings("Catppuccin Latte", config.theme.?.light);
    try testing.expectEqualStrings("Catppuccin Mocha", config.theme.?.dark);
    try testing.expectEqual(configpkg.Color{ .r = 0x1E, .g = 0x1E, .b = 0x2E }, config.background);
    try testing.expectEqual(@as(f64, 0.5), config.@"background-opacity");
    try testing.expectEqual(@as(u32, 7), config.@"window-padding-x".top_left);
    try testing.expectEqual(@as(u32, 9), config.@"window-padding-x".bottom_right);
    try testing.expectEqual(@as(u32, 11), config.@"window-padding-y".top_left);
    try testing.expectEqual(@as(u32, 13), config.@"window-padding-y".bottom_right);
    try testing.expect(config.@"window-padding-balance");
    try testing.expectEqual(configpkg.WindowPaddingColor.extend, config.@"window-padding-color");
}
