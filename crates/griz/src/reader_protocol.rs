//! Private transport shared by the frontend and read worker.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
#[derive(Serialize, Deserialize)]
pub(crate) struct Request {
    pub(crate) argv: Vec<String>,
    pub(crate) cwd: PathBuf,
    pub(crate) human: bool,
}
#[derive(Serialize, Deserialize)]
pub(crate) struct Reply {
    pub(crate) output: String,
    pub(crate) exit_code: i32,
}
pub(crate) fn accepts(args: &[String]) -> bool {
    args.first()
        .is_some_and(|arg| matches!(arg.as_str(), "find" | "read"))
        && !args.iter().any(|arg| {
            matches!(
                arg.as_str(),
                "--mcp"
                    | "--help"
                    | "-h"
                    | "--schema"
                    | "--llms"
                    | "--llms-full"
                    | "--version"
                    | "-"
            ) || arg.contains("/dev/stdin")
                || arg.starts_with('@')
        })
}
