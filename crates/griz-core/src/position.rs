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

/// Converts a batch in coordinate order, then returns offsets in caller order.
pub fn to_bytes(
    text: &str,
    positions: &[Position],
    encoding: PositionEncoding,
) -> Vec<Option<usize>> {
    let mut order: Vec<_> = (0..positions.len()).collect();
    order.sort_unstable_by_key(|&index| (positions[index].line, positions[index].character));
    let mut offsets = vec![None; positions.len()];
    let mut cursor = Cursor::new(text);
    for index in order {
        offsets[index] = cursor.at(positions[index], encoding);
    }
    offsets
}

struct Cursor<'a> {
    remaining: std::str::Split<'a, char>,
    text: &'a str,
    line: usize,
    start: usize,
    byte: usize,
    units: usize,
}

impl<'a> Cursor<'a> {
    fn new(text: &'a str) -> Self {
        let mut remaining = text.split('\n');
        let text = remaining.next().unwrap_or_default();
        Self {
            remaining,
            text,
            line: 0,
            start: 0,
            byte: 0,
            units: 0,
        }
    }

    fn advance_line(&mut self, line: u32) -> Option<()> {
        while self.line < line as usize {
            let next = self.remaining.next()?;
            self.start += self.text.len() + 1;
            self.text = next;
            self.line += 1;
            self.byte = 0;
            self.units = 0;
        }
        Some(())
    }

    fn at(&mut self, position: Position, encoding: PositionEncoding) -> Option<usize> {
        self.advance_line(position.line)?;
        let target = position.character as usize;
        if encoding == PositionEncoding::Utf8 {
            return self
                .start
                .checked_add(target)
                .filter(|_| target <= self.text.len());
        }
        while self.units < target {
            let ch = self.text[self.byte..].chars().next()?;
            self.units += character_units(ch, encoding);
            self.byte += ch.len_utf8();
        }
        (self.units == target).then_some(self.start + self.byte)
    }
}

fn character_units(ch: char, encoding: PositionEncoding) -> usize {
    match encoding {
        PositionEncoding::Utf16 => ch.len_utf16(),
        PositionEncoding::Utf8 | PositionEncoding::Utf32 => 1,
    }
}
