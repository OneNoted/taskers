use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

const SKIP_BUILD_RUNTIME_EMBED_ENV: &str = "TASKERS_GHOSTTY_SKIP_BUILD_RUNTIME_EMBED";

fn main() {
    println!("cargo:rustc-check-cfg=cfg(taskers_ghostty_bridge)");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../vendor/ghostty/build.zig");
    println!("cargo:rerun-if-changed=../../vendor/ghostty/src/taskers_bridge.zig");
    println!("cargo:rerun-if-changed=../../vendor/ghostty/src/taskers_bridge_glibc_compat.c");
    println!("cargo:rerun-if-changed=../../vendor/ghostty/src/taskers_bridge_build_info.zig");
    println!("cargo:rerun-if-changed=../../vendor/ghostty/include/taskers_ghostty_bridge.h");
    println!("cargo:rerun-if-changed=../../vendor/ghostty/src/apprt.zig");
    println!("cargo:rerun-if-changed=../../vendor/ghostty/src/apprt/gtk/Surface.zig");
    println!("cargo:rerun-if-changed=../../vendor/ghostty/src/apprt/gtk/class/surface.zig");
    println!("cargo:rerun-if-changed=../../vendor/ghostty/src/build/SharedDeps.zig");
    println!("cargo:rerun-if-changed=../../vendor/ghostty/src/os/resourcesdir.zig");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("linux") {
        return;
    }
    println!("cargo:rustc-cfg=taskers_ghostty_bridge");
    if let Ok(target) = env::var("TARGET") {
        println!("cargo:rustc-env=TASKERS_BUILD_TARGET={target}");
    }

    if env::var_os(SKIP_BUILD_RUNTIME_EMBED_ENV).is_some() {
        println!(
            "cargo:warning=skipping build-time Ghostty runtime embedding because {SKIP_BUILD_RUNTIME_EMBED_ENV} is set"
        );
        return;
    }

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let workspace_root = manifest_dir
        .ancestors()
        .nth(2)
        .expect("workspace root")
        .to_path_buf();
    let vendor_dir = workspace_root.join("vendor").join("ghostty");
    if !vendor_dir.exists() {
        println!(
            "cargo:warning=vendored Ghostty source tree not found; runtime bundle bootstrap will be required"
        );
        return;
    }
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("out dir"));
    let install_dir = out_dir.join("ghostty-bridge");

    build_bridge(&vendor_dir, &install_dir);

    println!(
        "cargo:rustc-env=TASKERS_GHOSTTY_BUILD_RESOURCES_DIR={}",
        install_dir.join("share").join("ghostty").display()
    );
    println!(
        "cargo:rustc-env=TASKERS_GHOSTTY_BUILD_BRIDGE_PATH={}",
        install_dir
            .join("lib")
            .join("libtaskers_ghostty_bridge.so")
            .display()
    );
    println!(
        "cargo:rustc-env=TASKERS_GHOSTTY_BUILD_TERMINFO_DIR={}",
        install_dir.join("share").join("terminfo").display()
    );
    println!("cargo:rustc-cfg=taskers_ghostty_bridge");
}

fn build_bridge(vendor_dir: &Path, install_dir: &Path) {
    let ghostty_version = vendored_ghostty_version(vendor_dir);
    let version_arg = format!("-Dversion-string={ghostty_version}");
    let output = Command::new("zig")
        .current_dir(vendor_dir)
        .args([
            "build",
            "taskers-bridge",
            "-Dapp-runtime=gtk",
            "-Demit-exe=false",
            "-Dgtk-wayland=false",
            "-Dstrip=true",
            "-Di18n=false",
        ])
        .arg(version_arg)
        .args(["--summary", "none", "--prefix"])
        .arg(install_dir)
        .output()
        .expect("failed to invoke zig");

    if output.status.success() {
        return;
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    panic!("failed to build vendored Ghostty bridge\nstdout:\n{stdout}\nstderr:\n{stderr}");
}

fn vendored_ghostty_version(vendor_dir: &Path) -> String {
    let zon_path = vendor_dir.join("build.zig.zon");
    let zon = fs::read_to_string(&zon_path).expect("failed to read vendored Ghostty build.zig.zon");
    zon.lines()
        .find_map(|line| {
            let (_, rest) = line.split_once(".version = \"")?;
            let (version, _) = rest.split_once('"')?;
            Some(version.to_owned())
        })
        .expect("failed to parse vendored Ghostty version from build.zig.zon")
}
