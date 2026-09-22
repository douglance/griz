//! Finds code across files and returns matches a program can edit directly.
//!
//! Text queries take a literal or a regular expression; structural queries take
//! an ast-grep pattern such as `foo($A, $$$REST)`. Every query answers with the
//! same match shape, so an edit built from one kind works for the other.

use crate::{
    ByteRange, content_hash,
    find_hits::{FileHits, Matcher},
    find_position::{self, Cursor, to_match},
    scope, structural,
};
use ast_grep_language::SupportLang;
use ignore::{WalkBuilder, overrides::OverrideBuilder};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    ops::Range,
    path::{Path, PathBuf},
};

/// What to search and where.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct FindQuery {
    /// Files or directories to search. Directories honor `.gitignore`.
    pub paths: Vec<PathBuf>,
    /// Glob patterns a file must match, such as `**/*.rs`. `!` excludes.
    #[serde(default)]
    pub globs: Vec<String>,
    /// Literal text to find. Give exactly one of `literal`, `regex`, `pattern`.
    pub literal: Option<String>,
    /// Regular expression to find, with capture groups.
    pub regex: Option<String>,
    /// Structural pattern with `$VAR` and `$$$VARS` metavariables.
    #[serde(default)]
    pub pattern: Option<String>,
    /// Language for a structural pattern; defaults to each file's extension.
    #[serde(default)]
    pub language: Option<String>,
    /// Matches to skip, for paging.
    #[serde(default)]
    pub offset: usize,
    /// Most matches to return.
    pub limit: usize,
    /// Only match inside syntax nodes of these kinds, such as `comment` or
    /// `string`, in enabled languages. Files in another language are
    /// skipped once this is set.
    #[serde(default)]
    pub within: Vec<String>,
}

/// One match, carrying everything needed to edit exactly this text later.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Match {
    /// File containing the match.
    pub path: PathBuf,
    /// 1-based line.
    pub line: usize,
    /// 1-based column in characters.
    pub column: usize,
    /// Byte range in the file.
    pub range: ByteRange,
    /// Matched text.
    pub text: String,
    /// Regex capture groups in order; group 0 is the whole match and is omitted.
    pub captures: Vec<Option<String>>,
    /// Text captured by each structural metavariable, by name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub vars: BTreeMap<String, String>,
    /// Byte range captured by each structural metavariable, by name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub var_ranges: BTreeMap<String, ByteRange>,
    /// Fingerprint of the whole file when searched.
    pub file_hash: String,
}

/// One page of matches.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FindPage {
    /// Matches on this page.
    pub matches: Vec<Match>,
    /// Matches across every file.
    pub total: usize,
    /// Files with at least one match.
    pub files: usize,
    /// Offset of the next page, when there is one.
    pub next: Option<usize>,
}

/// A located span, before it becomes a [`Match`].
#[derive(Debug, Clone)]
pub struct Hit {
    /// Byte range in the file.
    pub range: Range<usize>,
    /// Regex capture groups.
    pub captures: Vec<Option<String>>,
    /// Structural metavariables.
    pub vars: BTreeMap<String, String>,
    /// Byte range of each structural metavariable, by name.
    pub var_ranges: BTreeMap<String, Range<usize>>,
}

/// Runs a query.
///
/// # Errors
/// Returns a message for a bad pattern, a bad glob, or no pattern at all.
pub fn find(query: &FindQuery) -> Result<FindPage, String> {
    let matcher = Matcher::new(query)?;
    let paths = files(query)?;
    validate_within(query, &paths)?;
    let mut page = FindPage::default();
    for path in paths {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let window = (
            query.offset.saturating_sub(page.total),
            query.limit - page.matches.len(),
        );
        let Some(hits) = find_in_file(&matcher, query, &path, &text, window)? else {
            continue;
        };
        collect_file(&mut page, hits, (&path, &text));
    }
    let shown = query.offset + page.matches.len();
    page.next = (shown < page.total).then_some(shown);
    Ok(page)
}

/// Counts in-scope hits and selects this file's portion of the requested page.
/// `None` skips a file whose language cannot be checked against `within`.
fn find_in_file(
    matcher: &Matcher,
    query: &FindQuery,
    path: &Path,
    text: &str,
    window: (usize, usize),
) -> Result<Option<FileHits>, String> {
    if query.within.is_empty() {
        return matcher.hits(path, text, None, window).map(Some);
    }
    let Some(language) = structural::enabled_language(path) else {
        return Ok(None);
    };
    let spans = scope::spans(language, text, &query.within);
    matcher.hits(path, text, Some(&spans), window).map(Some)
}

/// Checks `within` before searching, once against every enabled language
/// among the matched files.
fn validate_within(query: &FindQuery, paths: &[PathBuf]) -> Result<(), String> {
    if query.within.is_empty() {
        return Ok(());
    }
    let languages: HashSet<SupportLang> = paths
        .iter()
        .filter_map(|path| structural::enabled_language(path))
        .collect();
    scope::validate(&query.within, &languages)
}

fn collect_file(page: &mut FindPage, hits: FileHits, (path, text): (&Path, &str)) {
    if hits.total == 0 {
        return;
    }
    page.files += 1;
    page.total += hits.total;
    if hits.hits.is_empty() {
        return;
    }
    let hash = content_hash(text);
    let mut cursor = Cursor::default();
    for hit in hits.hits {
        let position = find_position::position(&mut cursor, text, hit.range.start);
        page.matches
            .push(to_match(path, text, hit, &hash, position));
    }
}

/// Every searchable file, sorted so pages are stable.
fn files(query: &FindQuery) -> Result<Vec<PathBuf>, String> {
    let mut found = Vec::new();
    for root in &query.paths {
        if root.is_file() {
            found.push(root.clone());
            continue;
        }
        let mut walk = WalkBuilder::new(root);
        walk.overrides(overrides(root, &query.globs)?);
        found.extend(
            walk.build()
                .filter_map(Result::ok)
                .filter(|entry| entry.file_type().is_some_and(|kind| kind.is_file()))
                .map(ignore::DirEntry::into_path),
        );
    }
    found.sort();
    found.dedup();
    Ok(found)
}

fn overrides(root: &Path, globs: &[String]) -> Result<ignore::overrides::Override, String> {
    let mut builder = OverrideBuilder::new(root);
    for glob in globs {
        builder.add(glob).map_err(|error| error.to_string())?;
    }
    builder.build().map_err(|error| error.to_string())
}
