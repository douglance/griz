//! Definition identities retain owners and duplicate occurrences.

use griz_core::{DiffItem, diff_items};
use serde_json::{Value, json};
use std::path::Path;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const CASES: &[(&str, &str, &str, &str)] = &[
    (
        "a.rs",
        "impl A { fn run() { first(); } }\nimpl B { fn run() { second(); } }",
        "function",
        "impl A",
    ),
    (
        "a.ts",
        "class A { run() { first(); } }\nclass B { run() { second(); } }",
        "method",
        "A",
    ),
    (
        "a.tsx",
        "class A { run() { first(); } }\nclass B { run() { second(); } }",
        "method",
        "A",
    ),
    (
        "a.js",
        "class A { run() { first(); } }\nclass B { run() { second(); } }",
        "method",
        "A",
    ),
    (
        "a.py",
        "class A:\n    def run(self): first();\nclass B:\n    def run(self): second();\n",
        "function",
        "A",
    ),
    (
        "a.go",
        "package p\nfunc (a A) run() { first(); }\nfunc (b B) run() { second(); }\n",
        "method",
        "A",
    ),
    (
        "a.swift",
        "struct A { func run() { first(); } }\nstruct B { func run() { second(); } }",
        "function",
        "A",
    ),
    (
        "a.rs",
        "impl Left for A { fn run() { first(); } }\nimpl Right for A { fn run() { second(); } }",
        "function",
        "impl Left for A",
    ),
];

#[test]
fn changing_the_first_same_named_method_reports_its_owner() -> TestResult {
    for (path, before, kind, scope) in CASES {
        let after = before.replace("first();", "changed();");
        let items = diff_items(Path::new(path), Some(before), Some(&after));
        let methods: Vec<_> = items.iter().filter(|item| item.name == "run").collect();
        assert_eq!(methods.len(), 1, "{path}: {items:?}");
        assert_eq!(
            serde_json::to_value(methods[0])?,
            json!({
                "kind": kind, "name": "run", "scope": [scope], "change": "changed",
            }),
            "{path}"
        );
    }
    Ok(())
}

#[test]
fn nested_scopes_are_reported_outermost_first() -> TestResult {
    let before = "mod left { impl A { fn run() { first(); } } }";
    let after = before.replace("first();", "changed();");
    let items = diff_items(Path::new("a.rs"), Some(before), Some(&after));
    let item = items
        .iter()
        .find(|item| item.name == "run")
        .ok_or("missing run")?;
    assert_eq!(
        serde_json::to_value(item)?["scope"],
        json!(["left", "impl A"])
    );
    Ok(())
}

#[test]
fn reordering_owners_does_not_change_their_definitions() {
    let a = "impl A { fn run() { first(); } }\n";
    let b = "impl B { fn run() { second(); } }\n";
    let items = diff_items(
        Path::new("a.rs"),
        Some(&format!("{a}{b}")),
        Some(&format!("{b}{a}")),
    );
    assert!(items.is_empty(), "{items:?}");
}

#[test]
fn swapping_bodies_between_owners_changes_both() -> TestResult {
    let before = "impl A { fn run() { first(); } }\nimpl B { fn run() { second(); } }";
    let after = "impl A { fn run() { second(); } }\nimpl B { fn run() { first(); } }";
    let items = diff_items(Path::new("a.rs"), Some(before), Some(after));
    let values: Vec<Value> = items
        .iter()
        .map(serde_json::to_value)
        .collect::<Result<_, _>>()?;
    assert_eq!(
        values,
        vec![
            json!({"kind":"function", "name":"run", "scope":["impl A"], "change":"changed"}),
            json!({"kind":"function", "name":"run", "scope":["impl B"], "change":"changed"}),
        ]
    );
    Ok(())
}

#[test]
fn removing_one_overload_preserves_the_other() -> TestResult {
    let removed = "func run(_ value: Int) { first(); }\n";
    let kept = "func run(_ value: String) { second(); }\n";
    let items = diff_items(
        Path::new("a.swift"),
        Some(&format!("{removed}{kept}")),
        Some(kept),
    );
    assert_eq!(
        serde_json::to_value(items)?,
        json!([
            {"kind":"function", "name":"run", "change":"removed"},
        ])
    );
    Ok(())
}

#[test]
fn reordering_overloads_does_not_change_their_bodies() {
    let a = "func run(_ value: Int) { first(); }\n";
    let b = "func run(_ value: String) { second(); }\n";
    let items = diff_items(
        Path::new("a.swift"),
        Some(&format!("{a}{b}")),
        Some(&format!("{b}{a}")),
    );
    assert!(items.is_empty(), "{items:?}");
}

#[test]
fn owner_type_formatting_does_not_rename_a_definition() {
    let before = "impl Container < T > { fn run() { first(); } }";
    let after = "impl Container<T> { fn run() { first(); } }";
    let items = diff_items(Path::new("a.rs"), Some(before), Some(after));
    assert!(items.is_empty(), "{items:?}");
}

#[test]
fn owner_type_identity_preserves_spaces_inside_literals() -> TestResult {
    let before = "impl Container<{ \"a b\".len() }> { fn run() {} }";
    let after = "impl Container<{ \"ab\".len() }> { fn run() {} }";
    let items = diff_items(Path::new("a.rs"), Some(before), Some(after));
    assert_eq!(items.len(), 2, "{items:?}");
    let values: Vec<Value> = items
        .iter()
        .map(serde_json::to_value)
        .collect::<Result<_, _>>()?;
    assert!(values.iter().any(|v| v["change"] == "removed"));
    assert!(values.iter().any(|v| v["change"] == "added"));
    assert!(
        values
            .iter()
            .any(|v| v["scope"][0].as_str().is_some_and(|s| s.contains("a b")))
    );
    Ok(())
}

#[test]
fn older_item_records_round_trip_without_an_empty_scope() -> TestResult {
    let legacy = json!({"kind":"function", "name":"run", "change":"changed"});
    let item: DiffItem = serde_json::from_value(legacy.clone())?;
    assert_eq!(serde_json::to_value(item)?, legacy);
    Ok(())
}
