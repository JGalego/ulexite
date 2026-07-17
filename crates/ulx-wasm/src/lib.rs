//! WASM bindings exposing Ulexite's parser and single-file semantic
//! analysis to the browser, for the website's live playground
//! (`website/src/components/Playground`). Deliberately narrow: no import
//! resolution, no provider/manifest awareness — `ulx_sema::analyze`, the
//! same single-file fast path `ulx-lsp` runs on every keystroke (see
//! `crates/ulx-lsp/src/analysis.rs`'s `analyze_buffer`) — and no
//! execution at all. `ulx-runtime` depends on `ureq` (blocking HTTP) and
//! `std::thread::scope`, neither of which targets
//! `wasm32-unknown-unknown`, so "run a conversation" is out of scope for
//! this crate; the playground is "try the compiler," not "run a
//! provider."

mod line_index;

use std::ops::Range;

use serde::Serialize;
use wasm_bindgen::prelude::*;

use line_index::LineIndex;

#[derive(Serialize)]
struct Diagnostic {
    severity: &'static str,
    message: String,
    start_line: u32,
    start_col: u32,
    end_line: u32,
    end_col: u32,
}

/// Parses `source` as a standalone `.ulx` file and runs single-file
/// semantic analysis on it, returning every diagnostic as a JS array of
/// `{severity, message, start_line, start_col, end_line, end_col}`
/// objects (line/col are 0-indexed, UTF-16 columns — see `line_index`).
#[wasm_bindgen]
pub fn check(source: &str) -> Result<JsValue, JsValue> {
    let index = LineIndex::new(source);

    let diagnostics: Vec<Diagnostic> = match ulx_syntax::parse_source(source) {
        Err(errs) => errs
            .iter()
            .map(|e| {
                to_diagnostic(
                    &index,
                    source,
                    "error",
                    ulx_syntax::format_error(e),
                    e.span(),
                )
            })
            .collect(),
        Ok(program) => ulx_sema::analyze(&program)
            .into_iter()
            .map(|d| {
                let severity = match d.severity {
                    ulx_sema::Severity::Error => "error",
                    ulx_sema::Severity::Warning => "warning",
                };
                to_diagnostic(&index, source, severity, d.message, d.span)
            })
            .collect(),
    };

    serde_wasm_bindgen::to_value(&diagnostics).map_err(|e| JsValue::from_str(&e.to_string()))
}

fn to_diagnostic(
    index: &LineIndex,
    source: &str,
    severity: &'static str,
    message: String,
    span: Range<usize>,
) -> Diagnostic {
    let (start_line, start_col) = index.line_col(source, span.start);
    let (end_line, end_col) = index.line_col(source, span.end);
    Diagnostic {
        severity,
        message,
        start_line,
        start_col,
        end_line,
        end_col,
    }
}
