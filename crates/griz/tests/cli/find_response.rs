//! Search responses preserve complete result shapes across typed serialization.
use super::common::{Griz, TestResult};
use serde_json::{Value, json};
use std::error::Error;
const HASH_A: &str = "c3e73ec1a00ae8723ccea74442937c4e0f0714b1b499d3ebde1162205b0c6d52";
const HASH_B: &str = "1536ddb52f79262d59dbbcd2bd2e13c80b2741a9d71aeee59c12492f8b9cd86f";
fn fixture() -> Result<Griz, Box<dyn Error>> {
    let griz = Griz::new()?;
    griz.write("a.txt", "needle one\nneedle two\n")?;
    griz.write("b.txt", "prefix needle three\n")?;
    Ok(griz)
}
fn first_match(griz: &Griz, line: usize, start: usize) -> Result<Value, Box<dyn Error>> {
    Ok(json!({
        "path": std::fs::canonicalize(griz.path("a.txt"))?,
        "line": line, "column": 1, "range": {"start": start, "end": start + 6},
        "text": "needle", "captures": [], "file_hash": HASH_A,
    }))
}
fn literal_page(griz: &Griz) -> Result<Value, Box<dyn Error>> {
    Ok(json!({
        "matches": [
            first_match(griz, 1, 0)?,
            first_match(griz, 2, 11)?,
            {
                "path": std::fs::canonicalize(griz.path("b.txt"))?,
                "line": 1, "column": 8, "range": {"start": 7, "end": 13},
                "text": "needle", "captures": [], "file_hash": HASH_B,
            }
        ],
        "total": 3, "files": 2, "next": null,
    }))
}
#[test]
fn typed_find_literal_result_preserves_all_fields() -> TestResult {
    let griz = fixture()?;
    let run = griz.run(&[
        "find",
        "--paths",
        "a.txt",
        "--paths",
        "b.txt",
        "--literal",
        "needle",
    ])?;
    assert_eq!(run.code, Some(0));
    assert_eq!(run.json, literal_page(&griz)?);
    Ok(())
}
#[test]
fn typed_find_regex_result_preserves_captures_and_paging() -> TestResult {
    let griz = fixture()?;
    let run = griz.run(&[
        "find",
        "--paths",
        "a.txt",
        "--paths",
        "b.txt",
        "--regex",
        "needle (\\w+)",
        "--offset",
        "1",
        "--limit",
        "1",
    ])?;
    let expected = json!({
        "matches": [{
            "path": std::fs::canonicalize(griz.path("a.txt"))?,
            "line": 2, "column": 1, "range": {"start": 11, "end": 21},
            "text": "needle two", "captures": ["two"], "file_hash": HASH_A,
        }],
        "total": 3, "files": 2, "next": 2,
    });
    assert_eq!(run.code, Some(0));
    assert_eq!(run.json, expected);
    Ok(())
}
#[test]
fn typed_find_file_summary_preserves_counts_fingerprints_and_paging() -> TestResult {
    let griz = fixture()?;
    let run = griz.run(&[
        "find",
        "--paths",
        "a.txt",
        "--paths",
        "b.txt",
        "--literal",
        "needle",
        "--files-only",
        "--limit",
        "1",
    ])?;
    let expected = json!({
        "file_matches": [{
            "path": std::fs::canonicalize(griz.path("a.txt"))?,
            "count": 2, "file_hash": HASH_A,
        }],
        "total": 3, "files": 2, "next": 1,
    });
    assert_eq!(run.code, Some(0));
    assert_eq!(run.json, expected);
    Ok(())
}
#[test]
fn typed_find_expectations_preserve_outcomes_reasons_and_exit_codes() -> TestResult {
    let griz = fixture()?;
    for (count, code, outcome, reason) in [
        ("3", 0, "passed", None),
        ("4", 1, "failed", Some("expected 4 matches, observed 3")),
    ] {
        let run = griz.run(&[
            "find",
            "--paths",
            "a.txt",
            "--paths",
            "b.txt",
            "--literal",
            "needle",
            "--expect-matches",
            count,
        ])?;
        let mut expected = literal_page(&griz)?;
        expected["outcome"] = json!(outcome);
        if let Some(reason) = reason {
            expected["reason"] = json!(reason);
        }
        assert_eq!(run.code, Some(code));
        assert_eq!(run.json, expected);
    }
    Ok(())
}

#[test]
#[cfg(target_os = "linux")]
fn typed_find_retains_validation_error_for_non_utf8_result_paths() -> TestResult {
    use std::os::unix::ffi::OsStringExt;
    let griz = Griz::new()?;
    let name = std::ffi::OsString::from_vec(vec![b'n', 0xff]);
    std::fs::write(griz.work.path().join(name), "needle")?;
    for files_only in [false, true] {
        let mut args = vec!["find", "--literal", "needle"];
        if files_only {
            args.push("--files-only");
        }
        let run = griz.run(&args)?;
        assert_eq!(run.code, Some(1));
        assert_eq!(
            run.json,
            json!({
                "code": "VALIDATION_ERROR",
                "message": "path contains invalid UTF-8 characters"
            })
        );
    }
    Ok(())
}
