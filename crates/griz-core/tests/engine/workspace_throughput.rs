//! Manual timing for batched LSP position conversion.

use crate::common::source;
use griz_core::{
    Position, PositionEncoding, Range, TextEdit, WorkspaceEdit, build_plan, workspace_edit_to_ops,
};
use std::{
    collections::BTreeMap, error::Error, hint::black_box, num::TryFromIntError, time::Instant,
};

type BenchResult<T> = Result<T, Box<dyn Error>>;

fn input(count: usize, multiline: bool, reverse: bool) -> BenchResult<(WorkspaceEdit, String)> {
    let mut edits = (0..count)
        .map(|index| {
            let index = u32::try_from(index)?;
            let (line, character) = if multiline {
                (index, 0)
            } else {
                (0, index * 4)
            };
            Ok(TextEdit {
                range: Range {
                    start: Position { line, character },
                    end: Position {
                        line,
                        character: character + 3,
                    },
                },
                new_text: "new".to_string(),
            })
        })
        .collect::<Result<Vec<_>, TryFromIntError>>()?;
    if reverse {
        edits.reverse();
    }
    let edit = WorkspaceEdit {
        changes: Some(BTreeMap::from([("file:///bulk.txt".to_string(), edits)])),
        document_changes: None,
    };
    let text = if multiline { "old\n" } else { "old " }.repeat(count);
    Ok((edit, text))
}

fn measure(count: usize, multiline: bool, reverse: bool) -> BenchResult<u128> {
    let (edit, text) = input(count, multiline, reverse)?;
    let src = source(&[("/bulk.txt", &text)]);
    let expected = if multiline { "new\n" } else { "new " }.repeat(count);
    let mut samples = Vec::new();
    for _ in 0..3 {
        let start = Instant::now();
        let ops = black_box(workspace_edit_to_ops(&edit, PositionEncoding::Utf16, &src)?);
        samples.push(start.elapsed().as_micros());
        assert_eq!(ops.len(), count);
        let plan = build_plan(&ops, &src);
        assert!(plan.problems.is_empty(), "{:?}", plan.problems);
        assert_eq!(plan.files[0].after.as_deref(), Some(expected.as_str()));
    }
    samples.sort_unstable();
    Ok(samples[1])
}

#[test]
#[ignore = "manual release-mode workspace position timing"]
fn workspace_conversion_measurement() -> BenchResult<()> {
    for count in [1_000, 5_000, 10_000] {
        for (multiline, reverse) in [(true, false), (true, true), (false, false), (false, true)] {
            let median = measure(count, multiline, reverse)?;
            println!(
                "workspace_conversion count={count} multiline={multiline} reverse={reverse} median_us={median}"
            );
        }
    }
    Ok(())
}
