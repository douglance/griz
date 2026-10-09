//! Stage timings for complete searches with unchanged result checks.

use griz_core::{ContentHashCache, FindQuery, content_hash, find};
use std::{error::Error, fs, hint::black_box, io::Read, time::Instant};

type TestResult = Result<(), Box<dyn Error>>;

fn measure(
    label: &str,
    expected: usize,
    mut run: impl FnMut() -> Result<usize, Box<dyn Error>>,
) -> TestResult {
    let mut samples = Vec::new();
    for _ in 0..11 {
        let start = Instant::now();
        let actual = black_box(run()?);
        samples.push(start.elapsed().as_nanos());
        assert_eq!(actual, expected, "{label}");
    }
    samples.sort_unstable();
    println!("profile stage={label} median_ns={}", samples[5]);
    Ok(())
}

#[test]
#[ignore = "manual release-mode complete search and stage profile"]
fn search_stage_measurement() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("sparse.txt");
    let text = format!("{}needle\n", "ordinary text with no match\n".repeat(4096)).repeat(64);
    fs::write(&path, &text)?;
    let query = FindQuery {
        paths: vec![path.clone()],
        literal: Some("needle".into()),
        limit: 64,
        ..FindQuery::default()
    };
    measure_cold(&query)?;
    measure("read", 7_340_480, || Ok(fs::read_to_string(&path)?.len()))?;
    measure_reused_read(&path)?;
    measure("sha256_warm", 64, || {
        let hash = content_hash(black_box(&text));
        assert_eq!(hash, EXPECTED_HASH);
        Ok(hash.len())
    })?;

    measure("sha256_uncached", 64, || {
        let hash = ContentHashCache::new(0).hash(black_box(text.as_bytes()));
        assert_eq!(hash, EXPECTED_HASH);
        Ok(hash.len())
    })?;
    measure("literal_scan", 64, || {
        Ok(memchr::memmem::find_iter(text.as_bytes(), b"needle").count())
    })?;
    let regex = regex::Regex::new("(needle)")?;
    measure("capture_scan", 64, || {
        Ok(regex.captures_iter(&text).count())
    })?;
    measure("newlines", 262_208, || {
        Ok(memchr::memchr_iter(b'\n', text.as_bytes()).count())
    })?;
    measure("find_literal", 64, || {
        let page = find(&query)?;
        let last = page.matches.last().ok_or("missing match")?;
        assert_eq!((page.total, page.files, page.next), (64, 1, None));
        assert_eq!((last.line, last.column), (262_208, 1));
        assert_eq!(last.file_hash, EXPECTED_HASH);
        Ok(page.matches.len())
    })?;
    let query = FindQuery {
        literal: None,
        regex: Some("(needle)".into()),
        ..query
    };
    measure("find_regex", 64, || {
        let page = find(&query)?;
        let last = page.matches.last().ok_or("missing capture")?;
        assert_eq!((page.total, last.line, last.column), (64, 262_208, 1));
        assert_eq!(last.file_hash, EXPECTED_HASH);
        assert_eq!(last.captures, vec![Some("needle".into())]);
        Ok(page.matches.len())
    })?;
    Ok(())
}

const EXPECTED_HASH: &str = "095c9f2fc59713d124a55214013a8c80310ba0693a7bf46808809ba5890fddf1";

fn measure_cold(query: &FindQuery) -> TestResult {
    let start = Instant::now();
    let page = find(query)?;
    let elapsed = start.elapsed().as_nanos();
    assert_eq!((page.total, page.files, page.next), (64, 1, None));
    assert!(
        page.matches
            .iter()
            .all(|matched| matched.file_hash == EXPECTED_HASH)
    );
    println!("profile stage=find_literal_cold elapsed_ns={elapsed}");
    Ok(())
}

fn measure_reused_read(path: &std::path::Path) -> TestResult {
    let mut text = String::new();
    measure("read_reused", 7_340_480, || {
        text.clear();
        fs::File::open(path)?.read_to_string(&mut text)?;
        assert_eq!(text.as_bytes().last(), Some(&b'\n'));
        Ok(text.len())
    })
}
