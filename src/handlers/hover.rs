use tower_lsp::lsp_types::{Hover, HoverContents, MarkupContent, MarkupKind, Position, SymbolKind, Url};
use tree_sitter::{Node, Tree};

use crate::parsing::haddock::{extract_haddock, haddock_to_markdown};
use crate::parsing::haskell::{
    node_at_position, node_to_range, CONSTRUCTOR, DEFINITION_KINDS, IDENTIFIER_KINDS, VARIABLE,
};
use crate::symbols::extract::{find_type_signature, haskell_kind_for_node};
use crate::symbols::index::WorkspaceIndex;

pub fn hover(
    tree: &Tree,
    source: &str,
    _uri: &Url,
    pos: Position,
    index: &WorkspaceIndex,
) -> Option<Hover> {
    let root = tree.root_node();
    let node = node_at_position(root, source, pos)?;

    // Must be on an identifier
    if !IDENTIFIER_KINDS.contains(&node.kind()) {
        return None;
    }
    let name = node.utf8_text(source.as_bytes()).ok()?;
    if name.is_empty() {
        return None;
    }

    // 1. Same-file scope walk
    if let Some(h) = resolve_hover_in_file(node, source, name, index) {
        return Some(h);
    }

    // 2. Cross-file index lookup
    let matches = index.lookup_by_name(name);
    if matches.is_empty() {
        return None;
    }

    let info = &matches[0];
    let markdown = format_hover(
        name,
        info.kind,
        info.type_signature.as_deref(),
        info.doc_comment.as_deref(),
        index,
    );
    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: markdown,
        }),
        range: None,
    })
}

fn resolve_hover_in_file(
    start: Node<'_>,
    source: &str,
    name: &str,
    index: &WorkspaceIndex,
) -> Option<Hover> {
    let mut current = start.parent()?;
    loop {
        if let Some(h) = find_hover_in_scope(current, source, name, index) {
            return Some(h);
        }
        current = current.parent()?;
    }
}

fn find_hover_in_scope(
    scope: Node<'_>,
    source: &str,
    name: &str,
    index: &WorkspaceIndex,
) -> Option<Hover> {
    let mut cursor = scope.walk();
    for child in scope.children(&mut cursor) {
        if !DEFINITION_KINDS.contains(&child.kind()) {
            continue;
        }

        let Some(def_name) = get_name(child, source) else { continue; };
        if def_name != name {
            continue;
        }

        let sym_kind = haskell_kind_for_node(child.kind()).unwrap_or(SymbolKind::VARIABLE);
        let doc = extract_haddock(child, source);
        let sig = find_type_signature(child, source, name);

        let markdown = format_hover(name, sym_kind, sig.as_deref(), doc.as_deref(), index);
        return Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: markdown,
            }),
            range: Some(node_to_range(child)),
        });
    }
    None
}

fn get_name<'a>(node: Node<'a>, source: &'a str) -> Option<&'a str> {
    if let Some(n) = node.child_by_field_name("name") {
        return n.utf8_text(source.as_bytes()).ok();
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == VARIABLE || child.kind() == CONSTRUCTOR {
            return child.utf8_text(source.as_bytes()).ok();
        }
    }
    None
}

/// Build the Markdown hover string.
///
/// Format:
/// ```haskell
/// map :: (a -> b) -> [a] -> [b]
/// ```
///
/// ---
///
/// <Haddock rendered as Markdown>
fn format_hover(
    name: &str,
    kind: SymbolKind,
    sig: Option<&str>,
    doc: Option<&str>,
    index: &WorkspaceIndex,
) -> String {
    let code_block = if let Some(s) = sig {
        format!("```haskell\n{s}\n```")
    } else {
        let kind_label = symbol_kind_label(kind);
        if kind_label.is_empty() {
            format!("```haskell\n{name}\n```")
        } else {
            format!("```haskell\n{kind_label} {name}\n```")
        }
    };

    if let Some(d) = doc {
        let rendered = haddock_to_markdown(d, index);
        format!("{code_block}\n\n---\n\n{rendered}")
    } else {
        code_block
    }
}

fn symbol_kind_label(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::STRUCT       => "data",
        SymbolKind::INTERFACE    => "class",
        SymbolKind::TYPE_PARAMETER => "type",
        SymbolKind::OBJECT       => "instance",
        SymbolKind::ENUM_MEMBER  => "pattern",
        SymbolKind::FUNCTION     => "",
        _                        => "",
    }
}
