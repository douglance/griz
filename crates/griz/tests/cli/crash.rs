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
