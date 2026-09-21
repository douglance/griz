//! Locates anchor text, stepping down a ladder of tolerance.
//!
//! Each rung is tried only when every stricter rung found nothing, and the rung
//! that matched is reported, so a tolerant match can never pass as exact.
//!
//! Order matters: every indentation match is also a trimmed match, so the
//! indentation rung must run first or it could never be reached.

use crate::{Anchor, ByteRange, Rung, Window};
use similar::TextDiff;

/// One place an anchor matched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// Matched bytes in the searched text.
    pub range: ByteRange,
    /// Rung that matched.
    pub rung: Rung,
    /// Indentation of the anchor and of the matched lines, for re-indenting.
    pub indent: Option<(String, String)>,
}

/// Result of locating an anchor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Located {
    /// Every match at the strictest rung that matched anything.
    Found(Vec<Found>),
    /// Nothing matched; the closest real text, if any.
    Missing(Option<Window>),
}

/// Upper bound on window comparisons when searching for the nearest text.
const NEAREST_BUDGET: usize = 2_000_000;

/// Locates `anchor` in `text`.
#[must_use]
pub fn locate(text: &str, anchor: &Anchor) -> Located {
    let offset = match &anchor.after {
        Some(after) => match text.find(after.as_str()) {
            Some(at) => at + after.len(),
            None => return Located::Missing(None),
        },
        None => 0,
    };
    let region = &text[offset..];
    let found = exact(region, anchor)
        .or_else(|| by_lines(region, &anchor.text, Rung::TrailingWhitespace))
        .or_else(|| by_lines(region, &anchor.text, Rung::Indentation))
        .or_else(|| by_lines(region, &anchor.text, Rung::Trimmed));
    match found {
        Some(found) => Located::Found(found.into_iter().map(|f| shift(f, offset)).collect()),
        None => Located::Missing(nearest(text, &anchor.text)),
    }
}

fn shift(mut found: Found, offset: usize) -> Found {
    found.range.start += offset;
    found.range.end += offset;
    found
}

fn exact(text: &str, anchor: &Anchor) -> Option<Vec<Found>> {
    let needle = anchor.text.as_str();
    let found: Vec<_> = text
        .match_indices(needle)
        .map(|(start, _)| ByteRange {
            start,
            end: start + needle.len(),
        })
        .filter(|range| !anchor.whole_lines || on_line_bounds(text, *range))
        .map(|range| Found {
            range,
            rung: Rung::Exact,
            indent: None,
        })
        .collect();
    (!found.is_empty()).then_some(found)
}

fn on_line_bounds(text: &str, range: ByteRange) -> bool {
    let starts = range.start == 0 || text.as_bytes()[range.start - 1] == b'\n';
    let ends = range.end == text.len()
        || text.as_bytes()[range.end] == b'\n'
        || text[..range.end].ends_with('\n');
    starts && ends
}

/// Byte span of every line, excluding its newline.
fn line_spans(text: &str) -> Vec<ByteRange> {
    let mut spans = Vec::new();
    let mut start = 0;
    for (at, _) in text.match_indices('\n') {
        spans.push(ByteRange { start, end: at });
        start = at + 1;
    }
    if start < text.len() {
        spans.push(ByteRange {
            start,
            end: text.len(),
        });
    }
    spans
}

fn needle_lines(needle: &str) -> Vec<&str> {
    needle.trim_end_matches('\n').split('\n').collect()
}

fn by_lines(text: &str, needle: &str, rung: Rung) -> Option<Vec<Found>> {
    let wanted = needle_lines(needle);
    let spans = line_spans(text);
    if wanted.is_empty() || spans.len() < wanted.len() {
        return None;
    }
    let lines: Vec<&str> = spans.iter().map(|s| &text[s.start..s.end]).collect();
    let found: Vec<_> = (0..=lines.len() - wanted.len())
        .filter_map(|at| {
            let window = &lines[at..at + wanted.len()];
            let matched = window_matches(window, &wanted, rung)?;
            Some(Found {
                range: ByteRange {
                    start: spans[at].start,
                    end: spans[at + wanted.len() - 1].end,
                },
                rung,
                indent: matched.indent,
            })
        })
        .collect();
    (!found.is_empty()).then_some(found)
}

/// A window that matched, with indentation for the indentation rung.
struct Matched {
    indent: Option<(String, String)>,
}

fn window_matches(window: &[&str], wanted: &[&str], rung: Rung) -> Option<Matched> {
    let mut pairs = window.iter().zip(wanted);
    let have_indent = common_indent(window);
    let want_indent = common_indent(wanted);
    let matched = match rung {
        Rung::TrailingWhitespace => {
            return pairs
                .all(|(have, want)| have.trim_end() == want.trim_end())
                .then_some(Matched { indent: None });
        }
        Rung::Indentation => {
            pairs.all(|(have, want)| dedent(have, &have_indent) == dedent(want, &want_indent))
        }
        Rung::Trimmed => pairs.all(|(have, want)| have.trim() == want.trim()),
        _ => false,
    };
    matched.then_some(Matched {
        indent: Some((want_indent, have_indent)),
    })
}

/// Longest leading whitespace shared by every non-blank line.
pub fn common_indent(lines: &[&str]) -> String {
    lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| &line[..line.len() - line.trim_start().len()])
        .reduce(|left, right| {
            let shared = left
                .chars()
                .zip(right.chars())
                .take_while(|(a, b)| a == b)
                .map(|(a, _)| a.len_utf8())
                .sum();
            &left[..shared]
        })
        .unwrap_or_default()
        .to_string()
}

fn dedent<'a>(line: &'a str, indent: &str) -> &'a str {
    if line.trim().is_empty() {
        return "";
    }
    line.strip_prefix(indent).unwrap_or(line).trim_end()
}

/// Closest window of real text with the same number of lines as `needle`.
fn nearest(text: &str, needle: &str) -> Option<Window> {
    let wanted = needle_lines(needle).len();
    let spans = line_spans(text);
    if spans.is_empty() || wanted == 0 {
        return None;
    }
    let size = wanted.min(spans.len());
    let windows = spans.len() - size + 1;
    if windows.saturating_mul(needle.len().max(1)) > NEAREST_BUDGET {
        return None;
    }
    (0..windows)
        .map(|at| {
            let slice = &text[spans[at].start..spans[at + size - 1].end];
            let score = TextDiff::from_chars(needle.trim_end_matches('\n'), slice).ratio();
            (score, at, slice)
        })
        .max_by(|left, right| left.0.total_cmp(&right.0).then(right.1.cmp(&left.1)))
        .filter(|(score, _, _)| *score > 0.0)
        .map(|(_, at, slice)| Window {
            line: at + 1,
            text: slice.to_string(),
        })
}

/// 1-based line containing byte `at`.
#[must_use]
pub fn line_of(text: &str, at: usize) -> usize {
    text.as_bytes()[..at.min(text.len())]
        .split(|byte| *byte == b'\n')
        .count()
}
