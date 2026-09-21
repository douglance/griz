//! Idempotent replay for select and undo. apply is covered in verdicts.rs
//! and absorb in the replay-shape test alongside it.

use crate::common::{Griz, TestResult};

#[test]
fn a_repeated_select_key_replays_instead_of_making_a_new_plan() -> TestResult {
    let griz = Griz::new()?;
    griz.write("a.rs", "a\n")?;
    griz.write("b.rs", "b\n")?;
    let ops = r#"[{"op":"replace","path":"a.rs","find":{"text":"a"},"replace":"A"},{"op":"replace","path":"b.rs","find":{"text":"b"},"replace":"B"}]"#;
    let plan = griz.id(&["plan", "--ops", ops], "plan")?;
    let first = griz.run(&[
        "select",
        &plan,
        "--paths",
        "a.rs",
        "--purpose",
        "t",
        "--idempotency-key",
        "same",
    ])?;
    assert_eq!(first.json["outcome"], "passed", "{}", first.json);
    let again = griz.run(&[
        "select",
        &plan,
        "--paths",
        "a.rs",
        "--purpose",
        "t",
        "--idempotency-key",
        "same",
    ])?;
    assert_eq!(again.json["id"], first.json["id"]);
    assert_eq!(again.json["replayed"], true);
    Ok(())
}

#[test]
fn a_repeated_undo_key_replays_instead_of_restoring_twice() -> TestResult {
    let griz = Griz::new()?;
    griz.write("a.rs", "a\n")?;
    let ops = r#"[{"op":"replace","path":"a.rs","find":{"text":"a"},"replace":"A"}]"#;
    let plan = griz.id(&["plan", "--ops", ops], "plan")?;
    let op = griz.id(&["apply", &plan], "apply")?;
    let first = griz.run(&["undo", &op, "--purpose", "t", "--idempotency-key", "same"])?;
    assert_eq!(first.json["outcome"], "passed", "{}", first.json);
    assert_eq!(griz.read("a.rs")?, "a\n");
    // Tamper with the file so a wrongly re-executed undo would be visible.
    std::fs::write(griz.path("a.rs"), "tampered\n")?;
    let again = griz.run(&["undo", &op, "--purpose", "t", "--idempotency-key", "same"])?;
    assert_eq!(again.json["id"], first.json["id"]);
    assert_eq!(again.json["replayed"], true);
    assert_eq!(
        griz.read("a.rs")?,
        "tampered\n",
        "a replay must not touch the file again"
    );
    Ok(())
}

#[test]
fn a_repeated_undo_since_key_replays_instead_of_restoring_twice() -> TestResult {
    let griz = Griz::new()?;
    griz.write("a.rs", "one\n")?;
    let first_ops = r#"[{"op":"replace","path":"a.rs","find":{"text":"one"},"replace":"ONE"}]"#;
    let plan1 = griz.id(&["plan", "--ops", first_ops], "plan1")?;
    let op1 = griz.id(&["apply", &plan1], "apply1")?;
    let second_ops = r#"[{"op":"replace","path":"a.rs","find":{"text":"ONE"},"replace":"ONE2"}]"#;
    let plan2 = griz.id(&["plan", "--ops", second_ops], "plan2")?;
    griz.id(&["apply", &plan2], "apply2")?;
    let first = griz.run(&[
        "undo",
        "--since",
        &op1,
        "--purpose",
        "t",
        "--idempotency-key",
        "same",
    ])?;
    assert_eq!(first.json["outcome"], "passed", "{}", first.json);
    assert_eq!(griz.read("a.rs")?, "one\n");
    std::fs::write(griz.path("a.rs"), "tampered\n")?;
    let again = griz.run(&[
        "undo",
        "--since",
        &op1,
        "--purpose",
        "t",
        "--idempotency-key",
        "same",
    ])?;
    assert_eq!(again.json["id"], first.json["id"]);
    assert_eq!(again.json["replayed"], true);
    assert_eq!(
        griz.read("a.rs")?,
        "tampered\n",
        "a replay must not touch the file again"
    );
    Ok(())
}
