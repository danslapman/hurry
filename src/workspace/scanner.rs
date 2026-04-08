use std::path::{Path, PathBuf};
use std::sync::Arc;

use tower_lsp::lsp_types::Url;
use tree_sitter::Parser;

use crate::symbols::{extract, index::WorkspaceIndex};

/// Scan all .hs source files under `root` (and any `extra_paths`) and
/// populate the workspace index.  Trees are NOT retained after scanning.
pub async fn scan_workspace(root: Url, extra_paths: Vec<PathBuf>, index: Arc<WorkspaceIndex>) {
    let root_path = match root.to_file_path() {
        Ok(p) => p,
        Err(_) => return,
    };

    let mut all_paths = extra_paths;
    all_paths.insert(0, root_path);

    let files = tokio::task::spawn_blocking(move || {
        let mut files = Vec::new();
        for dir in &all_paths {
            collect_source_files(dir, &mut files);
        }
        files
    })
    .await
    .unwrap_or_default();

    let semaphore = Arc::new(tokio::sync::Semaphore::new(64));
    let mut handles = Vec::new();

    for file_path in files {
        let index = index.clone();
        let permit = semaphore.clone().acquire_owned().await.unwrap();

        let handle = tokio::spawn(async move {
            let _permit = permit;
            index_file(&file_path, &index).await;
        });
        handles.push(handle);
    }

    for handle in handles {
        let _ = handle.await;
    }
}

pub(crate) async fn index_file(path: &Path, index: &WorkspaceIndex) {
    let text = match tokio::fs::read_to_string(path).await {
        Ok(t) => t,
        Err(_) => return,
    };
    let uri = match Url::from_file_path(path) {
        Ok(u) => u,
        Err(_) => return,
    };

    let symbols = tokio::task::spawn_blocking({
        let text = text.clone();
        let uri = uri.clone();
        move || {
            let mut parser = create_parser()?;
            let tree = parser.parse(text.as_bytes(), None)?;
            Some(extract::workspace_symbols(&tree, &text, &uri))
        }
    })
    .await
    .unwrap_or(None)
    .unwrap_or_default();

    index.update_file(&uri, symbols);
}

fn collect_source_files(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            // Skip hidden dirs, build output dirs, and common non-source dirs
            if name.starts_with('.')
                || name == "target"
                || name == "dist-newstyle"
                || name == ".stack-work"
                || name == "node_modules"
            {
                continue;
            }
            collect_source_files(&path, out);
        } else if is_haskell_source(&path) {
            out.push(path);
        }
    }
}

fn is_haskell_source(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("hs") | Some("lhs")
    )
}

/// Create a Haskell parser (used by the backend for open document handling).
pub fn create_parser() -> Option<Parser> {
    let mut parser = Parser::new();
    let language: tree_sitter::Language = tree_sitter_haskell::LANGUAGE.into();
    parser.set_language(&language).ok()?;
    Some(parser)
}
