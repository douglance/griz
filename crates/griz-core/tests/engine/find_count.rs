//! Count-only searches retain regex and scope semantics.

use griz_core::{FindQuery, find};
use std::{error::Error, fs};

type TestResult = Result<(), Box<dyn Error>>;

#[test]
fn count_only_preserves_unicode_empty_optional_and_anchored_matches() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("a.txt");
    for (text, regex, total) in [
        ("é a b a22\n", "(a)([0-9]*)|(b)", 3),
        ("é\n", "()", 3),
        ("aa", "(a*)", 1),
        ("aa", "(a*?)", 3),
        ("a\nb\n", "(?m)^([ab])?$", 3),
        ("é猫\n", "(.)", 2),
        ("é猫\n", "(x)", 0),
    ] {
        fs::write(&path, text)?;
        for offset in [0, 1, usize::MAX] {
            let page = find(&FindQuery {
                paths: vec![path.clone()],
                regex: Some(regex.into()),
                offset,
                limit: 0,
                ..FindQuery::default()
            })?;
            assert!(page.matches.is_empty(), "{regex}");
            assert_eq!(
                (page.total, page.files),
                (total, usize::from(total > 0)),
                "{regex}"
            );
            assert_eq!(page.next, (offset < total).then_some(offset), "{regex}");
        }
    }
    Ok(())
}

#[test]
fn count_only_respects_scopes_and_skips_unsupported_files() -> TestResult {
    let dir = tempfile::tempdir()?;
    fs::write(
        dir.path().join("a.rs"),
        "let word = \"word\"; // word\n// word\n",
    )?;
    fs::write(dir.path().join("b.rs"), "fn word() {}\n")?;
    fs::write(dir.path().join("notes.txt"), "// word\n")?;
    let page = find(&FindQuery {
        paths: vec![dir.path().to_path_buf()],
        regex: Some("(w)(ord)".into()),
        within: vec!["comment".into()],
        offset: 1,
        limit: 0,
        ..FindQuery::default()
    })?;
    assert!(page.matches.is_empty());
    assert_eq!((page.total, page.files, page.next), (2, 1, Some(1)));
    Ok(())
}

#[test]
fn captures_survive_when_later_files_only_contribute_counts() -> TestResult {
    let dir = tempfile::tempdir()?;
    fs::write(dir.path().join("a.txt"), "a1 a22\n")?;
    fs::write(dir.path().join("b.txt"), "a33 a44\n")?;
    fs::write(dir.path().join("c.txt"), "a55\n")?;
    let page = find(&FindQuery {
        paths: vec![dir.path().to_path_buf()],
        regex: Some("(a)([0-9]*)".into()),
        offset: 1,
        limit: 1,
        ..FindQuery::default()
    })?;
    assert_eq!((page.total, page.files, page.next), (5, 3, Some(2)));
    assert_eq!(page.matches.len(), 1);
    let hit = page.matches.first().ok_or("missing match")?;
    assert_eq!(hit.path, dir.path().join("a.txt"));
    assert_eq!((hit.range.start, hit.range.end), (3, 6));
    assert_eq!(hit.captures, [Some("a".into()), Some("22".into())]);
    Ok(())
}
