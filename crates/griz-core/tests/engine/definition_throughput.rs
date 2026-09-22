//! Repeatable definition-index timing and an opt-in sampling workload.

use griz_core::diff_items;
use std::{
    fmt::Write,
    hint::black_box,
    path::Path,
    time::{Duration, Instant},
};

fn input(count: usize) -> Result<String, std::fmt::Error> {
    let padding = format!("    // {}\n", "x".repeat(180)).repeat(4);
    let mut text = String::new();
    for index in 0..count {
        writeln!(text, "fn item_{index}() {{\n{padding}    work();\n}}")?;
    }
    Ok(text)
}

#[test]
#[ignore = "manual release-mode definition index timing"]
fn definition_index_measurement() -> Result<(), std::fmt::Error> {
    for count in [100, 1000, 5000] {
        let before = input(count)?;
        let after = before.replacen("work();", "changed();", 1);
        let mut samples = Vec::new();
        for _ in 0..7 {
            let start = Instant::now();
            let items = black_box(diff_items(Path::new("a.rs"), Some(&before), Some(&after)));
            samples.push(start.elapsed().as_micros());
            assert_eq!(items.len(), 1);
            assert_eq!(items[0].name, "item_0");
        }
        samples.sort_unstable();
        println!("definition_index count={count} median_us={}", samples[3]);
    }
    Ok(())
}

#[test]
#[ignore = "manual release-mode definition index sampling"]
fn definition_index_profile() -> Result<(), std::fmt::Error> {
    let before = input(5000)?;
    let after = before.replacen("work();", "changed();", 1);
    println!("definition_profile_ready pid={}", std::process::id());
    let start = Instant::now();
    let mut rounds = 0;
    while start.elapsed() < Duration::from_secs(12) {
        let items = black_box(diff_items(Path::new("a.rs"), Some(&before), Some(&after)));
        assert_eq!(items.len(), 1);
        rounds += 1;
    }
    println!("definition_profile_rounds={rounds}");
    Ok(())
}
