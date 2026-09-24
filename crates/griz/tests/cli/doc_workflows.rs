//! Runs the documented programs through real apoc and griz tools.

use crate::{common::TestResult, compose::Apoc};
use serde_json::Value;
use std::{error::Error, fs, path::Path};

const README: &str = include_str!("../../../../README.md");
const SKILL: &str = include_str!("../../../../skills/griz/references/compose.md");
pub(super) const CONCURRENT: &str = "// changed after the check\n";

pub(super) struct Observed {
    pub state: Value,
    pub before: String,
    pub after: String,
}

fn js_blocks(markdown: &str) -> Vec<&str> {
    markdown
        .split("\x60\x60\x60js\n")
        .skip(1)
        .filter_map(|block| block.split("\x60\x60\x60").next())
        .collect()
}

fn example(which: &str) -> Result<String, Box<dyn Error>> {
    if which == "readme" {
        let blocks = js_blocks(README);
        let code = blocks.first().ok_or("README has no JS example")?;
        return Ok(code.replace("const root = \"/path/to/repo\";", "const root = W;"));
    }
    let blocks = js_blocks(SKILL);
    let first = blocks.first().ok_or("skill has no find example")?;
    let second = blocks.get(1).ok_or("skill has no apply example")?;
    Ok(format!("const root = W;\n{first}\n{second}"))
}

#[test]
fn documented_examples_are_available() -> TestResult {
    for which in ["readme", "skill"] {
        example(which)?;
    }
    Ok(())
}

fn fixture(work: &Path, count: usize, external_reference: bool) -> Result<String, Box<dyn Error>> {
    fs::create_dir_all(work.join(".git"))?;
    fs::create_dir_all(work.join("src"))?;
    fs::write(work.join(".gitignore"), "/target/\n")?;
    fs::write(
        work.join("Cargo.toml"),
        "[package]\nname = \"griz-doc-example\"\nversion = \"0.0.0\"\nedition = \"2024\"\n",
    )?;
    let calls = "    oldName();\n".repeat(count - 1);
    let extra = if external_reference {
        "    include!(\"caller.inc\");\n"
    } else {
        ""
    };
    let text =
        format!("#[allow(non_snake_case)]\nfn oldName() {{}}\nfn main() {{\n{calls}{extra}}}\n");
    fs::write(work.join("src/main.rs"), &text)?;
    fs::write(work.join("src/caller.inc"), "oldName()")?;
    Ok(text)
}

const OBSERVE: &str = r#"
const calls = [];
const check_logs = [];
let pending = null;
const observedGriz = {
  find: async args => { calls.push("find"); return await griz.find(args); },
  plan: async args => {
    calls.push("plan");
    return await griz.plan(S === "plan" ? {...args, expect_edits: 13} : args);
  },
  apply: async args => {
    calls.push("apply");
    return await griz.apply(S === "apply" ? {...args, expect_files: 2} : args);
  },
  undo: async args => {
    calls.push("undo");
    if (S === "undo") {
      const plan = await griz.plan({root: W,
        ops: [{op: "create", path: "src/main.rs", text: C, overwrite: true}],
        purpose: "concurrent change", idempotency_key: "concurrent-plan"});
      if (plan.outcome !== "passed") throw Error("concurrent plan failed");
      const written = await griz.apply({plan: plan.id,
        purpose: "concurrent change", idempotency_key: "concurrent-apply"});
      if (written.outcome !== "passed") throw Error("concurrent write failed");
    }
    return await griz.undo(args);
  },
};
const observedApoc = {
  execution_command: async args => {
    calls.push("check");
    if (S === "pending") {
      pending = await apoc.execution_start({...args,
        executable: "sleep", arg: ["30"], timeout_ms: 30000});
      return pending;
    }
    const result = await apoc.execution_command({...args, executable: CARGO,
      arg: [...args.arg, "--target-dir", `${W}/target/after`]});
    check_logs.push(await apoc.execution_logs({id: result.id,
      purpose: "inspect example compile check", grep: ".", max_matches: 40}));
    return result;
  },
};
"#;

fn check_before_rename(apoc: &Apoc, work: &Path) -> TestResult {
    let directory = serde_json::to_string(work)?;
    let cargo = serde_json::to_string(env!("CARGO"))?;
    let target = serde_json::to_string(&work.join("target/before"))?;
    let program = format!(
        "return await apoc.execution_command({{executable:{cargo},arg:['check','--target-dir',{target}],cwd:{directory},purpose:'check original fixture',idempotency_key:'before-rename',expect_exit_code:[0]}});"
    );
    let checked = apoc.run(work, &program, "before-rename")?;
    assert_eq!(checked["result"]["outcome"], "passed", "{checked}");
    Ok(())
}

fn program(which: &str, scenario: &str, work: &Path) -> Result<String, Box<dyn Error>> {
    let directory = serde_json::to_string(work)?;
    let scenario = serde_json::to_string(scenario)?;
    let cargo = serde_json::to_string(env!("CARGO"))?;
    let concurrent = serde_json::to_string(CONCURRENT)?;
    let code = example(which)?
        .replace("griz.", "observedGriz.")
        .replace("apoc.", "observedApoc.");
    Ok(format!(
        r#"
const W = {directory}, S = {scenario}, C = {concurrent}, CARGO = {cargo};
{OBSERVE}
try {{
  const workflow = await (async () => {{
{code}
  }})();
  const pending_state = pending
    ? await apoc.execution_get({{id: pending.id, purpose: "verify pending check"}}) : null;
  return {{workflow, calls, check_logs, pending_state, log: await griz.log({{}})}};
}} finally {{
  if (pending) await apoc.execution_cancel({{id: pending.id,
    purpose: "clean up pending example check", idempotency_key: "cancel-pending"}});
}}
"#
    ))
}

pub(super) fn run(which: &str, scenario: &str) -> Result<Observed, Box<dyn Error>> {
    let work = tempfile::tempdir()?;
    let home = tempfile::tempdir()?;
    let count = if scenario == "find" { 13 } else { 12 };
    let external_reference = matches!(scenario, "check" | "undo");
    let before = fixture(work.path(), count, external_reference)?;
    let apoc = Apoc::new(home.path())?;
    if external_reference {
        check_before_rename(&apoc, work.path())?;
    }
    let state = apoc.run(
        work.path(),
        &program(which, scenario, work.path())?,
        "documented-example",
    )?;
    let after = fs::read_to_string(work.path().join("src/main.rs"))?;
    Ok(Observed {
        state,
        before,
        after,
    })
}

fn assert_preflight(state: &Value, scenario: &str, expected_calls: &[&str]) -> TestResult {
    assert_eq!(state["status"], "completed", "{state}");
    let result = &state["result"];
    assert_eq!(
        result["calls"],
        serde_json::json!(expected_calls),
        "{state}"
    );
    assert_eq!(result["workflow"]["stage"], scenario, "{state}");
    assert_eq!(result["workflow"]["outcome"], "failed", "{state}");
    let operations = result["log"]["operations"]
        .as_array()
        .ok_or("no operation log")?;
    assert_eq!(
        operations.len(),
        usize::from(scenario == "apply"),
        "{state}"
    );
    assert!(operations.iter().all(|op| op["state"] == "failed"));
    Ok(())
}

fn preflight(which: &str, scenario: &str, calls: &[&str]) -> TestResult {
    let observed = run(which, scenario)?;
    assert_preflight(&observed.state, scenario, calls)?;
    assert_eq!(observed.after, observed.before);
    Ok(())
}

#[test]
#[ignore = "needs apoc and cargo on PATH"]
fn documented_examples_stop_after_find_failure() -> TestResult {
    for which in ["readme", "skill"] {
        preflight(which, "find", &["find"])?;
    }
    Ok(())
}

#[test]
#[ignore = "needs apoc and cargo on PATH"]
fn documented_examples_stop_after_plan_failure() -> TestResult {
    for which in ["readme", "skill"] {
        preflight(which, "plan", &["find", "plan"])?;
    }
    Ok(())
}

#[test]
#[ignore = "needs apoc and cargo on PATH"]
fn documented_examples_stop_after_apply_failure() -> TestResult {
    for which in ["readme", "skill"] {
        preflight(which, "apply", &["find", "plan", "apply"])?;
    }
    Ok(())
}
