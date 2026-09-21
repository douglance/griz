//! Structural find: code shapes with metavariables.

use griz_core::{FindQuery, find};
use std::{error::Error, fs};

type TestResult = Result<(), Box<dyn Error>>;

fn tree() -> Result<tempfile::TempDir, Box<dyn Error>> {
    let dir = tempfile::tempdir()?;
    fs::create_dir_all(dir.path().join(".git"))?;
    fs::write(
        dir.path().join("a.rs"),
        "fn f() {\n    foo(1, bar, baz);\n    foo(\n        x,\n        y,\n    );\n}\n",
    )?;
    fs::write(dir.path().join("b.ts"), "const a = foo(1, bar);\n")?;
    fs::write(dir.path().join("c.txt"), "foo(1, 2)\n")?;
    Ok(dir)
}

fn query(root: &std::path::Path, pattern: &str) -> FindQuery {
    FindQuery {
        paths: vec![root.to_path_buf()],
        pattern: Some(pattern.to_string()),
        limit: 100,
        ..FindQuery::default()
    }
}

#[test]
fn a_shape_matches_across_formatting_and_languages() -> TestResult {
    let dir = tree()?;
    let page = find(&query(dir.path(), "foo($A, $$$REST)"))?;
    assert_eq!((page.total, page.files), (3, 2), "{page:?}");
    let multi_line = page
        .matches
        .iter()
        .find(|m| m.line == 3)
        .ok_or("no multi-line match")?;
    assert_eq!(multi_line.vars.get("A").map(String::as_str), Some("x"));
    assert_eq!(multi_line.vars.get("REST").map(String::as_str), Some("y"));
    let text = fs::read_to_string(dir.path().join("a.rs"))?;
    assert_eq!(
        &text[multi_line.range.start..multi_line.range.end],
        multi_line.text
    );
    Ok(())
}

#[test]
fn a_named_language_limits_the_search() -> TestResult {
    let dir = tree()?;
    let page = find(&FindQuery {
        language: Some("typescript".to_string()),
        globs: vec!["*.ts".to_string()],
        ..query(dir.path(), "foo($A, $B)")
    })?;
    assert_eq!(page.total, 1);
    assert_eq!(
        page.matches[0].vars.get("B").map(String::as_str),
        Some("bar")
    );
    Ok(())
}

#[test]
fn text_is_not_parsed_as_code_and_bad_input_is_refused() -> TestResult {
    let dir = tree()?;
    let page = find(&query(dir.path(), "foo($A, $B)"))?;
    assert!(page.matches.iter().all(|m| !m.path.ends_with("c.txt")));
    assert!(
        find(&FindQuery {
            language: Some("cobol".to_string()),
            ..query(dir.path(), "x")
        })
        .is_err()
    );
    assert!(
        find(&FindQuery {
            regex: Some("x".to_string()),
            ..query(dir.path(), "x")
        })
        .is_err()
    );
    Ok(())
}

#[test]
fn a_named_language_searches_only_that_languages_extensions() -> TestResult {
    let dir = tree()?;
    fs::write(
        dir.path().join("rusty.ts"),
        "fn main() {\n    let x = 1;\n}\n",
    )?;
    let page = find(&FindQuery {
        language: Some("rust".to_string()),
        ..query(dir.path(), "fn $NAME() { $$$BODY }")
    })?;
    assert_eq!(page.total, 1, "{page:?}");
    assert!(page.matches[0].path.ends_with("a.rs"), "{page:?}");
    let named = find(&FindQuery {
        language: Some("typescript".to_string()),
        ..query(dir.path(), "foo($A, $B)")
    })?;
    assert_eq!(named.total, 1, "{named:?}");
    assert!(named.matches[0].path.ends_with("b.ts"));
    Ok(())
}

#[test]
fn a_disabled_language_extension_is_skipped_not_parsed() -> TestResult {
    let dir = tree()?;
    fs::write(dir.path().join("notes.md"), "# foo(1, 2)\n")?;
    fs::write(dir.path().join("data.json"), "{\"call\": \"foo(1, 2)\"}\n")?;
    fs::write(dir.path().join("config.yaml"), "call: foo(1, 2)\n")?;
    let page = find(&query(dir.path(), "foo($A, $B)"))?;
    let extensions: Vec<_> = page
        .matches
        .iter()
        .filter_map(|m| m.path.extension().and_then(|e| e.to_str()))
        .collect();
    assert!(
        extensions.iter().all(|ext| matches!(*ext, "rs" | "ts")),
        "{extensions:?}"
    );
    Ok(())
}

#[test]
fn an_unsupported_named_language_is_an_error_not_a_panic() -> TestResult {
    let dir = tree()?;
    let error = find(&FindQuery {
        language: Some("java".to_string()),
        ..query(dir.path(), "foo($A, $B)")
    })
    .err()
    .ok_or("expected an error for a language with no linked parser")?;
    assert!(error.contains("java"), "{error}");
    Ok(())
}

#[test]
fn an_invalid_pattern_is_an_error_naming_it_not_zero_matches() -> TestResult {
    let dir = tree()?;
    let error = find(&query(dir.path(), "((("))
        .err()
        .ok_or("expected an error for an unparseable pattern")?;
    assert!(error.contains("((("), "{error}");
    Ok(())
}
