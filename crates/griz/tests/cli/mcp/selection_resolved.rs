//! Resolved selection is discoverable and executable through direct MCP.
use super::{Mcp, TestResult};
use serde_json::{Value, json};
use std::error::Error;

fn tool(mcp: &mut Mcp, name: &str, arguments: &Value) -> Result<Value, Box<dyn Error>> {
    let reply = mcp.call("tools/call", &json!({"name":name,"arguments":arguments}))?;
    let text = reply["result"]["content"][0]["text"]
        .as_str()
        .ok_or_else(|| format!("no tool content: {reply}"))?;
    Ok(serde_json::from_str(text)?)
}

#[test]
fn resolved_selection_is_opt_in_in_mcp_schema() -> TestResult {
    let mut mcp = Mcp::ready()?;
    let listed = mcp.call("tools/list", &json!({}))?;
    let select = listed["result"]["tools"]
        .as_array()
        .ok_or("no tools")?
        .iter()
        .find(|tool| tool["name"] == "select")
        .ok_or("no select")?;
    assert_eq!(
        select["inputSchema"]["properties"]["resolved_only"]["type"],
        "boolean"
    );
    assert_eq!(
        select["inputSchema"]["properties"]["resolved_only"]["default"],
        false
    );
    Ok(())
}

#[test]
fn resolved_selection_is_opt_in_in_mcp_execution() -> TestResult {
    let mut mcp = Mcp::ready()?;
    let work = tempfile::tempdir()?;
    std::fs::write(work.path().join("a.txt"), "before\n")?;
    let original = tool(
        &mut mcp,
        "plan",
        &json!({
            "root":work.path(),"purpose":"partial","idempotency_key":"plan",
            "ops":[
                {"op":"replace","path":"a.txt","find":"before","replace":"after"},
                {"op":"replace","path":"a.txt","find":"absent","replace":"unused"}
            ]
        }),
    )?;
    assert_eq!(original["outcome"], "error");
    let unfiltered = tool(
        &mut mcp,
        "select",
        &json!({
            "plan":original["id"],"purpose":"default","idempotency_key":"default"
        }),
    )?;
    assert_eq!(unfiltered["outcome"], "error");
    let selected = tool(
        &mut mcp,
        "select",
        &json!({
            "plan":original["id"],"resolved_only":true,
            "purpose":"resolved","idempotency_key":"resolved"
        }),
    )?;
    assert_eq!(selected["outcome"], "passed");
    assert_eq!(
        std::fs::read_to_string(work.path().join("a.txt"))?,
        "before\n"
    );
    let applied = tool(
        &mut mcp,
        "apply",
        &json!({
            "plan":selected["id"],"purpose":"apply","idempotency_key":"apply"
        }),
    )?;
    assert_eq!(applied["outcome"], "passed");
    assert_eq!(
        std::fs::read_to_string(work.path().join("a.txt"))?,
        "after\n"
    );
    Ok(())
}
