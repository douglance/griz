//! Restricts text matches to syntax nodes of given kinds.
//!
//! `within` narrows literal and regex matches to spans inside comment,
//! string, or named node kinds, per enabled language. The aliases `comment`
//! and `string` resolve through one table per language; any other name is
//! taken as a raw node kind and validated against the language itself.

use ast_grep_core::language::Language;
use ast_grep_language::{LanguageExt, SupportLang};
use std::{
    collections::{BTreeSet, HashSet},
    ops::Range,
};

const COMMENT_ALIASES: &[(SupportLang, &[&str])] = &[
    (SupportLang::Rust, &["line_comment", "block_comment"]),
    (SupportLang::TypeScript, &["comment"]),
    (SupportLang::Tsx, &["comment"]),
    (SupportLang::JavaScript, &["comment"]),
    (SupportLang::Python, &["comment"]),
    (SupportLang::Go, &["comment"]),
    (SupportLang::Swift, &["comment", "multiline_comment"]),
];

const STRING_ALIASES: &[(SupportLang, &[&str])] = &[
    (SupportLang::Rust, &["string_literal", "raw_string_literal"]),
    (SupportLang::TypeScript, &["string", "template_string"]),
    (SupportLang::Tsx, &["string", "template_string"]),
    (SupportLang::JavaScript, &["string", "template_string"]),
    (SupportLang::Python, &["string"]),
    (
        SupportLang::Go,
        &["interpreted_string_literal", "raw_string_literal"],
    ),
    (
        SupportLang::Swift,
        &["line_string_literal", "multi_line_string_literal"],
    ),
];

/// Checks every requested kind resolves for at least one of `languages`.
///
/// # Errors
/// Names the first requested kind unknown to every one of them.
pub fn validate(within: &[String], languages: &HashSet<SupportLang>) -> Result<(), String> {
    for requested in within {
        let known = languages
            .iter()
            .any(|language| resolve_one(*language, requested).is_some());
        if !known {
            return Err(format!("unknown node kind `{requested}`"));
        }
    }
    Ok(())
}

/// Byte ranges of every node in `language`'s parse of `text` whose kind
/// resolves from `within`.
#[must_use]
pub fn spans(language: SupportLang, text: &str, within: &[String]) -> Vec<Range<usize>> {
    let kinds: BTreeSet<String> = within
        .iter()
        .filter_map(|requested| resolve_one(language, requested))
        .flatten()
        .collect();
    if kinds.is_empty() {
        return Vec::new();
    }
    let tree = language.ast_grep(text);
    tree.root()
        .dfs()
        .filter(|node| kinds.contains(node.kind().as_ref()))
        .map(|node| node.range())
        .collect()
}

/// The concrete kinds `requested` names for `language`: the alias table
/// entry, or the name itself when it is a real kind in this language.
fn resolve_one(language: SupportLang, requested: &str) -> Option<Vec<String>> {
    match requested {
        "comment" => lookup(COMMENT_ALIASES, language),
        "string" => lookup(STRING_ALIASES, language),
        raw => (language.kind_to_id(raw) != 0).then(|| vec![raw.to_string()]),
    }
}

fn lookup(table: &[(SupportLang, &[&str])], language: SupportLang) -> Option<Vec<String>> {
    table
        .iter()
        .find(|(lang, _)| *lang == language)
        .map(|(_, kinds)| kinds.iter().map(|kind| (*kind).to_string()).collect())
}
