//! Shared fixtures for griz-core tests.

use griz_core::{Anchor, MapSource, Occurrence, Op};
use std::{collections::BTreeMap, path::PathBuf};

/// A recorded source holding `files` as `(path, text)` pairs.
pub fn source(files: &[(&str, &str)]) -> MapSource {
    MapSource::new(
        files
            .iter()
            .map(|(path, text)| (PathBuf::from(path), Some((*text).to_string())))
            .collect::<BTreeMap<_, _>>(),
    )
}

/// A recorded source that also knows `absent` paths do not exist.
pub fn source_with_absent(files: &[(&str, &str)], absent: &[&str]) -> MapSource {
    let mut map: BTreeMap<PathBuf, Option<String>> = files
        .iter()
        .map(|(path, text)| (PathBuf::from(path), Some((*text).to_string())))
        .collect();
    for path in absent {
        map.insert(PathBuf::from(path), None);
    }
    MapSource::new(map)
}

/// An anchor for `text`.
pub fn anchor(text: &str) -> Anchor {
    Anchor {
        text: text.to_string(),
        after: None,
        whole_lines: false,
    }
}

/// A unique anchored replacement.
pub fn replace(path: &str, find: &str, with: &str) -> Op {
    Op::Replace {
        path: PathBuf::from(path),
        find: Some(anchor(find)),
        range: None,
        pattern: None,
        replace: with.to_string(),
        occurrence: Occurrence::Unique,
        target: None,
        expect_hash: None,
    }
}
