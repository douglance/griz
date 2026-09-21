//! A process killed between renames leaves a journal the next process finishes.

use crate::common::{Griz, TestResult};

#[test]
fn an_apply_killed_after_its_first_rename_is_rolled_forward() -> TestResult {
    let griz = Griz::new()?;
    griz.write("a.rs", "a\n")?;
    griz.write("b.rs", "b\n")?;
    let ops = r#"[{"op":"replace","path":"a.rs","find":{"text":"a"},"replace":"A"},{"op":"replace","path":"b.rs","find":{"text":"b"},"replace":"B"}]"#;
    let plan = griz.id(&["plan", "--ops", ops], "plan")?;
    let killed = griz
        .command(&[
            "apply",
            &plan,
            "--purpose",
            "t",
            "--idempotency-key",
            "apply",
        ])
        .env("GRIZ_FAILPOINT", "after_first_rename")
        .output()?;
    assert!(
        !killed.status.success(),
        "the failpoint must stop the process"
    );
    assert_eq!(
        (griz.read("a.rs")?, griz.read("b.rs")?),
        ("A\n".into(), "b\n".into()),
        "half-applied before recovery"
    );
    let log = griz.run(&["log"])?;
    assert_eq!(
        (griz.read("a.rs")?, griz.read("b.rs")?),
        ("A\n".into(), "B\n".into())
    );
    let op = &log.json["operations"][0];
    assert_eq!(op["state"], "applied");
    let id = op["id"].as_str().ok_or("no id")?;
    assert_eq!(griz.run(&["get", id])?.json["recovered"], true);
    Ok(())
}

#[test]
fn an_apply_killed_mid_stage_journals_nothing_to_recover() -> TestResult {
    // Every file must be staged before the operation is journaled `applying`.
    // Killing the process while staging is still in progress, before any
    // journal write, must leave no dangling operation for the next process
    // to roll forward: nothing was ready to land yet.
    let griz = Griz::new()?;
    griz.write("a.rs", "a\n")?;
    griz.write("b.rs", "b\n")?;
    let ops = r#"[{"op":"replace","path":"a.rs","find":{"text":"a"},"replace":"A"},{"op":"replace","path":"b.rs","find":{"text":"b"},"replace":"B"}]"#;
    let plan = griz.id(&["plan", "--ops", ops], "plan")?;
    let killed = griz
        .command(&[
            "apply",
            &plan,
            "--purpose",
            "t",
            "--idempotency-key",
            "apply",
        ])
        .env("GRIZ_FAILPOINT", "mid_stage")
        .output()?;
    assert!(
        !killed.status.success(),
        "the failpoint must stop the process"
    );
    let log = griz.run(&["log"])?;
    assert_eq!(
        log.json["operations"].as_array().map(Vec::len),
        Some(0),
        "nothing was journaled before the crash: {}",
        log.json
    );
    assert_eq!(
        (griz.read("a.rs")?, griz.read("b.rs")?),
        ("a\n".into(), "b\n".into()),
        "nothing was renamed before the crash"
    );
    Ok(())
}

#[test]
fn an_apply_killed_right_after_the_journal_is_rolled_forward_completely() -> TestResult {
    // Every file is staged before the operation is journaled `applying`, and
    // the journal lands before the first rename. Killing the process in that
    // narrow window, with every file still staged and none renamed yet, must
    // still leave the next process able to finish every file, not just some.
    let griz = Griz::new()?;
    griz.write("a.rs", "a\n")?;
    griz.write("b.rs", "b\n")?;
    let ops = r#"[{"op":"replace","path":"a.rs","find":{"text":"a"},"replace":"A"},{"op":"replace","path":"b.rs","find":{"text":"b"},"replace":"B"}]"#;
    let plan = griz.id(&["plan", "--ops", ops], "plan")?;
    let killed = griz
        .command(&[
            "apply",
            &plan,
            "--purpose",
            "t",
            "--idempotency-key",
            "apply",
        ])
        .env("GRIZ_FAILPOINT", "after_journal")
        .output()?;
    assert!(
        !killed.status.success(),
        "the failpoint must stop the process"
    );
    assert_eq!(
        (griz.read("a.rs")?, griz.read("b.rs")?),
        ("a\n".into(), "b\n".into()),
        "nothing was renamed before the crash"
    );
    let log = griz.run(&["log"])?;
    assert_eq!(
        (griz.read("a.rs")?, griz.read("b.rs")?),
        ("A\n".into(), "B\n".into()),
        "recovery must finish every staged file, not just the first"
    );
    let op = &log.json["operations"][0];
    assert_eq!(op["state"], "applied");
    let id = op["id"].as_str().ok_or("no id")?;
    assert_eq!(griz.run(&["get", id])?.json["recovered"], true);
    Ok(())
}
