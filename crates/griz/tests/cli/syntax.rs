//! Parse facts on `plan`: shown from info verbosity up, default unchanged.

use crate::common::{Griz, TestResult};

#[test]
fn default_plan_output_has_no_syntax_key() -> TestResult {
    let griz = Griz::new()?;
    griz.write("a.rs", "fn f() {\n    1;\n}\n")?;
    let ops = r#"[{"op":"replace","path":"a.rs","find":{"text":"}\n"},"replace":""}]"#;
    let run = griz.run(&[
        "plan",
        "--ops",
        ops,
        "--purpose",
        "t",
        "--idempotency-key",
        "d",
    ])?;
    assert_eq!(run.json["outcome"], "passed", "{}", run.json);
    assert!(
        run.json
            .as_object()
            .is_some_and(|o| !o.contains_key("summary")),
        "{}",
        run.json
    );
    Ok(())
}

#[test]
fn info_verbosity_reports_introduced_errors() -> TestResult {
    let griz = Griz::new()?;
    griz.write("a.rs", "fn f() {\n    1;\n}\n")?;
    let ops = r#"[{"op":"replace","path":"a.rs","find":{"text":"}\n"},"replace":""}]"#;
    let run = griz.run(&[
        "plan",
        "--ops",
        ops,
        "--purpose",
        "t",
        "--idempotency-key",
        "i",
        "--verbosity",
        "info",
    ])?;
    assert_eq!(run.json["outcome"], "passed", "{}", run.json);
    assert_eq!(
        run.json["summary"]["syntax"], "introduced_errors",
        "{}",
        run.json
    );
    Ok(())
}

#[test]
fn trace_verbosity_carries_per_file_syntax_facts() -> TestResult {
    let griz = Griz::new()?;
    griz.write("a.rs", "fn f() {\n    1;\n}\n")?;
    let ops = r#"[{"op":"replace","path":"a.rs","find":{"text":"}\n"},"replace":""}]"#;
    let run = griz.run(&[
        "plan",
        "--ops",
        ops,
        "--purpose",
        "t",
        "--idempotency-key",
        "tr",
        "--verbosity",
        "trace",
    ])?;
    assert_eq!(run.json["outcome"], "passed", "{}", run.json);
    assert_eq!(run.json["syntax"], "introduced_errors", "{}", run.json);
    let file = &run.json["files"][0];
    assert_eq!(file["syntax"]["after_errors"], 1, "{}", run.json);
    Ok(())
}
