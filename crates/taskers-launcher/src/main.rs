fn main() {
    let exit_code = match taskers::run() {
        Ok(status) => exit_code_from_status(status),
        Err(error) => {
            eprintln!("taskers launcher failed: {error:#}");
            1
        }
    };

    std::process::exit(exit_code);
}

fn exit_code_from_status(status: std::process::ExitStatus) -> i32 {
    if let Some(code) = status.code() {
        return code;
    }

    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;

        return status.signal().map_or(1, |signal| 128 + signal);
    }

    #[cfg(not(unix))]
    {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::exit_code_from_status;
    use std::process::Command;

    #[test]
    fn preserves_normal_exit_codes() {
        let status = Command::new("sh")
            .args(["-c", "exit 7"])
            .status()
            .expect("spawn shell");
        assert_eq!(exit_code_from_status(status), 7);
    }

    #[cfg(unix)]
    #[test]
    fn maps_signals_to_failure_exit_codes() {
        let status = Command::new("sh")
            .args(["-c", "kill -TERM $$"])
            .status()
            .expect("spawn shell");
        assert_eq!(exit_code_from_status(status), 143);
    }
}
