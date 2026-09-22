//! Addressed reads preserve match counts and deduplicate overlapping context.

use crate::common::{Griz, TestResult};
use serde_json::json;

#[test]
fn grep_context_merges_overlaps_and_counts_matching_lines() -> TestResult {
    let griz = Griz::new()?;
    griz.write(
        "a.txt",
        "one\ntwo\nneedle needle\nfour\nneedle\nsix\nseven\neight\nnine\nneedle\neleven\ntwelve\n",
    )?;
    let run = griz.run(&["read", "a.txt", "--grep", "needle", "--context", "1"])?;
    assert_eq!(run.code, Some(0));
    assert_eq!(run.json["matched"], 3);
    assert_eq!(run.json["total_lines"], 12);
    let lines = run.json["lines"].as_array().ok_or("missing lines")?;
    let numbers: Vec<_> = lines.iter().map(|line| line["line"].clone()).collect();
    assert_eq!(json!(numbers), json!([2, 3, 4, 5, 6, 9, 10, 11]));
    assert_eq!(lines[1]["text"], "needle needle");
    Ok(())
}

#[test]
fn grep_without_matches_has_no_context_lines() -> TestResult {
    let griz = Griz::new()?;
    griz.write("a.txt", "one\ntwo\nthree\n")?;
    let run = griz.run(&["read", "a.txt", "--grep", "absent", "--context", "1000"])?;
    assert_eq!(run.code, Some(0));
    assert_eq!(run.json["matched"], 0);
    assert_eq!(run.json["total_lines"], 3);
    assert_eq!(run.json["lines"], json!([]));
    Ok(())
}
