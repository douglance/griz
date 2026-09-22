//! Named-definition diff: which functions, types, and other declarations a
//! before/after pair added, removed, or changed, per enabled language.
//!
//! Fact only. A definition present in both with the same body is left out;
//! a rename shows up as one removed and one added.

mod compare;
mod identity;

use ast_grep_language::SupportLang;
pub use compare::diff_items;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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
    /// Enclosing named scopes, outermost first; omitted at file scope.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scope: Vec<String>,
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
