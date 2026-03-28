#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
compile_error!(
    "taskers on crates.io currently supports x86_64 Linux only. Mainline macOS support is not shipped from this repo root."
);

use std::ffi::OsString;

fn main() {
    let args = std::env::args_os().skip(1).collect::<Vec<OsString>>();
    let exit_code = match taskers::linux_install::run(&args) {
        Ok(status) => taskers::linux_install::exit_code_from_status(status),
        Err(error) => {
            eprintln!("taskers failed: {error:#}");
            1
        }
    };

    std::process::exit(exit_code);
}
