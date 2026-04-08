use tower_lsp::lsp_types::{Location, SymbolInformation};

use crate::symbols::index::WorkspaceIndex;

pub fn workspace_symbols(index: &WorkspaceIndex, query: &str) -> Vec<SymbolInformation> {
    index
        .search(query)
        .into_iter()
        .map(|info| {
            #[allow(deprecated)]
            SymbolInformation {
                name: info.name,
                kind: info.kind,
                location: Location {
                    uri: info.uri,
                    range: info.range,
                },
                container_name: info.container_name,
                deprecated: None,
                tags: None,
            }
        })
        .collect()
}
