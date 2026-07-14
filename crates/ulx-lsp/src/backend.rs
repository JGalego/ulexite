//! The `tower_lsp::LanguageServer` implementation: owns the open-document
//! store (one `LineIndex` per open `Url`, doubling as both the buffer's
//! text and its offset↔position table — see `line_index.rs`) and wires
//! requests through `analysis.rs` (diagnostics) and `index.rs` (hover/
//! goto-definition/completion data).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tokio::sync::Mutex;
use tower_lsp::jsonrpc::Result as RpcResult;
use tower_lsp::lsp_types::{
    CompletionItem, CompletionItemKind, CompletionParams, CompletionResponse,
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DidSaveTextDocumentParams, Documentation, DocumentSymbol, DocumentSymbolParams,
    DocumentSymbolResponse, GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverContents,
    HoverParams, InitializeParams, InitializeResult, InitializedParams, Location, MarkupContent,
    MarkupKind, MessageType, ServerInfo, SymbolKind, Url,
};
use tower_lsp::{async_trait, Client, LanguageServer};
use ulx_ast::ArtifactType;

use crate::analysis;
use crate::capabilities;
use crate::index::{DeclEntry, DeclKind, Index, RefTarget};
use crate::line_index::LineIndex;

pub struct Backend {
    client: Client,
    docs: Mutex<HashMap<Url, LineIndex>>,
}

/// A name resolved to its declaring file — `path`/`text` are the *target*
/// file's (which is the currently-open file for a local reference, or an
/// imported file read fresh from disk for a cross-file one), not
/// necessarily the file the hover/goto-definition request was made from.
struct Resolved {
    path: PathBuf,
    text: String,
    entry: DeclEntry,
}

impl Backend {
    pub fn new(client: Client) -> Self {
        Backend {
            client,
            docs: Mutex::new(HashMap::new()),
        }
    }

    /// Fast path: re-parses/re-checks the in-memory buffer only, updates
    /// the doc store, and publishes whatever diagnostics that yields.
    async fn on_change(&self, uri: Url, text: String) {
        let index = LineIndex::new(text);
        let (_program, diags) = analysis::analyze_buffer(&index);
        self.docs.lock().await.insert(uri.clone(), index);
        self.client.publish_diagnostics(uri, diags, None).await;
    }

    /// Full path: re-analyzes `uri`'s whole workspace from disk (imports
    /// included) and republishes diagnostics for every module touched.
    /// A no-op for documents with no real on-disk path (e.g. `untitled:`
    /// buffers) or that fail to read/parse.
    async fn full_reanalysis(&self, uri: &Url) {
        let Ok(path) = uri.to_file_path() else {
            return;
        };
        let Some(results) = analysis::analyze_workspace(&path) else {
            return;
        };
        for (url, diags) in results {
            self.client.publish_diagnostics(url, diags, None).await;
        }
    }

    /// Resolves `name` to its declaring file: first against `index`'s own
    /// (already-parsed) `decls`, falling back to reading+parsing an
    /// imported file via `index.import_sources` if it isn't declared
    /// locally.
    async fn resolve_name(
        &self,
        name: &str,
        index: &Index,
        current_path: &Path,
        current_text: &str,
    ) -> Option<Resolved> {
        if let Some(entry) = index.decls.get(name) {
            return Some(Resolved {
                path: current_path.to_path_buf(),
                text: current_text.to_string(),
                entry: entry.clone(),
            });
        }
        let from = index.import_sources.get(name)?;
        let target_path = current_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(from);
        let text = tokio::fs::read_to_string(&target_path).await.ok()?;
        let program = ulx_syntax::parse_source(&text).ok()?;
        let target_index = Index::build(&program);
        let entry = target_index.decls.get(name)?.clone();
        Some(Resolved {
            path: target_path,
            text,
            entry,
        })
    }

    async fn hover_value(
        &self,
        target: &RefTarget,
        index: &Index,
        current_path: Option<&Path>,
        current_text: &str,
    ) -> Option<String> {
        match target {
            RefTarget::Name(name) => {
                let resolved = match current_path {
                    Some(p) => self.resolve_name(name, index, p, current_text).await,
                    None => index.decls.get(name).cloned().map(|entry| Resolved {
                        path: PathBuf::new(),
                        text: current_text.to_string(),
                        entry,
                    }),
                }?;
                let mut md = format!("```ulexite\n{}\n```", resolved.entry.signature);
                if let Some(doc) = &resolved.entry.doc {
                    md.push_str(&format!("\n\n{doc}"));
                }
                Some(md)
            }
            RefTarget::Capability(name) => {
                let caps = ulx_sema::stdlib_capabilities();
                let cap = caps.iter().find(|c| c.name == name)?;
                Some(format!(
                    "```ulexite\ncapability {}\n```\naccepts: {}\n\nproduces: {}",
                    cap.name,
                    artifact_list(&cap.accepts),
                    artifact_list(&cap.produces),
                ))
            }
            RefTarget::ArtifactType(a) => Some(format!("artifact type `{}`", artifact_keyword(*a))),
        }
    }
}

fn artifact_keyword(a: ArtifactType) -> &'static str {
    ArtifactType::ALL
        .iter()
        .find(|(_, ty)| *ty == a)
        .map(|(kw, _)| *kw)
        .unwrap_or("text")
}

fn artifact_list(types: &[ArtifactType]) -> String {
    types
        .iter()
        .map(|t| artifact_keyword(*t))
        .collect::<Vec<_>>()
        .join(", ")
}

fn decl_kind_to_symbol_kind(kind: DeclKind) -> SymbolKind {
    match kind {
        DeclKind::Conversation => SymbolKind::FUNCTION,
        DeclKind::Judge | DeclKind::Validator => SymbolKind::INTERFACE,
        DeclKind::Dataset => SymbolKind::ARRAY,
        DeclKind::Type => SymbolKind::STRUCT,
        DeclKind::Benchmark => SymbolKind::CLASS,
        DeclKind::Provider => SymbolKind::OBJECT,
    }
}

fn decl_kind_to_completion_kind(kind: DeclKind) -> CompletionItemKind {
    match kind {
        DeclKind::Conversation => CompletionItemKind::FUNCTION,
        DeclKind::Judge | DeclKind::Validator => CompletionItemKind::INTERFACE,
        DeclKind::Dataset => CompletionItemKind::VARIABLE,
        DeclKind::Type => CompletionItemKind::STRUCT,
        DeclKind::Benchmark => CompletionItemKind::CLASS,
        DeclKind::Provider => CompletionItemKind::MODULE,
    }
}

/// Statement/expression keywords with no other source of truth to reuse —
/// the lexer recognizes keywords contextually over plain `Ident` tokens
/// (see `ulx-syntax/src/lexer.rs`'s module doc), so there's no existing
/// keyword-list constant anywhere in the compiler. Mirrors
/// `docs/spec/08-grammar.md`.
const KEYWORDS: &[&str] = &[
    "conversation",
    "judge",
    "validator",
    "dataset",
    "type",
    "benchmark",
    "provider",
    "import",
    "system",
    "user",
    "assistant",
    "ask",
    "match",
    "with",
    "for",
    "while",
    "retry",
    "escalate",
    "break",
    "if",
    "else",
];

#[async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> RpcResult<InitializeResult> {
        Ok(InitializeResult {
            capabilities: capabilities::server_capabilities(),
            server_info: Some(ServerInfo {
                name: "ulx-lsp".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "ulx-lsp initialized")
            .await;
    }

    async fn shutdown(&self) -> RpcResult<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        self.on_change(uri.clone(), params.text_document.text).await;
        self.full_reanalysis(&uri).await;
    }

    async fn did_change(&self, mut params: DidChangeTextDocumentParams) {
        // TextDocumentSyncKind::FULL guarantees exactly one change event
        // carrying the complete new text.
        if let Some(change) = params.content_changes.pop() {
            self.on_change(params.text_document.uri, change.text).await;
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        self.full_reanalysis(&params.text_document.uri).await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.docs.lock().await.remove(&params.text_document.uri);
    }

    async fn hover(&self, params: HoverParams) -> RpcResult<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let (text, offset) = {
            let docs = self.docs.lock().await;
            let Some(line_index) = docs.get(&uri) else {
                return Ok(None);
            };
            (
                line_index.text().to_string(),
                line_index.position_to_offset(position),
            )
        };

        let Ok(program) = ulx_syntax::parse_source(&text) else {
            return Ok(None);
        };
        let index = Index::build(&program);
        let Some(target) = index.lookup(offset) else {
            return Ok(None);
        };
        let current_path = uri.to_file_path().ok();
        let Some(value) = self
            .hover_value(target, &index, current_path.as_deref(), &text)
            .await
        else {
            return Ok(None);
        };

        Ok(Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value,
            }),
            range: None,
        }))
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> RpcResult<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let (text, offset) = {
            let docs = self.docs.lock().await;
            let Some(line_index) = docs.get(&uri) else {
                return Ok(None);
            };
            (
                line_index.text().to_string(),
                line_index.position_to_offset(position),
            )
        };

        let Ok(program) = ulx_syntax::parse_source(&text) else {
            return Ok(None);
        };
        let index = Index::build(&program);
        let Some(RefTarget::Name(name)) = index.lookup(offset) else {
            return Ok(None);
        };

        let resolved = match uri.to_file_path().ok() {
            Some(current_path) => self.resolve_name(name, &index, &current_path, &text).await,
            None => index.decls.get(name).cloned().map(|entry| Resolved {
                path: PathBuf::new(),
                text: text.clone(),
                entry,
            }),
        };
        let Some(resolved) = resolved else {
            return Ok(None);
        };

        let target_url = Url::from_file_path(&resolved.path).unwrap_or_else(|_| uri.clone());
        let range = LineIndex::new(resolved.text).span_to_range(&resolved.entry.span);
        Ok(Some(GotoDefinitionResponse::Scalar(Location::new(
            target_url, range,
        ))))
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> RpcResult<Option<DocumentSymbolResponse>> {
        let uri = params.text_document.uri;
        let docs = self.docs.lock().await;
        let Some(line_index) = docs.get(&uri) else {
            return Ok(None);
        };
        let Ok(program) = ulx_syntax::parse_source(line_index.text()) else {
            return Ok(None);
        };
        let index = Index::build(&program);

        let mut symbols: Vec<DocumentSymbol> = index
            .decls
            .values()
            .map(|entry| {
                let range = line_index.span_to_range(&entry.span);
                #[allow(deprecated)]
                DocumentSymbol {
                    name: entry.name.clone(),
                    detail: Some(entry.signature.clone()),
                    kind: decl_kind_to_symbol_kind(entry.kind),
                    tags: None,
                    deprecated: None,
                    range,
                    selection_range: range,
                    children: None,
                }
            })
            .collect();
        symbols.sort_by_key(|s| (s.range.start.line, s.range.start.character));

        Ok(Some(DocumentSymbolResponse::Nested(symbols)))
    }

    async fn completion(&self, params: CompletionParams) -> RpcResult<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let docs = self.docs.lock().await;
        let Some(line_index) = docs.get(&uri) else {
            return Ok(None);
        };
        let program = ulx_syntax::parse_source(line_index.text()).ok();
        drop(docs);

        let mut items = Vec::new();

        if let Some(program) = &program {
            let index = Index::build(program);
            for entry in index.decls.values() {
                items.push(CompletionItem {
                    label: entry.name.clone(),
                    kind: Some(decl_kind_to_completion_kind(entry.kind)),
                    detail: Some(entry.signature.clone()),
                    documentation: entry.doc.clone().map(Documentation::String),
                    ..Default::default()
                });
            }
        }

        for cap in ulx_sema::stdlib_capabilities() {
            items.push(CompletionItem {
                label: cap.name.to_string(),
                kind: Some(CompletionItemKind::FUNCTION),
                detail: Some(format!(
                    "accepts: {} -> produces: {}",
                    artifact_list(&cap.accepts),
                    artifact_list(&cap.produces)
                )),
                ..Default::default()
            });
        }

        for module in ulx_sema::STDLIB_MODULES {
            items.push(CompletionItem {
                label: module.to_string(),
                kind: Some(CompletionItemKind::MODULE),
                ..Default::default()
            });
        }

        for (kw, _) in ArtifactType::ALL {
            items.push(CompletionItem {
                label: kw.to_string(),
                kind: Some(CompletionItemKind::TYPE_PARAMETER),
                ..Default::default()
            });
        }

        for kw in KEYWORDS {
            items.push(CompletionItem {
                label: kw.to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                ..Default::default()
            });
        }

        Ok(Some(CompletionResponse::Array(items)))
    }
}
