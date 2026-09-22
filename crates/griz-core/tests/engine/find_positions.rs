//! Match positions and a repeatable dense-search measurement.

use griz_core::{FindQuery, find};
use std::{error::Error, fs, hint::black_box, time::Instant};

type TestResult = Result<(), Box<dyn Error>>;

#[test]
fn positions_preserve_unicode_newlines_and_paging() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("text.txt");
    fs::write(&path, "é hit hit\r\n\nhit\n😀 hit\n")?;
    let query = FindQuery {
        paths: vec![path],
        literal: Some("hit".to_string()),
        offset: 1,
        limit: 2,
        ..FindQuery::default()
    };
    let page = find(&query)?;
    let positions: Vec<_> = page
        .matches
        .iter()
        .map(|hit| (hit.line, hit.column))
        .collect();
    assert_eq!(positions, [(1, 7), (3, 1)]);
    assert_eq!((page.total, page.files, page.next), (4, 1, Some(3)));
    let last = find(&FindQuery { offset: 3, ..query })?;
    assert_eq!((last.matches[0].line, last.matches[0].column), (4, 3));
    Ok(())
}

#[test]
fn positions_use_match_start_for_multiline_and_empty_matches() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("text.txt");
    fs::write(&path, "é\nx\n")?;
    let query = FindQuery {
        paths: vec![path],
        regex: Some("é\\nx|$".to_string()),
        limit: 10,
        ..FindQuery::default()
    };
    let page = find(&query)?;
    let positions: Vec<_> = page
        .matches
        .iter()
        .map(|hit| (hit.line, hit.column))
        .collect();
    assert_eq!(positions, [(1, 1), (3, 1)]);
    let empty = find(&FindQuery {
        regex: Some(String::new()),
        ..query.clone()
    });
    assert!(empty.is_err());
    let boundaries = find(&FindQuery {
        regex: Some("(?:)".to_string()),
        ..query
    })?;
    let positions: Vec<_> = boundaries
        .matches
        .iter()
        .map(|hit| (hit.line, hit.column))
        .collect();
    assert_eq!(positions, [(1, 1), (1, 2), (2, 1), (2, 2), (3, 1)]);
    Ok(())
}

#[test]
#[ignore = "manual release-mode dense find timing"]
fn dense_find_measurement() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("dense.txt");
    for count in [10_000, 20_000, 40_000] {
        fs::write(&path, "prefix needle suffix\n".repeat(count))?;
        let query = FindQuery {
            paths: vec![path.clone()],
            literal: Some("needle".to_string()),
            limit: count,
            ..FindQuery::default()
        };
        let mut samples = Vec::new();
        for _ in 0..5 {
            let start = Instant::now();
            let page = black_box(find(&query)?);
            samples.push(start.elapsed().as_micros());
            assert_eq!(page.total, count);
            assert_eq!(page.matches.len(), count);
            assert_eq!(
                (page.matches[count - 1].line, page.matches[count - 1].column),
                (count, 8)
            );
        }
        samples.sort_unstable();
        println!("dense_find count={count} median_us={}", samples[2]);
    }
    Ok(())
}
