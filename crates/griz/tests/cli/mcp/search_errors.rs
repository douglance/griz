//! Missing search paths remain structured errors over MCP.

use super::{Mcp, TestResult};
use serde_json::{Value, json};

#[test]
fn missing_search_paths_are_mcp_errors() -> TestResult {
    let tree = tempfile::tempdir()?;
    std::fs::write(tree.path().join("good.txt"), "needle")?;
    let mut mcp = Mcp::ready()?;
    let response = mcp.call(
        "tools/call",
        &json!({
            "name": "find",
            "arguments": {
                "root": tree.path(), "paths": ["good.txt", "missing"],
                "literal": "needle", "expect_matches": 1
            }
        }),
    )?;
    let result = &response["result"];
    assert_eq!(result["isError"], true, "{response}");
    let text = result["content"][0]["text"]
        .as_str()
        .ok_or("no error text")?;
    let error: Value = serde_json::from_str(text)?;
    assert_eq!(error["code"], "VALIDATION_ERROR", "{error}");
    assert!(
        error["message"]
            .as_str()
            .is_some_and(|s| s.contains("missing"))
    );
    Ok(())
}
