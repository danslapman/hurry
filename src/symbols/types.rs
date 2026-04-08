use tower_lsp::lsp_types::{Range, SymbolKind, Url};

#[derive(Debug, Clone)]
pub struct SymbolInfo {
    pub name: String,
    pub kind: SymbolKind,
    pub uri: Url,
    pub range: Range,
    pub selection_range: Range,
    pub container_name: Option<String>,
    pub doc_comment: Option<String>,
    /// The Haskell type signature, e.g. `map :: (a -> b) -> [a] -> [b]`.
    /// Populated during extraction when a `signature` node precedes the `function`.
    pub type_signature: Option<String>,
}

impl SymbolInfo {
    pub fn new(
        name: impl Into<String>,
        kind: SymbolKind,
        uri: Url,
        range: Range,
        selection_range: Range,
    ) -> Self {
        Self {
            name: name.into(),
            kind,
            uri,
            range,
            selection_range,
            container_name: None,
            doc_comment: None,
            type_signature: None,
        }
    }

    pub fn with_container(mut self, container: impl Into<String>) -> Self {
        self.container_name = Some(container.into());
        self
    }

    pub fn with_doc(mut self, doc: impl Into<String>) -> Self {
        self.doc_comment = Some(doc.into());
        self
    }

    pub fn with_signature(mut self, sig: impl Into<String>) -> Self {
        self.type_signature = Some(sig.into());
        self
    }
}
