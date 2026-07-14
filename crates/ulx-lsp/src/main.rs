//! `ulx-lsp` (§20.2): the standard Language Server Protocol implementation
//! for Ulexite, speaking LSP over stdio — the transport every major
//! editor's client expects (VS Code's `vscode-languageclient`, Neovim's
//! built-in LSP client, etc.). See `docs/spec/20-ide-integration.md` for
//! the full aspirational scope; `src/lib.rs`'s module docs and the crate's
//! implementation plan note what's built now versus deferred.

use tower_lsp::{LspService, Server};
use ulx_lsp::backend::Backend;

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(Backend::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}
