use tower_lsp::lsp_types::{
    SemanticToken, SemanticTokenModifier, SemanticTokenType, SemanticTokens,
    SemanticTokensLegend,
};
use tree_sitter::{Node, Tree};

use crate::parsing::haskell::*;

// Token type indices — must match `token_legend()` order exactly
pub const TT_NAMESPACE: u32 = 0;
pub const TT_TYPE: u32 = 1;
pub const TT_CLASS: u32 = 2;
pub const TT_INTERFACE: u32 = 3;
pub const TT_FUNCTION: u32 = 4;
pub const TT_VARIABLE: u32 = 5;
pub const TT_PARAMETER: u32 = 6;
pub const TT_STRING: u32 = 7;
pub const TT_NUMBER: u32 = 8;
pub const TT_COMMENT: u32 = 9;
pub const TT_KEYWORD: u32 = 10;
pub const TT_DECORATOR: u32 = 11;

pub const MOD_DEFINITION: u32 = 1 << 0;
pub const MOD_READONLY: u32 = 1 << 1;

pub fn token_legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: vec![
            SemanticTokenType::NAMESPACE,
            SemanticTokenType::TYPE,
            SemanticTokenType::CLASS,
            SemanticTokenType::INTERFACE,
            SemanticTokenType::FUNCTION,
            SemanticTokenType::VARIABLE,
            SemanticTokenType::PARAMETER,
            SemanticTokenType::STRING,
            SemanticTokenType::NUMBER,
            SemanticTokenType::COMMENT,
            SemanticTokenType::KEYWORD,
            SemanticTokenType::DECORATOR,
        ],
        token_modifiers: vec![
            SemanticTokenModifier::DEFINITION,
            SemanticTokenModifier::READONLY,
        ],
    }
}

struct RawToken {
    line: u32,
    start: u32,
    length: u32,
    token_type: u32,
    modifiers: u32,
}

pub fn semantic_tokens_full(tree: &Tree, source: &str) -> SemanticTokens {
    let mut raw: Vec<RawToken> = Vec::new();
    collect_tokens(tree.root_node(), source, &mut raw);
    raw.sort_by(|a, b| a.line.cmp(&b.line).then(a.start.cmp(&b.start)));

    let mut tokens = Vec::with_capacity(raw.len());
    let mut prev_line = 0u32;
    let mut prev_start = 0u32;
    for t in &raw {
        let delta_line = t.line - prev_line;
        let delta_start = if delta_line == 0 { t.start - prev_start } else { t.start };
        tokens.push(SemanticToken {
            delta_line,
            delta_start,
            length: t.length,
            token_type: t.token_type,
            token_modifiers_bitset: t.modifiers,
        });
        prev_line = t.line;
        prev_start = t.start;
    }

    SemanticTokens { result_id: None, data: tokens }
}

fn collect_tokens(node: Node<'_>, source: &str, out: &mut Vec<RawToken>) {
    let kind = node.kind();

    match kind {
        // Comments / haddock
        k if k == COMMENT || k == BLOCK_COMMENT || k == HADDOCK => {
            emit(node, TT_COMMENT, 0, out);
            return;
        }
        // Pragmas
        k if k == PRAGMA => {
            emit(node, TT_DECORATOR, 0, out);
            return;
        }
        // String and char literals
        k if k == STRING || k == CHAR => {
            emit(node, TT_STRING, 0, out);
            return;
        }
        // Numeric literals
        k if k == INTEGER || k == FLOAT => {
            emit(node, TT_NUMBER, 0, out);
            return;
        }
        // Type-level constructors (uppercase identifiers)
        k if k == CONSTRUCTOR || k == CONSTRUCTOR_OPERATOR => {
            emit(node, TT_TYPE, 0, out);
            return;
        }
        // Keywords (unnamed nodes)
        k if !node.is_named() && is_keyword(k) => {
            emit(node, TT_KEYWORD, 0, out);
            return;
        }
        _ => {}
    }

    // Definition nodes: emit name with definition type, skip name when recursing
    if let Some((tt, mods)) = definition_token(kind) {
        // Try to find the name child and emit it with definition modifier
        let name_node = find_name_node(node);
        let name_id = name_node.map(|n| {
            emit(n, tt, mods, out);
            n.id()
        });
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if Some(child.id()) != name_id {
                collect_tokens(child, source, out);
            }
        }
        return;
    }

    // Default: recurse
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_tokens(child, source, out);
    }
}

fn find_name_node(node: Node<'_>) -> Option<Node<'_>> {
    // Try field "name" first
    if let Some(n) = node.child_by_field_name("name") {
        return Some(n);
    }
    // Otherwise find first variable or constructor child
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == VARIABLE || child.kind() == CONSTRUCTOR {
            return Some(child);
        }
    }
    None
}

fn emit(node: Node<'_>, token_type: u32, modifiers: u32, out: &mut Vec<RawToken>) {
    let start = node.start_position();
    let end = node.end_position();
    let length = if start.row == end.row {
        (end.column - start.column) as u32
    } else {
        80 // fallback for multiline tokens
    };
    if length == 0 {
        return;
    }
    out.push(RawToken {
        line: start.row as u32,
        start: start.column as u32,
        length,
        token_type,
        modifiers,
    });
}

fn definition_token(kind: &str) -> Option<(u32, u32)> {
    match kind {
        FUNCTION        => Some((TT_FUNCTION, MOD_DEFINITION)),
        DATA_TYPE       => Some((TT_TYPE, MOD_DEFINITION)),
        NEWTYPE         => Some((TT_TYPE, MOD_DEFINITION)),
        CLASS           => Some((TT_CLASS, MOD_DEFINITION)),
        INSTANCE        => Some((TT_INTERFACE, MOD_DEFINITION)),
        TYPE_SYNONYM    => Some((TT_TYPE, MOD_DEFINITION)),
        TYPE_FAMILY     => Some((TT_TYPE, MOD_DEFINITION)),
        FOREIGN_IMPORT  => Some((TT_FUNCTION, MOD_DEFINITION)),
        PATTERN_SYNONYM => Some((TT_FUNCTION, MOD_DEFINITION)),
        MODULE          => Some((TT_NAMESPACE, MOD_DEFINITION)),
        _               => None,
    }
}

fn is_keyword(s: &str) -> bool {
    matches!(
        s,
        "module" | "where" | "import" | "qualified" | "as" | "hiding"
            | "data" | "newtype" | "type" | "class" | "instance"
            | "deriving" | "stock" | "anyclass" | "via"
            | "do" | "let" | "in" | "case" | "of" | "if" | "then" | "else"
            | "forall" | "infixl" | "infixr" | "infix"
            | "foreign" | "export"
            | "default" | "pattern" | "family"
            | "True" | "False"
            | "\\" | "->" | "<-" | "=>" | "::" | "=" | "|" | "@" | "~"
    )
}
