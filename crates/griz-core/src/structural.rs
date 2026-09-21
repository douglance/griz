//! Structural matching: code shapes with metavariables, through ast-grep.
//!
//! A pattern such as `foo($A, $$$REST)` matches by syntax tree, so formatting
//! and comments inside the match do not matter. `$NAME` captures one node and
//! `$$$NAME` captures a run of nodes.

use crate::find::Hit;
use ast_grep_core::{Language, NodeMatch, Pattern, meta_var::MetaVariable, tree_sitter::StrDoc};
use ast_grep_language::{LanguageExt, SupportLang};
use std::{collections::BTreeMap, path::Path, str::FromStr};

/// A structural pattern and, optionally, the one language to search.
#[derive(Debug, Clone)]
pub struct Shape {
    pattern: String,
    language: Option<SupportLang>,
}

impl Shape {
    /// Builds a shape. Without a language, each file's extension chooses one.
    ///
    /// # Errors
    /// Returns a message for an empty pattern or an unsupported language.
    pub fn new(pattern: &str, language: Option<&str>) -> Result<Self, String> {
        if pattern.trim().is_empty() {
            return Err("the pattern is empty".to_string());
        }
        let language = language.map(parse_language).transpose()?;
        Ok(Self {
            pattern: pattern.to_string(),
            language,
        })
    }

    /// Every match in `text`, or none when the file's language is unknown.
    #[must_use]
    pub fn hits(&self, path: &Path, text: &str) -> Vec<Hit> {
        let Some(language) = self.language.or_else(|| SupportLang::from_path(path)) else {
            return Vec::new();
        };
        let Ok(pattern) = Pattern::try_new(&self.pattern, language) else {
            return Vec::new();
        };
        language
            .ast_grep(text)
            .root()
            .find_all(&pattern)
            .map(|found| Hit {
                range: found.range(),
                captures: Vec::new(),
                vars: vars(&found, text),
            })
            .collect()
    }
}

fn parse_language(name: &str) -> Result<SupportLang, String> {
    SupportLang::from_str(name).map_err(|_| {
        format!(
            "unsupported language `{name}`; use rust, typescript, tsx, javascript, python, go, or swift"
        )
    })
}

/// The text each metavariable captured. A run of nodes spans from its first to
/// its last named node, so separators such as a trailing comma stay out.
fn vars(found: &NodeMatch<'_, StrDoc<SupportLang>>, text: &str) -> BTreeMap<String, String> {
    let env = found.get_env();
    env.get_matched_variables()
        .filter_map(|var| match var {
            MetaVariable::Capture(name, _) => {
                let node = env.get_match(&name)?;
                Some((name, node.text().to_string()))
            }
            MetaVariable::MultiCapture(name) => {
                let nodes = env.get_multiple_matches(&name);
                let named: Vec<_> = nodes.iter().filter(|node| node.is_named()).collect();
                let (first, last) = (named.first()?, named.last()?);
                Some((
                    name,
                    text[first.range().start..last.range().end].to_string(),
                ))
            }
            _ => None,
        })
        .collect()
}
