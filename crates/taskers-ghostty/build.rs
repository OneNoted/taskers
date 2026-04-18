use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

const SKIP_BUILD_RUNTIME_EMBED_ENV: &str = "GHOSTTY_GTK_SKIP_BUILD_RUNTIME_EMBED";
const LEGACY_SKIP_BUILD_RUNTIME_EMBED_ENV: &str = "TASKERS_GHOSTTY_SKIP_BUILD_RUNTIME_EMBED";
const BRIDGE_PKG_CONFIG_PACKAGES: &[&str] = &[
    "gtk4",
    "libadwaita-1",
    "libxml-2.0",
    "x11",
    "xkbcommon-x11",
    // Keep Ghostty's font stack on the same system libraries that GTK/Pango
    // use. Statically embedding Ghostty's packaged fontconfig can make Pango
    // parse a newer distro fontconfig tree with an older parser and abort at
    // startup.
    "fontconfig",
    "freetype2",
    "harfbuzz",
];

fn main() {
    println!("cargo:rustc-check-cfg=cfg(ghostty_gtk_bridge)");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed={SKIP_BUILD_RUNTIME_EMBED_ENV}");
    println!("cargo:rerun-if-env-changed={LEGACY_SKIP_BUILD_RUNTIME_EMBED_ENV}");
    println!("cargo:rerun-if-changed=../../vendor/ghostty/build.zig");
    println!("cargo:rerun-if-changed=../../vendor/ghostty/src/ghostty_gtk_bridge.zig");
    println!("cargo:rerun-if-changed=../../vendor/ghostty/src/taskers_bridge_glibc_compat.c");
    println!("cargo:rerun-if-changed=../../vendor/ghostty/src/taskers_bridge_build_info.zig");
    println!("cargo:rerun-if-changed=../../vendor/ghostty/include/ghostty_gtk.h");
    println!("cargo:rerun-if-changed=../../vendor/ghostty/src/apprt.zig");
    println!("cargo:rerun-if-changed=../../vendor/ghostty/src/apprt/gtk/Surface.zig");
    println!("cargo:rerun-if-changed=../../vendor/ghostty/src/apprt/gtk/class/surface.zig");
    println!("cargo:rerun-if-changed=../../vendor/ghostty/src/build/LibtoolStep.zig");
    println!("cargo:rerun-if-changed=../../vendor/ghostty/src/build/SharedDeps.zig");
    println!("cargo:rerun-if-changed=../../vendor/ghostty/src/os/resourcesdir.zig");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("linux") {
        return;
    }
    if let Ok(target) = env::var("TARGET") {
        println!("cargo:rustc-env=TASKERS_BUILD_TARGET={target}");
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
            "cargo:warning=vendored Ghostty source tree not found; compile-time Ghostty GTK support is unavailable"
        );
        return;
    }
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("out dir"));
    let install_dir = out_dir.join("ghostty-gtk-bridge");

    build_bridge(&vendor_dir, &install_dir);
    emit_static_bridge_linkage(&install_dir);
    println!("cargo:rustc-cfg=ghostty_gtk_bridge");

    if let Some(skip_env) = skip_build_runtime_embed_env() {
        println!(
            "cargo:warning=skipping build-time Ghostty runtime embedding because {skip_env} is set"
        );
        return;
    }

    emit_build_runtime_env(&install_dir);
}

fn skip_build_runtime_embed_env() -> Option<&'static str> {
    if env::var_os(SKIP_BUILD_RUNTIME_EMBED_ENV).is_some() {
        Some(SKIP_BUILD_RUNTIME_EMBED_ENV)
    } else if env::var_os(LEGACY_SKIP_BUILD_RUNTIME_EMBED_ENV).is_some() {
        Some(LEGACY_SKIP_BUILD_RUNTIME_EMBED_ENV)
    } else {
        None
    }
}

fn build_bridge(vendor_dir: &Path, install_dir: &Path) {
    let ghostty_version = vendored_ghostty_version(vendor_dir);
    let version_arg = format!("-Dversion-string={ghostty_version}");
    let output = Command::new("zig")
        .current_dir(vendor_dir)
        .args([
            "build",
            "ghostty-gtk-bridge",
            "-Dapp-runtime=gtk",
            "-Demit-exe=false",
            "-Dgtk-wayland=false",
            "-Dstrip=true",
            "-Di18n=false",
            "-fsys=fontconfig",
            "-fsys=freetype",
            "-fsys=harfbuzz",
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

fn emit_build_runtime_env(install_dir: &Path) {
    println!(
        "cargo:rustc-env=GHOSTTY_GTK_BUILD_RESOURCES_DIR={}",
        install_dir.join("share").join("ghostty").display()
    );
    println!(
        "cargo:rustc-env=GHOSTTY_GTK_BUILD_BRIDGE_PATH={}",
        install_dir.join("lib").join("libghostty_gtk.so").display()
    );
    println!(
        "cargo:rustc-env=GHOSTTY_GTK_BUILD_TERMINFO_DIR={}",
        install_dir.join("share").join("terminfo").display()
    );
}

fn emit_static_bridge_linkage(install_dir: &Path) {
    println!(
        "cargo:rustc-link-search=native={}",
        install_dir.join("lib").display()
    );
    println!("cargo:rustc-link-lib=static=ghostty_gtk");

    let mut emitted_tokens = Vec::new();
    for package in BRIDGE_PKG_CONFIG_PACKAGES {
        emit_pkg_config_linkage(package, &mut emitted_tokens);
    }
}

fn emit_pkg_config_linkage(package: &str, emitted_tokens: &mut Vec<String>) {
    let output = Command::new("pkg-config")
        .args(["--libs", package])
        .output()
        .unwrap_or_else(|error| panic!("failed to invoke pkg-config for {package}: {error}"));
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        panic!("pkg-config --libs {package} failed\nstdout:\n{stdout}\nstderr:\n{stderr}");
    }

    for token in String::from_utf8_lossy(&output.stdout).split_whitespace() {
        if emitted_tokens.iter().any(|seen| seen == token) {
            continue;
        }
        emitted_tokens.push(token.to_owned());

        if let Some(path) = token.strip_prefix("-L") {
            println!("cargo:rustc-link-search=native={path}");
        } else if let Some(lib) = token.strip_prefix("-l") {
            println!("cargo:rustc-link-lib={lib}");
        } else {
            println!("cargo:rustc-link-arg={token}");
        }
    }
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
