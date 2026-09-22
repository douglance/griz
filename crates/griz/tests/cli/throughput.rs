//! Manual measurements of full CLI commands, including startup and storage.

use crate::common::Griz;
use serde_json::{Value, json};
use std::{error::Error, time::Instant};

type BenchResult<T> = Result<T, Box<dyn Error>>;

fn timed(griz: &Griz, args: &[&str]) -> BenchResult<(Value, u128)> {
    let start = Instant::now();
    let run = griz.run(args)?;
    let elapsed = start.elapsed().as_micros();
    assert_eq!(run.code, Some(0), "{}", run.json);
    assert_eq!(run.json["outcome"], "passed", "{}", run.json);
    Ok((run.json, elapsed))
}

fn prepare(count: usize) -> BenchResult<(Griz, String)> {
    let griz = Griz::new()?;
    let text = format!(
        "fn main() {{\n{}}}\n",
        "    let oldName = 1;\n".repeat(count)
    );
    griz.write("a.rs", &text)?;
    let count = count.to_string();
    let (found, _) = timed(
        &griz,
        &[
            "find",
            "--literal",
            "oldName",
            "--paths",
            "a.rs",
            "--limit",
            &count,
            "--expect-matches",
            &count,
        ],
    )?;
    let matches = found["matches"].as_array().ok_or("no matches")?;
    let ops: Vec<_> = matches
        .iter()
        .map(|hit| {
            json!({
                "op": "replace", "path": hit["path"], "range": hit["range"],
                "find": {"text": hit["text"]}, "replace": "new_name",
                "expect_hash": hit["file_hash"],
            })
        })
        .collect();
    griz.write("ops.json", &serde_json::to_string(&ops)?)?;
    Ok((griz, text))
}

fn sample(griz: &Griz, count: usize, trial: usize) -> BenchResult<[u128; 5]> {
    let key = format!("trial-{trial}");
    let count = count.to_string();
    let args = [
        "plan",
        "--ops",
        "@ops.json",
        "--expect-edits",
        &count,
        "--expect-syntax",
        "clean",
        "--purpose",
        "benchmark",
        "--idempotency-key",
        &key,
    ];
    let (plan, create) = timed(griz, &args)?;
    let (replayed, replay) = timed(griz, &args)?;
    assert_eq!(replayed["id"], plan["id"]);
    assert_eq!(replayed["replayed"], true);
    let (detail, info) = timed(griz, &[&args[..], &["--verbosity", "info"]].concat())?;
    assert_eq!(detail["summary"]["files"], 1);
    let plan = plan["id"].as_str().ok_or("no plan id")?;
    let (operation, apply) = timed(
        griz,
        &[
            "apply",
            plan,
            "--purpose",
            "benchmark",
            "--idempotency-key",
            &key,
        ],
    )?;
    let operation = operation["id"].as_str().ok_or("no operation id")?;
    let (_, undo) = timed(
        griz,
        &[
            "undo",
            operation,
            "--purpose",
            "benchmark",
            "--idempotency-key",
            &key,
        ],
    )?;
    Ok([create, replay, info, apply, undo])
}

#[test]
#[ignore = "manual release-mode CLI round-trip timing"]
fn cli_round_trip_measurement() -> BenchResult<()> {
    for count in [1_000, 10_000] {
        let (griz, original) = prepare(count)?;
        let mut samples = Vec::new();
        for trial in 0..3 {
            samples.push(sample(&griz, count, trial)?);
            assert_eq!(griz.read("a.rs")?, original);
        }
        for (index, stage) in ["plan", "replay", "replay_info", "apply", "undo"]
            .iter()
            .enumerate()
        {
            let mut timings: Vec<_> = samples.iter().map(|sample| sample[index]).collect();
            timings.sort_unstable();
            println!("cli count={count} stage={stage} median_us={}", timings[1]);
        }
    }
    Ok(())
}
