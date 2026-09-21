use crate::{Violation, relative_path};
use anyhow::{Context, Result};
use std::{fs, path::Path};

const TEST_ONLY_LINTS: &[&str] = &[
    "clippy::expect_used",
    "clippy::panic",
    "clippy::unwrap_used",
    "clippy::unwrap_in_result",
];

pub fn check_lint_suppressions(
    root: &Path,
    rust_files: &[std::path::PathBuf],
) -> Result<Vec<Violation>> {
    let mut violations = Vec::new();
    for path in rust_files {
        let source = fs::read_to_string(path)
            .with_context(|| format!("read Rust file {}", path.display()))?;
        violations.extend(
            lint_attrs(&source)
                .into_iter()
                .filter_map(|attr| lint_violation(root, path, &attr)),
        );
    }
    Ok(violations)
}

fn lint_violation(root: &Path, path: &Path, attr: &LintAttr) -> Option<Violation> {
    let relative = relative_path(root, path);
    (!attr.is_allowed(&relative)).then(|| {
        Violation::new(
            relative,
            attr.line,
            "lint suppression must be narrow and test-only",
        )
    })
}

#[derive(Debug)]
struct LintAttr {
    line: usize,
    text: String,
    test_context: bool,
}

impl LintAttr {
    fn is_allowed(&self, path: &Path) -> bool {
        let names = lint_names(&self.text);
        let narrow_test_lints = !names.is_empty()
            && names
                .iter()
                .all(|name| TEST_ONLY_LINTS.contains(&name.as_str()));
        if self.text.contains("cfg_attr(test") {
            return narrow_test_lints;
        }
        (self.test_context || path.starts_with("tests")) && narrow_test_lints
    }
}

fn lint_attrs(source: &str) -> Vec<LintAttr> {
    let lines: Vec<_> = source.lines().collect();
    let mut attrs = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if !(trimmed.starts_with("#[") || trimmed.starts_with("#![")) {
            continue;
        }
        if !(trimmed.contains("allow(") || trimmed.contains("cfg_attr(")) {
            continue;
        }
        let start = index.saturating_sub(3);
        let test_context = lines[start..=index]
            .iter()
            .any(|line| line.contains("cfg(test)"));
        attrs.push(LintAttr {
            line: index + 1,
            text: line.trim().to_owned(),
            test_context,
        });
    }
    attrs
}

fn lint_names(attr: &str) -> Vec<String> {
    let Some(start) = attr.find("allow(") else {
        return Vec::new();
    };
    let body = &attr[start + "allow(".len()..];
    let Some(end) = body.find(')') else {
        return Vec::new();
    };
    body[..end]
        .split(',')
        .map(|name| name.trim().replace(' ', ""))
        .filter(|name| !name.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cfg_attr_test_unwrap_is_narrow() {
        let attr = LintAttr {
            line: 1,
            text: "#[cfg_attr(test, allow(clippy::unwrap_used))]".to_owned(),
            test_context: false,
        };
        assert!(attr.is_allowed(Path::new("src/lib.rs")));
    }

    #[test]
    fn warnings_allow_is_broad() {
        let attr = LintAttr {
            line: 1,
            text: "#![allow(warnings)]".to_owned(),
            test_context: true,
        };
        assert!(!attr.is_allowed(Path::new("tests/policy.rs")));
    }
}
