//! griz: code edits as composable primitives.
//!
//! Each command is one primitive. Served over MCP, every command is its own
//! tool, so a Code Mode program in any host calls `griz.find`, `griz.plan`,
//! `griz.apply`, and `griz.undo` directly and composes them with other tools.

mod annotations;
mod cmd_apply;
mod cmd_inspect;
mod cmd_plan;
mod cmd_read;
mod context;
mod lines;
mod plan_schema;
mod receipt;
mod render;
mod verdict;

use incurs::{
    cli::Cli,
    command::TypedResult,
    mcp::{McpDiscovery, McpServeOptions, McpToolFilter},
};
use serde_json::Value;

const INSTRUCTIONS: &str = "griz edits code as composable primitives. find returns matches with byte ranges and file fingerprints; plan turns operations or Codex patch text into a plan id without writing; diff shows it; select keeps part of it; apply writes it all-or-nothing and returns an operation id; absorb folds a later formatter run into that operation; undo restores it. Mutations take purpose and idempotency_key and answer {id, outcome}; pass verbosity trace to read the full record.";

fn build_cli() -> Cli {
    Cli::create("griz")
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
        })
        .command("read", cmd_read::read_command())
        .command("find", cmd_read::find_command())
        .command("plan", cmd_plan::plan_command())
        .command("select", cmd_plan::select_command())
        .command("diff", cmd_inspect::diff_command())
        .command("apply", cmd_apply::apply_command())
        .command("undo", cmd_apply::undo_command())
        .command("absorb", cmd_apply::absorb_command())
        .command("log", cmd_inspect::log_command())
        .command("get", cmd_inspect::get_command())
}

/// Shapes a mutation's verdict: a verdict other than `passed` exits nonzero.
fn respond(result: Result<(Value, verdict::Outcome), context::CmdError>) -> TypedResult<Value> {
    match result {
        Ok((body, verdict::Outcome::Passed)) => TypedResult::ok(body),
        Ok((body, _)) => TypedResult::ok_with_exit_code(body, 1),
        Err(error) => error.result(),
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    build_cli().serve().await
}
