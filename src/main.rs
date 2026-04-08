use std::path::PathBuf;

use hurry::backend;
use simple_logger::SimpleLogger;
use tower_lsp::{LspService, Server};

#[tokio::main]
async fn main() {
    SimpleLogger::new()
        .env()
        .with_level(log::LevelFilter::Debug)
        .with_utc_timestamps()
        .init()
        .unwrap();

    let args: Vec<String> = std::env::args().collect();

    // Collect any explicit extra paths from --extra-path <dir> arguments
    let extra_paths: Vec<PathBuf> = args
        .windows(2)
        .filter(|w| w[0] == "--extra-path")
        .map(|w| PathBuf::from(&w[1]))
        .collect();

    let fetch_dep_sources = args.iter().any(|a| a == "--fetch-dep-sources");

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(move |client| {
        backend::Backend::new(client, extra_paths.clone(), fetch_dep_sources)
    });
    Server::new(stdin, stdout, socket).serve(service).await;
}
