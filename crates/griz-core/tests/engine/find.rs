//! Finding text across a directory tree.

use griz_core::{FindQuery, content_hash, find};
use std::{error::Error, fs};

type TestResult = Result<(), Box<dyn Error>>;

fn tree() -> Result<tempfile::TempDir, Box<dyn Error>> {
    let dir = tempfile::tempdir()?;
    fs::create_dir_all(dir.path().join("src"))?;
    fs::create_dir_all(dir.path().join("target"))?;
    fs::write(dir.path().join(".gitignore"), "target/\n")?;
    fs::write(
        dir.path().join("src/a.rs"),
        "let oldName = 1;\nuse(oldName);\n",
    )?;
    fs::write(dir.path().join("src/b.md"), "oldName in docs\n")?;
    fs::write(dir.path().join("target/c.rs"), "oldName ignored\n")?;
    // `ignore` honors .gitignore only inside a git repository.
    fs::create_dir_all(dir.path().join(".git"))?;
    Ok(dir)
}

fn query(root: &std::path::Path) -> FindQuery {
    FindQuery {
        paths: vec![root.to_path_buf()],
        literal: Some("oldName".to_string()),
        limit: 100,
        ..FindQuery::default()
    }
}

#[test]
fn find_honors_gitignore_and_globs() -> TestResult {
    let dir = tree()?;
    let page = find(&query(dir.path()))?;
    assert_eq!((page.total, page.files), (3, 2));
    let rust = find(&FindQuery {
        globs: vec!["**/*.rs".to_string()],
        ..query(dir.path())
    })?;
    assert_eq!((rust.total, rust.files), (2, 1));
    Ok(())
}

#[test]
fn matches_carry_position_captures_and_file_hash() -> TestResult {
    let dir = tree()?;
    let page = find(&FindQuery {
        literal: None,
        regex: Some(r"use\((\w+)\)".to_string()),
        ..query(dir.path())
    })?;
    let hit = page.matches.first().ok_or("no match")?;
    assert_eq!((hit.line, hit.column), (2, 1));
    assert_eq!(hit.captures, vec![Some("oldName".to_string())]);
    let text = fs::read_to_string(dir.path().join("src/a.rs"))?;
    assert_eq!(hit.file_hash, content_hash(&text));
    assert_eq!(&text[hit.range.start..hit.range.end], hit.text);
    Ok(())
}

#[test]
fn pages_are_stable_and_report_the_next_offset() -> TestResult {
    let dir = tree()?;
    let first = find(&FindQuery {
        limit: 2,
        ..query(dir.path())
    })?;
    assert_eq!((first.matches.len(), first.next), (2, Some(2)));
    let second = find(&FindQuery {
        limit: 2,
        offset: 2,
        ..query(dir.path())
    })?;
    assert_eq!((second.matches.len(), second.next), (1, None));
    assert_ne!(first.matches[0], second.matches[0]);
    Ok(())
}

#[test]
fn a_query_needs_exactly_one_pattern() -> TestResult {
    let dir = tree()?;
    assert!(
        find(&FindQuery {
            literal: None,
            ..query(dir.path())
        })
        .is_err()
    );
    assert!(
        find(&FindQuery {
            regex: Some("x".to_string()),
            ..query(dir.path())
        })
        .is_err()
    );
    Ok(())
}
