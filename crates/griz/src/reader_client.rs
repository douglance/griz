//! Connects ordinary read commands to an owned, idle-expiring engine.
use crate::reader_protocol::{Reply, Request, accepts};
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    io::{self, BufRead, BufReader, IsTerminal, Write},
    os::unix::{
        fs::{DirBuilderExt, FileTypeExt, MetadataExt, OpenOptionsExt},
        net::UnixStream,
    },
    path::{Path, PathBuf},
    process::{Command, ExitCode, Stdio},
    time::{Duration, SystemTime},
};
pub(crate) fn run(engine: &Path) -> Option<ExitCode> {
    let argv: Vec<_> = std::env::args().skip(1).collect();
    if !accepts(&argv) {
        return None;
    }
    let request = Request {
        argv,
        cwd: std::env::current_dir().ok()?,
        human: io::stdout().is_terminal(),
    };
    let socket = socket_path(engine).ok()?;
    let mut stream = connect(engine, &socket).ok()?;
    stream.set_read_timeout(Some(Duration::from_mins(2))).ok()?;
    stream
        .set_write_timeout(Some(Duration::from_mins(2)))
        .ok()?;
    serde_json::to_writer(&mut stream, &request).ok()?;
    stream.write_all(b"\n").ok()?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line).ok()?;
    let reply: Reply = serde_json::from_str(&line).ok()?;
    io::stdout().write_all(reply.output.as_bytes()).ok()?;
    let code = u8::try_from(reply.exit_code).unwrap_or(1);
    Some(ExitCode::from(code))
}
fn socket_path(engine: &Path) -> io::Result<PathBuf> {
    let uid = rustix::process::getuid().as_raw();
    let directory = std::env::temp_dir().join(format!("griz-reader-{uid}"));
    match std::fs::DirBuilder::new().mode(0o700).create(&directory) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    let metadata = std::fs::symlink_metadata(&directory)?;
    if !metadata.is_dir() || metadata.uid() != uid || metadata.mode() & 0o077 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "read worker directory is not private",
        ));
    }
    let mut hash = DefaultHasher::new();
    engine.hash(&mut hash);
    env!("CARGO_PKG_VERSION").hash(&mut hash);
    let metadata = std::fs::metadata(engine)?;
    metadata.len().hash(&mut hash);
    metadata
        .modified()?
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos()
        .hash(&mut hash);
    let mut environment: Vec<_> = std::env::vars_os()
        .filter(|(key, _)| {
            let key = key.to_string_lossy();
            key.starts_with("GRIZ_")
                || matches!(
                    key.as_ref(),
                    "HOME"
                        | "XDG_DATA_HOME"
                        | "NO_COLOR"
                        | "CLICOLOR"
                        | "CLICOLOR_FORCE"
                        | "TERM"
                        | "LANG"
                        | "LC_ALL"
                )
        })
        .collect();
    environment.sort_unstable();
    environment.hash(&mut hash);
    Ok(directory.join(format!("{:016x}.sock", hash.finish())))
}
fn connect(engine: &Path, socket: &Path) -> io::Result<UnixStream> {
    if let Ok(stream) = UnixStream::connect(socket) {
        return Ok(stream);
    }
    let lock = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(socket.with_extension("lock"))?;
    lock.lock()?;
    if let Ok(stream) = UnixStream::connect(socket) {
        return Ok(stream);
    }
    if socket.exists() {
        let metadata = std::fs::symlink_metadata(socket)?;
        if !metadata.file_type().is_socket() || metadata.uid() != rustix::process::getuid().as_raw()
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "invalid worker socket",
            ));
        }
        std::fs::remove_file(socket)?;
    }
    let _child = Command::new(engine)
        .arg("--reader-host")
        .arg(socket)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let mut last = io::Error::new(io::ErrorKind::NotConnected, "read worker did not start");
    for _ in 0..100 {
        match UnixStream::connect(socket) {
            Ok(stream) => return Ok(stream),
            Err(error) => last = error,
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    Err(last)
}
