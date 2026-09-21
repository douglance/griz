//! Maps a `file://` URI to a filesystem path.

use std::path::PathBuf;

/// Decodes `uri` as a `file://` URI into a path.
///
/// # Errors
/// Returns `uri` itself when it is not a `file://` URI, or does not
/// percent-decode as valid UTF-8.
pub fn to_path(uri: &str) -> Result<PathBuf, String> {
    let rest = uri.strip_prefix("file://").ok_or_else(|| uri.to_string())?;
    percent_decode(rest)
        .map(PathBuf::from)
        .ok_or_else(|| uri.to_string())
}

fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'%' if at + 3 <= bytes.len() => {
                let hex = std::str::from_utf8(&bytes[at + 1..at + 3]).ok()?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                at += 3;
            }
            byte => {
                out.push(byte);
                at += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}
