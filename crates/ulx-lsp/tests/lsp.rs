//! Integration tests exercising `Backend` directly. Its
//! `tower_lsp::LanguageServer` methods are plain async fns, so a real
//! editor or subprocess isn't needed: build an in-process `LspService`,
//! pull the `Backend` back out via `.inner()` (an existing `tower_lsp`
//! accessor, not something added for this), and drive `did_open`/`hover`/
//! `goto_definition`/`document_symbol`/`completion` the same way a client
//! would.
//!
//! `did_open`/`did_change`/`did_save` are notifications, not requests —
//! they report diagnostics by calling back into `Client::publish_
//! diagnostics` rather than returning them, and no real transport is
//! wired up to receive that here. So these tests assert on the *request*
//! methods' return values (hover/goto-definition/document-symbol/
//! completion), and only check that `did_open` on a broken file doesn't
//! panic — `analysis.rs`'s own unit tests already cover the diagnostic
//! content itself.

use std::path::{Path, PathBuf};

use tower_lsp::lsp_types::*;
use tower_lsp::{LanguageServer, LspService};
use ulx_lsp::backend::Backend;
use ulx_lsp::line_index::LineIndex;

fn examples_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/ulx-lsp is two levels under the repo root")
        .join("examples")
}

fn file_url(path: &Path) -> Url {
    Url::from_file_path(path).expect("test fixtures use absolute paths")
}

fn open_params(uri: Url, text: String) -> DidOpenTextDocumentParams {
    DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri,
            language_id: "ulexite".to_string(),
            version: 1,
            text,
        },
    }
}

fn position_of(text: &str, needle: &str) -> Position {
    let offset = text
        .find(needle)
        .unwrap_or_else(|| panic!("fixture must contain {needle:?}"));
    LineIndex::new(text.to_string()).offset_to_position(offset)
}

fn text_document_position(uri: Url, position: Position) -> TextDocumentPositionParams {
    TextDocumentPositionParams {
        text_document: TextDocumentIdentifier { uri },
        position,
    }
}

#[tokio::test]
async fn did_open_does_not_panic_on_a_broken_file() {
    let (service, _socket) = LspService::new(Backend::new);
    let backend = service.inner();

    let uri = Url::parse("file:///scratch/broken.ulx").unwrap();
    backend
        .did_open(open_params(uri, "conversation {\n".to_string()))
        .await;
}

#[tokio::test]
async fn hover_on_a_judge_reference_shows_its_signature() {
    let (service, _socket) = LspService::new(Backend::new);
    let backend = service.inner();

    let path = examples_dir().join("translate.ulx");
    let text = std::fs::read_to_string(&path).expect("fixture exists");
    let uri = file_url(&path);
    backend
        .did_open(open_params(uri.clone(), text.clone()))
        .await;

    // `match judge Fluency(draft)` — not the declaration site.
    let position = position_of(&text, "Fluency(draft)");
    let hover = backend
        .hover(HoverParams {
            text_document_position_params: text_document_position(uri, position),
            work_done_progress_params: Default::default(),
        })
        .await
        .expect("hover request should not error")
        .expect("hovering a judge reference should produce a Hover");

    let HoverContents::Markup(markup) = hover.contents else {
        panic!("expected Markdown hover contents");
    };
    assert!(
        markup.value.contains("judge Fluency"),
        "hover text was: {}",
        markup.value
    );
}

#[tokio::test]
async fn goto_definition_on_a_judge_reference_finds_its_declaration() {
    let (service, _socket) = LspService::new(Backend::new);
    let backend = service.inner();

    let path = examples_dir().join("translate.ulx");
    let text = std::fs::read_to_string(&path).expect("fixture exists");
    let uri = file_url(&path);
    backend
        .did_open(open_params(uri.clone(), text.clone()))
        .await;

    let position = position_of(&text, "Fluency(draft)");
    let response = backend
        .goto_definition(GotoDefinitionParams {
            text_document_position_params: text_document_position(uri.clone(), position),
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .expect("goto_definition request should not error")
        .expect("should resolve to the judge's own declaration");

    let GotoDefinitionResponse::Scalar(location) = response else {
        panic!("expected a single Location");
    };
    assert_eq!(location.uri, uri, "Fluency is declared in the same file");
    // The reported location is the `Fluency` name token itself (its own
    // precise `name_span`), not the whole `judge Fluency(...) { ... }`
    // declaration starting at the `judge` keyword — landing the cursor on
    // just the identifier is the whole point of a precise name span.
    let decl_offset = text.find("judge Fluency").unwrap() + "judge ".len();
    let decl_position = LineIndex::new(text.clone()).offset_to_position(decl_offset);
    assert_eq!(location.range.start, decl_position);
    let decl_end_offset = decl_offset + "Fluency".len();
    let decl_end_position = LineIndex::new(text.clone()).offset_to_position(decl_end_offset);
    assert_eq!(location.range.end, decl_end_position);
}

#[tokio::test]
async fn document_symbol_lists_every_top_level_decl() {
    let (service, _socket) = LspService::new(Backend::new);
    let backend = service.inner();

    let path = examples_dir().join("translate.ulx");
    let text = std::fs::read_to_string(&path).expect("fixture exists");
    let uri = file_url(&path);
    backend.did_open(open_params(uri.clone(), text)).await;

    let response = backend
        .document_symbol(DocumentSymbolParams {
            text_document: TextDocumentIdentifier { uri },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .expect("document_symbol request should not error")
        .expect("translate.ulx declares symbols");

    let DocumentSymbolResponse::Nested(symbols) = response else {
        panic!("expected the Nested DocumentSymbol form");
    };
    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"Fluency"), "symbols were: {names:?}");
    assert!(names.contains(&"Translate"), "symbols were: {names:?}");

    // `selection_range` (§20's precise per-name span) must be the name
    // token alone — strictly smaller than, and contained within, `range`
    // (the whole multi-line declaration) — not a duplicate of it.
    let translate = symbols
        .iter()
        .find(|s| s.name == "Translate")
        .expect("Translate symbol");
    assert_ne!(
        translate.range, translate.selection_range,
        "selection_range should be tighter than the whole-declaration range"
    );
    assert_eq!(
        translate.selection_range.start.line,
        translate.range.start.line
    );
    assert!(
        translate.selection_range.end.character > translate.selection_range.start.character,
        "selection_range should cover the `Translate` identifier, not be empty"
    );
    assert!(
        translate.range.end.line > translate.selection_range.end.line,
        "the whole declaration should extend well past its own name"
    );
}

#[tokio::test]
async fn completion_includes_decls_capabilities_and_keywords() {
    let (service, _socket) = LspService::new(Backend::new);
    let backend = service.inner();

    let path = examples_dir().join("translate.ulx");
    let text = std::fs::read_to_string(&path).expect("fixture exists");
    let uri = file_url(&path);
    backend
        .did_open(open_params(uri.clone(), text.clone()))
        .await;

    let response = backend
        .completion(CompletionParams {
            text_document_position: text_document_position(uri, Position::new(0, 0)),
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: None,
        })
        .await
        .expect("completion request should not error")
        .expect("completion should return items");

    let CompletionResponse::Array(items) = response else {
        panic!("expected the flat Array completion form");
    };
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(labels.contains(&"Translate"), "labels were: {labels:?}");
    assert!(labels.contains(&"chat"), "missing stdlib capability");
    assert!(labels.contains(&"text"), "missing artifact type keyword");
    assert!(labels.contains(&"conversation"), "missing grammar keyword");
}
