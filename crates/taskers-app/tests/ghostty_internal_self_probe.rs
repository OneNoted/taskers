use std::process::Command;

#[test]
fn ghostty_internal_self_probe_boots_host_and_surface() {
    let binary = env!("CARGO_BIN_EXE_taskers-gtk");

    for mode in ["host", "surface"] {
        let output = Command::new(binary)
            .arg("--internal-ghostty-probe")
            .arg(mode)
            .output()
            .unwrap_or_else(|error| panic!("failed to launch Ghostty {mode} self-probe: {error}"));

        assert!(
            output.status.success(),
            "Ghostty {mode} self-probe failed with status {:?}\nstdout:\n{}\nstderr:\n{}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }
}
