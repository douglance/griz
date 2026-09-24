//! Range edit positions follow the current overlay after every operation.

use crate::{
    common::{anchor, replace, source},
    range_edits::range_edit,
};
use griz_core::{ByteRange, Op, build_plan};
use std::path::PathBuf;

const ORIGINAL: &str = "aa\nbb\ncc\ndd\n";

fn cc(with: &str) -> Op {
    range_edit(ByteRange { start: 6, end: 8 }, Some("cc"), with)
}

fn dd() -> Op {
    range_edit(ByteRange { start: 9, end: 11 }, Some("dd"), "DD")
}

fn assert_positions(ops: &[Op], lines: &[usize], after: &str) {
    let plan = build_plan(ops, &source(&[("a.txt", ORIGINAL)]));
    assert!(plan.problems.is_empty(), "{:?}", plan.problems);
    let actual: Vec<_> = plan.edits.iter().map(|edit| edit.line).collect();
    assert_eq!(actual, lines);
    assert_eq!(plan.files[0].after.as_deref(), Some(after));
}

#[test]
fn range_positions_include_newlines_inserted_by_previous_ranges() {
    assert_positions(&[cc("C\nC"), dd()], &[3, 5], "aa\nbb\nC\nC\nDD\n");
}

#[test]
fn range_positions_move_backwards_when_edits_do() {
    assert_positions(&[dd(), cc("CC")], &[4, 3], "aa\nbb\nCC\nDD\n");
}

#[test]
fn range_positions_reset_after_an_earlier_anchor_change() {
    let ops = [cc("CC"), replace("a.txt", "aa", "AA\nextra"), dd()];
    assert_positions(&ops, &[3, 1, 5], "AA\nextra\nbb\nCC\nDD\n");
}

#[test]
fn range_positions_reset_before_an_old_offset_splits_a_new_unicode_character() {
    let ops = [cc("CC"), replace("a.txt", "aa", "😀😀"), dd()];
    assert_positions(&ops, &[3, 1, 4], "😀😀\nbb\nCC\nDD\n");
}

#[test]
fn range_positions_reset_after_an_insertion() {
    let insertion = Op::Insert {
        path: PathBuf::from("a.txt"),
        anchor: anchor("aa"),
        after: false,
        text: "pre\n".to_string(),
        expect_hash: None,
    };
    assert_positions(
        &[cc("CC"), insertion, dd()],
        &[3, 1, 5],
        "pre\naa\nbb\nCC\nDD\n",
    );
}

#[test]
fn range_positions_reset_after_whole_file_replacement() {
    let overwrite = Op::Create {
        path: PathBuf::from("a.txt"),
        text: "a\n\nbb\nCC\ndd\n".to_string(),
        overwrite: true,
        expect_hash: None,
    };
    assert_positions(
        &[cc("CC"), overwrite, dd()],
        &[3, 0, 5],
        "a\n\nbb\nCC\nDD\n",
    );
}
