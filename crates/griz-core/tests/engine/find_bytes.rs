//! Exact literal semantics and repeatable sparse-search timing.

use griz_core::{FindQuery, find, find_files};
use std::{error::Error, fs, hint::black_box, time::Instant};

type TestResult = Result<(), Box<dyn Error>>;

#[test]
fn literal_bytes_preserve_exact_nonoverlapping_ranges() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("text.txt");
    for (text, needle, ranges) in [
        ("aaaaa", "aa", vec![(0, 2), (2, 4)]),
        ("é猫é猫", "猫", vec![(2, 5), (7, 10)]),
        ("a.b aXb", "a.b", vec![(0, 3)]),
        ("a\0b a\0b", "a\0b", vec![(0, 3), (4, 7)]),
        ("A a", "a", vec![(2, 3)]),
        ("x\r\ny\r\n", "\r\n", vec![(1, 3), (4, 6)]),
    ] {
        fs::write(&path, text)?;
        let page = find(&FindQuery {
            paths: vec![path.clone()],
            literal: Some(needle.into()),
            limit: 20,
            ..FindQuery::default()
        })?;
        let actual: Vec<_> = page
            .matches
            .iter()
            .map(|hit| (hit.range.start, hit.range.end))
            .collect();
        assert_eq!(actual, ranges);
        assert_eq!(page.total, ranges.len());
        assert!(page.matches.iter().all(|hit| hit.captures.is_empty()));
    }
    assert!(
        find(&FindQuery {
            paths: vec![path],
            literal: Some(String::new()),
            limit: 20,
            ..FindQuery::default()
        })
        .is_err()
    );
    Ok(())
}

#[test]
fn literal_scopes_paging_and_hashes_follow_fresh_text() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("a.rs");
    fs::write(&path, "fn word() {} // word word\n// word\n")?;
    let query = FindQuery {
        paths: vec![path.clone()],
        literal: Some("word".into()),
        within: vec!["comment".into()],
        offset: 1,
        limit: 1,
        ..FindQuery::default()
    };
    let page = find(&query)?;
    assert_eq!((page.total, page.files, page.next), (3, 1, Some(2)));
    let hit = page.matches.first().ok_or("missing literal match")?;
    assert_eq!(
        (hit.line, hit.column, hit.range.start, hit.range.end),
        (1, 22, 21, 25)
    );
    let changed = "// word\n// word\n";
    fs::write(&path, changed)?;
    let fresh = find(&query)?;
    let hit = fresh.matches.first().ok_or("missing fresh match")?;
    assert_eq!((fresh.total, hit.line, hit.column), (2, 2, 4));
    assert_eq!(
        hit.file_hash,
        "94f4cab7cd0340298b366d746b776203668de70d7a9c9b83d6c9bda61d0ae598"
    );
    let counted = find(&FindQuery {
        limit: 0,
        ..query.clone()
    })?;
    assert_eq!((counted.total, counted.files), (2, 1));
    let files = find_files(&FindQuery { offset: 0, ..query })?;
    let file = files.file_matches.first().ok_or("missing file summary")?;
    assert_eq!((files.total, file.count), (2, 2));
    assert_eq!(
        file.file_hash,
        "94f4cab7cd0340298b366d746b776203668de70d7a9c9b83d6c9bda61d0ae598"
    );
    Ok(())
}

#[test]
#[ignore = "manual release-mode sparse literal and regex timing"]
fn sparse_byte_search_measurement() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("sparse.txt");
    let block = format!("{}needle\n", "ordinary text with no match\n".repeat(4096));
    fs::write(&path, block.repeat(64))?;
    for regex in [false, true] {
        let query = FindQuery {
            paths: vec![path.clone()],
            literal: (!regex).then(|| "needle".into()),
            regex: regex.then(|| "(needle)".into()),
            limit: 64,
            ..FindQuery::default()
        };
        let mut samples = Vec::new();
        for _ in 0..9 {
            let start = Instant::now();
            let page = black_box(find(&query)?);
            samples.push(start.elapsed().as_micros());
            assert_eq!((page.total, page.matches.len()), (64, 64));
            let last = page.matches.last().ok_or("missing last match")?;
            assert_eq!((last.line, last.column), (262_208, 1));
        }
        samples.sort_unstable();
        println!(
            "sparse_byte_search regex={regex} bytes={} median_us={}",
            block.len() * 64,
            samples[4]
        );
    }
    Ok(())
}
