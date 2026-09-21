//! `diff` reports which named definitions a plan added, removed, or changed.

use crate::common::{Griz, TestResult};

#[test]
fn diff_reports_a_renamed_function_as_removed_and_added() -> TestResult {
    let griz = Griz::new()?;
    griz.write("a.rs", "fn old_name() {\n    1;\n}\n")?;
    let ops = r#"[{"op":"replace","path":"a.rs","find":{"text":"old_name"},"replace":"new_name"}]"#;
    let plan = griz.id(&["plan", "--ops", ops], "plan")?;
    let diff = griz.run(&["diff", &plan])?;
    let items = diff.json["files"][0]["items"]
        .as_array()
        .ok_or("no items")?;
    let changes: Vec<_> = items
        .iter()
        .map(|item| (item["name"].as_str(), item["change"].as_str()))
        .collect();
    assert!(
        changes.contains(&(Some("old_name"), Some("removed"))),
        "{items:?}"
    );
    assert!(
        changes.contains(&(Some("new_name"), Some("added"))),
        "{items:?}"
    );
    Ok(())
}

#[test]
fn diff_of_a_non_source_file_has_empty_items() -> TestResult {
    let griz = Griz::new()?;
    griz.write("notes.md", "old text\n")?;
    let ops = r#"[{"op":"replace","path":"notes.md","find":{"text":"old"},"replace":"new"}]"#;
    let plan = griz.id(&["plan", "--ops", ops], "plan")?;
    let diff = griz.run(&["diff", &plan])?;
    assert_eq!(diff.json["files"][0]["items"], serde_json::json!([]));
    Ok(())
}
