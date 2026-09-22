//! Large context values are clamped to the text, without integer overflow.

use super::{Mcp, TestResult};
use serde_json::{Value, json};

#[test]
fn oversized_context_preserves_the_match_and_keeps_mcp_usable() -> TestResult {
    let work = tempfile::tempdir()?;
    let path = work.path().join("a.txt");
    std::fs::write(&path, "first\nneedle\nlast\n")?;
    let mut mcp = Mcp::ready()?;
    let called = mcp.call(
        "tools/call",
        &json!({
            "name": "read",
            "arguments": { "path": path, "grep": "needle", "context": usize::MAX },
        }),
    )?;
    let content = called["result"]["content"][0]["text"]
        .as_str()
        .ok_or("no text")?;
    let body: Value = serde_json::from_str(content)?;
    assert_eq!(body["matched"], 1);
    assert_eq!(body["total_lines"], 3);
    assert_eq!(
        body["lines"],
        json!([
            { "line": 1, "text": "first" },
            { "line": 2, "text": "needle" },
            { "line": 3, "text": "last" },
        ])
    );
    let listed = mcp.call("tools/list", &json!({}))?;
    assert_eq!(
        listed["result"]["tools"]
            .as_array()
            .ok_or("no tools")?
            .len(),
        10
    );
    Ok(())
}
