//! Converting an LSP `WorkspaceEdit` into operations.

use crate::common::{source, source_with_absent};
use griz_core::{
    ChangeKind, DocumentChange, Op, Position, PositionEncoding, ProblemKind, Range, RenameFile,
    TextEdit, WorkspaceEdit, build_plan, workspace_edit_to_ops,
};
use std::{collections::BTreeMap, error::Error};

type TestResult = Result<(), Box<dyn Error>>;

fn after_of<'a>(plan: &'a griz_core::Plan, path: &str) -> Option<&'a str> {
    plan.files
        .iter()
        .find(|file| file.path.as_path() == std::path::Path::new(path))
        .and_then(|file| file.after.as_deref())
}

fn one_edit(uri: &str, range: Range, new_text: &str) -> WorkspaceEdit {
    WorkspaceEdit {
        changes: Some(BTreeMap::from([(
            uri.to_string(),
            vec![TextEdit {
                range,
                new_text: new_text.to_string(),
            }],
        )])),
        document_changes: None,
    }
}

fn pos(line: u32, character: u32) -> Position {
    Position { line, character }
}

#[test]
fn utf16_positions_with_emoji_and_accents_land_on_the_right_bytes() -> TestResult {
    // "café " is 5 UTF-16 units (é is one BMP unit) but 6 bytes (é is 2
    // UTF-8 bytes); the emoji after it is 2 UTF-16 units but 4 bytes.
    let text = "café \u{1F600}!\n";
    let src = source(&[("/a.rs", text)]);
    let edit = one_edit(
        "file:///a.rs",
        Range {
            start: pos(0, 5),
            end: pos(0, 7),
        },
        "XX",
    );
    let ops = workspace_edit_to_ops(&edit, PositionEncoding::Utf16, &src)?;
    let plan = build_plan(&ops, &src);
    assert!(plan.problems.is_empty(), "{:?}", plan.problems);
    assert_eq!(after_of(&plan, "/a.rs"), Some("café XX!\n"));
    Ok(())
}

#[test]
fn a_file_changed_after_conversion_is_refused_as_stale() -> TestResult {
    let at_conversion = source(&[("/a.rs", "let x = 1;\n")]);
    let edit = one_edit(
        "file:///a.rs",
        Range {
            start: pos(0, 8),
            end: pos(0, 9),
        },
        "2",
    );
    let ops = workspace_edit_to_ops(&edit, PositionEncoding::Utf16, &at_conversion)?;
    let changed = source(&[("/a.rs", "let x = 9;\n")]);
    let plan = build_plan(&ops, &changed);
    assert!(
        matches!(plan.problems[0].kind, ProblemKind::Stale { .. }),
        "{:?}",
        plan.problems
    );
    Ok(())
}

#[test]
fn rename_file_becomes_a_move() -> TestResult {
    let src = source_with_absent(&[("/old.rs", "x\n")], &["/new.rs"]);
    let edit = WorkspaceEdit {
        changes: None,
        document_changes: Some(vec![DocumentChange::Rename(RenameFile {
            old_uri: "file:///old.rs".to_string(),
            new_uri: "file:///new.rs".to_string(),
        })]),
    };
    let ops = workspace_edit_to_ops(&edit, PositionEncoding::Utf16, &src)?;
    assert!(matches!(ops[0], Op::Move { .. }), "{ops:?}");
    let plan = build_plan(&ops, &src);
    assert!(plan.problems.is_empty(), "{:?}", plan.problems);
    let kind = |path: &str| {
        plan.files
            .iter()
            .find(|f| f.path.as_path() == std::path::Path::new(path))
            .map(|f| f.kind)
    };
    assert_eq!(kind("/old.rs"), Some(ChangeKind::Delete));
    assert_eq!(kind("/new.rs"), Some(ChangeKind::Create));
    Ok(())
}

#[test]
fn utf8_and_utf32_encodings_count_characters_differently_than_utf16() -> TestResult {
    let text = "é!\n";
    let src = source(&[("/a.rs", text)]);
    // utf-32: "é" is one scalar value, so character 1 lands right after it.
    let edit32 = one_edit(
        "file:///a.rs",
        Range {
            start: pos(0, 1),
            end: pos(0, 2),
        },
        "X",
    );
    let ops32 = workspace_edit_to_ops(&edit32, PositionEncoding::Utf32, &src)?;
    assert_eq!(after_of(&build_plan(&ops32, &src), "/a.rs"), Some("éX\n"));
    // utf-8: "é" is two bytes, so byte 1 lands inside it, refused as a bad range.
    let edit8 = one_edit(
        "file:///a.rs",
        Range {
            start: pos(0, 1),
            end: pos(0, 2),
        },
        "X",
    );
    let ops8 = workspace_edit_to_ops(&edit8, PositionEncoding::Utf8, &src)?;
    let plan8 = build_plan(&ops8, &src);
    assert!(!plan8.problems.is_empty(), "{plan8:?}");
    Ok(())
}

#[test]
fn document_changes_is_preferred_over_changes() -> TestResult {
    let src = source(&[("/a.rs", "one\n")]);
    let mut edit = one_edit(
        "file:///a.rs",
        Range {
            start: pos(0, 0),
            end: pos(0, 3),
        },
        "from changes",
    );
    edit.document_changes = Some(vec![DocumentChange::Edit(griz_core::TextDocumentEdit {
        text_document: griz_core::TextDocumentIdentifier {
            uri: "file:///a.rs".to_string(),
        },
        edits: vec![TextEdit {
            range: Range {
                start: pos(0, 0),
                end: pos(0, 3),
            },
            new_text: "from document_changes".to_string(),
        }],
    })]);
    let ops = workspace_edit_to_ops(&edit, PositionEncoding::Utf16, &src)?;
    let plan = build_plan(&ops, &src);
    assert_eq!(after_of(&plan, "/a.rs"), Some("from document_changes\n"));
    Ok(())
}
