//! Turning `ulx_syntax`/`ulx_sema` output into `lsp_types::Diagnostic`s, and
//! the two-tier diagnostics strategy described in the plan: a fast,
//! buffer-only pass on every keystroke, and a full cross-file pass
//! (reading imports from disk, mirroring `ulx check`) on open/save.
//!
//! There is no incremental *re-analysis* pipeline yet (every module in the
//! workspace still gets a full semantic pass on every `didOpen`/`didSave`
//! — see `docs/spec/13-compiler-architecture.md` §13.7 for the not-yet-built
//! subtree-level design) — acceptable for the small scripts this language
//! targets, but worth naming as a known simplification rather than
//! pretending sub-tree incrementality exists. `analyze_workspace` does
//! avoid the *re-parse* half via the caller-held `ulx_sema::ParseCache`
//! (see its docs): a transitively-imported file whose mtime hasn't moved
//! since the last save is reused rather than read+parsed again. This
//! function still re-reads every module's source once more itself, to
//! build the `LineIndex` each diagnostic's span is rendered against —
//! that's a separate, smaller cost this cache doesn't cover.

use std::collections::HashSet;
use std::path::Path;

use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, Range, Url};
use ulx_ast::Program;

use crate::line_index::LineIndex;

fn parse_error_to_diagnostic(index: &LineIndex, e: &ulx_syntax::Err) -> Diagnostic {
    let span = e.span();
    let expected: Vec<String> = e
        .expected()
        .map(|tok| match tok {
            Some(t) => format!("{t}"),
            None => "end of input".to_string(),
        })
        .collect();
    let found = e
        .found()
        .map(|t| format!("{t}"))
        .unwrap_or_else(|| "end of input".to_string());
    let message = if expected.is_empty() {
        format!("unexpected {found}")
    } else {
        format!("found {found} but expected one of: {}", expected.join(", "))
    };

    Diagnostic::new(
        Range::new(
            index.offset_to_position(span.start),
            index.offset_to_position(span.end),
        ),
        Some(DiagnosticSeverity::ERROR),
        None,
        Some("ulx-parser".to_string()),
        message,
        None,
        None,
    )
}

fn sema_diagnostic_to_lsp(index: &LineIndex, d: &ulx_sema::Diagnostic) -> Diagnostic {
    let severity = match d.severity {
        ulx_sema::Severity::Error => DiagnosticSeverity::ERROR,
        ulx_sema::Severity::Warning => DiagnosticSeverity::WARNING,
    };
    Diagnostic::new(
        Range::new(
            index.offset_to_position(d.span.start),
            index.offset_to_position(d.span.end),
        ),
        Some(severity),
        None,
        Some("ulx-sema".to_string()),
        d.message.clone(),
        None,
        None,
    )
}

/// Parses the in-memory buffer in isolation (no import resolution) — the
/// fast path run on every `didOpen`/`didChange` before publishing
/// diagnostics. Returns the parsed `Program` (so the caller can rebuild its
/// reference index) alongside whatever diagnostics resulted; `Program` is
/// `None` exactly when parsing failed outright.
pub fn analyze_buffer(index: &LineIndex) -> (Option<Program>, Vec<Diagnostic>) {
    match ulx_syntax::parse_source(index.text()) {
        Err(errs) => (
            None,
            errs.iter()
                .map(|e| parse_error_to_diagnostic(index, e))
                .collect(),
        ),
        Ok(program) => {
            let diags = ulx_sema::analyze(&program)
                .iter()
                .map(|d| sema_diagnostic_to_lsp(index, d))
                .collect();
            (Some(program), diags)
        }
    }
}

/// `ulexite.toml`'s `[providers.*]` entry names next to `file`, if a
/// manifest exists there — same discovery convention as `ulx-cli`'s
/// `pipeline::known_manifest_providers` (a manifest directly next to the
/// file, not searched up the ancestor chain), reimplemented minimally here
/// since `ulx-cli` is a bin-only crate with no `[lib]` target to depend on.
/// Only the `providers` table's keys are needed, so this reads it as a raw
/// `toml::Value` rather than duplicating `ulx-cli`'s typed `Manifest`.
fn known_manifest_providers(file: &Path) -> Option<HashSet<String>> {
    let dir = file.parent()?;
    let text = std::fs::read_to_string(dir.join("ulexite.toml")).ok()?;
    let value: toml::Value = toml::from_str(&text).ok()?;
    let providers = value.get("providers")?.as_table()?;
    Some(providers.keys().cloned().collect())
}

/// Full cross-file analysis (imports resolved, read from disk) — run on
/// `didOpen`/`didSave`, not every keystroke. Returns one diagnostics list
/// per module touched (the file being edited *and* every file it
/// transitively imports), so an error introduced in an imported file shows
/// up there too, not just at the entry point. `None` if the file can't be
/// read/parsed at all (a hard I/O error, not a normal diagnostic).
///
/// `cache` is `Backend`'s single long-lived `ParseCache`, reused across
/// every call for the lifetime of the server — the whole point being that
/// editing and saving one file in a multi-file workspace no longer forces
/// every *other*, unchanged, transitively-imported file to be re-read and
/// re-parsed off disk on every single save. No `[dependencies]` resolution
/// (`DependencyPaths::default()`) — matches what `analyze_file` (the
/// non-cached call this replaced) already did; the LSP doesn't resolve
/// cross-package deps for `from "..."` imports today regardless of caching.
pub fn analyze_workspace(
    entry: &Path,
    cache: &mut ulx_sema::ParseCache,
) -> Option<Vec<(Url, Vec<Diagnostic>)>> {
    let known_providers = known_manifest_providers(entry);
    let workspace = ulx_sema::analyze_file_with_deps_cached(
        entry,
        known_providers.as_ref(),
        &ulx_sema::DependencyPaths::default(),
        cache,
    )
    .ok()?;

    let mut result = Vec::new();
    for module in workspace.modules.values() {
        let Ok(text) = std::fs::read_to_string(&module.path) else {
            continue;
        };
        let Ok(url) = Url::from_file_path(&module.path) else {
            continue;
        };
        let index = LineIndex::new(text);
        let diags = module
            .diagnostics
            .iter()
            .map(|d| sema_diagnostic_to_lsp(&index, d))
            .collect();
        result.push((url, diags));
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_error_produces_a_diagnostic() {
        let index = LineIndex::new("conversation {\n");
        let (program, diags) = analyze_buffer(&index);
        assert!(program.is_none());
        assert!(!diags.is_empty());
        assert_eq!(diags[0].source.as_deref(), Some("ulx-parser"));
    }

    #[test]
    fn valid_program_has_no_diagnostics() {
        let index = LineIndex::new("conversation Greet(name: text) -> text {\n  \"hi\"\n}\n");
        let (program, diags) = analyze_buffer(&index);
        assert!(program.is_some());
        assert!(diags.is_empty());
    }

    #[test]
    fn undefined_capability_is_a_sema_diagnostic() {
        let src = "conversation C() -> text {\n  ask nonexistent_cap() {\n    user: \"\"\"hi\"\"\"\n  } -> out: text\n  out\n}\n";
        let index = LineIndex::new(src);
        let (program, diags) = analyze_buffer(&index);
        assert!(program.is_some());
        assert!(
            !diags.is_empty(),
            "expected a diagnostic for an unknown capability"
        );
    }
}
