//! MCP rejects unsupported edit fields and advertises that contract.
use super::{Mcp, TestResult, plan_input_schema};
use serde_json::{Value, json};

#[test]
fn unknown_edit_fields_are_actionable_mcp_errors() -> TestResult {
    let tree = tempfile::tempdir()?;
    std::fs::write(tree.path().join("probe.txt"), "before\n")?;
    let mut mcp = Mcp::ready()?;
    for (key, op, field) in [
        (
            "guard",
            json!({"op":"replace","path":"probe.txt","find":"before",
            "replace":"after","expect_hahs":"incorrect"}),
            "expect_hahs",
        ),
        (
            "anchor",
            json!({"op":"insert","path":"probe.txt","text":"after",
            "anchor":{"text":"before","whole_line":true}}),
            "whole_line",
        ),
    ] {
        let response = mcp.call(
            "tools/call",
            &json!({
                "name":"plan", "arguments":{
                    "root":tree.path(), "ops":[op], "purpose":"test", "idempotency_key":key
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
        let message = error["message"].as_str().ok_or("no error message")?;
        assert!(message.contains("ops[0]"), "{message}");
        assert!(
            message.contains(&format!("unknown field `{field}`")),
            "{message}"
        );
        assert!(error.get("id").is_none(), "{error}");
    }
    assert_eq!(
        std::fs::read_to_string(tree.path().join("probe.txt"))?,
        "before\n"
    );
    Ok(())
}

#[test]
fn native_operation_schema_disallows_unknown_fields() -> TestResult {
    let mut mcp = Mcp::ready()?;
    let schema = plan_input_schema(&mut mcp)?;
    let branches = schema["properties"]["ops"]["items"]["oneOf"]
        .as_array()
        .ok_or("no operation schema branches")?;
    let variants = branches
        .iter()
        .find_map(|v| v["oneOf"].as_array())
        .ok_or("no operation variants")?;
    assert_eq!(variants.len(), 5);
    for variant in variants {
        assert_closed_objects(&schema, variant)?;
    }
    Ok(())
}

fn assert_closed_objects(schema: &Value, node: &Value) -> TestResult {
    if let Some(reference) = node["$ref"].as_str() {
        let pointer = reference
            .strip_prefix('#')
            .ok_or("external schema reference")?;
        let target = schema
            .pointer(pointer)
            .ok_or("unresolved schema reference")?;
        assert_closed_objects(schema, target)?;
    }
    if node["type"] == "object" {
        assert_eq!(node["additionalProperties"], false, "{node}");
    }
    match node {
        Value::Object(values) => {
            for value in values.values() {
                assert_closed_objects(schema, value)?;
            }
        }
        Value::Array(values) => {
            for value in values {
                assert_closed_objects(schema, value)?;
            }
        }
        _ => {}
    }
    Ok(())
}
