use tower_lsp::lsp_types::{FoldingRange, FoldingRangeKind};
use tree_sitter::{Node, Tree};

use crate::parsing::haskell::{
    ALTERNATIVE, BLOCK_COMMENT, CASE, CLASS, COMMENT, DO, HADDOCK, IMPORTS, INSTANCE, LET,
    NEWTYPE, WHERE,
};

pub fn folding_ranges(tree: &Tree, _source: &str) -> Vec<FoldingRange> {
    let mut ranges = Vec::new();
    collect_folds(tree.root_node(), &mut ranges);
    ranges
}

fn collect_folds(node: Node<'_>, out: &mut Vec<FoldingRange>) {
    let start_line = node.start_position().row as u32;
    let end_line = node.end_position().row as u32;

    if end_line <= start_line {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            collect_folds(child, out);
        }
        return;
    }

    match node.kind() {
        // Foldable body nodes: where clause, class/instance bodies, case bodies, do blocks
        k if k == WHERE || k == CLASS || k == INSTANCE || k == DO
            || k == CASE || k == ALTERNATIVE || k == LET || k == NEWTYPE =>
        {
            out.push(region(start_line, end_line));
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_folds(child, out);
            }
        }

        // Comment blocks
        k if k == COMMENT || k == BLOCK_COMMENT || k == HADDOCK => {
            out.push(FoldingRange {
                start_line,
                start_character: None,
                end_line,
                end_character: None,
                kind: Some(FoldingRangeKind::Comment),
                collapsed_text: None,
            });
            // Don't recurse into comments
        }

        // Import group — tree-sitter-haskell wraps all consecutive imports in
        // a single `imports` node, so we fold the whole group at once.
        k if k == IMPORTS => {
            out.push(FoldingRange {
                start_line,
                start_character: None,
                end_line,
                end_character: None,
                kind: Some(FoldingRangeKind::Imports),
                collapsed_text: None,
            });
            // Don't recurse into individual imports.
        }

        _ => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_folds(child, out);
            }
        }
    }
}

fn region(start_line: u32, end_line: u32) -> FoldingRange {
    FoldingRange {
        start_line,
        start_character: None,
        end_line,
        end_character: None,
        kind: Some(FoldingRangeKind::Region),
        collapsed_text: None,
    }
}

