//! griz composed with apoc in one Code Mode program.
//!
//! Needs `apoc` on PATH, so it is opt-in: `cargo test -p griz -- --ignored`.
//! apoc discovers griz from an isolated configuration root, never the
//! developer's real agent configuration, and its daemon is stopped on drop.

use crate::common::TestResult;
use serde_json::Value;
use std::{path::Path, process::Command};

struct Apoc {
    root: tempfile::TempDir,
    runtime: tempfile::TempDir,
}

impl Apoc {
    fn new(griz_home: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        // A Unix socket path must stay short, so the runtime lives under /tmp.
        let runtime = tempfile::Builder::new().prefix("gz").tempdir_in("/tmp")?;
        let config = serde_json::json!({ "mcpServers": { "griz": {
            "type": "stdio",
            "command": env!("CARGO_BIN_EXE_griz"),
            "args": ["--mcp"],
            "env": { "GRIZ_HOME": griz_home },
        }}});
        std::fs::write(root.path().join(".claude.json"), config.to_string())?;
        Ok(Self { root, runtime })
    }

    fn command(&self, cwd: &Path) -> Command {
        let mut command = Command::new("apoc");
        command
            .current_dir(cwd)
            .env("APOC_HOME", self.root.path())
            .env("APOC_MCP_ROOT", self.root.path())
            .env("APOC_RUNTIME_DIR", self.runtime.path())
            .env("XDG_DATA_HOME", self.root.path().join("xdg"))
            .env("APOC_MCP_DISCOVERY", "1");
        command
    }

    fn run(
        &self,
        cwd: &Path,
        program: &str,
        key: &str,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let output = self
            .command(cwd)
            .args(["code", "run", program, "--purpose", "griz composition test"])
            .args(["--idempotency-key", key, "--format", "json"])
            .output()?;
        Ok(serde_json::from_slice(&output.stdout)?)
    }
}

impl Drop for Apoc {
    fn drop(&mut self) {
        let _ = self
            .command(self.root.path())
            .args(["daemon", "stop", "--purpose", "stop test daemon"])
            .args(["--idempotency-key", "griz-compose-stop"])
            .output();
    }
}

const PROGRAM: &str = r#"
const found = await griz.find({ literal: "oldName", root: W, expect_matches: 3 });
const ops = found.matches.map(m => ({ op: "replace", path: m.path, range: m.range,
  find: { text: m.text }, replace: "new_name", expect_hash: m.file_hash }));
const plan = await griz.plan({ ops, root: W, purpose: "rename", idempotency_key: "plan", expect_edits: 3 });
const applied = await griz.apply({ plan: plan.id, purpose: "write", idempotency_key: "apply" });
const check = await apoc.execution_command({ executable: "sh", cwd: W,
  arg: ["-c", "grep -q new_name a.rs && exit 3 || exit 0"], expect_exit_code: [0],
  purpose: "a check that fails after the rename", idempotency_key: "check" });
let undone = null;
if (check.outcome !== "passed") undone = await griz.undo({ operation: applied.id, purpose: "roll back", idempotency_key: "undo" });
return { matches: found.total, applied: applied.outcome, check: check.outcome, undone: undone && undone.outcome };
"#;

#[test]
#[ignore = "needs apoc on PATH"]
fn apoc_program_applies_checks_and_rolls_back_through_griz() -> TestResult {
    let work = tempfile::tempdir()?;
    let griz_home = tempfile::tempdir()?;
    std::fs::create_dir_all(work.path().join(".git"))?;
    std::fs::write(
        work.path().join("a.rs"),
        "let oldName = 1;\nuse_it(oldName);\n",
    )?;
    std::fs::write(work.path().join("b.rs"), "oldName();\n")?;
    let apoc = Apoc::new(griz_home.path())?;
    let program = format!("const W = {:?};\n{PROGRAM}", work.path());
    for attempt in ["first", "retry"] {
        let state = apoc.run(work.path(), &program, attempt)?;
        assert_eq!(state["status"], "completed", "{state}");
        let result = &state["result"];
        assert_eq!(result["matches"], 3, "{state}");
        assert_eq!(
            (result["applied"].as_str(), result["check"].as_str()),
            (Some("passed"), Some("failed"))
        );
        assert_eq!(result["undone"], "passed");
    }
    assert_eq!(
        std::fs::read_to_string(work.path().join("a.rs"))?,
        "let oldName = 1;\nuse_it(oldName);\n"
    );
    let log = Command::new(env!("CARGO_BIN_EXE_griz"))
        .args(["log", "--json"])
        .env("GRIZ_HOME", griz_home.path())
        .env("XDG_DATA_HOME", griz_home.path().join("xdg"))
        .output()?;
    let log: Value = serde_json::from_slice(&log.stdout)?;
    let kinds: Vec<_> = log["operations"]
        .as_array()
        .ok_or("no log")?
        .iter()
        .map(|op| op["kind"].clone())
        .collect();
    assert_eq!(
        kinds,
        [serde_json::json!("undo"), serde_json::json!("apply")],
        "a retried program must not write twice"
    );
    Ok(())
}

const COUNT_PROGRAM: &str = r#"
const plan = await griz.plan({root: W,
  ops: [{op: "replace", path: "a.txt", find: "before", replace: "after"}],
  purpose: "plan", idempotency_key: "plan"});
if (plan.outcome !== "passed") return plan;
const args = {plan: plan.id, expect_files: 2, purpose: "apply", idempotency_key: "apply"};
const first = await griz.apply(args);
const replay = await griz.apply(args);
const record = await griz.get({id: first.id});
const log = await griz.log({});
return {first, replay, record, operations: log.operations};
"#;

fn assert_count_refusal(result: &Value) -> TestResult {
    assert_eq!(result["first"]["outcome"], "failed", "{result}");
    assert_eq!(result["replay"]["outcome"], "failed");
    assert_eq!(result["first"]["id"], result["replay"]["id"]);
    assert_eq!(result["replay"]["replayed"], true);
    assert_eq!(result["record"]["state"], "failed");
    assert_eq!(result["record"]["files"], serde_json::json!([]));
    let operations = result["operations"].as_array().ok_or("no operations")?;
    assert_eq!(operations.len(), 1, "{result}");
    assert_eq!(operations[0]["kind"], "apply");
    Ok(())
}

#[test]
#[ignore = "needs apoc on PATH"]
fn apoc_apply_count_failure_needs_no_rollback() -> TestResult {
    let work = tempfile::tempdir()?;
    let griz_home = tempfile::tempdir()?;
    std::fs::write(work.path().join("a.txt"), "before")?;
    let apoc = Apoc::new(griz_home.path())?;
    let program = format!("const W = {:?};\n{COUNT_PROGRAM}", work.path());
    let state = apoc.run(work.path(), &program, "count-failure")?;
    assert_eq!(state["status"], "completed", "{state}");
    assert_count_refusal(&state["result"])?;
    assert_eq!(
        std::fs::read_to_string(work.path().join("a.txt"))?,
        "before"
    );
    Ok(())
}
