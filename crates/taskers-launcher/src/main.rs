fn main() {
    let exit_code = match taskers::run() {
        Ok(status) => status.code().unwrap_or(0),
        Err(error) => {
            eprintln!("taskers launcher failed: {error:#}");
            1
        }
    };

    std::process::exit(exit_code);
}
