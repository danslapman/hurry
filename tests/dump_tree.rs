/// Run with `cargo test dump -- --nocapture` to inspect actual node kinds.
/// This test always passes; it's a diagnostic tool only.

fn parse(source: &str) -> tree_sitter::Tree {
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&tree_sitter_haskell::LANGUAGE.into()).unwrap();
    parser.parse(source, None).unwrap()
}

fn print_tree(node: tree_sitter::Node<'_>, source: &str, depth: usize) {
    let indent = "  ".repeat(depth);
    let text = if node.child_count() == 0 {
        format!(" {:?}", node.utf8_text(source.as_bytes()).unwrap_or("?"))
    } else {
        String::new()
    };
    println!(
        "{}{} (named={}) [{}, {}]{}",
        indent,
        node.kind(),
        node.is_named(),
        node.start_position().row,
        node.start_position().column,
        text
    );
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        print_tree(child, source, depth + 1);
    }
}

#[test]
fn dump_simple_module() {
    let source = include_str!("fixtures/simple_module.hs");
    let tree = parse(source);
    println!("\n=== simple_module.hs ===");
    print_tree(tree.root_node(), source, 0);
}

#[test]
fn dump_hover_test() {
    let source = include_str!("fixtures/hover_test.hs");
    let tree = parse(source);
    println!("\n=== hover_test.hs ===");
    print_tree(tree.root_node(), source, 0);
}

#[test]
fn dump_folding_test() {
    let source = include_str!("fixtures/folding_test.hs");
    let tree = parse(source);
    println!("\n=== folding_test.hs ===");
    print_tree(tree.root_node(), source, 0);
}

#[test]
fn dump_cross_ref() {
    let source = include_str!("fixtures/cross_ref.hs");
    let tree = parse(source);
    println!("\n=== cross_ref.hs ===");
    print_tree(tree.root_node(), source, 0);
}

#[test]
fn dump_qualified() {
    let source = r#"module Main where

import qualified Data.Text.IO as TIO

main :: IO ()
main = TIO.hPutStrLn stderr "hello"
"#;
    let tree = parse(source);
    println!("\n=== qualified name ===");
    print_tree(tree.root_node(), source, 0);
}

#[test]
fn dump_qualified_in_do() {
    let source = r#"module Main where

import qualified Data.Text.IO as TIO

main :: IO ()
main = do
  TIO.hPutStrLn stderr "hello"
"#;
    let tree = parse(source);
    println!("\n=== qualified name in do block ===");
    print_tree(tree.root_node(), source, 0);
}

#[test]
fn dump_qualified_dollar() {
    // Matches Main.hs line 73: TIO.hPutStrLn hOut $ mkSepString $ columns
    let source = "f hOut columns = do\n  TIO.hPutStrLn hOut $ foo $ columns\n";
    let tree = parse(source);
    println!("\n=== qualified name with $ in do block ===");
    print_tree(tree.root_node(), source, 0);
}
