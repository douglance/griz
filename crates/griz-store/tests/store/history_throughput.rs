//! Manual timing of first, deep, and sparse-filtered history pages.

use crate::{
    common::{Fixture, TestResult},
    history_pagination::seed,
};
use std::{hint::black_box, time::Instant};

fn measure(fx: &Fixture, count: usize, mode: &str) -> TestResult {
    let cursor = format!("op_{:032x}", count - 1_000);
    let before = (mode == "deep").then_some(cursor.as_str());
    let under = if mode == "filtered" {
        vec![fx.path("keep")]
    } else {
        Vec::new()
    };
    let expected: Vec<String> = match mode {
        "deep" => (980..1_000).rev().map(|n| n.to_string()).collect(),
        "filtered" => (1..=count)
            .rev()
            .filter(|n| n % 1_000 == 0)
            .take(20)
            .map(|n| n.to_string())
            .collect(),
        _ => (count - 19..=count).rev().map(|n| n.to_string()).collect(),
    };
    let mut samples = Vec::new();
    for _ in 0..3 {
        let start = Instant::now();
        let page = black_box(fx.store.operations_under(20, before, &under)?);
        samples.push(start.elapsed().as_micros());
        let actual: Vec<_> = page
            .operations
            .iter()
            .map(|op| op.purpose.clone())
            .collect();
        assert_eq!(actual, expected);
    }
    samples.sort_unstable();
    println!("history count={count} mode={mode} median_us={}", samples[1]);
    Ok(())
}

#[test]
#[ignore = "manual release-mode history timing"]
fn history_pagination_measurement() -> TestResult {
    for count in [10_000, 100_000] {
        let fx = Fixture::new()?;
        seed(&fx, count, 1_000)?;
        for mode in ["first", "deep", "filtered"] {
            measure(&fx, count, mode)?;
        }
    }
    Ok(())
}
