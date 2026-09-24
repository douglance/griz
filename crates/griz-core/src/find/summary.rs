//! File-level search results for callers that do not need individual spans.

use super::{FindQuery, Matcher, content_hash, files, find_in_file, read_text, validate_within};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// A matching file, its match count, and the fingerprint searched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FileSummary {
    /// File containing the matches.
    pub path: PathBuf,
    /// Matches in this file.
    pub count: usize,
    /// Fingerprint of the whole file when searched.
    pub file_hash: String,
}

/// One page of matching files.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FindFilesPage {
    /// Matching files on this page.
    pub file_matches: Vec<FileSummary>,
    /// Matches across every file, including files outside this page.
    pub total: usize,
    /// Files with at least one match, including files outside this page.
    pub files: usize,
    /// Offset of the next file page, when there is one.
    pub next: Option<usize>,
}

/// Searches with the same filters as `super::find`, paging by matching file.
///
/// `offset` and `limit` count files instead of individual matches.
///
/// # Errors
/// Returns a message for an invalid query or a filesystem traversal/read failure.
pub fn find_files(query: &FindQuery) -> Result<FindFilesPage, String> {
    let matcher = Matcher::new(query)?;
    let paths = files(query)?;
    validate_within(query, &paths)?;
    let mut page = FindFilesPage::default();
    for path in paths {
        let Some(text) = read_text(&path)? else {
            continue;
        };
        let Some(hits) = find_in_file(&matcher, query, &path, &text, (0, 0))? else {
            continue;
        };
        collect(&mut page, query, &path, &text, hits.total);
    }
    let shown = query.offset.saturating_add(page.file_matches.len());
    page.next = (shown < page.files).then_some(shown);
    Ok(page)
}

fn collect(page: &mut FindFilesPage, query: &FindQuery, path: &Path, text: &str, count: usize) {
    if count == 0 {
        return;
    }
    page.files += 1;
    page.total += count;
    if page.files - 1 < query.offset || page.file_matches.len() >= query.limit {
        return;
    }
    page.file_matches.push(FileSummary {
        path: path.to_path_buf(),
        count,
        file_hash: content_hash(text),
    });
}
