use tower_lsp::lsp_types::{Position, Range};
use tree_sitter::{Node, Point};

// ── Definition node kinds ────────────────────────────────────────────────────
// NOTE: these are tree-sitter-haskell 0.23 node kind strings. If the grammar
// uses different names, update these constants after inspecting the parse tree.
pub const FUNCTION: &str = "function";
pub const SIGNATURE: &str = "signature";
pub const DATA_TYPE: &str = "data_type";
pub const NEWTYPE: &str = "newtype";
pub const CLASS: &str = "class";
pub const INSTANCE: &str = "instance_decl";
/// NOTE: "type_synomym" is a typo in the tree-sitter-haskell 0.23 grammar.
pub const TYPE_SYNONYM: &str = "type_synomym";
pub const TYPE_FAMILY: &str = "type_family";
pub const FOREIGN_IMPORT: &str = "foreign_import";
pub const PATTERN_SYNONYM: &str = "pattern_synonym";

pub const DEFINITION_KINDS: &[&str] = &[
    FUNCTION,
    BIND,
    DATA_TYPE,
    NEWTYPE,
    CLASS,
    INSTANCE,
    TYPE_SYNONYM,
    TYPE_FAMILY,
    FOREIGN_IMPORT,
    PATTERN_SYNONYM,
];

// ── Identifier node kinds ────────────────────────────────────────────────────
pub const VARIABLE: &str = "variable";
pub const CONSTRUCTOR: &str = "constructor";
pub const OPERATOR: &str = "operator";
pub const CONSTRUCTOR_OPERATOR: &str = "constructor_operator";

/// Type-level name used for type constructors, class names, etc.
pub const NAME: &str = "name";

pub const IDENTIFIER_KINDS: &[&str] = &[VARIABLE, CONSTRUCTOR, OPERATOR, CONSTRUCTOR_OPERATOR, NAME];

// ── Structural / scope node kinds ────────────────────────────────────────────
pub const MODULE: &str = "module";
pub const IMPORT: &str = "import";
/// Group node containing consecutive import statements.
pub const IMPORTS: &str = "imports";
pub const DECLARATIONS: &str = "declarations";
/// Body of a type class or instance declaration.
pub const CLASS_DECLARATIONS: &str = "class_declarations";
pub const WHERE: &str = "where";
pub const LET: &str = "let";
pub const LET_IN: &str = "let_in";
pub const DO: &str = "do";
pub const LAMBDA: &str = "lambda";
pub const CASE: &str = "case";
pub const ALTERNATIVE: &str = "alternative";
pub const GUARDS: &str = "guards";
pub const MATCH: &str = "match";
/// Container for where/let bindings.
pub const LOCAL_BINDS: &str = "local_binds";
/// A single binding within local_binds.
pub const BIND: &str = "bind";

// ── Literal node kinds ───────────────────────────────────────────────────────
pub const INTEGER: &str = "integer";
pub const FLOAT: &str = "float";
pub const STRING: &str = "string";
pub const CHAR: &str = "char";

// ── Comment / doc node kinds ─────────────────────────────────────────────────
pub const COMMENT: &str = "comment";
pub const BLOCK_COMMENT: &str = "block_comment";
pub const HADDOCK: &str = "haddock";
pub const PRAGMA: &str = "pragma";

pub const COMMENT_KINDS: &[&str] = &[COMMENT, BLOCK_COMMENT, HADDOCK];

// ── Coordinate helpers (identical logic to blisk's scala.rs) ─────────────────

pub fn node_to_range(node: Node<'_>) -> Range {
    Range {
        start: point_to_position(node.start_position()),
        end: point_to_position(node.end_position()),
    }
}

pub fn point_to_position(p: Point) -> Position {
    Position {
        line: p.row as u32,
        character: p.column as u32,
    }
}

pub fn position_to_point(p: Position) -> Point {
    Point {
        row: p.line as usize,
        column: p.character as usize,
    }
}

/// Convert an LSP Position to a byte offset within `text` (UTF-16 aware).
pub fn pos_to_byte(text: &str, pos: Position) -> usize {
    let mut byte_offset = 0usize;
    for (line_idx, line) in text.split('\n').enumerate() {
        if line_idx == pos.line as usize {
            return byte_offset + utf16_to_byte_offset(line, pos.character as usize);
        }
        byte_offset += line.len() + 1; // +1 for '\n'
    }
    text.len()
}

/// Convert a byte offset within `text` to a tree-sitter Point.
pub fn byte_to_point(text: &str, byte_offset: usize) -> Point {
    let capped = byte_offset.min(text.len());
    let before = &text[..capped];
    let row = before.bytes().filter(|&b| b == b'\n').count();
    let col = before.rfind('\n').map(|i| capped - i - 1).unwrap_or(capped);
    Point { row, column: col }
}

/// Find the innermost named AST node at the given LSP cursor position.
///
/// Uses a 1-byte range instead of zero-length to work around tree-sitter's
/// behaviour where zero-length ranges stop at an outer node rather than
/// descending to leaf nodes.
pub fn node_at_position<'tree>(
    root: tree_sitter::Node<'tree>,
    source: &str,
    pos: Position,
) -> Option<tree_sitter::Node<'tree>> {
    let byte = pos_to_byte(source, pos);
    let end_byte = (byte + 1).min(source.len());
    root.named_descendant_for_byte_range(byte, end_byte)
}

fn utf16_to_byte_offset(line: &str, utf16_offset: usize) -> usize {
    let mut count = 0usize;
    for (byte_idx, ch) in line.char_indices() {
        if count >= utf16_offset {
            return byte_idx;
        }
        count += ch.len_utf16();
    }
    line.len()
}

// ── Name extraction ───────────────────────────────────────────────────────────

/// Try to extract the name text from a definition node.
/// Haskell conventions:
/// - `function`/`signature`  → first `variable` child (term-level name)
/// - `data_type`/`newtype`/`class`/`type_synonym`/`type_family` → first `constructor` child (uppercase name)
/// - `instance_decl` → synthesise "ClassName Type" from text between "instance" and "where"
/// - `foreign_import` → first `variable` child after the string literal
pub fn node_name<'a>(node: Node<'a>, source: &'a str) -> Option<&'a str> {
    // Try field "name" first (works for many grammars)
    if let Some(name_node) = node.child_by_field_name("name") {
        return name_node.utf8_text(source.as_bytes()).ok();
    }

    match node.kind() {
        FUNCTION | SIGNATURE | FOREIGN_IMPORT | PATTERN_SYNONYM | BIND => {
            first_child_of_kind(node, source, VARIABLE)
        }
        DATA_TYPE | NEWTYPE | CLASS | TYPE_SYNONYM | TYPE_FAMILY => {
            first_child_of_kind(node, source, CONSTRUCTOR)
        }
        INSTANCE => {
            // Build a short label "Class Type" by taking text between start and first where/=
            let raw = node.utf8_text(source.as_bytes()).ok()?;
            let body = raw
                .trim_start_matches("instance")
                .trim();
            let end = body.find(" where").unwrap_or(body.len());
            let label = body[..end].trim();
            if label.is_empty() { None } else { Some(label) }
        }
        _ => None,
    }
}

fn first_child_of_kind<'a>(node: Node<'a>, source: &'a str, kind: &str) -> Option<&'a str> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == kind {
            return child.utf8_text(source.as_bytes()).ok();
        }
    }
    None
}
