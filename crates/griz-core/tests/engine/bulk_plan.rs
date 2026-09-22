//! Measures the find-to-plan path used by bulk agent renames.

use crate::common::anchor;
use griz_core::{DiskSource, FindQuery, Match, Occurrence, Op, build_plan, find};
use std::{error::Error, fs, hint::black_box, time::Instant};

type TestResult = Result<(), Box<dyn Error>>;

fn operations(matches: &[Match], guarded: bool) -> Vec<Op> {
    matches
        .iter()
        .map(|hit| Op::Replace {
            path: hit.path.clone(),
            find: Some(anchor(&hit.text)),
            range: Some(hit.range),
            pattern: None,
            replace: "new_name".to_string(),
            occurrence: Occurrence::Unique,
            target: None,
            expect_hash: guarded.then(|| hit.file_hash.clone()),
        })
        .collect()
}

fn measure(ops: &[Op], count: usize) -> Result<u128, Box<dyn Error>> {
    let mut samples = Vec::new();
    let expected = "new_name payload\n".repeat(count);
    for _ in 0..3 {
        let start = Instant::now();
        let plan = black_box(build_plan(ops, &DiskSource));
        samples.push(start.elapsed().as_micros());
        assert!(plan.problems.is_empty(), "{:?}", plan.problems);
        assert_eq!(plan.edits.len(), count);
        let file = plan.files.first().ok_or("no changed file")?;
        assert_eq!(file.after.as_deref(), Some(expected.as_str()));
    }
    samples.sort_unstable();
    Ok(samples[1])
}

#[test]
#[ignore = "manual release-mode find-to-plan timing"]
fn bulk_plan_measurement() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("bulk.txt");
    for count in [1_000, 5_000, 10_000] {
        fs::write(&path, "oldName payload\n".repeat(count))?;
        let start = Instant::now();
        let page = find(&FindQuery {
            paths: vec![path.clone()],
            literal: Some("oldName".to_string()),
            limit: count,
            ..FindQuery::default()
        })?;
        println!("bulk_find count={count} us={}", start.elapsed().as_micros());
        assert_eq!(page.total, count);
        for guarded in [false, true] {
            let ops = operations(&page.matches, guarded);
            let median = measure(&ops, count)?;
            println!("bulk_plan count={count} guarded={guarded} median_us={median}");
        }
    }
    Ok(())
}
