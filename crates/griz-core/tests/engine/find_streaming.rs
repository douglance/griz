//! Paging preserves capture and scope semantics while counting every match.

use griz_core::{FindQuery, find};
use std::{error::Error, fs};

type TestResult = Result<(), Box<dyn Error>>;

#[test]
fn optional_captures_survive_page_selection() -> TestResult {
    let dir = tempfile::tempdir()?;
    fs::write(dir.path().join("a.txt"), "é a b a22\n")?;
    let expected = [
        vec![Some("a".into()), Some(String::new()), None],
        vec![None, None, Some("b".into())],
        vec![Some("a".into()), Some("22".into()), None],
    ];
    for (offset, limit) in [(0, 3), (1, 2), (2, 1), (3, 0)] {
        let page = find(&FindQuery {
            paths: vec![dir.path().to_path_buf()],
            regex: Some("(a)([0-9]*)|(b)".into()),
            offset,
            limit,
            ..FindQuery::default()
        })?;
        assert_eq!((page.total, page.files, page.next), (3, 1, None));
        let captures: Vec<_> = page.matches.into_iter().map(|hit| hit.captures).collect();
        assert_eq!(captures, expected[offset..]);
    }
    Ok(())
}

#[test]
fn empty_matches_keep_unicode_boundaries_with_and_without_captures() -> TestResult {
    let dir = tempfile::tempdir()?;
    fs::write(dir.path().join("a.txt"), "é\n")?;
    for regex in ["()", "(?:)"] {
        let page = find(&FindQuery {
            paths: vec![dir.path().to_path_buf()],
            regex: Some(regex.into()),
            offset: 1,
            limit: 1,
            ..FindQuery::default()
        })?;
        assert_eq!((page.total, page.files, page.next), (3, 1, Some(2)));
        let hit = page.matches.first().ok_or("missing empty match")?;
        assert_eq!(
            (hit.line, hit.column, hit.range.start, hit.range.end),
            (1, 2, 2, 2)
        );
        assert_eq!(hit.text, "");
        let expected = if regex == "()" {
            vec![Some(String::new())]
        } else {
            vec![]
        };
        assert_eq!(hit.captures, expected);
    }
    Ok(())
}

#[test]
fn scoped_text_is_filtered_before_counting_and_paging() -> TestResult {
    let dir = tempfile::tempdir()?;
    fs::write(
        dir.path().join("a.rs"),
        "let word = \"word\"; // word\n// word\nfn word() {}\n",
    )?;
    for capturing in [false, true] {
        let page = find(&FindQuery {
            paths: vec![dir.path().to_path_buf()],
            literal: (!capturing).then(|| "word".into()),
            regex: capturing.then(|| "(w)(ord)".into()),
            within: vec!["comment".into()],
            offset: 1,
            limit: 1,
            ..FindQuery::default()
        })?;
        assert_eq!((page.total, page.files, page.next), (2, 1, None));
        let hit = page.matches.first().ok_or("missing comment match")?;
        assert_eq!(
            (hit.line, hit.column, hit.range.start, hit.range.end),
            (2, 4, 30, 34)
        );
        let expected = if capturing {
            vec![Some("w".into()), Some("ord".into())]
        } else {
            vec![]
        };
        assert_eq!(hit.captures, expected);
    }
    Ok(())
}

#[test]
fn structural_paging_preserves_metavariables_and_overlapping_scopes() -> TestResult {
    let dir = tempfile::tempdir()?;
    let text = "const X: i32 = call(zero);\nfn first(){ call(one); call(two); }\nfn last(){ call(three); }\n";
    fs::write(dir.path().join("a.rs"), text)?;
    let page = find(&FindQuery {
        paths: vec![dir.path().to_path_buf()],
        pattern: Some("call($A)".into()),
        within: vec!["function_item".into(), "block".into()],
        offset: 1,
        limit: 1,
        ..FindQuery::default()
    })?;
    assert_eq!((page.total, page.files, page.next), (3, 1, Some(2)));
    let hit = page.matches.first().ok_or("missing structural match")?;
    assert_eq!(hit.text, "call(two)");
    assert_eq!(hit.vars.get("A").map(String::as_str), Some("two"));
    let range = hit.var_ranges.get("A").ok_or("missing capture range")?;
    assert_eq!((range.start, range.end), (55, 58));
    Ok(())
}
