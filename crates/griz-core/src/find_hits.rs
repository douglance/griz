//! Selects a file's page before allocating owned text matches.

use crate::{
    find::{FindQuery, Hit},
    structural::Shape,
};
use regex::{Captures, Regex};
use std::{collections::BTreeMap, ops::Range, path::Path};

#[derive(Default)]
pub(crate) struct FileHits {
    pub total: usize,
    pub hits: Vec<Hit>,
}

pub(crate) enum Matcher {
    Text(Regex),
    Shape(Shape),
}

impl Matcher {
    pub(crate) fn new(query: &FindQuery) -> Result<Self, String> {
        match (&query.literal, &query.regex, &query.pattern) {
            (Some(literal), None, None) => text(&regex::escape(literal)),
            (None, Some(regex), None) => text(regex),
            (None, None, Some(pattern)) => {
                Shape::new(pattern, query.language.as_deref()).map(Self::Shape)
            }
            _ => Err("give exactly one of `literal`, `regex`, or `pattern`".to_string()),
        }
    }

    pub(crate) fn hits(
        &self,
        path: &Path,
        text: &str,
        spans: Option<&[Range<usize>]>,
        window: (usize, usize),
    ) -> Result<FileHits, String> {
        match self {
            Self::Text(regex) => Ok(text_hits(regex, text, spans, window)),
            Self::Shape(shape) => {
                let hits = shape.hits(path, text)?;
                let scoped = hits.into_iter().filter(|hit| within(spans, &hit.range));
                Ok(select(scoped, window, std::convert::identity))
            }
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

fn text_hits(
    regex: &Regex,
    text: &str,
    spans: Option<&[Range<usize>]>,
    window: (usize, usize),
) -> FileHits {
    if window.1 == 0 {
        return FileHits {
            total: regex
                .find_iter(text)
                .filter(|found| within(spans, &found.range()))
                .count(),
            hits: Vec::new(),
        };
    }
    if regex.captures_len() == 1 {
        let hits = regex
            .find_iter(text)
            .filter(|found| within(spans, &found.range()));
        return select(hits, window, |found| plain_hit(found.range()));
    }
    let hits = regex
        .captures_iter(text)
        .filter(|found| within(spans, &capture_range(found)));
    select(hits, window, |found| captured_hit(&found))
}

fn within(spans: Option<&[Range<usize>]>, range: &Range<usize>) -> bool {
    spans.is_none_or(|spans| {
        spans
            .iter()
            .any(|span| span.start <= range.start && range.end <= span.end)
    })
}

fn select<T>(
    hits: impl Iterator<Item = T>,
    (offset, limit): (usize, usize),
    into_hit: impl Fn(T) -> Hit,
) -> FileHits {
    let mut page = FileHits::default();
    for found in hits {
        let keep = page.total >= offset && page.hits.len() < limit;
        page.total += 1;
        if keep {
            page.hits.push(into_hit(found));
        }
    }
    page
}

fn capture_range(captures: &Captures<'_>) -> Range<usize> {
    captures.get(0).map_or(0..0, |found| found.range())
}

fn captured_hit(captures: &Captures<'_>) -> Hit {
    let mut hit = plain_hit(capture_range(captures));
    hit.captures = captures
        .iter()
        .skip(1)
        .map(|group| group.map(|m| m.as_str().to_string()))
        .collect();
    hit
}

fn plain_hit(range: Range<usize>) -> Hit {
    Hit {
        range,
        captures: Vec::new(),
        vars: BTreeMap::new(),
        var_ranges: BTreeMap::new(),
    }
}
