use std::collections::HashSet;

use tower_lsp::lsp_types::{DocumentSymbol, Range, SymbolKind, Url};
use tree_sitter::{Node, Tree};

use crate::parsing::haddock::extract_haddock;
use crate::parsing::haskell::{
    self, BIND, CLASS, CLASS_DECLARATIONS, COMMENT_KINDS, CONSTRUCTOR, DATA_TYPE,
    FOREIGN_IMPORT, FUNCTION, IDENTIFIER_KINDS, INSTANCE, NEWTYPE,
    PATTERN_SYNONYM, SIGNATURE, TYPE_FAMILY, TYPE_SYNONYM, VARIABLE, WHERE,
};
use crate::symbols::types::SymbolInfo;

// ── Symbol kind mapping ───────────────────────────────────────────────────────

pub fn haskell_kind_for_node(kind: &str) -> Option<SymbolKind> {
    match kind {
        FUNCTION | BIND  => Some(SymbolKind::FUNCTION),
        DATA_TYPE        => Some(SymbolKind::STRUCT),
        NEWTYPE          => Some(SymbolKind::STRUCT),
        CLASS            => Some(SymbolKind::INTERFACE),
        INSTANCE         => Some(SymbolKind::OBJECT),
        TYPE_SYNONYM     => Some(SymbolKind::TYPE_PARAMETER),
        TYPE_FAMILY      => Some(SymbolKind::TYPE_PARAMETER),
        FOREIGN_IMPORT   => Some(SymbolKind::FUNCTION),
        PATTERN_SYNONYM  => Some(SymbolKind::ENUM_MEMBER),
        _                => None,
    }
}

// ── Document symbols (hierarchical) ──────────────────────────────────────────

pub fn document_symbols(tree: &Tree, source: &str) -> Vec<DocumentSymbol> {
    extract_children(tree.root_node(), source)
}

fn extract_children(node: Node<'_>, source: &str) -> Vec<DocumentSymbol> {
    let mut symbols = Vec::new();
    // Track seen function names to deduplicate multi-equation functions
    let mut seen_functions: HashSet<String> = HashSet::new();
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        if let Some(sym) = try_extract_doc_symbol(child, source, &mut seen_functions) {
            symbols.push(sym);
        } else {
            // Descend into non-definition nodes
            let nested = extract_children(child, source);
            symbols.extend(nested);
        }
    }
    symbols
}

fn try_extract_doc_symbol(
    node: Node<'_>,
    source: &str,
    seen: &mut HashSet<String>,
) -> Option<DocumentSymbol> {
    // Skip signatures — they are not separate symbols
    if node.kind() == SIGNATURE {
        return None;
    }

    let sym_kind = haskell_kind_for_node(node.kind())?;
    let name = haskell::node_name(node, source)?;

    // Deduplicate multi-equation function definitions
    if node.kind() == FUNCTION {
        if seen.contains(name) {
            return None;
        }
        seen.insert(name.to_string());
    }

    let range = haskell::node_to_range(node);
    let selection_range = name_selection_range(node, source).unwrap_or(range);
    let children = extract_body_symbols(node, source);

    #[allow(deprecated)]
    Some(DocumentSymbol {
        name: name.to_string(),
        detail: None,
        kind: sym_kind,
        deprecated: None,
        range,
        selection_range,
        children: if children.is_empty() { None } else { Some(children) },
        tags: None,
    })
}

fn extract_body_symbols(node: Node<'_>, source: &str) -> Vec<DocumentSymbol> {
    let mut children = Vec::new();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        // Recurse into class/instance bodies, where clauses, and declarations
        if child.kind() == WHERE || child.kind() == "declarations" || child.kind() == CLASS_DECLARATIONS {
            children.extend(extract_children(child, source));
        }
    }
    children
}

fn name_selection_range(node: Node<'_>, source: &str) -> Option<Range> {
    // Try field "name" first
    if let Some(n) = node.child_by_field_name("name") {
        return Some(haskell::node_to_range(n));
    }
    // Try first variable/constructor child
    let target_kinds = [VARIABLE, CONSTRUCTOR];
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if target_kinds.contains(&child.kind()) {
            let _ = child.utf8_text(source.as_bytes()); // verify readable
            return Some(haskell::node_to_range(child));
        }
    }
    None
}

// ── Workspace symbols (flat list for index) ───────────────────────────────────

pub fn workspace_symbols(tree: &Tree, source: &str, uri: &Url) -> Vec<SymbolInfo> {
    let mut infos = Vec::new();
    let mut seen_functions: HashSet<String> = HashSet::new();
    collect_symbols(tree.root_node(), source, uri, None, &mut seen_functions, &mut infos);
    infos
}

fn collect_symbols(
    node: Node<'_>,
    source: &str,
    uri: &Url,
    container: Option<&str>,
    seen_functions: &mut HashSet<String>,
    out: &mut Vec<SymbolInfo>,
) {
    // Skip signature nodes — stored on the function's SymbolInfo instead
    if node.kind() == SIGNATURE {
        return;
    }

    if let Some(sym_kind) = haskell_kind_for_node(node.kind()) {
        if let Some(name) = haskell::node_name(node, source) {
            // Deduplicate multi-equation functions
            if node.kind() == FUNCTION {
                if seen_functions.contains(name) {
                    // Still recurse into where clauses of later equations
                    recurse_into_body(node, source, uri, Some(name), seen_functions, out);
                    return;
                }
                seen_functions.insert(name.to_string());
            }

            let range = haskell::node_to_range(node);
            let selection_range = name_selection_range(node, source).unwrap_or(range);

            let mut info = SymbolInfo::new(name.to_string(), sym_kind, uri.clone(), range, selection_range);
            if let Some(c) = container {
                info = info.with_container(c);
            }
            if let Some(doc) = extract_haddock(node, source) {
                info = info.with_doc(doc);
            }
            if node.kind() == FUNCTION {
                if let Some(sig) = find_type_signature(node, source, name) {
                    info = info.with_signature(sig);
                }
            }
            let name_owned = name.to_string();
            out.push(info);

            // Recurse into body with this symbol as container
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_symbols(child, source, uri, Some(&name_owned), seen_functions, out);
            }
            return;
        }
    }

    // Non-definition node: just recurse
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_symbols(child, source, uri, container, seen_functions, out);
    }
}

fn recurse_into_body(
    node: Node<'_>,
    source: &str,
    uri: &Url,
    container: Option<&str>,
    seen_functions: &mut HashSet<String>,
    out: &mut Vec<SymbolInfo>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_symbols(child, source, uri, container, seen_functions, out);
    }
}

/// Walk backwards from `func_node` looking for a `signature` node with a
/// matching name, skipping over contiguous comment nodes.
pub fn find_type_signature(func_node: Node<'_>, source: &str, func_name: &str) -> Option<String> {
    let mut sib = func_node.prev_named_sibling();
    while let Some(s) = sib {
        match s.kind() {
            k if k == SIGNATURE => {
                // Check if the signature name matches
                let sig_name = haskell::node_name(s, source);
                if sig_name == Some(func_name) {
                    return s.utf8_text(source.as_bytes()).ok().map(|t| t.to_string());
                }
                // Different name — stop looking
                return None;
            }
            k if COMMENT_KINDS.contains(&k) => {
                // Skip comments between signature and function
                sib = s.prev_named_sibling();
                continue;
            }
            _ => return None,
        }
    }
    None
}

// ── Identifier collection (for find-references) ───────────────────────────────

pub fn find_identifiers(tree: &Tree, source: &str, name: &str) -> Vec<Range> {
    let mut ranges = Vec::new();
    collect_identifiers(tree.root_node(), source, name, &mut ranges);
    ranges
}

fn collect_identifiers(node: Node<'_>, source: &str, name: &str, out: &mut Vec<Range>) {
    if IDENTIFIER_KINDS.contains(&node.kind())
        && node.utf8_text(source.as_bytes()).ok() == Some(name)
    {
        out.push(haskell::node_to_range(node));
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_identifiers(child, source, name, out);
    }
}
