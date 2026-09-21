//! `diff --text`: the diff as one string, for a person reading it, instead
//! of the numbered lines a program addresses.

use crate::common::{Griz, TestResult};

/// Plans one edit and returns the plan id.
fn plan(griz: &Griz) -> Result<String, Box<dyn std::error::Error>> {
    griz.write("a.rs", "fn f() {\n    old();\n}\n")?;
    let ops = r#"[{"op":"replace","path":"a.rs","find":{"text":"old();"},"replace":"new();"}]"#;
    griz.id(
        &[
            "plan",
            "--ops",
            ops,
            "--purpose",
            "t",
            "--idempotency-key",
            "d",
        ],
        "d",
    )
}

#[test]
fn diff_text_returns_the_unified_diff_as_one_string() -> TestResult {
    let griz = Griz::new()?;
    let id = plan(&griz)?;
    let run = griz.run(&["diff", &id, "--text"])?;
    let text = run.json["text"].as_str().unwrap_or_default();
    assert!(text.contains("-    old();"), "{}", run.json);
    assert!(text.contains("+    new();"), "{}", run.json);
    assert!(text.contains('\n'), "{}", run.json);
    Ok(())
}

#[test]
fn diff_text_honors_grep_like_the_numbered_lines_do() -> TestResult {
    let griz = Griz::new()?;
    let id = plan(&griz)?;
    let run = griz.run(&["diff", &id, "--text", "--grep", "new"])?;
    let text = run.json["text"].as_str().unwrap_or_default();
    assert!(text.contains("+    new();"), "{}", run.json);
    assert!(!text.contains("-    old();"), "{}", run.json);
    Ok(())
}

#[test]
fn diff_without_text_still_returns_numbered_lines() -> TestResult {
    let griz = Griz::new()?;
    let id = plan(&griz)?;
    let run = griz.run(&["diff", &id])?;
    assert!(run.json["lines"].is_array(), "{}", run.json);
    assert!(run.json["text"].is_null(), "{}", run.json);
    Ok(())
}
