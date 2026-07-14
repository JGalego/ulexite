//! Semantic analysis for Ulexite (§13.3): name/import resolution, a
//! best-effort artifact-type checker for `ask` calls (§9.2, §11.5),
//! `Verdict` match-exhaustiveness checking (§9.4), and `with`-block
//! independence checking (§9.7).
//!
//! This is a v0.1 semantic pass, not the full static guarantee described in
//! the spec: type inference is best-effort (it infers what it can from
//! declared parameter/binding types and skips silently rather than
//! guessing when it can't), and there is no unification-based type system
//! yet. See `docs/spec/24-limitations.md` for the honest accounting this
//! extends.

mod capability;
mod diagnostic;
mod resolve;
mod scope;
mod typecheck;

pub use capability::{stdlib_capabilities, CapabilitySpec};
pub use diagnostic::{Diagnostic, Severity};
pub use resolve::{load_and_analyze, AnalyzedModule, Workspace, STDLIB_MODULES};

use std::collections::HashSet;
use std::path::Path;
use ulx_ast::Program;

/// Analyze a single already-parsed program with no import resolution
/// (useful for unit tests and for analyzing a program that has no
/// filesystem home, e.g. a REPL fragment).
pub fn analyze(program: &Program) -> Vec<Diagnostic> {
    let caps = stdlib_capabilities();
    let mut diags = Vec::new();
    resolve::check_duplicate_top_level_names(program, &mut diags);
    for (decl, _) in &program.decls {
        typecheck::check_decl(decl, &caps, &mut diags);
    }
    diags
}

/// Parse `entry` and every file it (transitively) imports, then run
/// semantic analysis across the whole workspace. `known_manifest_providers`
/// is the set of `ulexite.toml` `[providers.*]` entry names next to `entry`,
/// if the caller (`ulx-cli`) found one — `None` if there's no manifest, in
/// which case a `provider` decl's `from "name"` can't be validated here.
pub fn analyze_file(
    entry: &Path,
    known_manifest_providers: Option<&HashSet<String>>,
) -> Result<Workspace, String> {
    load_and_analyze(entry, known_manifest_providers)
}
