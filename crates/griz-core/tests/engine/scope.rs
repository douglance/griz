//! `within` narrows text matches to spans inside named syntax nodes.

use griz_core::{FindQuery, find};
use std::{error::Error, fs};

type TestResult = Result<(), Box<dyn Error>>;

fn query(root: &std::path::Path, literal: &str, within: &[&str]) -> FindQuery {
    FindQuery {
        paths: vec![root.to_path_buf()],
        literal: Some(literal.to_string()),
        within: within.iter().map(|kind| (*kind).to_string()).collect(),
        limit: 100,
        ..FindQuery::default()
    }
}

#[test]
fn within_comment_matches_only_inside_the_comment() -> TestResult {
    let dir = tempfile::tempdir()?;
    fs::write(
        dir.path().join("a.rs"),
        "// word here\nfn word() {\n    let word = 1;\n}\n",
    )?;
    let page = find(&query(dir.path(), "word", &["comment"]))?;
    assert_eq!(page.total, 1, "{page:?}");
    assert_eq!(page.matches[0].line, 1);
    Ok(())
}

#[test]
fn within_string_matches_in_rust_and_python() -> TestResult {
    let dir = tempfile::tempdir()?;
    fs::write(
        dir.path().join("a.rs"),
        "let word = 1;\nlet s = \"word\";\n",
    )?;
    fs::write(dir.path().join("b.py"), "word = 1\ns = 'word'\n")?;
    let rust = find(&FindQuery {
        globs: vec!["*.rs".to_string()],
        ..query(dir.path(), "word", &["string"])
    })?;
    let rust_line = rust.matches.first().map(|m| m.line);
    assert_eq!((rust.total, rust_line), (1, Some(2)), "{rust:?}");
    let python = find(&FindQuery {
        globs: vec!["*.py".to_string()],
        ..query(dir.path(), "word", &["string"])
    })?;
    let python_line = python.matches.first().map(|m| m.line);
    assert_eq!((python.total, python_line), (1, Some(2)), "{python:?}");
    Ok(())
}

#[test]
fn a_non_enabled_language_file_is_skipped_when_within_is_set() -> TestResult {
    let dir = tempfile::tempdir()?;
    fs::write(dir.path().join("a.rs"), "// word\nfn f() {}\n")?;
    fs::write(dir.path().join("notes.md"), "word word word\n")?;
    let page = find(&query(dir.path(), "word", &["comment"]))?;
    assert!(page.matches.iter().all(|m| !m.path.ends_with("notes.md")));
    Ok(())
}

#[test]
fn an_unknown_node_kind_is_an_error_naming_it() -> TestResult {
    let dir = tempfile::tempdir()?;
    fs::write(dir.path().join("a.rs"), "fn f() {}\n")?;
    let error = find(&query(dir.path(), "f", &["bogus_kind_xyz"]))
        .err()
        .ok_or("expected an error for an unknown node kind")?;
    assert!(error.contains("bogus_kind_xyz"), "{error}");
    Ok(())
}
