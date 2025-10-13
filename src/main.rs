mod lsp;

use lsp::Backend;
use std::collections::HashMap;
use std::sync::RwLock;
use tower_lsp::{LspService, Server};

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(|client| Backend {
        client,
        documents: RwLock::new(HashMap::new()),
        last_tokens: RwLock::new(HashMap::new()),
        reference_table: RwLock::new(HashMap::new()),
        variable_maps: RwLock::new(HashMap::new()),
    });
    Server::new(stdin, stdout, socket).serve(service).await;
}
