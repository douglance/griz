//! Addressed reads of long text: numbered lines by pattern or by range.

use regex::Regex;
use schemars::JsonSchema;
use serde::Serialize;

/// One numbered line.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Line {
    /// 1-based line number.
    pub line: usize,
    /// Line text.
    pub text: String,
}

/// Lines selected from a text.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Selected {
    /// Selected lines.
    pub lines: Vec<Line>,
    /// Lines in the whole text.
    pub total_lines: usize,
    /// Lines matching `grep`, when given.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched: Option<usize>,
}

/// How to address a text.
#[derive(Debug, Clone, Default)]
pub struct Address {
    /// Pattern selecting lines.
    pub grep: Option<String>,
    /// Lines of context around each match.
    pub context: usize,
    /// Inclusive `A-B` range.
    pub lines: Option<String>,
}

/// Selects lines from `text`.
///
/// # Errors
/// Returns a message for a bad pattern or range.
pub fn address(text: &str, address: &Address) -> Result<Selected, String> {
    let all: Vec<&str> = text.lines().collect();
    let (keep, matched) = match (&address.grep, &address.lines) {
        (Some(pattern), _) => {
            let (keep, matched) = by_pattern(&all, pattern, address.context)?;
            (keep, Some(matched))
        }
        (None, Some(range)) => (by_range(all.len(), range)?, None),
        (None, None) => (vec![true; all.len()], None),
    };
    Ok(Selected {
        lines: all
            .iter()
            .enumerate()
            .filter(|(index, _)| keep[*index])
            .map(|(index, text)| Line {
                line: index + 1,
                text: (*text).to_string(),
            })
            .collect(),
        total_lines: all.len(),
        matched,
    })
}

fn by_pattern(all: &[&str], pattern: &str, context: usize) -> Result<(Vec<bool>, usize), String> {
    let regex = Regex::new(pattern).map_err(|e| e.to_string())?;
    let mut keep = vec![false; all.len()];
    let mut covered = 0;
    let mut matched = 0;
    for (index, _) in all
        .iter()
        .enumerate()
        .filter(|(_, line)| regex.is_match(line))
    {
        let end = index
            .saturating_add(context)
            .saturating_add(1)
            .min(all.len());
        let start = index.saturating_sub(context).max(covered);
        keep[start..end].fill(true);
        covered = end;
        matched += 1;
    }
    Ok((keep, matched))
}

fn by_range(total: usize, range: &str) -> Result<Vec<bool>, String> {
    let (start, end) = range
        .split_once('-')
        .and_then(|(a, b)| {
            Some((
                a.trim().parse::<usize>().ok()?,
                b.trim().parse::<usize>().ok()?,
            ))
        })
        .filter(|(a, b)| *a >= 1 && a <= b)
        .ok_or_else(|| format!("line range `{range}` must look like 10-20"))?;
    Ok((1..=total)
        .map(|line| (start..=end).contains(&line))
        .collect())
}

/// Adds selected lines to a response object, omitting `matched` when no
/// pattern was given.
pub fn merge_lines(body: &mut serde_json::Value, selected: Selected) {
    if let (serde_json::Value::Object(map), Ok(serde_json::Value::Object(lines))) =
        (body, serde_json::to_value(selected))
    {
        map.extend(lines);
    }
}
