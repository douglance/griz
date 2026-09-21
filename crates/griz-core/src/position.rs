//! Converts an LSP position (0-based line and character) into a byte
//! offset, honoring the encoding `character` counts in.

use schemars::JsonSchema;
use serde::Deserialize;

/// A 0-based line and character inside a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
pub struct Position {
    /// 0-based line.
    pub line: u32,
    /// 0-based offset into the line, in units of the chosen encoding.
    pub character: u32,
}

/// How a position's `character` counts into a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PositionEncoding {
    /// `character` counts UTF-8 bytes.
    Utf8,
    /// `character` counts UTF-16 code units. The LSP default.
    #[default]
    Utf16,
    /// `character` counts Unicode scalar values.
    Utf32,
}

impl PositionEncoding {
    /// Parses `utf-8`, `utf-16`, or `utf-32`.
    ///
    /// # Errors
    /// Returns a message naming the accepted values.
    pub fn parse(text: &str) -> Result<Self, String> {
        match text {
            "utf-8" => Ok(Self::Utf8),
            "utf-16" => Ok(Self::Utf16),
            "utf-32" => Ok(Self::Utf32),
            other => Err(format!(
                "unknown position_encoding `{other}`; use utf-8, utf-16, or utf-32"
            )),
        }
    }
}

/// Converts `position` in `text` to a byte offset, or `None` when its line
/// or character falls outside the text.
#[must_use]
pub fn to_byte(text: &str, position: Position, encoding: PositionEncoding) -> Option<usize> {
    let line_start = line_start_byte(text, position.line)?;
    let line_end = text[line_start..]
        .find('\n')
        .map_or(text.len(), |at| line_start + at);
    let line = &text[line_start..line_end];
    character_to_byte(line, position.character, encoding).map(|at| line_start + at)
}

fn line_start_byte(text: &str, line: u32) -> Option<usize> {
    if line == 0 {
        return Some(0);
    }
    let mut seen = 0u32;
    for (at, byte) in text.bytes().enumerate() {
        if byte != b'\n' {
            continue;
        }
        seen += 1;
        if seen == line {
            return Some(at + 1);
        }
    }
    None
}

fn character_to_byte(line: &str, character: u32, encoding: PositionEncoding) -> Option<usize> {
    match encoding {
        PositionEncoding::Utf8 => utf8_char_byte(line, character),
        PositionEncoding::Utf32 => nth_char_byte(line, character),
        PositionEncoding::Utf16 => utf16_char_byte(line, character),
    }
}

fn utf8_char_byte(line: &str, character: u32) -> Option<usize> {
    let at = character as usize;
    (at <= line.len()).then_some(at)
}

fn nth_char_byte(line: &str, character: u32) -> Option<usize> {
    if character as usize == line.chars().count() {
        return Some(line.len());
    }
    line.char_indices()
        .nth(character as usize)
        .map(|(at, _)| at)
}

fn utf16_char_byte(line: &str, character: u32) -> Option<usize> {
    let mut units = 0u32;
    for (at, ch) in line.char_indices() {
        if units == character {
            return Some(at);
        }
        units += u32::try_from(ch.len_utf16()).unwrap_or(1);
    }
    (units == character).then_some(line.len())
}
