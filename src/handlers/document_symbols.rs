use tower_lsp::lsp_types::DocumentSymbol;
use tree_sitter::Tree;

use crate::symbols::extract;

pub fn document_symbols(tree: &Tree, source: &str) -> Vec<DocumentSymbol> {
    extract::document_symbols(tree, source)
}
