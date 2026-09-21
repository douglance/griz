//! Named-definition diff between a file's before and after text.

use crate::common::replace;
use griz_core::{ItemChange, build_plan, render_diff};
use std::path::Path;

fn items_of(before: &str, after_ops: &[griz_core::Op]) -> Vec<griz_core::DiffItem> {
    let plan = build_plan(after_ops, &crate::common::source(&[("a.rs", before)]));
    assert!(plan.problems.is_empty(), "{:?}", plan.problems);
    let diffs = render_diff(&plan.files, Path::new("/"));
    diffs
        .into_iter()
        .find(|d| d.path.ends_with("a.rs"))
        .map(|d| d.items)
        .unwrap_or_default()
}

#[test]
fn renaming_a_function_reports_removed_and_added() {
    let before = "fn old_name() {\n    1;\n}\n";
    let items = items_of(before, &[replace("a.rs", "old_name", "new_name")]);
    let mut names: Vec<_> = items
        .iter()
        .map(|item| (item.kind.as_str(), item.name.as_str(), item.change))
        .collect();
    names.sort_by_key(|(_, name, _)| *name);
    assert_eq!(
        names,
        [
            ("function", "new_name", ItemChange::Added),
            ("function", "old_name", ItemChange::Removed),
        ]
    );
}

#[test]
fn editing_a_body_reports_changed() {
    let before = "fn f() {\n    1;\n}\n";
    let items = items_of(before, &[replace("a.rs", "1;", "2;")]);
    assert_eq!(items.len(), 1, "{items:?}");
    assert_eq!(items[0].kind, "function");
    assert_eq!(items[0].name, "f");
    assert_eq!(items[0].change, ItemChange::Changed);
}

#[test]
fn an_untouched_function_is_absent() {
    let before = "fn f() {\n    1;\n}\n\nfn g() {\n    2;\n}\n";
    let items = items_of(before, &[replace("a.rs", "1;", "3;")]);
    assert_eq!(items.len(), 1, "{items:?}");
    assert_eq!(items[0].name, "f");
    assert!(items.iter().all(|item| item.name != "g"));
}
