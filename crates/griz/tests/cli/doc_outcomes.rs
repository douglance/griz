//! Checks success, pending execution, rollback, and refused rollback in the examples.

use crate::{
    common::TestResult,
    doc_workflows::{self, Observed},
};
use serde_json::{Value, json};

fn assert_check(observed: &Observed, check: &str, undo: Option<&str>) {
    let state = &observed.state;
    assert_eq!(state["status"], "completed", "{state}");
    let workflow = &state["result"]["workflow"];
    assert_eq!(workflow["stage"], "check", "{state}");
    assert_eq!(workflow["check"]["outcome"], check, "{state}");
    assert_eq!(workflow["undo"]["outcome"].as_str(), undo, "{state}");
    assert!(
        workflow["operation"]
            .as_str()
            .is_some_and(|id| id.starts_with("op_"))
    );
}

fn assert_operations(state: &Value, count: usize, rolled_back: bool) {
    let result = &state["result"];
    let calls = if rolled_back {
        json!(["find", "plan", "apply", "check", "undo"])
    } else {
        json!(["find", "plan", "apply", "check"])
    };
    assert_eq!(result["calls"], calls, "{state}");
    assert_eq!(
        result["log"]["operations"].as_array().map(Vec::len),
        Some(count),
        "{state}"
    );
}

#[test]
#[ignore = "needs apoc and cargo on PATH"]
fn documented_examples_keep_successful_and_pending_writes() -> TestResult {
    for which in ["readme", "skill"] {
        let passed = doc_workflows::run(which, "pass")?;
        assert_check(&passed, "passed", None);
        assert_operations(&passed.state, 1, false);
        assert_eq!(passed.after, passed.before.replace("oldName", "new_name"));
        let pending = doc_workflows::run(which, "pending")?;
        assert_check(&pending, "pending", None);
        assert_operations(&pending.state, 1, false);
        assert_eq!(
            pending.state["result"]["pending_state"]["outcome"],
            "pending"
        );
        assert_eq!(pending.after, pending.before.replace("oldName", "new_name"));
    }
    Ok(())
}

#[test]
#[ignore = "needs apoc and cargo on PATH"]
fn documented_examples_report_successful_and_refused_undo() -> TestResult {
    for which in ["readme", "skill"] {
        let restored = doc_workflows::run(which, "check")?;
        assert_check(&restored, "failed", Some("passed"));
        assert_operations(&restored.state, 2, true);
        assert_eq!(restored.after, restored.before);
        let refused = doc_workflows::run(which, "undo")?;
        assert_check(&refused, "failed", Some("error"));
        assert_operations(&refused.state, 3, true);
        assert_eq!(refused.after, doc_workflows::CONCURRENT);
    }
    Ok(())
}
