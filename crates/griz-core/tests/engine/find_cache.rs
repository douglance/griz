//! Cached pages are equivalent to an uncached search and retain fresh fingerprints.
use griz_core::{FindPage, FindQuery, find};
use std::{error::Error, fs, path::Path};
type TestResult = Result<(), Box<dyn Error>>;
fn uncached(query: &FindQuery) -> Result<FindPage, String> {
    let mut baseline = query.clone();
    baseline.limit = 129;
    let mut page = find(&baseline)?;
    page.matches.truncate(query.limit);
    page.next = (query.offset + page.matches.len() < page.total)
        .then_some(query.offset + page.matches.len());
    Ok(page)
}
#[test]
fn cached_search_pages_equal_uncached_literal_regex_and_windows() -> TestResult {
    let dir = tempfile::tempdir()?;
    let first = dir.path().join("a.txt");
    let second = dir.path().join("b.txt");
    fs::write(&first, "needle one\nneedle two\n")?;
    fs::write(&second, "needle three\nneedle four\n")?;
    for (literal, regex) in [
        (Some("needle".to_string()), None),
        (None, Some("needle (\\w+)".to_string())),
    ] {
        for (offset, limit) in [(0, 1), (1, 2), (2, 2), (5, 1)] {
            let query = FindQuery {
                paths: vec![first.clone(), second.clone()],
                literal: literal.clone(),
                regex: regex.clone(),
                offset,
                limit,
                ..FindQuery::default()
            };
            let baseline = uncached(&query)?;
            assert_eq!(find(&query)?, baseline);
            assert_eq!(find(&query)?, baseline);
        }
    }
    Ok(())
}
#[test]
fn cached_search_page_rechecks_the_final_byte_before_reuse() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("fresh.txt");
    let mut text = format!("needle\n{}needl!", ".".repeat(2 * 1024 * 1024));
    fs::write(&path, &text)?;
    let query = FindQuery {
        paths: vec![path.clone()],
        literal: Some("needle".into()),
        limit: 1,
        ..FindQuery::default()
    };
    let first = find(&query)?;
    assert_eq!(first.total, 1);
    assert_eq!(find(&query)?, first);
    text.pop();
    text.push('e');
    fs::write(&path, &text)?;
    let changed = find(&query)?;
    assert_eq!(changed.total, 2);
    assert_eq!(changed.next, Some(1));
    assert_eq!(changed.matches[0].file_hash, FINAL_BYTE_HASH);
    assert_ne!(changed.matches[0].file_hash, first.matches[0].file_hash);
    assert_eq!(changed, uncached(&query)?);
    Ok(())
}
#[test]
fn cached_pages_keep_path_pattern_and_capture_identity() -> TestResult {
    let dir = tempfile::tempdir()?;
    let first = dir.path().join("first.txt");
    let second = dir.path().join("second.txt");
    fs::write(&first, "needle one\n")?;
    fs::write(&second, "needle one\n")?;
    check_query(&first, Some("needle"), None, (0, 6, "needle", &[]))?;
    check_query(&second, Some("needle"), None, (0, 6, "needle", &[]))?;
    check_query(&first, Some("one"), None, (7, 10, "one", &[]))?;
    check_query(
        &first,
        None,
        Some("needle (one)"),
        (0, 10, "needle one", &[Some("one")]),
    )?;
    check_query(
        &first,
        None,
        Some("(needle) one"),
        (0, 10, "needle one", &[Some("needle")]),
    )?;
    fs::remove_file(&first)?;
    let query = FindQuery {
        paths: vec![first],
        literal: Some("needle".into()),
        limit: 1,
        ..FindQuery::default()
    };
    assert!(find(&query).is_err());
    Ok(())
}
fn check_query(
    path: &Path,
    literal: Option<&str>,
    regex: Option<&str>,
    known: (usize, usize, &str, &[Option<&str>]),
) -> TestResult {
    let query = FindQuery {
        paths: vec![path.to_path_buf()],
        literal: literal.map(str::to_string),
        regex: regex.map(str::to_string),
        limit: 64,
        ..FindQuery::default()
    };
    let expected = uncached(&query)?;
    assert_eq!(find(&query)?, expected);
    let cached = find(&query)?;
    assert_eq!(cached, expected);
    let found = cached.matches.first().ok_or("expected one cached match")?;
    assert_eq!(found.path, path);
    assert_eq!((found.range.start, found.range.end), (known.0, known.1));
    assert_eq!(found.text, known.2);
    let captures: Vec<_> = found
        .captures
        .iter()
        .map(|capture| capture.as_deref())
        .collect();
    assert_eq!(captures, known.3);
    Ok(())
}

const FINAL_BYTE_HASH: &str = "b155210fc1c4be890836235a6e103cd0e06fbd86f5807b9456c2f1283f9c0e6b";
