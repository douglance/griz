//! `@path` input: every JSON input a command takes can come from a file, as
//! patch text already does, so a program never has to pass a large document
//! on the command line.

use crate::common::{Griz, TestResult};

#[test]
fn ops_can_be_read_from_a_file() -> TestResult {
    let griz = Griz::new()?;
    griz.write("a.rs", "fn f() {\n    1;\n}\n")?;
    griz.write(
        "ops.json",
        r#"[{"op":"replace","path":"a.rs","find":{"text":"1;"},"replace":"2;"}]"#,
    )?;
    let run = griz.run(&[
        "plan",
        "--ops",
        "@ops.json",
        "--purpose",
        "t",
        "--idempotency-key",
        "ops-at-file",
        "--expect-edits",
        "1",
    ])?;
    assert_eq!(run.json["outcome"], "passed", "{}", run.json);
    Ok(())
}

#[test]
fn a_workspace_edit_can_be_read_from_a_file() -> TestResult {
    let griz = Griz::new()?;
    griz.write("a.rs", "const OLD: u8 = 1;\n")?;
    let uri = format!("file://{}", griz.path("a.rs").display());
    let edit = format!(
        r#"{{"changes":{{"{uri}":[{{"range":{{"start":{{"line":0,"character":6}},"end":{{"line":0,"character":9}}}},"newText":"NEW"}}]}}}}"#
    );
    griz.write("edit.json", &edit)?;
    let run = griz.run(&[
        "plan",
        "--workspace-edit",
        "@edit.json",
        "--purpose",
        "t",
        "--idempotency-key",
        "we-at-file",
        "--expect-edits",
        "1",
    ])?;
    assert_eq!(run.json["outcome"], "passed", "{}", run.json);
    Ok(())
}

#[test]
fn a_missing_input_file_is_an_error_naming_it() -> TestResult {
    let griz = Griz::new()?;
    let run = griz.run(&[
        "plan",
        "--ops",
        "@nope.json",
        "--purpose",
        "t",
        "--idempotency-key",
        "missing-at-file",
    ])?;
    assert_ne!(run.json["outcome"], "passed", "{}", run.json);
    let text = run.json.to_string();
    assert!(text.contains("nope.json"), "{text}");
    Ok(())
}
