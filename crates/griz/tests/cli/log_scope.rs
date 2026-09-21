//! `log --paths`: one store holds every repository's operations, so the
//! history can be narrowed to the files a caller cares about.

use crate::common::{Griz, TestResult};

/// Applies one single-file edit and returns its operation id.
fn edit(
    griz: &Griz,
    name: &str,
    from: &str,
    to: &str,
    key: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let ops = format!(
        r#"[{{"op":"replace","path":"{name}","find":{{"text":"{from}"}},"replace":"{to}"}}]"#
    );
    let plan = griz.id(
        &[
            "plan",
            "--ops",
            &ops,
            "--purpose",
            "t",
            "--idempotency-key",
            key,
        ],
        key,
    )?;
    let run = griz.run(&["apply", &plan, "--purpose", "t", "--idempotency-key", key])?;
    assert_eq!(run.json["outcome"], "passed", "{}", run.json);
    Ok(run.json["id"].as_str().unwrap_or_default().to_string())
}

#[test]
fn log_paths_hides_operations_that_touch_no_named_file() -> TestResult {
    let griz = Griz::new()?;
    griz.write("keep/a.rs", "fn f() { 1; }\n")?;
    griz.write("other/b.rs", "fn g() { 2; }\n")?;
    let kept = edit(&griz, "keep/a.rs", "1;", "11;", "k1")?;
    edit(&griz, "other/b.rs", "2;", "22;", "o1")?;

    let all = griz.run(&["log"])?;
    assert_eq!(all.json["operations"].as_array().map(Vec::len), Some(2));

    let scoped = griz.run(&["log", "--paths", "keep"])?;
    let ops = scoped.json["operations"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert_eq!(ops.len(), 1, "{}", scoped.json);
    assert_eq!(ops[0]["id"], kept);
    Ok(())
}

#[test]
fn log_paths_pages_past_operations_it_filtered_out() -> TestResult {
    let griz = Griz::new()?;
    griz.write("keep/a.rs", "fn f() { 1; }\n")?;
    griz.write("other/b.rs", "fn g() { 2; }\n")?;
    let first = edit(&griz, "keep/a.rs", "1;", "11;", "k1")?;
    edit(&griz, "other/b.rs", "2;", "22;", "o1")?;
    let second = edit(&griz, "keep/a.rs", "11;", "111;", "k2")?;

    let page = griz.run(&["log", "--paths", "keep", "--limit", "1"])?;
    let ops = page.json["operations"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert_eq!(ops.len(), 1);
    assert_eq!(ops[0]["id"], second);
    let next = page.json["next"].as_str().unwrap_or_default().to_string();
    assert!(!next.is_empty(), "{}", page.json);

    let rest = griz.run(&["log", "--paths", "keep", "--limit", "1", "--before", &next])?;
    let ops = rest.json["operations"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert_eq!(ops.len(), 1, "{}", rest.json);
    assert_eq!(ops[0]["id"], first);
    Ok(())
}
