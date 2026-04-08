# hurry — Best-Effort Haskell LSP Server

## Description

hurry is a fast, zero-configuration Haskell language server built in Rust on top of tree-sitter-haskell. It requires no GHC integration and no build tool, starts instantly, and targets the sweet spot between a plain text search and a full-featured server like HLS.

hurry was primarily created to be used with [Fresh](https://getfresh.dev), an open-source terminal text editor with LSP support.

## Features

### Tree-sitter (solid)
1. `textDocument/documentSymbol` — hierarchical: functions, data types, newtypes, type classes, instances, type synonyms, type families
2. `workspace/symbol` — search all top-level definitions across the workspace
3. `textDocument/foldingRange` — `where` blocks, `class`/`instance` bodies, `do` blocks, import groups, multi-line comments
4. `textDocument/selectionRange` — expand/shrink selection by AST node
5. `textDocument/semanticTokens/full` — syntax-aware highlighting
6. `textDocument/publishDiagnostics` — parse/syntax errors only
7. `textDocument/documentLink` — URLs in comments and strings

### Heuristic-based
1. `textDocument/definition` — same-file scope walk first (`where`, `let`, `do`, `case`, `lambda`), cross-file via workspace symbol index fallback
2. `textDocument/references` — same-file tree walk + cross-file pre-filter then parse
3. `textDocument/hover` — type signature (`foo :: Int -> Int`) in a Haskell code fence; Haddock markup rendered to Markdown (`-- |`, `-- ^`, `@code@`, `/italic/`, `__bold__`, `'identifier'`, bird-track code blocks)

## Dependency Source Unpacking

hurry can fetch and index Haskell sources for all transitive Stack dependencies so that go-to-definition and workspace symbol search work across library code.

**Prerequisites:** `stack` on your PATH and a `stack.yaml` at the workspace root.

**How it works:**
1. Runs `stack ls dependencies --filter '$locals'` to enumerate all transitive dependencies.
2. Unpacks source tarballs into `<workspace>/.dep-srcs/` via `stack unpack`.
3. Indexes all `.hs`/`.lhs` files found in the unpacked directories.

Unpacking is incremental — already-resolved packages are recorded in `.dep-srcs/.resolved.list` and skipped on subsequent runs; they are re-indexed from disk without a network round-trip.

You may want to add `.dep-srcs/` to your `.gitignore`.

**Enable via CLI flag:**
```
hurry --fetch-dep-sources
```

**Enable via LSP initialization options:**
```json
{
  "initializationOptions": { "retrieveSrc": true }
}
```
When set, the fetch runs in the background after the server initializes, in parallel with the regular workspace scan.

To add further search paths manually, pass `--extra-path`:

```
hurry --extra-path /path/to/additional/sources
```

Multiple `--extra-path` flags are supported.

## Installation

```
cargo install --path .
```

## Limitations

hurry is **best-effort** and does not implement type inference, renaming, code actions, completions, or any feature that requires running GHC. For full IDE support, use [HLS](https://haskell-language-server.readthedocs.io/). hurry is designed for situations where HLS is unavailable, too slow, or simply more than you need.

- Go-to-definition is heuristic: it matches by name, not by type. Overloaded names will produce multiple results.
- Hover shows the type signature only if it appears as a standalone `::` declaration immediately before the function definition in the same file.
- Qualified names (e.g. `Data.Map.lookup`) are resolved by the unqualified part (`lookup`), which may return unrelated definitions.
