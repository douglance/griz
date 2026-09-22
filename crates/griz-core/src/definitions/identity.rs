//! Definition identity includes named owners and a method receiver.

use ast_grep_core::{Node, tree_sitter::StrDoc};
use ast_grep_language::SupportLang;

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Identity {
    pub kind: &'static str,
    pub name: String,
    pub scope: Vec<String>,
}

pub(super) fn scope(node: &Node<'_, StrDoc<SupportLang>>) -> Vec<String> {
    let mut names = Vec::new();
    let mut parent = node.parent();
    while let Some(owner) = parent {
        parent = owner.parent();
        if parent.is_none() {
            break;
        }
        if let Some(name) = owner_name(&owner) {
            names.push(name);
        }
    }
    names.reverse();
    if let Some(receiver) = receiver_type(node) {
        names.push(receiver);
    }
    names
}

fn owner_name(node: &Node<'_, StrDoc<SupportLang>>) -> Option<String> {
    if node.kind() == "impl_item" {
        let target = compact(&node.field("type")?);
        return Some(match node.field("trait") {
            Some(trait_name) => format!("impl {} for {target}", compact(&trait_name)),
            None => format!("impl {target}"),
        });
    }
    node.field("name").map(|name| compact(&name))
}

fn receiver_type(node: &Node<'_, StrDoc<SupportLang>>) -> Option<String> {
    let receiver = node.field("receiver")?;
    receiver
        .dfs()
        .find_map(|part| part.field("type"))
        .map(|kind| compact(&kind))
}
fn compact(node: &Node<'_, StrDoc<SupportLang>>) -> String {
    let mut text = String::new();
    for part in node
        .dfs()
        .filter(|part| part.children().next().is_none() && !in_comment(part))
    {
        text.push_str(&part.text());
    }
    text
}

fn in_comment(node: &Node<'_, StrDoc<SupportLang>>) -> bool {
    node.kind().contains("comment")
        || node
            .ancestors()
            .any(|parent| parent.kind().contains("comment"))
}
