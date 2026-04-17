//! A zig builder step that runs "libtool" against a list of libraries
//! in order to create a single combined static library.
const LibtoolStep = @This();

const std = @import("std");
const Step = std.Build.Step;
const RunStep = std.Build.Step.Run;
const LazyPath = std.Build.LazyPath;

pub const Options = struct {
    /// The name of this step.
    name: []const u8,

    /// The filename (not the path) of the file to create. This will
    /// be placed in a unique hashed directory. Use out_path to access.
    out_name: []const u8,

    /// Library files (.a) to combine.
    sources: []LazyPath,
};

/// The step to depend on.
step: *Step,

/// The output file from the libtool run.
output: LazyPath,

/// Run libtool against a list of library files to combine into a single
/// static library.
pub fn create(b: *std.Build, opts: Options) *LibtoolStep {
    const self = b.allocator.create(LibtoolStep) catch @panic("OOM");

    const run_step = RunStep.create(b, b.fmt("libtool {s}", .{opts.name}));
    const output = if (b.graph.host.result.os.tag.isDarwin()) blk: {
        run_step.addArgs(&.{ "libtool", "-static", "-o" });
        const output = run_step.addOutputFileArg(opts.out_name);
        for (opts.sources) |source| run_step.addFileArg(source);
        break :blk output;
    } else blk: {
        run_step.addArgs(&.{
            "sh",
            "-c",
            \\set -e
            \\out="$1"
            \\shift
            \\tmp="$(mktemp -d)"
            \\trap 'rm -rf "$tmp"' EXIT
            \\i=0
            \\for archive in "$@"; do
            \\  members="$tmp/members.txt"
            \\  llvm-ar t "$archive" > "$members"
            \\  while IFS= read -r member; do
            \\    case "$member" in
            \\      *.o|*.obj)
            \\        llvm-ar p "$archive" "$member" > "$tmp/$i.o"
            \\        i=$((i + 1))
            \\        ;;
            \\    esac
            \\  done < "$members"
            \\done
            \\if [ "$i" -eq 0 ]; then
            \\  echo "no object files extracted from static inputs" >&2
            \\  exit 1
            \\fi
            \\llvm-ar crs "$out" "$tmp"/*.o
            ,
            "libtool-step",
        });
        const output = run_step.addOutputFileArg(opts.out_name);
        for (opts.sources) |source| run_step.addFileArg(source);
        break :blk output;
    };

    self.* = .{
        .step = &run_step.step,
        .output = output,
    };

    return self;
}
