//! Structural matching: code shapes with metavariables, through ast-grep.
//!
//! A pattern such as `foo($A, $$$REST)` matches by syntax tree, so formatting
//! and comments inside the match do not matter. `$NAME` captures one node and
//! `$$$NAME` captures a run of nodes.

use crate::find::Hit;
use ast_grep_core::{Language, NodeMatch, Pattern, meta_var::MetaVariable, tree_sitter::StrDoc};
use ast_grep_language::{LanguageExt, SupportLang};
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap},
    ops::Range,
    path::Path,
    str::FromStr,
};

/// The languages whose tree-sitter parser is linked into this build, paired
/// with the name a caller passes as `language`. This is the single source of
/// truth for what `find` will parse: an explicit name outside this list is
/// refused, and a file whose extension resolves outside it is skipped.
const ENABLED_LANGUAGES: &[(SupportLang, &str)] = &[
    (SupportLang::Rust, "rust"),
    (SupportLang::TypeScript, "typescript"),
    (SupportLang::Tsx, "tsx"),
    (SupportLang::JavaScript, "javascript"),
    (SupportLang::Python, "python"),
    (SupportLang::Go, "go"),
    (SupportLang::Swift, "swift"),
];

/// A structural pattern and, optionally, the one language to search.
pub struct Shape {
    pattern: String,
    language: Option<SupportLang>,
    compiled: RefCell<HashMap<SupportLang, Pattern>>,
}

impl Shape {
    /// Builds a shape. Without a language, each file's extension chooses one.
    ///
    /// # Errors
    /// Returns a message for an empty pattern, an unsupported language, or a
    /// pattern that fails to compile for a given language.
    pub fn new(pattern: &str, language: Option<&str>) -> Result<Self, String> {
        if pattern.trim().is_empty() {
            return Err("the pattern is empty".to_string());
        }
        let language = language.map(parse_language).transpose()?;
        let shape = Self {
            pattern: pattern.to_string(),
            language,
            compiled: RefCell::new(HashMap::new()),
        };
        if let Some(explicit) = language {
            shape.compiled_pattern(explicit)?;
        }
        Ok(shape)
    }

    /// Every match in `text`, or none when the file's language is unknown,
    /// disabled, or (with an explicit language) not this file's own.
    ///
    /// # Errors
    /// Returns a message naming the pattern when it fails to compile for the
    /// file's language.
    pub fn hits(&self, path: &Path, text: &str) -> Result<Vec<Hit>, String> {
        let Some(language) = self.resolve(path) else {
            return Ok(Vec::new());
        };
        let pattern = self.compiled_pattern(language)?;
        Ok(language
            .ast_grep(text)
            .root()
            .find_all(&pattern)
            .map(|found| hit(&found, text))
            .collect())
    }

    /// The language to search `path` with, honoring an explicit override and
    /// otherwise the file's own extension, both narrowed to the enabled set.
    fn resolve(&self, path: &Path) -> Option<SupportLang> {
        let from_extension = SupportLang::from_path(path).filter(|lang| is_enabled(*lang));
        match self.language {
            Some(explicit) => from_extension.filter(|lang| *lang == explicit),
            None => from_extension,
        }
    }

    /// The pattern compiled for `language`, from cache once compiled once.
    fn compiled_pattern(&self, language: SupportLang) -> Result<Pattern, String> {
        if let Some(pattern) = self.compiled.borrow().get(&language) {
            return Ok(pattern.clone());
        }
        let pattern = Pattern::try_new(&self.pattern, language)
            .map_err(|error| format!("bad pattern `{}`: {error}", self.pattern))?;
        if pattern.has_error() {
            return Err(format!(
                "bad pattern `{}`: contains a syntax error",
                self.pattern
            ));
        }
        self.compiled.borrow_mut().insert(language, pattern.clone());
        Ok(pattern)
    }
}

fn is_enabled(language: SupportLang) -> bool {
    ENABLED_LANGUAGES.iter().any(|(lang, _)| *lang == language)
}

fn parse_language(name: &str) -> Result<SupportLang, String> {
    match SupportLang::from_str(name) {
        Ok(language) if is_enabled(language) => Ok(language),
        _ => Err(format!(
            "unsupported language `{name}`; use {}",
            enabled_names()
        )),
    }
}

fn enabled_names() -> String {
    let names: Vec<&str> = ENABLED_LANGUAGES.iter().map(|(_, name)| *name).collect();
    match names.split_last() {
        Some((last, rest)) => format!("{}, or {last}", rest.join(", ")),
        None => String::new(),
    }
}

/// One metavariable's captured text and byte range.
struct Capture {
    text: String,
    range: Range<usize>,
}

fn hit(found: &NodeMatch<'_, StrDoc<SupportLang>>, text: &str) -> Hit {
    let captures = captures(found, text);
    Hit {
        range: found.range(),
        captures: Vec::new(),
        vars: captures
            .iter()
            .map(|(name, capture)| (name.clone(), capture.text.clone()))
            .collect(),
        var_ranges: captures
            .into_iter()
            .map(|(name, capture)| (name, capture.range))
            .collect(),
    }
}

/// Each metavariable's captured text and range. A run of nodes spans from
/// its first to its last named node, so separators such as a trailing comma
/// stay out.
fn captures(found: &NodeMatch<'_, StrDoc<SupportLang>>, text: &str) -> BTreeMap<String, Capture> {
    let env = found.get_env();
    env.get_matched_variables()
        .filter_map(|var| match var {
            MetaVariable::Capture(name, _) => {
                let node = env.get_match(&name)?;
                let range = node.range();
                Some((
                    name,
                    Capture {
                        text: node.text().to_string(),
                        range,
                    },
                ))
            }
            MetaVariable::MultiCapture(name) => {
                let nodes = env.get_multiple_matches(&name);
                let named: Vec<_> = nodes.iter().filter(|node| node.is_named()).collect();
                let (first, last) = (named.first()?, named.last()?);
                let range = first.range().start..last.range().end;
                let captured = text[range.clone()].to_string();
                Some((
                    name,
                    Capture {
                        text: captured,
                        range,
                    },
                ))
            }
            _ => None,
        })
        .collect()
}
