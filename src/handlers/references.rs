use tower_lsp::lsp_types::{Location, Position, ReferenceContext, Url};
use tree_sitter::Tree;

use crate::parsing::haskell::node_at_position;
use crate::symbols::extract::find_identifiers;
use crate::symbols::index::WorkspaceIndex;

pub fn find_references(
    tree: &Tree,
    source: &str,
    uri: &Url,
    pos: Position,
    _context: ReferenceContext,
    index: &WorkspaceIndex,
    get_file: &dyn Fn(&Url) -> Option<(String, Tree)>,
) -> Vec<Location> {
    let root = tree.root_node();
    let Some(node) = node_at_position(root, source, pos) else {
        return vec![];
    };

    let name = match node.utf8_text(source.as_bytes()).ok() {
        Some(n) if !n.is_empty() => n.to_string(),
        _ => return vec![],
    };

    let mut locations = Vec::new();

    // 1. Same-file references
    for range in find_identifiers(tree, source, &name) {
        locations.push(Location { uri: uri.clone(), range });
    }

    // 2. Cross-file references via workspace index
    for file_uri in index.all_uris() {
        if &file_uri == uri {
            continue;
        }

        if let Some((file_text, file_tree)) = get_file(&file_uri) {
            if !file_text.contains(name.as_str()) {
                continue;
            }
            for range in find_identifiers(&file_tree, &file_text, &name) {
                locations.push(Location { uri: file_uri.clone(), range });
            }
        }
    }

    locations
}
