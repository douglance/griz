//! Re-indents replacement text found by the indentation rung.

/// Moves every line of `text` from the anchor's indentation `from` to the
/// matched text's indentation `to`, leaving blank lines empty.
#[must_use]
pub fn reindent(text: &str, from: &str, to: &str) -> String {
    text.split('\n')
        .map(|line| {
            if line.trim().is_empty() {
                String::new()
            } else {
                format!(
                    "{to}{}",
                    line.strip_prefix(from).unwrap_or(line.trim_start())
                )
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
