use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use dashmap::DashMap;
use tower_lsp::{
    jsonrpc::Result,
    lsp_types::*,
    Client, LanguageServer,
};
use tree_sitter::Tree;

use crate::{
    capabilities,
    handlers::{
        definition, diagnostics, document_links, document_symbols,
        folding, hover, references, selection, semantic_tokens, workspace_symbols,
    },
    parsing::document::Document,
    progress,
    symbols::{extract, index::WorkspaceIndex},
    workspace::scanner::{create_parser, scan_workspace},
};

pub struct Backend {
    client: Client,
    /// tree-sitter Parser wrapped in Mutex (not Sync on its own).
    parser: Mutex<tree_sitter::Parser>,
    /// Open document cache.
    documents: DashMap<Url, Document>,
    /// Workspace-wide symbol index shared with the scanner task.
    index: Arc<WorkspaceIndex>,
    /// Workspace root URI, set during initialize.
    workspace_root: tokio::sync::RwLock<Option<Url>>,
    /// Extra search paths supplied via CLI --extra-path or init options.
    extra_paths: Vec<PathBuf>,
    /// Whether to unpack and index dependency sources on startup.
    retrieve_src: AtomicBool,
}

impl Backend {
    pub fn new(client: Client, extra_paths: Vec<PathBuf>, fetch_dep_sources: bool) -> Self {
        let parser = create_parser().expect("Failed to create Haskell parser");
        Self {
            client,
            parser: Mutex::new(parser),
            documents: DashMap::new(),
            index: Arc::new(WorkspaceIndex::new()),
            workspace_root: tokio::sync::RwLock::new(None),
            extra_paths,
            retrieve_src: AtomicBool::new(fetch_dep_sources),
        }
    }

    fn reindex(&self, uri: &Url, tree: &Tree, text: &str) {
        let symbols = extract::workspace_symbols(tree, text, uri);
        self.index.update_file(uri, symbols);
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        let root = params
            .root_uri
            .or_else(|| {
                params
                    .workspace_folders
                    .as_ref()
                    .and_then(|f| f.first())
                    .map(|f| f.uri.clone())
            });
        *self.workspace_root.write().await = root;

        if let Some(opts) = &params.initialization_options {
            if opts.get("retrieveSrc").and_then(|v| v.as_bool()) == Some(true) {
                self.retrieve_src.store(true, Ordering::Relaxed);
            }
        }

        Ok(InitializeResult {
            server_info: Some(ServerInfo {
                name: "hurry".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
            capabilities: capabilities::server_capabilities(),
            ..Default::default()
        })
    }

    async fn initialized(&self, _params: InitializedParams) {
        if let Some(root) = self.workspace_root.read().await.clone() {
            let index = self.index.clone();
            let extra_paths = self.extra_paths.clone();
            let root_for_scan = root.clone();
            let client = self.client.clone();
            tokio::spawn(async move {
                progress::begin(Some(&client), "hurry/scan", "hurry", Some("Scanning workspace...".into())).await;
                scan_workspace(root_for_scan, extra_paths, index).await;
                progress::end(Some(&client), "hurry/scan", None).await;
            });

            if self.retrieve_src.load(Ordering::Relaxed) {
                if let Ok(root_path) = root.to_file_path() {
                    let index = self.index.clone();
                    let client = self.client.clone();
                    tokio::spawn(async move {
                        crate::workspace::deps::unpack_dep_sources(&root_path, index, Some(&client)).await;
                    });
                }
            }
        }
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        let version = params.text_document.version;
        let text = params.text_document.text;

        let doc = {
            let mut parser = self.parser.lock().unwrap();
            Document::new(uri.clone(), version, text, &mut parser)
        };

        let diags = diagnostics::get_diagnostics(&doc.tree, &doc.text);
        self.reindex(&uri, &doc.tree, &doc.text);
        self.documents.insert(uri.clone(), doc);
        self.client.publish_diagnostics(uri, diags, None).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        let version = params.text_document.version;

        let mut doc_ref = match self.documents.get_mut(&uri) {
            Some(d) => d,
            None => return,
        };

        {
            let mut parser = self.parser.lock().unwrap();
            doc_ref.apply_changes(version, params.content_changes, &mut parser);
        }

        let (tree_clone, text_clone) = (doc_ref.tree.clone(), doc_ref.text.clone());
        drop(doc_ref);

        self.reindex(&uri, &tree_clone, &text_clone);
        let diags = diagnostics::get_diagnostics(&tree_clone, &text_clone);
        self.client.publish_diagnostics(uri, diags, None).await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        self.documents.remove(&uri);
        // Keep index entries — file still exists on disk
        self.client.publish_diagnostics(uri, vec![], None).await;
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let uri = &params.text_document.uri;
        let Some(doc) = self.documents.get(uri) else {
            return Ok(None);
        };
        let symbols = document_symbols::document_symbols(&doc.tree, &doc.text);
        Ok(Some(DocumentSymbolResponse::Nested(symbols)))
    }

    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<Vec<SymbolInformation>>> {
        let results = workspace_symbols::workspace_symbols(&self.index, &params.query);
        Ok(Some(results))
    }

    async fn folding_range(
        &self,
        params: FoldingRangeParams,
    ) -> Result<Option<Vec<FoldingRange>>> {
        let uri = &params.text_document.uri;
        let Some(doc) = self.documents.get(uri) else {
            return Ok(None);
        };
        let ranges = folding::folding_ranges(&doc.tree, &doc.text);
        Ok(Some(ranges))
    }

    async fn selection_range(
        &self,
        params: SelectionRangeParams,
    ) -> Result<Option<Vec<SelectionRange>>> {
        let uri = &params.text_document.uri;
        let Some(doc) = self.documents.get(uri) else {
            return Ok(None);
        };
        let ranges = selection::selection_ranges(&doc.tree, params.positions);
        Ok(Some(ranges))
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        let uri = &params.text_document.uri;
        let Some(doc) = self.documents.get(uri) else {
            return Ok(None);
        };
        let tokens = semantic_tokens::semantic_tokens_full(&doc.tree, &doc.text);
        Ok(Some(SemanticTokensResult::Tokens(tokens)))
    }

    async fn document_link(
        &self,
        params: DocumentLinkParams,
    ) -> Result<Option<Vec<DocumentLink>>> {
        let uri = &params.text_document.uri;
        let Some(doc) = self.documents.get(uri) else {
            return Ok(None);
        };
        let links = document_links::document_links(&doc.tree, &doc.text);
        Ok(Some(links))
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let Some(doc) = self.documents.get(uri) else {
            return Ok(None);
        };
        Ok(hover::hover(&doc.tree, &doc.text, uri, pos, &self.index))
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let Some(doc) = self.documents.get(uri) else {
            return Ok(None);
        };
        let result = definition::goto_definition(&doc.tree, &doc.text, uri, pos, &self.index);
        Ok(result)
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let uri = &params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;
        let context = params.context;

        let (text, tree) = {
            let Some(doc) = self.documents.get(uri) else {
                return Ok(None);
            };
            (doc.text.clone(), doc.tree.clone())
        };

        let documents = &self.documents;
        let get_file = |file_uri: &Url| -> Option<(String, Tree)> {
            // Fast path: open document
            if let Some(d) = documents.get(file_uri) {
                return Some((d.text.clone(), d.tree.clone()));
            }
            // Slow path: read from disk
            let path = file_uri.to_file_path().ok()?;
            let file_text = std::fs::read_to_string(&path).ok()?;
            let mut parser = create_parser()?;
            let file_tree = parser.parse(file_text.as_bytes(), None)?;
            Some((file_text, file_tree))
        };

        let locs = references::find_references(
            &tree, &text, uri, pos, context, &self.index, &get_file,
        );
        Ok(Some(locs))
    }
}
