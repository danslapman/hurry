use dashmap::DashMap;
use tower_lsp::lsp_types::Url;

use crate::symbols::types::SymbolInfo;

/// Concurrent dual-index symbol store.
/// - `by_name`: O(1) lookup by exact name
/// - `by_file`: O(1) bulk replacement when a file changes
pub struct WorkspaceIndex {
    by_name: DashMap<String, Vec<SymbolInfo>>,
    by_file: DashMap<Url, Vec<SymbolInfo>>,
}

impl WorkspaceIndex {
    pub fn new() -> Self {
        Self {
            by_name: DashMap::new(),
            by_file: DashMap::new(),
        }
    }

    /// Replace all symbols for a file.
    pub fn update_file(&self, uri: &Url, symbols: Vec<SymbolInfo>) {
        // Remove old entries for this file from by_name
        if let Some(old) = self.by_file.get(uri) {
            for sym in old.iter() {
                if let Some(mut entry) = self.by_name.get_mut(&sym.name) {
                    entry.retain(|s| &s.uri != uri);
                }
            }
        }

        // Insert new entries
        for sym in &symbols {
            self.by_name
                .entry(sym.name.clone())
                .or_default()
                .push(sym.clone());
        }
        self.by_file.insert(uri.clone(), symbols);
    }

    /// Remove all symbols for a file (e.g., file deleted).
    pub fn remove_file(&self, uri: &Url) {
        if let Some((_, old)) = self.by_file.remove(uri) {
            for sym in old {
                if let Some(mut entry) = self.by_name.get_mut(&sym.name) {
                    entry.retain(|s| &s.uri != uri);
                }
            }
        }
    }

    /// Look up symbols by exact name.
    pub fn lookup_by_name(&self, name: &str) -> Vec<SymbolInfo> {
        self.by_name
            .get(name)
            .map(|v| v.clone())
            .unwrap_or_default()
    }

    /// Search symbols by prefix (case-insensitive) for workspace/symbol.
    pub fn search(&self, query: &str) -> Vec<SymbolInfo> {
        if query.is_empty() {
            return self
                .by_file
                .iter()
                .flat_map(|e| e.value().clone())
                .collect();
        }
        let query_lower = query.to_lowercase();
        self.by_name
            .iter()
            .filter(|e| e.key().to_lowercase().contains(&query_lower))
            .flat_map(|e| e.value().clone())
            .collect()
    }

    /// Get all symbols for a given file.
    pub fn symbols_for_file(&self, uri: &Url) -> Vec<SymbolInfo> {
        self.by_file
            .get(uri)
            .map(|v| v.clone())
            .unwrap_or_default()
    }

    /// Iterate over all indexed file URIs.
    pub fn all_uris(&self) -> Vec<Url> {
        self.by_file.iter().map(|e| e.key().clone()).collect()
    }
}

impl Default for WorkspaceIndex {
    fn default() -> Self {
        Self::new()
    }
}
