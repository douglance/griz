//! Plan discovery follows real CLI options and exposes the syntax expectation.
use super::{Mcp, TestResult, plan_input_schema};
use crate::common::Griz;
use serde_json::{Value, json};
use std::{collections::BTreeSet, error::Error, path::Path};

#[test]
fn plan_mcp_schema_matches_all_cli_options() -> TestResult {
    let griz = Griz::new()?;
    let cli = griz.run(&["plan", "--schema"])?;
    assert_eq!(cli.code, Some(0), "{}", cli.json);
    let mut mcp = Mcp::ready()?;
    let schema = plan_input_schema(&mut mcp)?;
    let expected = cli.json["options"]["properties"]
        .as_object()
        .ok_or("no CLI options")?;
    let actual = schema["properties"]
        .as_object()
        .ok_or("no MCP properties")?;
    let expected_names: BTreeSet<_> = expected.keys().map(|s| s.replace('-', "_")).collect();
    let actual_names: BTreeSet<_> = actual.keys().cloned().collect();
    assert_eq!(actual_names, expected_names);
    assert_eq!(required(&schema)?, required(&cli.json["options"])?);
    assert_plain_metadata(&cli.json["options"], &schema)?;
    assert_eq!(schema["properties"]["expect_syntax"]["type"], "string");
    assert_eq!(
        schema["properties"]["expect_syntax"]["enum"],
        json!(["clean"])
    );
    assert!(!required(&schema)?.contains("expect_syntax"));
    Ok(())
}

fn required(schema: &Value) -> Result<BTreeSet<String>, Box<dyn Error>> {
    schema["required"]
        .as_array()
        .ok_or("no required fields")?
        .iter()
        .map(|value| {
            Ok(value
                .as_str()
                .ok_or("required name is not a string")?
                .replace('-', "_"))
        })
        .collect()
}

fn assert_plain_metadata(cli: &Value, mcp: &Value) -> TestResult {
    for (key, value) in cli["properties"].as_object().ok_or("no CLI properties")? {
        let name = key.replace('-', "_");
        if matches!(name.as_str(), "ops" | "workspace_edit") {
            continue;
        }
        assert_eq!(mcp["properties"][&name]["type"], value["type"], "{name}");
        assert_eq!(
            mcp["properties"][&name]["description"], value["description"],
            "{name}"
        );
    }
    Ok(())
}

#[test]
fn mcp_syntax_expectation_changes_plan_verdict() -> TestResult {
    let tree = tempfile::tempdir()?;
    std::fs::write(tree.path().join("main.rs"), "fn main() {}\n")?;
    let mut mcp = Mcp::ready()?;
    let unchecked = syntax_plan(&mut mcp, tree.path(), "unchecked", false)?;
    let checked = syntax_plan(&mut mcp, tree.path(), "checked", true)?;
    assert_eq!(unchecked["outcome"], "passed", "{unchecked}");
    assert_eq!(checked["outcome"], "failed", "{checked}");
    assert!(checked["id"].as_str().is_some());
    assert_eq!(
        std::fs::read_to_string(tree.path().join("main.rs"))?,
        "fn main() {}\n"
    );
    Ok(())
}

fn syntax_plan(
    mcp: &mut Mcp,
    root: &Path,
    key: &str,
    clean: bool,
) -> Result<Value, Box<dyn Error>> {
    let mut args = json!({
        "root":root, "purpose":"test", "idempotency_key":key,
        "ops":[{"op":"replace","path":"main.rs","find":"{}","replace":"{"}]
    });
    if clean {
        args["expect_syntax"] = json!("clean");
    }
    let response = mcp.call("tools/call", &json!({"name":"plan","arguments":args}))?;
    let text = response["result"]["content"][0]["text"]
        .as_str()
        .ok_or("no plan response")?;
    Ok(serde_json::from_str(text)?)
}
