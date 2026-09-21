use crate::{MAX_FILE_LINES, Package, Violation, relative_path};
use anyhow::{Context, Result};
use std::{
    fs,
    iter::Peekable,
    path::{Path, PathBuf},
    process::Command,
    str::Chars,
};

/// Calls that would let the pure engine write, spawn, or persist. Only the store
/// crate touches disk for writing; the engine reads files it is handed.
const FORBIDDEN_CORE_REFS: &[&str] = &[
    "fs::write",
    "fs::rename",
    "fs::remove",
    "fs::create_dir",
    "file::create",
    "openoptions",
    "std::process",
    "command::new",
    "rusqlite",
];

pub fn discovered_rust_files(root: &Path) -> Result<Vec<PathBuf>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("ls-files")
        .arg("-z")
        .arg("--cached")
        .arg("--others")
        .arg("--exclude-standard")
        .arg("--")
        .arg("*.rs")
        .output()
        .context("run git ls-files for Rust files")?;
    if !output.status.success() {
        anyhow::bail!("git ls-files failed with status {}", output.status);
    }
    Ok(output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
        .map(|entry| root.join(String::from_utf8_lossy(entry).as_ref()))
        .collect())
}

pub fn policy_rust_files(packages: &[Package], rust_files: Vec<PathBuf>) -> Vec<PathBuf> {
    let roots = policy_source_roots(packages);
    rust_files
        .into_iter()
        .map(|path| path.canonicalize().unwrap_or(path))
        .filter(|path| roots.iter().any(|root| path.starts_with(root)))
        .collect()
}

fn policy_source_roots(packages: &[Package]) -> Vec<PathBuf> {
    packages
        .iter()
        .map(|package| package.root.clone())
        .collect()
}

pub fn check_file_lengths(root: &Path, rust_files: &[PathBuf]) -> Result<Vec<Violation>> {
    let mut violations = Vec::new();
    for path in rust_files {
        let source = fs::read_to_string(path)
            .with_context(|| format!("read Rust file {}", path.display()))?;
        let line_count = source.lines().count();
        if line_count > MAX_FILE_LINES {
            violations.push(Violation::new(
                relative_path(root, path),
                0,
                format!("file has {line_count} physical lines; limit is {MAX_FILE_LINES}"),
            ));
        }
    }
    Ok(violations)
}

pub fn check_core_purity(
    root: &Path,
    packages: &[Package],
    rust_files: &[PathBuf],
) -> Result<Vec<Violation>> {
    let core_roots: Vec<_> = packages
        .iter()
        .filter(|package| package.name == "griz-core")
        .map(|package| package.root.join("src"))
        .collect();
    let mut violations = Vec::new();
    for path in rust_files {
        if !core_roots
            .iter()
            .any(|core_root| path.starts_with(core_root))
        {
            continue;
        }
        let source = fs::read_to_string(path)
            .with_context(|| format!("read Rust file {}", path.display()))?;
        violations.extend(
            source
                .lines()
                .enumerate()
                .filter_map(|(index, line)| forbidden_ref(line).map(|term| (index + 1, term)))
                .map(|(line, term)| {
                    Violation::new(
                        relative_path(root, path),
                        line,
                        format!("griz-core must stay pure but mentions `{term}`"),
                    )
                }),
        );
    }
    Ok(violations)
}

fn forbidden_ref(line: &str) -> Option<&'static str> {
    let lower = line.to_lowercase();
    FORBIDDEN_CORE_REFS
        .iter()
        .copied()
        .find(|term| lower.contains(*term))
}

pub fn code_line_count(source: &str, start: usize, end: usize) -> usize {
    let mut in_block = false;
    source
        .lines()
        .enumerate()
        .filter(|(index, line)| {
            let line_number = index + 1;
            (start..=end).contains(&line_number) && !strip_comments(line, &mut in_block).is_empty()
        })
        .count()
}

fn strip_comments(line: &str, in_block: &mut bool) -> String {
    let mut output = String::new();
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if *in_block {
            *in_block = !ends_block_comment(ch, &mut chars);
            continue;
        }
        if ch == '/' && chars.peek() == Some(&'/') {
            break;
        }
        if ch == '/' && chars.peek() == Some(&'*') {
            chars.next();
            *in_block = true;
            continue;
        }
        output.push(ch);
    }
    output.trim().to_owned()
}

fn ends_block_comment(ch: char, chars: &mut Peekable<Chars<'_>>) -> bool {
    let ended = ch == '*' && chars.peek() == Some(&'/');
    if ended {
        chars.next();
    }
    ended
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_lines_ignore_blank_and_comment_only_lines() {
        let source = "fn f() {\n\n// comment\nlet x = 1;\n/* hidden */\nx\n}\n";
        assert_eq!(code_line_count(source, 1, 7), 4);
    }
}
