//! Finds text across files and returns matches a program can edit directly.

use crate::{ByteRange, content_hash, matcher::line_of};
use ignore::{WalkBuilder, overrides::OverrideBuilder};
use regex::Regex;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// What to search and where.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct FindQuery {
    /// Files or directories to search. Directories honor `.gitignore`.
    pub paths: Vec<PathBuf>,
    /// Glob patterns a file must match, such as `**/*.rs`. `!` excludes.
    #[serde(default)]
    pub globs: Vec<String>,
    /// Literal text to find. Exactly one of `literal` and `regex` is required.
    pub literal: Option<String>,
    /// Regular expression to find, with capture groups.
    pub regex: Option<String>,
    /// Matches to skip, for paging.
    #[serde(default)]
    pub offset: usize,
    /// Most matches to return.
    pub limit: usize,
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
    /// Capture groups in order; group 0 is the whole match and is omitted.
    pub captures: Vec<Option<String>>,
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

/// Runs a query.
///
/// # Errors
/// Returns a message for a bad pattern, a bad glob, or no pattern at all.
pub fn find(query: &FindQuery) -> Result<FindPage, String> {
    let pattern = pattern(query)?;
    let mut page = FindPage::default();
    for path in files(query)? {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        collect_file(&mut page, query, &pattern, (&path, &text));
    }
    let shown = query.offset + page.matches.len();
    page.next = (shown < page.total).then_some(shown);
    Ok(page)
}

fn collect_file(
    page: &mut FindPage,
    query: &FindQuery,
    pattern: &Regex,
    (path, text): (&Path, &str),
) {
    let hits: Vec<_> = pattern.captures_iter(text).collect();
    if hits.is_empty() {
        return;
    }
    page.files += 1;
    let hash = content_hash(text);
    for captures in hits {
        let keep = page.total >= query.offset && page.matches.len() < query.limit;
        page.total += 1;
        if keep {
            page.matches.push(to_match(path, text, &captures, &hash));
        }
    }
}

fn pattern(query: &FindQuery) -> Result<Regex, String> {
    let source = match (&query.literal, &query.regex) {
        (Some(literal), None) => regex::escape(literal),
        (None, Some(regex)) => regex.clone(),
        _ => return Err("give exactly one of `literal` or `regex`".to_string()),
    };
    if source.is_empty() {
        return Err("the pattern is empty".to_string());
    }
    Regex::new(&source).map_err(|error| error.to_string())
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

fn to_match(path: &Path, text: &str, captures: &regex::Captures<'_>, hash: &str) -> Match {
    let whole = captures.get(0).map_or(0..0, |m| m.range());
    let line_start = text[..whole.start].rfind('\n').map_or(0, |at| at + 1);
    Match {
        path: path.to_path_buf(),
        line: line_of(text, whole.start),
        column: text[line_start..whole.start].chars().count() + 1,
        range: ByteRange {
            start: whole.start,
            end: whole.end,
        },
        text: text[whole.clone()].to_string(),
        captures: captures
            .iter()
            .skip(1)
            .map(|group| group.map(|m| m.as_str().to_string()))
            .collect(),
        file_hash: hash.to_string(),
    }
}
