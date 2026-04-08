use tower_lsp::lsp_types::{Position, SelectionRange};
use tree_sitter::Tree;

use crate::parsing::haskell::{node_to_range, position_to_point};

pub fn selection_ranges(tree: &Tree, positions: Vec<Position>) -> Vec<SelectionRange> {
    positions
        .into_iter()
        .map(|pos| selection_range_at(tree, pos))
        .collect()
}

fn selection_range_at(tree: &Tree, pos: Position) -> SelectionRange {
    let point = position_to_point(pos);
    let root = tree.root_node();

    let mut node = root
        .named_descendant_for_point_range(point, point)
        .unwrap_or(root);

    // Collect the ancestor chain: [innermost, ..., root]
    let mut chain: Vec<tree_sitter::Node<'_>> = vec![node];
    while let Some(parent) = node.parent() {
        chain.push(parent);
        node = parent;
    }

    // Build SelectionRange linked list from outermost to innermost.
    let mut current: Option<Box<SelectionRange>> = None;
    for ancestor in chain.iter().rev() {
        current = Some(Box::new(SelectionRange {
            range: node_to_range(*ancestor),
            parent: current,
        }));
    }

    *current.unwrap_or_else(|| {
        Box::new(SelectionRange {
            range: node_to_range(root),
            parent: None,
        })
    })
}
