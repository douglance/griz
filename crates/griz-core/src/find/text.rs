//! Fresh file bytes validate a cached page before another UTF-8 scan.
use super::{FindPage, FindQuery, Matcher, find_cache, page_in_file};
use std::path::Path;

pub(super) fn read(path: &Path) -> Result<Option<String>, String> {
    Ok(bytes(path)?.and_then(|bytes| String::from_utf8(bytes).ok()))
}

pub(super) fn page(
    matcher: &Matcher,
    query: &FindQuery,
    path: &Path,
    window: (usize, usize),
) -> Result<Option<FindPage>, String> {
    let Some(bytes) = bytes(path)? else {
        return Ok(None);
    };
    if let Some(page) = find_cache::lookup(query, path, &bytes, window) {
        return Ok(Some(page));
    }
    let Ok(text) = String::from_utf8(bytes) else {
        return Ok(None);
    };
    page_in_file(matcher, query, path, &text, window).map(Some)
}

fn bytes(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::InvalidData => Ok(None),
        Err(error) => Err(format!("reading {}: {error}", path.display())),
    }
}
