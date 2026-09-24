//! Parses Codex patch text into operations.
//!
//! The format agents already write fluently:
//!
//! ```text
//! *** Begin Patch
//! *** Add File: path
//! +line
//! *** Delete File: path
//! *** Update File: path
//! *** Move to: new/path
//! @@ optional line that precedes the hunk
//!  context
//! -removed
//! +added
//! *** End Patch
//! ```
//!
//! Each hunk becomes a whole-line anchored replacement, so it inherits the
//! matching ladder, uniqueness rule, and nearest-text reporting of every other
//! operation. Several blocks for one file simply plan in sequence.

use crate::{Anchor, Occurrence, Op};
use std::path::{Path, PathBuf};

/// Why patch text could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("patch line {line}: {message}")]
pub struct PatchError {
    /// 1-based line in the patch text.
    pub line: usize,
    /// What is wrong.
    pub message: String,
}

#[derive(Default)]
struct Hunk {
    header: Option<String>,
    old: Vec<String>,
    new: Vec<String>,
}

enum Block {
    Add {
        path: PathBuf,
        lines: Vec<String>,
    },
    Delete {
        path: PathBuf,
    },
    Update {
        path: PathBuf,
        to: Option<PathBuf>,
        hunks: Vec<Hunk>,
    },
}

/// Parses patch text into operations, resolving each path with `resolve`.
///
/// Several patch documents may follow one another, each its own
/// `*** Begin Patch` … `*** End Patch` pair, so a program can concatenate
/// what it built separately.
///
/// # Errors
/// Returns the first line that does not fit the format.
pub fn parse_patch(text: &str, resolve: &dyn Fn(&str) -> PathBuf) -> Result<Vec<Op>, PatchError> {
    let mut lines = text.lines().enumerate().peekable();
    let begins = lines
        .next()
        .is_some_and(|(_, line)| line.trim() == "*** Begin Patch");
    if !begins {
        return Err(error(0, "patch must start with `*** Begin Patch`"));
    }
    let mut blocks = Vec::new();
    while let Some((at, line)) = lines.next() {
        let line = line.trim_end();
        if line == "*** End Patch" && !another_document(&mut lines)? {
            return Ok(blocks.into_iter().flat_map(to_ops).collect());
        }
        if line == "*** End Patch" {
            continue;
        }
        let block = if let Some(path) = line.strip_prefix("*** Add File: ") {
            Block::Add {
                path: resolve(path.trim()),
                lines: Vec::new(),
            }
        } else if let Some(path) = line.strip_prefix("*** Delete File: ") {
            Block::Delete {
                path: resolve(path.trim()),
            }
        } else if let Some(path) = line.strip_prefix("*** Update File: ") {
            Block::Update {
                path: resolve(path.trim()),
                to: None,
                hunks: Vec::new(),
            }
        } else {
            return Err(error(at, &format!("unexpected line `{line}`")));
        };
        blocks.push(block);
        let block = blocks.last_mut().ok_or_else(|| error(at, "no block"))?;
        while let Some((at, body)) = lines.next_if(|(_, next)| {
            !next.starts_with("*** ")
                || next.starts_with("*** Move to: ")
                || next.starts_with("*** End of File")
        }) {
            body_line(block, at, body, resolve)?;
        }
    }
    Err(error(
        text.lines().count(),
        "patch must end with `*** End Patch`",
    ))
}

/// Whether another patch document follows, consuming its `*** Begin Patch`
/// and any blank lines before it. Anything else after a document ends is an
/// error, so stray text is never silently dropped.
fn another_document<'a, I>(lines: &mut std::iter::Peekable<I>) -> Result<bool, PatchError>
where
    I: Iterator<Item = (usize, &'a str)>,
{
    while let Some(&(at, line)) = lines.peek() {
        let line = line.trim();
        if line.is_empty() {
            lines.next();
            continue;
        }
        if line == "*** Begin Patch" {
            lines.next();
            return Ok(true);
        }
        return Err(error(at, "text after `*** End Patch`"));
    }
    Ok(false)
}

fn body_line(
    block: &mut Block,
    at: usize,
    body: &str,
    resolve: &dyn Fn(&str) -> PathBuf,
) -> Result<(), PatchError> {
    match block {
        Block::Add { lines, .. } => {
            let line = body
                .strip_prefix('+')
                .ok_or_else(|| error(at, "added file lines must start with `+`"))?;
            lines.push(line.to_string());
            Ok(())
        }
        Block::Delete { .. } => Err(error(at, "a deleted file has no body")),
        Block::Update { to, hunks, .. } => update_line(to, hunks, (at, body), resolve),
    }
}

fn update_line(
    to: &mut Option<PathBuf>,
    hunks: &mut Vec<Hunk>,
    (at, body): (usize, &str),
    resolve: &dyn Fn(&str) -> PathBuf,
) -> Result<(), PatchError> {
    if let Some(path) = body.strip_prefix("*** Move to: ") {
        *to = Some(resolve(path.trim()));
        return Ok(());
    }
    if body.starts_with("*** End of File") {
        return Ok(());
    }
    if let Some(header) = body.strip_prefix("@@") {
        let header = header.trim();
        hunks.push(Hunk {
            header: (!header.is_empty()).then(|| header.to_string()),
            ..Hunk::default()
        });
        return Ok(());
    }
    if hunks.is_empty() {
        hunks.push(Hunk::default());
    }
    let hunk = hunks.last_mut().ok_or_else(|| error(at, "no hunk"))?;
    match body.chars().next() {
        Some('+') => hunk.new.push(body[1..].to_string()),
        Some('-') => hunk.old.push(body[1..].to_string()),
        Some(' ') => {
            hunk.old.push(body[1..].to_string());
            hunk.new.push(body[1..].to_string());
        }
        None => {
            hunk.old.push(String::new());
            hunk.new.push(String::new());
        }
        Some(_) => {
            return Err(error(
                at,
                &format!("hunk line must start with ` `, `+`, or `-`: `{body}`"),
            ));
        }
    }
    Ok(())
}

fn to_ops(block: Block) -> Vec<Op> {
    match block {
        Block::Add { path, lines } => vec![Op::Create {
            path,
            text: joined(&lines),
            overwrite: false,
            expect_hash: None,
        }],
        Block::Delete { path } => vec![Op::Delete {
            path,
            expect_hash: None,
        }],
        Block::Update { path, to, hunks } => {
            let mut ops: Vec<Op> = hunks
                .into_iter()
                .filter(|hunk| hunk.old != hunk.new)
                .map(|hunk| hunk_op(&path, hunk))
                .collect();
            if let Some(to) = to {
                ops.push(Op::Move {
                    path,
                    to,
                    expect_hash: None,
                });
            }
            ops
        }
    }
}

fn hunk_op(path: &Path, hunk: Hunk) -> Op {
    if hunk.old.is_empty() {
        // Pure additions: after the header line, or at the end of the file.
        let (anchor, text) = match hunk.header {
            Some(header) => (header, format!("\n{}", hunk.new.join("\n"))),
            None => (String::new(), joined(&hunk.new)),
        };
        return Op::Insert {
            path: path.to_path_buf(),
            anchor: Anchor {
                text: anchor,
                after: None,
                whole_lines: true,
            },
            after: true,
            text,
            expect_hash: None,
        };
    }
    Op::Replace {
        path: path.to_path_buf(),
        find: Some(Anchor {
            text: hunk.old.join("\n"),
            after: hunk.header,
            whole_lines: true,
        }),
        range: None,
        pattern: None,
        replace: hunk.new.join("\n"),
        occurrence: Occurrence::Unique,
        target: None,
        expect_hash: None,
    }
}

fn joined(lines: &[String]) -> String {
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

fn error(line: usize, message: &str) -> PatchError {
    PatchError {
        line: line + 1,
        message: message.to_string(),
    }
}
