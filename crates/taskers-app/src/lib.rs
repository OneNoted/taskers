#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
compile_error!(
    "taskers on crates.io currently supports x86_64 Linux only. Mainline macOS support is not shipped from this repo root."
);

pub mod linux_install;
