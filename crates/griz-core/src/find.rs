//! Finds code across files and returns matches a program can edit directly.
//!
//! Text queries take a literal or a regular expression; structural queries take
//! an ast-grep pattern such as `foo($A, $$$REST)`. Every query answers with the
//! same match shape, so an edit built from one kind works for the other.

use crate::{
    ByteRange, content_hash,
    find_position::{self, Cursor, to_match},
    scope,
    structural::{self, Shape},
};
use ast_grep_language::SupportLang;
use ignore::{WalkBuilder, overrides::OverrideBuilder};
use regex::Regex;
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

enum Matcher {
    Text(Regex),
    Shape(Shape),
}

impl Matcher {
    fn new(query: &FindQuery) -> Result<Self, String> {
        match (&query.literal, &query.regex, &query.pattern) {
            (Some(literal), None, None) => text(&regex::escape(literal)),
            (None, Some(regex), None) => text(regex),
            (None, None, Some(pattern)) => {
                Shape::new(pattern, query.language.as_deref()).map(Self::Shape)
            }
            _ => Err("give exactly one of `literal`, `regex`, or `pattern`".to_string()),
        }
    }

    fn hits(&self, path: &Path, text: &str) -> Result<Vec<Hit>, String> {
        match self {
            Self::Text(regex) => Ok(regex
                .captures_iter(text)
                .map(|captures| Hit {
                    range: captures.get(0).map_or(0..0, |m| m.range()),
                    captures: captures
                        .iter()
                        .skip(1)
                        .map(|group| group.map(|m| m.as_str().to_string()))
                        .collect(),
                    vars: BTreeMap::new(),
                    var_ranges: BTreeMap::new(),
                })
                .collect()),
            Self::Shape(shape) => shape.hits(path, text),
        }
    }
}

fn text(source: &str) -> Result<Matcher, String> {
    if source.is_empty() {
        return Err("the pattern is empty".to_string());
    }
    Regex::new(source)
        .map(Matcher::Text)
        .map_err(|error| error.to_string())
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
        let Some(hits) = find_in_file(&matcher, query, &path, &text)? else {
            continue;
        };
        collect_file(&mut page, query, hits, (&path, &text));
    }
    let shown = query.offset + page.matches.len();
    page.next = (shown < page.total).then_some(shown);
    Ok(page)
}

/// Every hit in `text`, scoped to `query.within` when it is set; `None`
/// skips a file whose language cannot be checked against it.
fn find_in_file(
    matcher: &Matcher,
    query: &FindQuery,
    path: &Path,
    text: &str,
) -> Result<Option<Vec<Hit>>, String> {
    if query.within.is_empty() {
        return Ok(Some(matcher.hits(path, text)?));
    }
    let Some(language) = structural::enabled_language(path) else {
        return Ok(None);
    };
    let spans = scope::spans(language, text, &query.within);
    let hits = matcher.hits(path, text)?;
    Ok(Some(
        hits.into_iter()
            .filter(|hit| spans.iter().any(|span| encloses(span, &hit.range)))
            .collect(),
    ))
}

fn encloses(span: &Range<usize>, range: &Range<usize>) -> bool {
    span.start <= range.start && range.end <= span.end
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

fn collect_file(
    page: &mut FindPage,
    query: &FindQuery,
    hits: Vec<Hit>,
    (path, text): (&Path, &str),
) {
    if hits.is_empty() {
        return;
    }
    page.files += 1;
    let skip = query.offset.saturating_sub(page.total);
    page.total += hits.len();
    let remaining = query.limit - page.matches.len();
    if skip >= hits.len() || remaining == 0 {
        return;
    }
    let hash = content_hash(text);
    let mut cursor = Cursor::default();
    for hit in hits.into_iter().skip(skip).take(remaining) {
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
