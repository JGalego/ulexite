use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use ulx_ast::{Import, ImportKind, Program, TopDecl};

use crate::capability::stdlib_capabilities;
use crate::diagnostic::Diagnostic;
use crate::typecheck::{check_decl_with, Ctx};

/// Stdlib module names a `import "..." as ident` may reference (§15). Not
/// exhaustive against every stdlib submodule in the spec — this is the v0.1
/// set the runtime (`ulx-runtime`) actually implements something for.
pub const STDLIB_MODULES: &[&str] = &[
    "llm",
    "judge",
    "vision",
    "image",
    "audio",
    "video",
    "pdf",
    "json",
    "xml",
    "html",
    "csv",
    "http",
    "python",
    "javascript",
    "shell",
    "trace",
    "dataset",
    "cache",
    "retry",
    "metrics",
    "assert",
    "vector",
    "embedding",
];

pub struct AnalyzedModule {
    pub path: PathBuf,
    pub program: Program,
    pub diagnostics: Vec<Diagnostic>,
}

pub struct Workspace {
    pub entry: PathBuf,
    pub modules: HashMap<PathBuf, AnalyzedModule>,
}

impl Workspace {
    pub fn entry_module(&self) -> &AnalyzedModule {
        &self.modules[&self.entry]
    }

    pub fn has_errors(&self) -> bool {
        self.modules.values().any(|m| {
            m.diagnostics
                .iter()
                .any(|d| d.severity == crate::Severity::Error)
        })
    }
}

pub fn decl_name(decl: &TopDecl) -> &str {
    match decl {
        TopDecl::Conversation(c) => &c.name,
        TopDecl::Judge(r) | TopDecl::Validator(r) => &r.name,
        TopDecl::Dataset(d) => &d.name,
        TopDecl::Type(t) => &t.name,
        TopDecl::Benchmark(b) => &b.name,
        TopDecl::Provider(p) => &p.name,
    }
}

fn decl_kind(decl: &TopDecl) -> ImportKind {
    match decl {
        TopDecl::Conversation(_) => ImportKind::Conversation,
        TopDecl::Judge(_) => ImportKind::Judge,
        TopDecl::Validator(_) => ImportKind::Validator,
        TopDecl::Dataset(_) => ImportKind::Dataset,
        TopDecl::Type(_) => ImportKind::Type,
        TopDecl::Benchmark(_) => ImportKind::Conversation, // benchmarks aren't importable by kind
        TopDecl::Provider(_) => ImportKind::Provider,
    }
}

pub fn check_duplicate_top_level_names(program: &Program, diags: &mut Vec<Diagnostic>) {
    let mut seen: HashMap<&str, ulx_ast::Span> = HashMap::new();
    for (decl, span) in &program.decls {
        let name = decl_name(decl);
        if let Some(prev_span) = seen.get(name) {
            diags.push(Diagnostic::error(
                format!(
                    "duplicate top-level declaration `{name}` (first declared at {prev_span:?})"
                ),
                span.clone(),
            ));
        } else {
            seen.insert(name, span.clone());
        }
    }
}

/// Parse `entry` and every file it (transitively) imports via `import kind
/// Name from "path"`, then run semantic analysis over each module with a
/// full picture of its available global names (own declarations + imported
/// names + stdlib module aliases). `known_manifest_providers` is the set of
/// `[providers.*]` entry names in `ulexite.toml` next to `entry`, if the
/// caller found one (`ulx-cli`'s `pipeline::check` — `ulx-sema` itself never
/// reads the manifest); `None` means no manifest was found, so a `provider`
/// decl's `from "name"` clause can't be validated here and is deferred
/// entirely to `ulx run`.
pub fn load_and_analyze(
    entry: &Path,
    known_manifest_providers: Option<&HashSet<String>>,
) -> Result<Workspace, String> {
    let entry = entry
        .canonicalize()
        .map_err(|e| format!("could not read {}: {e}", entry.display()))?;

    let mut modules: HashMap<PathBuf, Program> = HashMap::new();
    let mut loading: HashSet<PathBuf> = HashSet::new();
    load_recursive(&entry, &mut modules, &mut loading)?;

    let caps = stdlib_capabilities();
    let mut analyzed: HashMap<PathBuf, AnalyzedModule> = HashMap::new();

    for (path, program) in &modules {
        let mut diags = Vec::new();
        check_duplicate_top_level_names(program, &mut diags);

        let mut globals: HashSet<String> = HashSet::new();
        let mut judges_and_validators: HashSet<String> = HashSet::new();
        let mut providers: HashSet<String> = HashSet::new();
        for (decl, _) in &program.decls {
            globals.insert(decl_name(decl).to_string());
            if matches!(decl, TopDecl::Judge(_) | TopDecl::Validator(_)) {
                judges_and_validators.insert(decl_name(decl).to_string());
            }
            if matches!(decl, TopDecl::Provider(_)) {
                providers.insert(decl_name(decl).to_string());
            }
        }
        for (import, span) in &program.imports {
            match import {
                Import::Named { kind, name, from } => {
                    globals.insert(name.clone());
                    if matches!(kind, ImportKind::Judge | ImportKind::Validator) {
                        judges_and_validators.insert(name.clone());
                    }
                    if matches!(kind, ImportKind::Provider) {
                        providers.insert(name.clone());
                    }
                    let target_path = resolve_import_path(path, from);
                    match modules.get(&target_path) {
                        None => diags.push(Diagnostic::error(
                            format!("could not resolve import `{from}`"),
                            span.clone(),
                        )),
                        Some(target_program) => {
                            let found = target_program
                                .decls
                                .iter()
                                .any(|(d, _)| decl_name(d) == name && decl_kind(d) == *kind);
                            if !found {
                                diags.push(Diagnostic::error(
                                    format!("`{name}` is not declared as a {kind:?} in `{from}`"),
                                    span.clone(),
                                ));
                            }
                        }
                    }
                }
                Import::Module {
                    path: mod_path,
                    alias,
                } => {
                    globals.insert(alias.clone());
                    if !STDLIB_MODULES.contains(&mod_path.as_str()) {
                        diags.push(Diagnostic::warning(
                            format!("`{mod_path}` is not a recognized stdlib module (§15)"),
                            span.clone(),
                        ));
                    }
                }
            }
        }

        for (decl, _) in &program.decls {
            let mut ctx = Ctx {
                caps: &caps,
                globals: Some(&globals),
                judges_and_validators: Some(&judges_and_validators),
                providers: Some(&providers),
                known_manifest_providers,
                diags: &mut diags,
            };
            check_decl_with(decl, &mut ctx);
        }

        analyzed.insert(
            path.clone(),
            AnalyzedModule {
                path: path.clone(),
                program: program.clone(),
                diagnostics: diags,
            },
        );
    }

    Ok(Workspace {
        entry,
        modules: analyzed,
    })
}

fn resolve_import_path(from_file: &Path, relative: &str) -> PathBuf {
    let dir = from_file.parent().unwrap_or_else(|| Path::new("."));
    let candidate = dir.join(relative);
    candidate.canonicalize().unwrap_or(candidate)
}

fn load_recursive(
    path: &Path,
    modules: &mut HashMap<PathBuf, Program>,
    loading: &mut HashSet<PathBuf>,
) -> Result<(), String> {
    if modules.contains_key(path) {
        return Ok(());
    }
    if !loading.insert(path.to_path_buf()) {
        return Err(format!("import cycle detected at {}", path.display()));
    }

    let src = std::fs::read_to_string(path)
        .map_err(|e| format!("could not read {}: {e}", path.display()))?;
    let program = ulx_syntax::parse_source(&src)
        .map_err(|errs| format!("{} failed to parse: {errs:?}", path.display()))?;

    for (import, _) in &program.imports {
        if let Import::Named { from, .. } = import {
            let target = resolve_import_path(path, from);
            load_recursive(&target, modules, loading)?;
        }
    }

    loading.remove(path);
    modules.insert(path.to_path_buf(), program);
    Ok(())
}
