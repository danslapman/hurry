use hurry::handlers::{
    definition, diagnostics, document_links, document_symbols, folding, hover, references,
    selection, semantic_tokens,
};
use hurry::symbols::{extract, index::WorkspaceIndex};
use tower_lsp::lsp_types::*;
use tree_sitter::Tree;

fn parse(source: &str) -> Tree {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_haskell::LANGUAGE.into())
        .unwrap();
    parser.parse(source, None).unwrap()
}

// ── Diagnostics ───────────────────────────────────────────────────────────────

#[test]
fn no_diags_for_valid_haskell() {
    let source = include_str!("fixtures/simple_module.hs");
    let tree = parse(source);
    let diags = diagnostics::get_diagnostics(&tree, source);
    assert!(diags.is_empty(), "Expected no diagnostics, got: {:?}", diags);
}

#[test]
fn diags_for_syntax_error() {
    let source = include_str!("fixtures/with_errors.hs");
    let tree = parse(source);
    let diags = diagnostics::get_diagnostics(&tree, source);
    assert!(!diags.is_empty(), "Expected diagnostics for broken Haskell");
    assert!(
        diags.iter().all(|d| d.severity == Some(DiagnosticSeverity::ERROR)),
        "All diagnostics should be ERROR severity"
    );
}

// ── Document Symbols ──────────────────────────────────────────────────────────

#[test]
fn symbols_top_level_names() {
    let source = include_str!("fixtures/simple_module.hs");
    let tree = parse(source);
    let symbols = document_symbols::document_symbols(&tree, source);

    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"greet"),  "Missing greet; got: {:?}", names);
    assert!(names.contains(&"Animal"), "Missing Animal; got: {:?}", names);
    assert!(names.contains(&"Speak"),  "Missing Speak; got: {:?}", names);

    assert!(
        symbols.iter().any(|s| s.name == "greet" && s.kind == SymbolKind::FUNCTION),
        "greet should be FUNCTION"
    );
    assert!(
        symbols.iter().any(|s| s.name == "Animal" && s.kind == SymbolKind::STRUCT),
        "Animal should be STRUCT (data type)"
    );
    assert!(
        symbols.iter().any(|s| s.name == "Speak" && s.kind == SymbolKind::INTERFACE),
        "Speak should be INTERFACE (type class)"
    );
}

#[test]
fn symbols_dedup_multi_equation() {
    // `fib` has three equations; it should appear exactly once in document symbols.
    let source = include_str!("fixtures/simple_module.hs");
    let tree = parse(source);
    let symbols = document_symbols::document_symbols(&tree, source);

    let fib_count = symbols.iter().filter(|s| s.name == "fib").count();
    assert_eq!(fib_count, 1, "fib should appear exactly once, not {fib_count} times");
}

#[test]
fn symbols_nested_in_typeclass() {
    // `speakLoudly` has a default implementation inside class Speak — should appear
    // as a child of Speak.
    let source = include_str!("fixtures/simple_module.hs");
    let tree = parse(source);
    let symbols = document_symbols::document_symbols(&tree, source);

    let speak_class = symbols
        .iter()
        .find(|s| s.name == "Speak")
        .expect("Speak class not found");

    let children = speak_class
        .children
        .as_ref()
        .expect("Speak class should have children (speakLoudly default impl)");
    let child_names: Vec<&str> = children.iter().map(|s| s.name.as_str()).collect();
    assert!(
        child_names.contains(&"speakLoudly"),
        "Missing speakLoudly default impl; got: {:?}", child_names
    );
}

// ── Type Signature Association ────────────────────────────────────────────────

#[test]
fn type_signature_associated_with_function() {
    let source = include_str!("fixtures/simple_module.hs");
    let tree = parse(source);
    let uri = Url::parse("file:///test/simple_module.hs").unwrap();
    let syms = extract::workspace_symbols(&tree, source, &uri);

    let greet = syms.iter().find(|s| s.name == "greet").expect("greet not found");
    let sig = greet.type_signature.as_deref()
        .expect("greet should have a type_signature");
    assert!(
        sig.contains("String"),
        "Type signature should contain 'String'; got: {sig}"
    );
    assert!(
        sig.contains("greet"),
        "Type signature should contain function name; got: {sig}"
    );
}

// ── Folding Ranges ────────────────────────────────────────────────────────────

#[test]
fn folding_region_for_where_block() {
    let source = include_str!("fixtures/folding_test.hs");
    let tree = parse(source);
    let folds = folding::folding_ranges(&tree, source);
    assert!(
        folds.iter().any(|f| f.kind == Some(FoldingRangeKind::Region)),
        "Expected at least one Region fold (where block); got: {:?}", folds
    );
}

#[test]
fn folding_imports_for_import_group() {
    let source = include_str!("fixtures/folding_test.hs");
    let tree = parse(source);
    let folds = folding::folding_ranges(&tree, source);
    assert!(
        folds.iter().any(|f| f.kind == Some(FoldingRangeKind::Imports)),
        "Expected an Imports fold for consecutive import block; got: {:?}", folds
    );
}

// ── Semantic Tokens ───────────────────────────────────────────────────────────

#[test]
fn semantic_tokens_not_empty() {
    let source = include_str!("fixtures/simple_module.hs");
    let tree = parse(source);
    let tokens = semantic_tokens::semantic_tokens_full(&tree, source);
    assert!(
        !tokens.data.is_empty(),
        "Expected semantic tokens to be non-empty"
    );
}

// ── Selection Range ───────────────────────────────────────────────────────────

#[test]
fn selection_range_returns_parent_chain() {
    let source = include_str!("fixtures/simple_module.hs");
    let tree = parse(source);
    // Position inside "greet" on the function body line (line 4)
    let pos = Position { line: 4, character: 0 };
    let ranges = selection::selection_ranges(&tree, vec![pos]);
    assert_eq!(ranges.len(), 1);
    assert!(
        ranges[0].parent.is_some(),
        "Selection range should have a parent scope"
    );
}

// ── Document Links ────────────────────────────────────────────────────────────

#[test]
fn document_link_url_in_comment() {
    let source = include_str!("fixtures/simple_module.hs");
    let tree = parse(source);
    let links = document_links::document_links(&tree, source);
    assert!(!links.is_empty(), "Expected at least one document link");
    assert!(
        links.iter().any(|l| l
            .target
            .as_ref()
            .map(|u| u.as_str().contains("example.com"))
            .unwrap_or(false)),
        "Expected a link pointing to example.com"
    );
}

// ── Goto Definition ───────────────────────────────────────────────────────────

#[test]
fn definition_same_file_where_binding() {
    // In cross_ref.hs: `compute x y = total`  (line 4, `total` at char 14)
    // `total` is bound in the where clause at line 6, char 4.
    let source = include_str!("fixtures/cross_ref.hs");
    let tree = parse(source);
    let uri = Url::parse("file:///test/cross_ref.hs").unwrap();
    let index = WorkspaceIndex::new();

    // `total` in `compute x y = total`
    //  line 4:  "compute x y = total"
    //  0123456789012345678...
    //                    ^ char 14
    let pos = Position { line: 4, character: 14 };
    let result = definition::goto_definition(&tree, source, &uri, pos, &index);
    assert!(result.is_some(), "Expected definition result for where-bound 'total'");
}

#[test]
fn definition_same_file_let_binding() {
    // In cross_ref.hs: `  in double + 1` (line 12, `double` at char 5)
    // `double` is bound in the let clause at line 11, char 6.
    let source = include_str!("fixtures/cross_ref.hs");
    let tree = parse(source);
    let uri = Url::parse("file:///test/cross_ref.hs").unwrap();
    let index = WorkspaceIndex::new();

    // `double` in `  in double + 1`
    //  line 12: "  in double + 1"
    //  01234567890...
    //       ^ char 5
    let pos = Position { line: 12, character: 5 };
    let result = definition::goto_definition(&tree, source, &uri, pos, &index);
    assert!(result.is_some(), "Expected definition result for let-bound 'double'");
}

#[test]
fn definition_cross_file_function() {
    // `greet` used in usage.hs resolves to simple_module.hs via the index.
    let simple_src = include_str!("fixtures/simple_module.hs");
    let simple_tree = parse(simple_src);
    let simple_uri = Url::parse("file:///test/simple_module.hs").unwrap();

    let usage_src = include_str!("fixtures/usage.hs");
    let usage_tree = parse(usage_src);
    let usage_uri = Url::parse("file:///test/usage.hs").unwrap();

    let index = WorkspaceIndex::new();
    index.update_file(
        &simple_uri,
        extract::workspace_symbols(&simple_tree, simple_src, &simple_uri),
    );

    // `greet` in `sayHello = putStrLn (greet "World")`
    // line 5: "sayHello = putStrLn (greet \"World\")"
    //          0         1         2
    //          0123456789012345678901234567
    //                               ^ char 21
    let pos = Position { line: 5, character: 21 };
    let result = definition::goto_definition(&usage_tree, usage_src, &usage_uri, pos, &index);
    assert!(result.is_some(), "Expected cross-file definition for 'greet'");

    let resolved_uris: Vec<Url> = match result.unwrap() {
        GotoDefinitionResponse::Scalar(loc) => vec![loc.uri],
        GotoDefinitionResponse::Array(locs) => locs.into_iter().map(|l| l.uri).collect(),
        GotoDefinitionResponse::Link(links) => links.into_iter().map(|l| l.target_uri).collect(),
    };
    assert!(
        resolved_uris.contains(&simple_uri),
        "Definition should resolve to simple_module.hs; got: {:?}", resolved_uris
    );
}

#[test]
fn definition_qualified_name_cross_file() {
    // `TIO.hPutStrLn` in a do block — cursor on `hPutStrLn` (the variable leaf).
    // The index is pre-populated with a definition of `hPutStrLn` in a dep-source file.
    //
    // Source (line/col, 0-indexed):
    //   line 0: "module Main where"
    //   line 1: ""
    //   line 2: "import qualified Data.Text.IO as TIO"
    //   line 3: ""
    //   line 4: "main :: IO ()"
    //   line 5: "main = do"
    //   line 6: "  TIO.hPutStrLn stderr \"hello\""
    //            0123456789...
    //            col 6 = 'h' in hPutStrLn
    let source = "module Main where\n\nimport qualified Data.Text.IO as TIO\n\nmain :: IO ()\nmain = do\n  TIO.hPutStrLn stderr \"hello\"\n";
    let tree = parse(source);
    let uri = Url::parse("file:///test/Main.hs").unwrap();

    // Simulate hPutStrLn being indexed from a dep-source file
    let dep_uri = Url::parse("file:///project/.dep-srcs/text-2.0.2/Data/Text/IO.hs").unwrap();
    let dep_src = "module Data.Text.IO where\nhPutStrLn :: Handle -> Text -> IO ()\nhPutStrLn h t = undefined\n";
    let dep_tree = parse(dep_src);
    let index = WorkspaceIndex::new();
    index.update_file(&dep_uri, extract::workspace_symbols(&dep_tree, dep_src, &dep_uri));

    // Cursor on `hPutStrLn` at line 6, col 6
    let pos = Position { line: 6, character: 6 };
    let result = definition::goto_definition(&tree, source, &uri, pos, &index);
    assert!(
        result.is_some(),
        "Expected goto_definition to find hPutStrLn from dep sources"
    );

    let resolved_uris: Vec<Url> = match result.unwrap() {
        GotoDefinitionResponse::Scalar(loc) => vec![loc.uri],
        GotoDefinitionResponse::Array(locs) => locs.into_iter().map(|l| l.uri).collect(),
        GotoDefinitionResponse::Link(links) => links.into_iter().map(|l| l.target_uri).collect(),
    };
    assert!(
        resolved_uris.contains(&dep_uri),
        "Definition should resolve to dep-source file; got: {:?}", resolved_uris
    );
}

#[test]
fn definition_on_qualifier_resolves_to_function() {
    // Cursor is on `TIO` (col 2) in `  TIO.hPutStrLn hOut $ foo $ columns`.
    // Should resolve to the same dep-source definition as when cursor is on `hPutStrLn`.
    let source = "f hOut columns = do\n  TIO.hPutStrLn hOut $ foo $ columns\n";
    let tree = parse(source);
    let uri = Url::parse("file:///test/Main.hs").unwrap();

    let dep_uri = Url::parse("file:///project/.dep-srcs/text-2.0.2/Data/Text/IO.hs").unwrap();
    let dep_src = "module Data.Text.IO where\nhPutStrLn :: Handle -> Text -> IO ()\nhPutStrLn h t = undefined\n";
    let dep_tree = parse(dep_src);
    let index = WorkspaceIndex::new();
    index.update_file(&dep_uri, extract::workspace_symbols(&dep_tree, dep_src, &dep_uri));

    // Cursor on `TIO` at line 1, col 2
    let pos_on_qualifier = Position { line: 1, character: 2 };
    let result = definition::goto_definition(&tree, source, &uri, pos_on_qualifier, &index);
    assert!(
        result.is_some(),
        "Expected goto_definition to resolve when cursor is on qualifier 'TIO'"
    );

    let resolved_uris: Vec<Url> = match result.unwrap() {
        GotoDefinitionResponse::Scalar(loc) => vec![loc.uri],
        GotoDefinitionResponse::Array(locs) => locs.into_iter().map(|l| l.uri).collect(),
        GotoDefinitionResponse::Link(links) => links.into_iter().map(|l| l.target_uri).collect(),
    };
    assert!(
        resolved_uris.contains(&dep_uri),
        "Definition should resolve to dep-source file; got: {:?}", resolved_uris
    );
}

// ── Find References ───────────────────────────────────────────────────────────

#[test]
fn references_same_file() {
    // In cross_ref.hs, `total` is defined on line 6 and used on line 4.
    let source = include_str!("fixtures/cross_ref.hs");
    let tree = parse(source);
    let uri = Url::parse("file:///test/cross_ref.hs").unwrap();
    let index = WorkspaceIndex::new();

    // `total` on line 6, char 4 (the definition site)
    let pos = Position { line: 6, character: 4 };
    let context = ReferenceContext { include_declaration: true };
    let refs =
        references::find_references(&tree, source, &uri, pos, context, &index, &|_| None);

    assert!(
        refs.len() >= 2,
        "Expected ≥2 references to 'total' (definition + use), got {} ({:?})",
        refs.len(), refs
    );
}

#[test]
fn references_cross_file() {
    // `greet` defined in simple_module.hs and used in usage.hs.
    let simple_src = include_str!("fixtures/simple_module.hs");
    let simple_tree = parse(simple_src);
    let simple_uri = Url::parse("file:///test/simple_module.hs").unwrap();

    let usage_src = include_str!("fixtures/usage.hs");
    let usage_uri = Url::parse("file:///test/usage.hs").unwrap();

    let index = WorkspaceIndex::new();
    index.update_file(
        &usage_uri,
        extract::workspace_symbols(&parse(usage_src), usage_src, &usage_uri),
    );

    let get_file = |uri: &Url| -> Option<(String, Tree)> {
        if uri == &usage_uri {
            Some((usage_src.to_string(), parse(usage_src)))
        } else {
            None
        }
    };

    // `greet` definition at line 4, char 0 in simple_module.hs
    let pos = Position { line: 4, character: 0 };
    let context = ReferenceContext { include_declaration: true };
    let refs = references::find_references(
        &simple_tree, simple_src, &simple_uri, pos, context, &index, &get_file,
    );

    let has_usage = refs.iter().any(|l| l.uri == usage_uri);
    assert!(has_usage, "Expected 'greet' references in usage.hs; got: {:?}", refs);
}

// ── Hover ─────────────────────────────────────────────────────────────────────

#[test]
fn hover_function_shows_type_signature() {
    // `greet` at line 5, char 0 — should show `greet :: String -> String -> String`
    let source = include_str!("fixtures/hover_test.hs");
    let tree = parse(source);
    let uri = Url::parse("file:///test/hover_test.hs").unwrap();
    let index = WorkspaceIndex::new();
    index.update_file(&uri, extract::workspace_symbols(&tree, source, &uri));

    let pos = Position { line: 5, character: 0 };
    let result = hover::hover(&tree, source, &uri, pos, &index);

    let h = result.expect("Expected hover for 'greet'");
    let md = match_markdown(h);
    assert!(md.contains("greet"), "Expected 'greet' in hover; got: {md}");
    assert!(md.contains("String"), "Expected type signature with 'String'; got: {md}");
    assert!(md.contains("```"), "Expected fenced code block for signature; got: {md}");
}

#[test]
fn hover_function_shows_haddock_doc() {
    // `greet` at line 5, char 0 — should include its Haddock doc text
    let source = include_str!("fixtures/hover_test.hs");
    let tree = parse(source);
    let uri = Url::parse("file:///test/hover_test.hs").unwrap();
    let index = WorkspaceIndex::new();

    let pos = Position { line: 5, character: 0 };
    let result = hover::hover(&tree, source, &uri, pos, &index);

    let h = result.expect("Expected hover for 'greet'");
    let md = match_markdown(h);
    assert!(
        md.contains("documented greeter"),
        "Expected Haddock doc text; got: {md}"
    );
    assert!(md.contains("---"), "Expected doc separator; got: {md}");
}

#[test]
fn hover_data_type_shows_data_label() {
    // `Color` at line 21, char 5 — should show `data Color`
    let source = include_str!("fixtures/hover_test.hs");
    let tree = parse(source);
    let uri = Url::parse("file:///test/hover_test.hs").unwrap();
    let index = WorkspaceIndex::new();

    let pos = Position { line: 21, character: 5 };
    let result = hover::hover(&tree, source, &uri, pos, &index);

    let h = result.expect("Expected hover for 'Color'");
    let md = match_markdown(h);
    assert!(md.contains("Color"), "Expected 'Color' in hover; got: {md}");
    assert!(md.contains("data"), "Expected 'data' label; got: {md}");
}

#[test]
fn hover_no_doc_no_separator() {
    // `undocumented` at line 25, char 0 — no Haddock, no `---` separator
    let source = include_str!("fixtures/hover_test.hs");
    let tree = parse(source);
    let uri = Url::parse("file:///test/hover_test.hs").unwrap();
    let index = WorkspaceIndex::new();

    let pos = Position { line: 25, character: 0 };
    let result = hover::hover(&tree, source, &uri, pos, &index);

    let h = result.expect("Expected hover for 'undocumented'");
    let md = match_markdown(h);
    assert!(md.contains("undocumented"), "Expected function name; got: {md}");
    assert!(!md.contains("---"), "Unexpected doc separator for undocumented function; got: {md}");
}

#[test]
fn hover_haddock_inline_code_rendered() {
    // `addOne` at line 18, char 0 — doc contains @inline code@
    let source = include_str!("fixtures/hover_test.hs");
    let tree = parse(source);
    let uri = Url::parse("file:///test/hover_test.hs").unwrap();
    let index = WorkspaceIndex::new();

    let pos = Position { line: 18, character: 0 };
    let result = hover::hover(&tree, source, &uri, pos, &index);

    let h = result.expect("Expected hover for 'addOne'");
    let md = match_markdown(h);
    // @inline code@ should be rendered as `inline code`, not kept as @...@
    assert!(!md.contains("@inline"), "Raw @...@ should be rendered; got: {md}");
    assert!(md.contains("`inline code`"), "Expected backtick-wrapped code; got: {md}");
}

#[test]
fn hover_haddock_bird_tracks_rendered() {
    // `countItems` at line 14, char 0 — doc has bird-track code lines
    let source = include_str!("fixtures/hover_test.hs");
    let tree = parse(source);
    let uri = Url::parse("file:///test/hover_test.hs").unwrap();
    let index = WorkspaceIndex::new();

    let pos = Position { line: 14, character: 0 };
    let result = hover::hover(&tree, source, &uri, pos, &index);

    let h = result.expect("Expected hover for 'countItems'");
    let md = match_markdown(h);
    // Bird-track `> code` lines should produce a fenced code block
    assert!(md.contains("```"), "Expected fenced code block from bird tracks; got: {md}");
}

#[test]
fn hover_cross_file_with_doc() {
    // `greet` used in usage.hs — hover should show doc from simple_module.hs
    let simple_src = include_str!("fixtures/simple_module.hs");
    let simple_tree = parse(simple_src);
    let simple_uri = Url::parse("file:///test/simple_module.hs").unwrap();

    let usage_src = include_str!("fixtures/usage.hs");
    let usage_tree = parse(usage_src);
    let usage_uri = Url::parse("file:///test/usage.hs").unwrap();

    let index = WorkspaceIndex::new();
    index.update_file(
        &simple_uri,
        extract::workspace_symbols(&simple_tree, simple_src, &simple_uri),
    );

    // `greet` at line 5, char 21 in usage.hs
    let pos = Position { line: 5, character: 21 };
    let result = hover::hover(&usage_tree, usage_src, &usage_uri, pos, &index);

    let h = result.expect("Expected cross-file hover for 'greet'");
    let md = match_markdown(h);
    assert!(md.contains("greet"), "Expected function name; got: {md}");
    assert!(md.contains("greeting"), "Expected doc text; got: {md}");
}

// ── Workspace Symbols ─────────────────────────────────────────────────────────

#[test]
fn workspace_symbol_search() {
    let source = include_str!("fixtures/simple_module.hs");
    let tree = parse(source);
    let uri = Url::parse("file:///test/simple_module.hs").unwrap();

    let index = WorkspaceIndex::new();
    index.update_file(&uri, extract::workspace_symbols(&tree, source, &uri));

    let results = index.search("gree");
    assert!(
        results.iter().any(|s| s.name == "greet"),
        "Expected 'greet' in search for 'gree'; got: {:?}",
        results.iter().map(|s| &s.name).collect::<Vec<_>>()
    );
}

#[test]
fn workspace_symbols_include_data_types() {
    let source = include_str!("fixtures/simple_module.hs");
    let tree = parse(source);
    let uri = Url::parse("file:///test/simple_module.hs").unwrap();

    let index = WorkspaceIndex::new();
    let syms = extract::workspace_symbols(&tree, source, &uri);
    index.update_file(&uri, syms.clone());

    let names: Vec<&str> = syms.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"Animal"), "Missing data type 'Animal'; got: {:?}", names);

    let animal = syms.iter().find(|s| s.name == "Animal").unwrap();
    assert_eq!(animal.kind, SymbolKind::STRUCT, "Animal should be SymbolKind::STRUCT");
}

#[test]
fn workspace_symbols_dedup_multi_equation() {
    // `fib` has 3 equations — workspace index should have exactly one entry for it.
    let source = include_str!("fixtures/simple_module.hs");
    let tree = parse(source);
    let uri = Url::parse("file:///test/simple_module.hs").unwrap();
    let syms = extract::workspace_symbols(&tree, source, &uri);

    let fib_count = syms.iter().filter(|s| s.name == "fib").count();
    assert_eq!(fib_count, 1, "fib should appear once in workspace index, got {fib_count}");
}

// ── Haddock extraction unit tests ─────────────────────────────────────────────

#[test]
fn haddock_forward_doc_extracted() {
    // `greet` in simple_module.hs has a `-- |` comment above it.
    let source = include_str!("fixtures/simple_module.hs");
    let tree = parse(source);
    let uri = Url::parse("file:///test/simple_module.hs").unwrap();
    let syms = extract::workspace_symbols(&tree, source, &uri);

    let greet = syms.iter().find(|s| s.name == "greet").expect("greet not found");
    let doc = greet.doc_comment.as_deref().expect("greet should have a doc comment");
    assert!(
        doc.contains("greeting"),
        "Expected doc comment text; got: {doc}"
    );
}

#[test]
fn no_haddock_for_regular_comment() {
    // `undocumented` in hover_test.hs has a plain `--` comment (no `|`).
    let source = include_str!("fixtures/hover_test.hs");
    let tree = parse(source);
    let uri = Url::parse("file:///test/hover_test.hs").unwrap();
    let syms = extract::workspace_symbols(&tree, source, &uri);

    let sym = syms.iter().find(|s| s.name == "undocumented").expect("undocumented not found");
    assert!(
        sym.doc_comment.is_none(),
        "Regular comment should NOT be extracted as Haddock; got: {:?}",
        sym.doc_comment
    );
}

// ── Helper ────────────────────────────────────────────────────────────────────

fn match_markdown(h: Hover) -> String {
    match h.contents {
        HoverContents::Markup(mc) => mc.value,
        _ => panic!("Expected MarkupContent hover"),
    }
}
