//! The `parse -> semantic analysis -> lower` pipeline shared by every
//! runtime-facing CLI command (§13.3's stages, glued together for the CLI).

use std::path::Path;

use crate::diagnostics;

pub struct Loaded {
    pub ir: ulx_ir::IrProgram,
}

/// Parse + semantic analysis only (no lowering) — what `ulx check` reports.
/// Returns `true` iff there were no errors (warnings are printed but don't
/// fail the check).
pub fn check(file: &Path) -> bool {
    let ws = match ulx_sema::analyze_file(file) {
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
    let ws = match ulx_sema::analyze_file(file) {
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
    Some(Loaded { ir })
}
