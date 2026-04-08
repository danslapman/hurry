use tower_lsp::lsp_types::{GotoDefinitionResponse, Location, Position, Url};
use tree_sitter::{Node, Tree};

use crate::parsing::haskell::{
    node_at_position, node_to_range, ALTERNATIVE, BIND, CASE, CONSTRUCTOR,
    DEFINITION_KINDS, DO, FUNCTION, IDENTIFIER_KINDS, LAMBDA, LET, LET_IN, LOCAL_BINDS,
    OPERATOR, VARIABLE,
};
use crate::symbols::index::WorkspaceIndex;

pub fn goto_definition(
    tree: &Tree,
    source: &str,
    uri: &Url,
    pos: Position,
    index: &WorkspaceIndex,
) -> Option<GotoDefinitionResponse> {
    let root = tree.root_node();
    let raw = node_at_position(root, source, pos)?;

    // Resolve the effective identifier node.
    // When the cursor lands on the module-qualifier part of a qualified name
    // (e.g. `TIO` in `TIO.hPutStrLn`), the leaf is a `module_id` which is not
    // in IDENTIFIER_KINDS.  In that case we walk up to find the enclosing
    // `qualified` node and use its identifier child instead.
    let node = if IDENTIFIER_KINDS.contains(&raw.kind()) {
        raw
    } else {
        resolve_qualified_identifier(raw)?
    };

    // Must be on a Haskell identifier
    if !IDENTIFIER_KINDS.contains(&node.kind()) {
        return None;
    }
    let name = node.utf8_text(source.as_bytes()).ok()?;
    if name.is_empty() {
        return None;
    }

    // 1. Same-file scope walk
    if let Some(loc) = resolve_in_file(node, source, uri, name) {
        return Some(GotoDefinitionResponse::Scalar(loc));
    }

    // 2. Cross-file index lookup
    let matches = index.lookup_by_name(name);
    if matches.is_empty() {
        return None;
    }

    let locations: Vec<Location> = matches
        .into_iter()
        .map(|info| Location { uri: info.uri, range: info.range })
        .collect();

    Some(if locations.len() == 1 {
        GotoDefinitionResponse::Scalar(locations.into_iter().next().unwrap())
    } else {
        GotoDefinitionResponse::Array(locations)
    })
}

/// Walk up the scope tree from `node` looking for a binding of `name`.
fn resolve_in_file(start: Node<'_>, source: &str, uri: &Url, name: &str) -> Option<Location> {
    let mut current = start.parent()?;
    loop {
        if let Some(loc) = find_definition_in_scope(current, source, uri, name) {
            return Some(loc);
        }
        current = current.parent()?;
    }
}

/// Search the immediate children of `scope` for a definition of `name`.
/// Handles Haskell scoping constructs: function equations, where bindings,
/// let bindings, lambda parameters, case alternatives.
fn find_definition_in_scope(scope: Node<'_>, source: &str, uri: &Url, name: &str) -> Option<Location> {
    let scope_kind = scope.kind();

    // Function body: check pattern parameters and `local_binds` (where-clauses).
    // In tree-sitter-haskell, `local_binds` is a direct child of `function`.
    if scope_kind == FUNCTION {
        let mut cursor = scope.walk();
        for child in scope.children(&mut cursor) {
            // Pattern-bound parameters (e.g. `compute x y = …`)
            if child.kind() == "patterns" {
                if let Some(loc) = find_variable_in_subtree(child, source, uri, name) {
                    return Some(loc);
                }
            }
            // Where-bound definitions live in `local_binds`
            if child.kind() == LOCAL_BINDS {
                if let Some(loc) = find_in_local_binds(child, source, uri, name) {
                    return Some(loc);
                }
            }
        }
    }

    // `let … in …` expression: definitions in its `local_binds` child.
    if scope_kind == LET_IN {
        let mut cursor = scope.walk();
        for child in scope.children(&mut cursor) {
            if child.kind() == LOCAL_BINDS {
                if let Some(loc) = find_in_local_binds(child, source, uri, name) {
                    return Some(loc);
                }
            }
        }
    }

    // `do`-notation: look for `let` statements whose `local_binds` define `name`.
    if scope_kind == DO {
        let mut cursor = scope.walk();
        for child in scope.children(&mut cursor) {
            if child.kind() == LET {
                let mut inner = child.walk();
                for item in child.children(&mut inner) {
                    if item.kind() == LOCAL_BINDS {
                        if let Some(loc) = find_in_local_binds(item, source, uri, name) {
                            return Some(loc);
                        }
                    }
                }
            }
        }
    }

    // In `lambda`, look for bound parameter names
    if scope_kind == LAMBDA {
        if let Some(loc) = find_variable_in_subtree(scope, source, uri, name) {
            return Some(loc);
        }
    }

    // In `alternative` (case arm), look for pattern-bound names
    if scope_kind == ALTERNATIVE {
        if let Some(loc) = find_variable_in_subtree(scope, source, uri, name) {
            return Some(loc);
        }
    }

    // In `case` expression, check alternatives for pattern bindings
    if scope_kind == CASE {
        let mut cursor = scope.walk();
        for child in scope.children(&mut cursor) {
            if child.kind() == ALTERNATIVE {
                if let Some(loc) = find_variable_in_subtree(child, source, uri, name) {
                    return Some(loc);
                }
            }
        }
    }

    None
}

/// Search `local_binds` for a `bind` whose first variable/constructor child
/// matches `name`.  Only the LHS of each bind is inspected (not the RHS).
fn find_in_local_binds(local_binds: Node<'_>, source: &str, uri: &Url, name: &str) -> Option<Location> {
    let mut cursor = local_binds.walk();
    for child in local_binds.children(&mut cursor) {
        if child.kind() == BIND {
            let mut inner = child.walk();
            for item in child.children(&mut inner) {
                // Stop at the match/= node — we only want the LHS name.
                if item.kind() == "match" || item.kind() == "=" {
                    break;
                }
                if (item.kind() == VARIABLE || item.kind() == CONSTRUCTOR)
                    && item.utf8_text(source.as_bytes()).ok() == Some(name)
                {
                    return Some(Location { uri: uri.clone(), range: node_to_range(item) });
                }
            }
        }
    }
    None
}

/// Shallowly walk `node`'s direct children looking for a `variable` or
/// `constructor` whose text equals `name`.  Does NOT recurse into definitions.
fn find_variable_in_subtree(node: Node<'_>, source: &str, uri: &Url, name: &str) -> Option<Location> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if (child.kind() == VARIABLE || child.kind() == OPERATOR)
            && child.utf8_text(source.as_bytes()).ok() == Some(name)
        {
            return Some(Location { uri: uri.clone(), range: node_to_range(child) });
        }
        if DEFINITION_KINDS.contains(&child.kind()) {
            continue;
        }
        if let Some(loc) = find_variable_in_subtree(child, source, uri, name) {
            return Some(loc);
        }
    }
    None
}

/// When the cursor is on a module qualifier (e.g. `TIO` in `TIO.hPutStrLn`),
/// walk up to the enclosing `qualified` node and return its identifier child
/// (the `variable` or `constructor` leaf after the `.`).
fn resolve_qualified_identifier<'tree>(node: tree_sitter::Node<'tree>) -> Option<tree_sitter::Node<'tree>> {
    let mut current = node;
    loop {
        if current.kind() == "qualified" {
            let mut cursor = current.walk();
            for child in current.children(&mut cursor) {
                if child.kind() == VARIABLE || child.kind() == CONSTRUCTOR {
                    return Some(child);
                }
            }
            return None;
        }
        current = current.parent()?;
    }
}
