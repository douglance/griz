//! Parse facts on `plan`: shown from info verbosity up, default unchanged.

use crate::common::{Griz, TestResult};

#[test]
fn expect_syntax_clean_fails_a_plan_that_breaks_the_parse() -> TestResult {
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
        "es1",
        "--expect-syntax",
        "clean",
        "--verbosity",
        "warn",
    ])?;
    assert_eq!(run.json["outcome"], "failed", "{}", run.json);
    assert!(
        run.json["reason"]
            .as_str()
            .unwrap_or_default()
            .contains("syntax"),
        "the reason names syntax: {}",
        run.json
    );
    Ok(())
}

#[test]
fn expect_syntax_clean_passes_a_plan_that_still_parses() -> TestResult {
    let griz = Griz::new()?;
    griz.write("a.rs", "fn f() {\n    1;\n}\n")?;
    let ops = r#"[{"op":"replace","path":"a.rs","find":{"text":"1;"},"replace":"2;"}]"#;
    let run = griz.run(&[
        "plan",
        "--ops",
        ops,
        "--purpose",
        "t",
        "--idempotency-key",
        "es2",
        "--expect-syntax",
        "clean",
    ])?;
    assert_eq!(run.json["outcome"], "passed", "{}", run.json);
    Ok(())
}

#[test]
fn expect_syntax_clean_passes_a_file_in_no_supported_language() -> TestResult {
    let griz = Griz::new()?;
    griz.write("notes.txt", "one\n")?;
    let ops = r#"[{"op":"replace","path":"notes.txt","find":{"text":"one"},"replace":"two"}]"#;
    let run = griz.run(&[
        "plan",
        "--ops",
        ops,
        "--purpose",
        "t",
        "--idempotency-key",
        "es3",
        "--expect-syntax",
        "clean",
    ])?;
    assert_eq!(run.json["outcome"], "passed", "{}", run.json);
    Ok(())
}

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
