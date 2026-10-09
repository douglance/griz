//! A private worker for read and find only; each request rereads its inputs.
use crate::{
    reader_protocol::{Reply, Request, accepts},
    request_scope,
};
use incurs::cli::Cli;
use std::{io, os::unix::fs::PermissionsExt, path::Path, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
};
pub(crate) async fn run(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    rustix::process::setsid()?;
    let listener = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    let _socket = SocketGuard(path.to_path_buf());
    let cli = crate::build_cli(None);
    let timeout = std::env::var("GRIZ_READER_IDLE_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map_or(Duration::from_mins(2), Duration::from_millis);
    while let Ok(accepted) = tokio::time::timeout(timeout, listener.accept()).await {
        let (stream, _) = accepted?;
        if let Err(_error) = serve(&cli, stream).await {}
    }
    Ok(())
}
async fn serve(cli: &Cli, stream: UnixStream) -> Result<(), Box<dyn std::error::Error>> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    tokio::time::timeout(Duration::from_mins(2), reader.read_line(&mut line)).await??;
    let request: Request = serde_json::from_str(&line)?;
    if !accepts(&request.argv) {
        return Err(io::Error::other("read worker accepts only read and find").into());
    }
    let mut output = Vec::new();
    let exit = request_scope::DIRECTORY
        .scope(
            request.cwd,
            cli.serve_to(request.argv, &mut output, request.human),
        )
        .await?;
    let reply = Reply {
        output: String::from_utf8(output)?,
        exit_code: exit.unwrap_or(0),
    };
    let mut encoded = serde_json::to_vec(&reply)?;
    encoded.push(b'\n');
    reader.get_mut().write_all(&encoded).await?;
    Ok(())
}
struct SocketGuard(std::path::PathBuf);
impl Drop for SocketGuard {
    fn drop(&mut self) {
        let _removed = std::fs::remove_file(&self.0);
    }
}
