//! Paging keeps complete counts while returning exact selected-file identities.

use griz_core::{FindQuery, find};
use std::{error::Error, fs};

type TestResult = Result<(), Box<dyn Error>>;
const A_HASH: &str = "55fbc69f6e18b29f3d8888a65b351b17bf03c448a4b1f1b61773facc9316ebfb";
const B_HASH: &str = "a991ae1249e0ba69240632666e2524773854dddf857d669f4a05badf0fe21fe5";
const C_HASH: &str = "c97ecfda4d205190b973232dcfdb0c29748521c2534dd866bcc782f30b086738";

fn tree() -> Result<tempfile::TempDir, Box<dyn Error>> {
    let dir = tempfile::tempdir()?;
    for (name, text) in [
        ("a.txt", "a target\nx target\n"),
        ("b.txt", "target b\nother\ntarget b\ntarget b\n"),
        ("c.txt", "target\n"),
        ("d.txt", "nothing here\n"),
    ] {
        fs::write(dir.path().join(name), text)?;
    }
    Ok(dir)
}

fn query(dir: &std::path::Path, offset: usize, limit: usize) -> FindQuery {
    FindQuery {
        paths: vec![dir.to_path_buf()],
        regex: Some("(target)".into()),
        offset,
        limit,
        ..FindQuery::default()
    }
}

#[test]
fn a_page_crossing_files_keeps_positions_captures_and_fingerprints() -> TestResult {
    let dir = tree()?;
    let page = find(&query(dir.path(), 1, 3))?;
    assert_eq!((page.total, page.files, page.next), (6, 3, Some(4)));
    let rows: Vec<_> = page
        .matches
        .iter()
        .map(|hit| {
            (
                hit.path.clone(),
                hit.line,
                hit.column,
                hit.range.start,
                hit.range.end,
                hit.file_hash.as_str(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            (dir.path().join("a.txt"), 2, 3, 11, 17, A_HASH),
            (dir.path().join("b.txt"), 1, 1, 0, 6, B_HASH),
            (dir.path().join("b.txt"), 3, 1, 15, 21, B_HASH),
        ]
    );
    for hit in page.matches {
        assert_eq!(hit.text, "target");
        assert_eq!(hit.captures, [Some("target".into())]);
    }
    Ok(())
}

#[test]
fn empty_pages_still_count_every_matching_file() -> TestResult {
    let dir = tree()?;
    for (offset, limit, next) in [
        (0, 0, Some(0)),
        (4, 0, Some(4)),
        (6, 1, None),
        (usize::MAX, usize::MAX, None),
    ] {
        let page = find(&query(dir.path(), offset, limit))?;
        assert!(page.matches.is_empty());
        assert_eq!((page.total, page.files, page.next), (6, 3, next));
    }
    Ok(())
}

#[test]
fn the_last_page_skips_complete_files_and_stops_at_the_end() -> TestResult {
    let dir = tree()?;
    let page = find(&query(dir.path(), 4, usize::MAX))?;
    assert_eq!((page.total, page.files, page.next), (6, 3, None));
    let rows: Vec<_> = page
        .matches
        .iter()
        .map(|hit| {
            (
                hit.path.clone(),
                hit.line,
                hit.range.start,
                hit.file_hash.as_str(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            (dir.path().join("b.txt"), 4, 24, B_HASH),
            (dir.path().join("c.txt"), 1, 0, C_HASH),
        ]
    );
    Ok(())
}
