//! The `parse -> semantic analysis -> lower` pipeline shared by every
//! runtime-facing CLI command (§13.3's stages, glued together for the CLI).

use std::collections::HashSet;
use std::path::Path;

use ulx_ast::TopDecl;

use crate::diagnostics;
use crate::project_manifest;

pub struct Loaded {
    pub ir: ulx_ir::IrProgram,
    /// Every `provider` decl visible to `file` (its own + every
    /// transitively imported module's) — collected straight off the parsed
    /// `Workspace`, not routed through `ir`: `ulx-ir` only ever lowers the
    /// entry file's own `Program` (see `load()` below), so an IR-routed
    /// design would silently never see a provider declared in an imported
    /// file. Providers are pure config, not executable, so there's nothing
    /// to lower anyway.
    pub provider_decls: Vec<ulx_ast::ProviderDecl>,
}

/// `ulexite.toml`'s `[providers.*]` entry names next to `file`, if a
/// manifest exists there — the same discovery convention
/// `providers::resolve_providers` uses for `ulx run`. `Ok(None)` covers
/// "no manifest, so a `provider` decl's `from`/`provider:` references
/// can't be validated here" (not an error — deferred to `ulx run`).
fn known_manifest_providers(file: &Path) -> Result<Option<HashSet<String>>, String> {
    let dir = crate::manifest::base_dir_of(file);
    let manifest = project_manifest::discover(&dir).map_err(|e| e.to_string())?;
    Ok(manifest.map(|m| m.providers.keys().cloned().collect()))
}

/// Collects every `TopDecl::Provider` visible across the whole workspace
/// (own file + transitively imported modules), erroring if two visible
/// providers share a name — ambiguous regardless of what per-module
/// duplicate-name checking does or doesn't already catch.
fn collect_provider_decls(ws: &ulx_sema::Workspace) -> Result<Vec<ulx_ast::ProviderDecl>, String> {
    let mut seen = HashSet::new();
    let mut decls = Vec::new();
    for module in ws.modules.values() {
        for (decl, _) in &module.program.decls {
            if let TopDecl::Provider(p) = decl {
                if !seen.insert(p.name.clone()) {
                    return Err(format!(
                        "provider `{}` is declared more than once across `{}` and its imports",
                        p.name,
                        ws.entry.display()
                    ));
                }
                decls.push(p.clone());
            }
        }
    }
    Ok(decls)
}

/// Parse + semantic analysis only (no lowering) — what `ulx check` reports.
/// Returns `true` iff there were no errors (warnings are printed but don't
/// fail the check).
pub fn check(file: &Path) -> bool {
    let known_providers = match known_manifest_providers(file) {
        Ok(k) => k,
        Err(e) => {
            eprintln!("error: {e}");
            return false;
        }
    };
    let ws = match ulx_sema::analyze_file(file, known_providers.as_ref()) {
        Ok(ws) => ws,
        Err(e) => {
            eprintln!("error: {e}");
            return false;
        }
    };
    let mut ok = true;
    let mut diag_count = 0;
    for module in ws.modules.values() {
        let Ok(src) = std::fs::read_to_string(&module.path) else {
            continue;
        };
        let module_name = module.path.display().to_string();
        for d in &module.diagnostics {
            diagnostics::report_diagnostic(&module_name, &src, d);
            diag_count += 1;
            if d.severity == ulx_sema::Severity::Error {
                ok = false;
            }
        }
    }
    if diag_count == 0 {
        println!("OK: {} module(s), no diagnostics", ws.modules.len());
    }
    ok
}

/// Loads and fully checks `file`, printing any diagnostics. Returns `None`
/// (having already printed everything relevant) if parsing or semantic
/// analysis fails with errors, or if lowering hits an unsupported
/// construct (§13.4's documented v0.1 restrictions).
pub fn load(file: &Path) -> Option<Loaded> {
    let name = file.display().to_string();
    let known_providers = match known_manifest_providers(file) {
        Ok(k) => k,
        Err(e) => {
            eprintln!("error: {e}");
            return None;
        }
    };
    let ws = match ulx_sema::analyze_file(file, known_providers.as_ref()) {
        Ok(ws) => ws,
        Err(e) => {
            eprintln!("error: {e}");
            return None;
        }
    };

    let mut had_errors = false;
    for module in ws.modules.values() {
        let src = std::fs::read_to_string(&module.path).ok()?;
        let module_name = module.path.display().to_string();
        for d in &module.diagnostics {
            diagnostics::report_diagnostic(&module_name, &src, d);
            if d.severity == ulx_sema::Severity::Error {
                had_errors = true;
            }
        }
    }
    if had_errors {
        return None;
    }

    let provider_decls = match collect_provider_decls(&ws) {
        Ok(decls) => decls,
        Err(e) => {
            eprintln!("error: {e}");
            return None;
        }
    };

    let entry = ws.entry_module();
    let ir = match ulx_ir::lower_program(&entry.program) {
        Ok(ir) => ir,
        Err(e) => {
            eprintln!("error: {name}: lowering failed: {e:?}");
            eprintln!(
                "note: ulx-ir v0.1 only supports `ask` bodies that are plain `system:`/`user:` \
                 message sequences (§13.4's documented restriction) — see docs/spec/24-limitations.md"
            );
            return None;
        }
    };
    Some(Loaded { ir, provider_decls })
}
