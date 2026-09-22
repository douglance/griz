//! Manual measurement of selection over large stored plans.

use crate::common::Fixture;
use griz_core::{ByteRange, Confidence, Occurrence, Op};
use griz_store::{PlanRecord, Selection};
use std::{error::Error, hint::black_box, time::Instant};

type BenchResult<T> = Result<T, Box<dyn Error>>;

fn plan(fx: &Fixture, count: usize) -> BenchResult<PlanRecord> {
    fx.write("bulk.txt", &"old\n".repeat(count))?;
    let ops = (0..count)
        .map(|index| Op::Replace {
            path: fx.path("bulk.txt"),
            find: None,
            range: Some(ByteRange {
                start: index * 4,
                end: index * 4 + 3,
            }),
            pattern: None,
            replace: "new".into(),
            occurrence: Occurrence::Unique,
            target: None,
            expect_hash: None,
        })
        .collect();
    let plan = fx.plan(ops)?;
    assert!(plan.problems.is_empty(), "{:?}", plan.problems);
    Ok(plan)
}

fn measure(
    fx: &Fixture,
    original: &PlanRecord,
    selection: &Selection,
    expected: &str,
) -> BenchResult<u128> {
    let mut samples = Vec::new();
    for _ in 0..3 {
        let start = Instant::now();
        let selected = black_box(fx.store.select(&original.id, selection, "benchmark")?);
        samples.push(start.elapsed().as_micros());
        assert!(selected.problems.is_empty(), "{:?}", selected.problems);
        assert_eq!(selected.files.len(), 1);
        let changes = fx.store.plan_changes(&selected)?;
        assert_eq!(changes[0].after.as_deref(), Some(expected));
    }
    samples.sort_unstable();
    Ok(samples[1])
}

#[test]
#[ignore = "manual release-mode selection timing"]
fn selection_measurement() -> BenchResult<()> {
    for count in [1_000, 5_000, 10_000] {
        let fx = Fixture::new()?;
        let original = plan(&fx, count)?;
        let cases = [
            ("all", Selection::default(), "new\n".repeat(count)),
            (
                "half",
                Selection {
                    edits: (0..count).step_by(2).map(|i| format!("e{i}")).collect(),
                    ..Selection::default()
                },
                "new\nold\n".repeat(count / 2),
            ),
            (
                "machine",
                Selection {
                    min_confidence: Some(Confidence::Machine),
                    ..Selection::default()
                },
                "new\n".repeat(count),
            ),
        ];
        for (name, selection, expected) in cases {
            let median = measure(&fx, &original, &selection, &expected)?;
            println!("selection count={count} mode={name} median_us={median}");
        }
    }
    Ok(())
}
