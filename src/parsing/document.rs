use tower_lsp::lsp_types::{Position, TextDocumentContentChangeEvent, Url};
use tree_sitter::{InputEdit, Node, Parser, Tree};

use crate::parsing::haskell::{byte_to_point, pos_to_byte, position_to_point};

pub struct Document {
    pub uri: Url,
    pub version: i32,
    pub text: String,
    pub tree: Tree,
}

impl Document {
    pub fn new(uri: Url, version: i32, text: String, parser: &mut Parser) -> Self {
        let tree = parser
            .parse(text.as_bytes(), None)
            .expect("Initial parse failed");
        Self { uri, version, text, tree }
    }

    /// Apply a list of incremental (or full) changes and re-parse.
    pub fn apply_changes(
        &mut self,
        version: i32,
        changes: Vec<TextDocumentContentChangeEvent>,
        parser: &mut Parser,
    ) {
        for change in changes {
            if let Some(range) = change.range {
                let start_byte = pos_to_byte(&self.text, range.start);
                let old_end_byte = pos_to_byte(&self.text, range.end);
                let new_end_byte = start_byte + change.text.len();

                let start_pos = byte_to_point(&self.text, start_byte);
                let old_end_pos = byte_to_point(&self.text, old_end_byte);

                self.text.replace_range(start_byte..old_end_byte, &change.text);

                let new_end_pos = byte_to_point(&self.text, new_end_byte);

                let edit = InputEdit {
                    start_byte,
                    old_end_byte,
                    new_end_byte,
                    start_position: start_pos,
                    old_end_position: old_end_pos,
                    new_end_position: new_end_pos,
                };
                self.tree.edit(&edit);
            } else {
                // Full document replacement
                self.text = change.text;
            }
        }

        self.tree = parser
            .parse(self.text.as_bytes(), Some(&self.tree))
            .expect("Re-parse failed");
        self.version = version;
    }

    /// Find the smallest named node at the given position.
    pub fn node_at_position(&self, pos: Position) -> Option<Node<'_>> {
        let point = position_to_point(pos);
        let root = self.tree.root_node();
        root.named_descendant_for_point_range(point, point)
    }
}
