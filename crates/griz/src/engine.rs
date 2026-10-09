//! griz: code edits as composable primitives.
//!
//! Each command is one primitive. Served over MCP, every command is its own
//! tool, so a Code Mode program in any host calls `griz.find`, `griz.plan`,
//! `griz.apply`, and `griz.undo` directly and composes them with other tools.

mod annotations;
mod apply_expect;
mod cmd_apply;
mod cmd_inspect;
mod cmd_plan;
mod cmd_read;
mod context;
mod find_response;
mod lines;
mod path_filters;
mod plan_input;
mod plan_schema;
#[cfg(unix)]
mod reader_host;
#[cfg(unix)]
mod reader_protocol;
mod receipt;
mod render;
mod request_scope;
mod usage;
mod verdict;

use incurs::{
    cli::Cli,
    command::{CommandDef, TypedResult},
    mcp::{McpDiscovery, McpServeOptions, McpToolFilter},
};
use serde_json::Value;

const INSTRUCTIONS: &str = "griz edits code as composable primitives. find returns matches with byte ranges and file fingerprints; plan turns operations or Codex patch text into a plan id without writing; diff shows it; select keeps part of it; apply writes it all-or-nothing and returns an operation id; absorb folds a later formatter run into that operation; undo restores it. Mutations take purpose and idempotency_key and answer {id, outcome}; pass verbosity trace to read the full record.";

fn build_cli(selected: Option<&str>) -> Cli {
    let mut cli = Cli::create("griz")
        .version(env!("CARGO_PKG_VERSION"))
        .description("Code edits as composable primitives: find, plan, diff, select, apply, undo")
        .mcp(McpServeOptions {
            version: Some(env!("CARGO_PKG_VERSION").to_string()),
            instructions: Some(INSTRUCTIONS.to_string()),
            tools: McpToolFilter {
                discovery: McpDiscovery::Direct,
                ..McpToolFilter::default()
            },
            ..McpServeOptions::default()
        });
    for &(name, create) in COMMANDS {
        if selected.is_none_or(|command| command == name) {
            cli = cli.command(name, create());
        }
    }
    cli
}

type CommandFactory = fn() -> CommandDef;

const COMMANDS: &[(&str, CommandFactory)] = &[
    ("read", cmd_read::read_command),
    ("find", cmd_read::find_command),
    ("plan", cmd_plan::plan_command),
    ("select", cmd_plan::select_command),
    ("diff", cmd_inspect::diff_command),
    ("apply", cmd_apply::apply_command),
    ("undo", cmd_apply::undo_command),
    ("absorb", cmd_apply::absorb_command),
    ("log", cmd_inspect::log_command),
    ("get", cmd_inspect::get_command),
];

fn selected_command() -> Option<String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() == 1 && args[0] == "--version" {
        return Some(String::new());
    }
    if args.iter().any(|arg| {
        matches!(
            arg.as_str(),
            "--mcp" | "--help" | "-h" | "--schema" | "--llms" | "--llms-full"
        )
    }) {
        return None;
    }
    let first = args.first()?;
    COMMANDS
        .iter()
        .any(|(name, _)| first == name)
        .then(|| first.clone())
}

/// Shapes a mutation's verdict: a verdict other than `passed` exits nonzero.
fn respond(result: Result<(Value, verdict::Outcome), context::CmdError>) -> TypedResult<Value> {
    match result {
        Ok((body, verdict::Outcome::Passed)) => TypedResult::ok(body),
        Ok((body, _)) => TypedResult::ok_with_exit_code(body, 1),
        Err(error) => error.result(),
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(unix)]
    if let Some(path) = host_path() {
        return reader_host::run(&path).await;
    }
    build_cli(selected_command().as_deref()).serve().await
}

#[cfg(unix)]
fn host_path() -> Option<std::path::PathBuf> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    (args.len() == 2 && args[0] == "--reader-host").then(|| std::path::PathBuf::from(&args[1]))
}
