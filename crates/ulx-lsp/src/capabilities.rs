//! `initialize`'s advertised `ServerCapabilities` — split into its own
//! module since it's a big, mostly-declarative literal that would
//! otherwise clutter `backend.rs`'s actual request handling.

use tower_lsp::lsp_types::{
    CompletionOptions, HoverProviderCapability, OneOf, ServerCapabilities,
    TextDocumentSyncCapability, TextDocumentSyncKind,
};

/// Full-document sync (not incremental): every `didChange` carries the
/// whole new text. Simpler than tracking incremental edits, and cheap
/// enough for the small scripts this language targets.
pub fn server_capabilities() -> ServerCapabilities {
    ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        definition_provider: Some(OneOf::Left(true)),
        document_symbol_provider: Some(OneOf::Left(true)),
        completion_provider: Some(CompletionOptions::default()),
        ..ServerCapabilities::default()
    }
}
