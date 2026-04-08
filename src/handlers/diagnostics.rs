use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, Range};
use tree_sitter::{Node, Tree};

use crate::parsing::haskell::{node_to_range, point_to_position};

pub fn get_diagnostics(tree: &Tree, source: &str) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    collect_errors(tree.root_node(), source, &mut diags);
    diags
}

fn collect_errors(node: Node<'_>, source: &str, out: &mut Vec<Diagnostic>) {
    if node.is_error() {
        let range = node_to_range(node);
        let message = if node.child_count() == 0 {
            let text = node.utf8_text(source.as_bytes()).unwrap_or("?");
            format!("Unexpected token: `{text}`")
        } else {
            "Syntax error".to_string()
        };
        out.push(Diagnostic {
            range,
            severity: Some(DiagnosticSeverity::ERROR),
            message,
            source: Some("hurry".to_string()),
            ..Default::default()
        });
        return;
    }

    if node.is_missing() {
        let range = Range {
            start: point_to_position(node.start_position()),
            end: point_to_position(node.start_position()),
        };
        let text = node.kind();
        out.push(Diagnostic {
            range,
            severity: Some(DiagnosticSeverity::ERROR),
            message: format!("Missing `{text}`"),
            source: Some("hurry".to_string()),
            ..Default::default()
        });
        return;
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_errors(child, source, out);
    }
}
