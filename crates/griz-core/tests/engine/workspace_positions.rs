//! Batch position conversion keeps caller order and boundary semantics.

use crate::common::source;
use griz_core::{
    Op, Position, PositionEncoding, Range, TextEdit, WorkspaceEdit, WorkspaceEditError,
    workspace_edit_to_ops,
};
use std::{collections::BTreeMap, error::Error};

type TestResult = Result<(), Box<dyn Error>>;
const TEXT: &str = "A😀é\r\n\nβZ\n";
#[test]
fn workspace_positions_handle_empty_and_unterminated_files() -> TestResult {
    for (text, line, column) in [("", 0, 0), ("\n", 1, 0), ("é", 0, 1), ("é\nβ", 1, 1)] {
        let src = source(&[("/a.txt", text)]);
        let request = edit(&[(line, column, line, column)]);
        let ops = workspace_edit_to_ops(&request, PositionEncoding::Utf16, &src)?;
        let plan = griz_core::build_plan(&ops, &src);
        assert!(plan.problems.is_empty(), "{:?}", plan.problems);
        assert_eq!(
            plan.files[0].after.as_deref(),
            Some(format!("{text}replacement-0").as_str())
        );
    }
    Ok(())
}

fn edit(positions: &[(u32, u32, u32, u32)]) -> WorkspaceEdit {
    let edits = positions
        .iter()
        .enumerate()
        .map(|(index, &(line, column, end_line, end_column))| TextEdit {
            range: Range {
                start: Position {
                    line,
                    character: column,
                },
                end: Position {
                    line: end_line,
                    character: end_column,
                },
            },
            new_text: format!("replacement-{index}"),
        })
        .collect();
    WorkspaceEdit {
        changes: Some(BTreeMap::from([("file:///a.txt".to_string(), edits)])),
        document_changes: None,
    }
}

fn check_offsets(encoding: PositionEncoding, points: &[(u32, u32, usize)]) -> TestResult {
    let positions: Vec<_> = points
        .iter()
        .map(|&(line, column, _)| (line, column, line, column))
        .collect();
    let ops = workspace_edit_to_ops(&edit(&positions), encoding, &source(&[("/a.txt", TEXT)]))?;
    assert_eq!(ops.len(), points.len());
    for (index, (op, &(_, _, byte))) in ops.iter().zip(points).enumerate() {
        let Op::Replace {
            range: Some(range),
            replace,
            ..
        } = op
        else {
            panic!("expected ranged replacement: {op:?}");
        };
        assert_eq!((range.start, range.end), (byte, byte));
        assert_eq!(replace, &format!("replacement-{index}"));
    }
    Ok(())
}

#[test]
fn workspace_positions_preserve_order_duplicates_and_unicode_boundaries() -> TestResult {
    check_offsets(
        PositionEncoding::Utf16,
        &[
            (2, 2, 13),
            (0, 4, 7),
            (3, 0, 14),
            (0, 0, 0),
            (1, 0, 9),
            (0, 3, 5),
            (0, 1, 1),
            (0, 5, 8),
            (2, 1, 12),
            (0, 1, 1),
        ],
    )?;
    check_offsets(
        PositionEncoding::Utf32,
        &[
            (0, 4, 8),
            (2, 2, 13),
            (0, 2, 5),
            (1, 0, 9),
            (0, 3, 7),
            (2, 0, 10),
            (0, 0, 0),
            (3, 0, 14),
            (0, 1, 1),
        ],
    )?;
    check_offsets(
        PositionEncoding::Utf8,
        &[
            (0, 8, 8),
            (0, 2, 2),
            (2, 1, 11),
            (3, 0, 14),
            (0, 5, 5),
            (0, 1, 1),
            (2, 3, 13),
            (1, 0, 9),
            (0, 0, 0),
        ],
    )
}

fn check_invalid(
    encoding: PositionEncoding,
    positions: &[(u32, u32, u32, u32)],
    expected: (u32, u32),
) {
    let result = workspace_edit_to_ops(&edit(positions), encoding, &source(&[("/a.txt", TEXT)]));
    let Err(WorkspaceEditError::BadPosition {
        line, character, ..
    }) = result
    else {
        panic!("expected a bad position: {result:?}");
    };
    assert_eq!((line, character), expected);
}

#[test]
fn workspace_positions_report_errors_in_caller_order() {
    check_invalid(
        PositionEncoding::Utf16,
        &[(4, 0, 4, 0), (0, 2, 0, 2)],
        (4, 0),
    );
    check_invalid(PositionEncoding::Utf16, &[(0, 2, 4, 0)], (0, 2));
    check_invalid(PositionEncoding::Utf16, &[(0, 1, 0, 2)], (0, 2));
}

#[test]
fn workspace_positions_reject_out_of_line_and_surrogate_offsets() {
    for (encoding, column) in [
        (PositionEncoding::Utf8, 9),
        (PositionEncoding::Utf16, 2),
        (PositionEncoding::Utf16, 6),
        (PositionEncoding::Utf32, 5),
    ] {
        check_invalid(encoding, &[(0, column, 0, column)], (0, column));
    }
    check_invalid(
        PositionEncoding::Utf8,
        &[(1, u32::MAX, 1, 0)],
        (1, u32::MAX),
    );
    check_invalid(PositionEncoding::Utf16, &[(1, 1, 1, 1)], (1, 1));
    check_invalid(PositionEncoding::Utf16, &[(3, 1, 3, 1)], (3, 1));
    check_invalid(
        PositionEncoding::Utf16,
        &[(u32::MAX, u32::MAX, 0, 0)],
        (u32::MAX, u32::MAX),
    );
}
