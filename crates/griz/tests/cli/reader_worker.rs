//! Repeated CLI reads retain process state but resolve current request inputs.
use super::common::{Griz, TestResult};
use serde_json::Value;
fn query(griz: &Griz, directory: &std::path::Path) -> Result<Value, Box<dyn std::error::Error>> {
    let output = griz
        .command(&["find", "--paths", "value.txt", "--literal", "needle"])
        .current_dir(directory)
        .env("GRIZ_READER_IDLE_MS", "5000")
        .output()?;
    assert!(output.status.success(), "{:?}", output.stderr);
    Ok(serde_json::from_slice(&output.stdout)?)
}
#[test]
fn worker_resolves_each_clients_directory_and_fresh_bytes() -> TestResult {
    let griz = Griz::new()?;
    griz.write("first/value.txt", "needle one\n")?;
    griz.write("second/value.txt", "needle two\n")?;
    let first = query(&griz, &griz.path("first"))?;
    let second = query(&griz, &griz.path("second"))?;
    assert_eq!(first["total"], 1);
    assert_eq!(second["total"], 1);
    assert_ne!(first["matches"][0]["path"], second["matches"][0]["path"]);
    assert_ne!(
        first["matches"][0]["file_hash"],
        second["matches"][0]["file_hash"]
    );
    griz.write("first/value.txt", "needle new\n")?;
    let changed = query(&griz, &griz.path("first"))?;
    assert_ne!(
        first["matches"][0]["file_hash"],
        changed["matches"][0]["file_hash"]
    );
    assert_eq!(first["matches"][0]["range"], changed["matches"][0]["range"]);
    Ok(())
}
#[test]
fn worker_output_and_errors_equal_direct_engine() -> TestResult {
    let griz = Griz::new()?;
    griz.write("value.txt", "needle\n")?;
    for args in [
        vec!["find", "--paths", "value.txt", "--literal", "needle"],
        vec!["find", "--paths", "value.txt", "--regex", "("],
        vec!["read", "value.txt"],
        vec!["read", "missing.txt"],
    ] {
        let frontend = griz.command(&args).output()?;
        let command = griz.command(&args);
        let mut engine = std::process::Command::new(env!("CARGO_BIN_EXE_griz-engine"));
        engine
            .args(command.get_args())
            .current_dir(griz.work.path());
        copy_environment(&command, &mut engine);
        let direct = engine.output()?;
        assert_eq!(frontend.status.code(), direct.status.code(), "{args:?}");
        assert_eq!(frontend.stdout, direct.stdout, "{args:?}");
        assert_eq!(frontend.stderr, direct.stderr, "{args:?}");
    }
    Ok(())
}

fn copy_environment(source: &std::process::Command, target: &mut std::process::Command) {
    for (name, value) in source.get_envs() {
        match value {
            Some(value) => {
                target.env(name, value);
            }
            None => {
                target.env_remove(name);
            }
        }
    }
}
#[test]
fn read_worker_serves_multiple_requests_and_expires() -> TestResult {
    use std::io::{BufRead, BufReader, Write};
    let griz = Griz::new()?;
    griz.write("value.txt", "needle\n")?;
    let socket = griz.path("reader.sock");
    let child = griz.command(&["--reader-host", socket.to_str().ok_or("socket path")?]);
    // The frontend only forwards the exact internal host arguments.
    let mut engine = std::process::Command::new(env!("CARGO_BIN_EXE_griz-engine"));
    engine.args(["--reader-host", socket.to_str().ok_or("socket path")?]);
    copy_environment(&child, &mut engine);
    engine.env("GRIZ_READER_IDLE_MS", "5000");
    engine.stderr(std::process::Stdio::piped());
    let child = engine.spawn()?;
    for _ in 0..2 {
        let mut stream = connect_ready(&socket)?;
        stream.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;
        let request = serde_json::json!({
            "argv": ["find", "--paths", "value.txt", "--literal", "needle", "--json"],
            "cwd": griz.work.path(), "human": false,
        });
        writeln!(stream, "{request}")?;
        let mut line = String::new();
        BufReader::new(stream).read_line(&mut line)?;
        let reply: Value = serde_json::from_str(&line)?;
        assert_eq!(reply["exit_code"], 0);
        let output: Value =
            serde_json::from_str(reply["output"].as_str().ok_or("missing output")?)?;
        assert_eq!(output["total"], 1);
    }
    let output = wait_for_expiry(child)?;
    assert!(output.status.success(), "{:?}", output.stderr);
    assert!(!socket.exists());
    Ok(())
}

fn wait_for_expiry(
    mut child: std::process::Child,
) -> Result<std::process::Output, Box<dyn std::error::Error>> {
    let start = std::time::Instant::now();
    while child.try_wait()?.is_none() {
        if start.elapsed() > std::time::Duration::from_secs(15) {
            child.kill()?;
            child.wait()?;
            return Err("read worker did not expire within 15 seconds".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    Ok(child.wait_with_output()?)
}

fn connect_ready(
    path: &std::path::Path,
) -> Result<std::os::unix::net::UnixStream, Box<dyn std::error::Error>> {
    for _ in 0..500 {
        match std::os::unix::net::UnixStream::connect(path) {
            Ok(stream) => return Ok(stream),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
                ) => {}
            Err(error) => return Err(error.into()),
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    Err("read worker did not accept a connection within five seconds".into())
}
