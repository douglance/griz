//! Named-definition diff: which functions, types, and other declarations a
//! before/after pair added, removed, or changed, per enabled language.
//!
//! Fact only. A definition present in both with the same body is left out;
//! a rename shows up as one removed and one added.

use crate::structural;
use ast_grep_language::{LanguageExt, SupportLang};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

/// What happened to one named definition between before and after.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ItemChange {
    /// Present after, not before.
    Added,
    /// Present before, not after.
    Removed,
    /// Present in both, with a different body.
    Changed,
}

/// One named definition a diff added, removed, or changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DiffItem {
    /// Kind of definition: function, method, struct, class, enum, trait,
    /// `type_alias`, or const.
    pub kind: String,
    /// The definition's name.
    pub name: String,
    /// What happened to it.
    pub change: ItemChange,
}

struct DefKind {
    node_kind: &'static str,
    label: &'static str,
    name_field: &'static str,
}

const RUST: &[DefKind] = &[
    DefKind {
        node_kind: "function_item",
        label: "function",
        name_field: "name",
    },
    DefKind {
        node_kind: "struct_item",
        label: "struct",
        name_field: "name",
    },
    DefKind {
        node_kind: "enum_item",
        label: "enum",
        name_field: "name",
    },
    DefKind {
        node_kind: "trait_item",
        label: "trait",
        name_field: "name",
    },
    DefKind {
        node_kind: "type_item",
        label: "type_alias",
        name_field: "name",
    },
    DefKind {
        node_kind: "const_item",
        label: "const",
        name_field: "name",
    },
];

const TS: &[DefKind] = &[
    DefKind {
        node_kind: "function_declaration",
        label: "function",
        name_field: "name",
    },
    DefKind {
        node_kind: "method_definition",
        label: "method",
        name_field: "name",
    },
    DefKind {
        node_kind: "class_declaration",
        label: "class",
        name_field: "name",
    },
    DefKind {
        node_kind: "interface_declaration",
        label: "trait",
        name_field: "name",
    },
    DefKind {
        node_kind: "type_alias_declaration",
        label: "type_alias",
        name_field: "name",
    },
    DefKind {
        node_kind: "enum_declaration",
        label: "enum",
        name_field: "name",
    },
];

const JS: &[DefKind] = &[
    DefKind {
        node_kind: "function_declaration",
        label: "function",
        name_field: "name",
    },
    DefKind {
        node_kind: "method_definition",
        label: "method",
        name_field: "name",
    },
    DefKind {
        node_kind: "class_declaration",
        label: "class",
        name_field: "name",
    },
];

const PYTHON: &[DefKind] = &[
    DefKind {
        node_kind: "function_definition",
        label: "function",
        name_field: "name",
    },
    DefKind {
        node_kind: "class_definition",
        label: "class",
        name_field: "name",
    },
];

const GO: &[DefKind] = &[
    DefKind {
        node_kind: "function_declaration",
        label: "function",
        name_field: "name",
    },
    DefKind {
        node_kind: "method_declaration",
        label: "method",
        name_field: "name",
    },
    DefKind {
        node_kind: "type_spec",
        label: "type_alias",
        name_field: "name",
    },
];

const SWIFT: &[DefKind] = &[
    DefKind {
        node_kind: "function_declaration",
        label: "function",
        name_field: "name",
    },
    DefKind {
        node_kind: "class_declaration",
        label: "class",
        name_field: "name",
    },
    DefKind {
        node_kind: "protocol_declaration",
        label: "trait",
        name_field: "name",
    },
    DefKind {
        node_kind: "enum_declaration",
        label: "enum",
        name_field: "name",
    },
];

fn definitions_for(language: SupportLang) -> &'static [DefKind] {
    match language {
        SupportLang::Rust => RUST,
        SupportLang::TypeScript | SupportLang::Tsx => TS,
        SupportLang::JavaScript => JS,
        SupportLang::Python => PYTHON,
        SupportLang::Go => GO,
        SupportLang::Swift => SWIFT,
        _ => &[],
    }
}

/// Named definitions `before` and `after` disagree on, for `path`'s
/// language. Empty for a disabled language or with nothing named.
#[must_use]
pub fn diff_items(path: &Path, before: Option<&str>, after: Option<&str>) -> Vec<DiffItem> {
    let Some(language) = structural::enabled_language(path) else {
        return Vec::new();
    };
    let before_items = before
        .map(|text| collect(language, text))
        .unwrap_or_default();
    let after_items = after
        .map(|text| collect(language, text))
        .unwrap_or_default();
    let mut items: Vec<DiffItem> = before_items
        .iter()
        .filter_map(|(key, body)| changed_or_removed(key, body, &after_items))
        .chain(
            after_items
                .keys()
                .filter(|key| !before_items.contains_key(*key))
                .map(|key| item(key, ItemChange::Added)),
        )
        .collect();
    items.sort_by(|a, b| (&a.kind, &a.name).cmp(&(&b.kind, &b.name)));
    items
}

fn changed_or_removed(
    key: &(String, String),
    body: &str,
    after_items: &BTreeMap<(String, String), String>,
) -> Option<DiffItem> {
    match after_items.get(key) {
        None => Some(item(key, ItemChange::Removed)),
        Some(after_body) if after_body != body => Some(item(key, ItemChange::Changed)),
        Some(_) => None,
    }
}

fn item(key: &(String, String), change: ItemChange) -> DiffItem {
    DiffItem {
        kind: key.0.clone(),
        name: key.1.clone(),
        change,
    }
}

fn collect(language: SupportLang, text: &str) -> BTreeMap<(String, String), String> {
    let table = definitions_for(language);
    let tree = language.ast_grep(text);
    tree.root()
        .dfs()
        .filter_map(|node| {
            let def = table
                .iter()
                .find(|def| def.node_kind == node.kind().as_ref())?;
            let name = node.field(def.name_field)?.text().to_string();
            Some(((def.label.to_string(), name), node.text().to_string()))
        })
        .collect()
}
