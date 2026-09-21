use crate::{MAX_BLOCK_LINES, Violation, files::code_line_count, relative_path};
use anyhow::{Context, Result};
use proc_macro2::Span;
use std::{fs, path::Path};
use syn::{spanned::Spanned, visit::Visit};

pub fn check_blocks(root: &Path, rust_files: &[std::path::PathBuf]) -> Result<Vec<Violation>> {
    let mut violations = Vec::new();
    for path in rust_files {
        let source = fs::read_to_string(path)
            .with_context(|| format!("read Rust file {}", path.display()))?;
        let parsed = syn::parse_file(&source)
            .with_context(|| format!("parse Rust file {}", path.display()))?;
        let mut visitor = BlockVisitor {
            root,
            path,
            source: &source,
            violations: Vec::new(),
        };
        visitor.visit_file(&parsed);
        violations.extend(visitor.violations);
    }
    Ok(violations)
}

struct BlockVisitor<'a> {
    root: &'a Path,
    path: &'a Path,
    source: &'a str,
    violations: Vec<Violation>,
}

impl BlockVisitor<'_> {
    fn check_span(&mut self, label: &str, span: Span) {
        let start = span.start().line;
        let end = span.end().line;
        let lines = code_line_count(self.source, start, end);
        if lines > MAX_BLOCK_LINES {
            self.violations.push(Violation::new(
                relative_path(self.root, self.path),
                start,
                format!("{label} has {lines} code lines; limit is {MAX_BLOCK_LINES}"),
            ));
        }
    }
}

impl<'ast> Visit<'ast> for BlockVisitor<'_> {
    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        self.check_span(&format!("function `{}`", node.sig.ident), node.span());
        syn::visit::visit_item_fn(self, node);
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        self.check_span(&format!("method `{}`", node.sig.ident), node.span());
        syn::visit::visit_impl_item_fn(self, node);
    }

    fn visit_trait_item_fn(&mut self, node: &'ast syn::TraitItemFn) {
        if node.default.is_some() {
            self.check_span(&format!("trait method `{}`", node.sig.ident), node.span());
        }
        syn::visit::visit_trait_item_fn(self, node);
    }

    fn visit_expr_closure(&mut self, node: &'ast syn::ExprClosure) {
        let start = node.span().start().line;
        let end = node.span().end().line;
        if end > start {
            self.check_span("closure", node.span());
        }
        syn::visit::visit_expr_closure(self, node);
    }
}
