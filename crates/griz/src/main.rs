//! Small CLI frontend; the engine owns every command implementation.
#[cfg(unix)]
mod reader_client;
#[cfg(unix)]
mod reader_protocol;
use std::process::{Command, ExitCode};
fn main() -> ExitCode {
    let engine = match std::env::current_exe() {
        Ok(path) => path.with_file_name(if cfg!(windows) {
            "griz-engine.exe"
        } else {
            "griz-engine"
        }),
        Err(error) => {
            eprintln!("griz: {error}");
            return ExitCode::FAILURE;
        }
    };
    #[cfg(unix)]
    if let Some(code) = reader_client::run(&engine) {
        return code;
    }
    forward(&engine)
}
#[cfg(unix)]
fn forward(engine: &std::path::Path) -> ExitCode {
    use std::os::unix::process::CommandExt;
    let error = Command::new(engine)
        .arg0("griz")
        .args(std::env::args_os().skip(1))
        .exec();
    eprintln!("griz: {error}");
    ExitCode::FAILURE
}
#[cfg(not(unix))]
fn forward(engine: &std::path::Path) -> ExitCode {
    match Command::new(engine)
        .args(std::env::args_os().skip(1))
        .status()
    {
        Ok(status) => {
            if status.success() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(error) => {
            eprintln!("griz: {error}");
            ExitCode::FAILURE
        }
    }
}
