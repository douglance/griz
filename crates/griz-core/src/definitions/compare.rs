//! Compare scoped definitions without dropping duplicate names.

use super::{
    DiffItem, ItemChange, definitions_for,
    identity::{Identity, scope},
};
use crate::structural;
use ast_grep_language::{LanguageExt, SupportLang};
use std::{collections::BTreeMap, path::Path};

type Definitions<'text> = BTreeMap<Identity, Vec<&'text str>>;

/// Named definitions `before` and `after` disagree on, for `path`'s
/// language. Empty for a disabled language or with nothing named.
#[must_use]
pub fn diff_items(path: &Path, before: Option<&str>, after: Option<&str>) -> Vec<DiffItem> {
    let Some(language) = structural::enabled_language(path) else {
        return Vec::new();
    };
    let before = before
        .map(|text| collect(language, text))
        .unwrap_or_default();
    let after = after
        .map(|text| collect(language, text))
        .unwrap_or_default();
    let mut items = Vec::new();
    for (key, bodies) in &before {
        changes(
            key,
            bodies,
            after.get(key).map_or(&[], Vec::as_slice),
            &mut items,
        );
    }
    for (key, bodies) in after.iter().filter(|(key, _)| !before.contains_key(*key)) {
        changes(key, &[], bodies, &mut items);
    }
    items.sort_by(|a, b| (&a.kind, &a.name, &a.scope).cmp(&(&b.kind, &b.name, &b.scope)));
    items
}

fn changes(key: &Identity, before: &[&str], after: &[&str], items: &mut Vec<DiffItem>) {
    let (removed, added) = unmatched(before, after);
    let changed = removed.min(added);
    for (change, count) in [
        (ItemChange::Changed, changed),
        (ItemChange::Removed, removed - changed),
        (ItemChange::Added, added - changed),
    ] {
        items.extend((0..count).map(|_| item(key, change)));
    }
}

fn unmatched(before: &[&str], after: &[&str]) -> (usize, usize) {
    if before == after {
        return (0, 0);
    }
    if before.is_empty() || after.is_empty() || (before.len() == 1 && after.len() == 1) {
        return (before.len(), after.len());
    }
    let mut counts = BTreeMap::<&str, (usize, usize)>::new();
    for body in before {
        counts.entry(body).or_default().0 += 1;
    }
    for body in after {
        counts.entry(body).or_default().1 += 1;
    }
    counts
        .values()
        .fold((0, 0), |(removed, added), (old, new)| {
            (
                removed + old.saturating_sub(*new),
                added + new.saturating_sub(*old),
            )
        })
}

fn item(key: &Identity, change: ItemChange) -> DiffItem {
    DiffItem {
        kind: key.kind.to_string(),
        name: key.name.clone(),
        scope: key.scope.clone(),
        change,
    }
}

fn collect(language: SupportLang, text: &str) -> Definitions<'_> {
    let table = definitions_for(language);
    let tree = language.ast_grep(text);
    let mut definitions = Definitions::new();
    for node in tree.root().dfs() {
        let Some(def) = table
            .iter()
            .find(|def| def.node_kind == node.kind().as_ref())
        else {
            continue;
        };
        let Some(name) = node.field(def.name_field) else {
            continue;
        };
        let identity = Identity {
            kind: def.label,
            name: name.text().into_owned(),
            scope: scope(&node),
        };
        definitions
            .entry(identity)
            .or_default()
            .push(&text[node.range()]);
    }
    definitions
}
