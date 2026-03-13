use std::{
    env,
    path::{Path, PathBuf},
    process::Command,
};

fn main() {
    println!("cargo:rustc-check-cfg=cfg(taskers_ghostty_bridge)");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../vendor/ghostty/build.zig");
    println!("cargo:rerun-if-changed=../../vendor/ghostty/src/taskers_bridge.zig");
    println!("cargo:rerun-if-changed=../../vendor/ghostty/include/taskers_ghostty_bridge.h");
    println!("cargo:rerun-if-changed=../../vendor/ghostty/src/apprt.zig");
    println!("cargo:rerun-if-changed=../../vendor/ghostty/src/apprt/gtk/Surface.zig");
    println!("cargo:rerun-if-changed=../../vendor/ghostty/src/apprt/gtk/class/surface.zig");
    println!("cargo:rerun-if-changed=../../vendor/ghostty/src/build/SharedDeps.zig");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("linux") {
        return;
    }

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let workspace_root = manifest_dir
        .ancestors()
        .nth(2)
        .expect("workspace root")
        .to_path_buf();
    let vendor_dir = workspace_root.join("vendor").join("ghostty");
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
    println!("cargo:rustc-cfg=taskers_ghostty_bridge");
}

fn build_bridge(vendor_dir: &Path, install_dir: &Path) {
    let output = Command::new("zig")
        .current_dir(vendor_dir)
        .args([
            "build",
            "taskers-bridge",
            "-Dapp-runtime=gtk",
            "-Demit-exe=false",
            "-Dgtk-wayland=false",
            "--summary",
            "none",
            "--prefix",
        ])
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
