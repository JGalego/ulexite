//! The `parse -> semantic analysis -> lower` pipeline shared by every
//! runtime-facing CLI command (§13.3's stages, glued together for the CLI).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use ulx_ast::{Program, TopDecl};

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

/// Builds `ulx-sema`'s `DependencyPaths` from `ulexite.toml`'s
/// `[dependencies]` table next to `file`, if a manifest exists there (same
/// discovery convention as `known_manifest_providers` above) — `path`
/// entries are joined against the manifest's own directory (so a relative
/// `path = "../other-pkg"` is resolved the same way a human reading the
/// manifest would expect); a `git` entry is actually cloned/checked-out
/// via `crate::git_dep::resolve` (§14.2's real escape hatch, no hosted
/// registry) into a local vendored directory, which then resolves exactly
/// like a `path` dependency. A bare version string (no `git`/`path` at
/// all — there's no registry to resolve a named version against) becomes
/// an `unresolvable` name so a cross-package import referencing it gets a
/// clear error instead of silently falling through to relative-path
/// resolution. A `git` dependency that fails to clone/check out is a hard
/// error here, immediately, rather than deferred to whatever import
/// happens to reference it first.
fn dependency_paths(file: &Path) -> Result<ulx_sema::DependencyPaths, String> {
    let dir = crate::manifest::base_dir_of(file);
    let manifest = project_manifest::discover(&dir).map_err(|e| e.to_string())?;
    let mut deps = ulx_sema::DependencyPaths::default();
    let Some(manifest) = manifest else {
        return Ok(deps);
    };
    for (name, dep) in &manifest.dependencies {
        match dep {
            project_manifest::Dependency::Detailed {
                path: Some(path), ..
            } => {
                deps.path_deps.insert(name.clone(), dir.join(path));
            }
            project_manifest::Dependency::Detailed {
                git: Some(url),
                tag,
                ..
            } => {
                let checkout = crate::git_dep::resolve(&dir, url, tag.as_deref())
                    .map_err(|e| format!("dependency `{name}`: {e}"))?;
                deps.path_deps.insert(name.clone(), checkout);
            }
            _ => {
                deps.unresolvable.insert(name.clone());
            }
        }
    }
    Ok(deps)
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

/// If `file` is a Markdown source (the dialect `md.rs` documents), compiles
/// it to `.ulx` and returns the path of the freshly generated file for
/// `load`/`check` to actually parse — every other part of the pipeline
/// (provider/dependency discovery, `base_dir_of`, `RunManifest.file`) keeps
/// using `file` itself, so a project's `ulexite.toml`/relative paths resolve
/// exactly as if `file` had been `.ulx` all along; only the source that
/// `ulx-sema` reads for `file`'s own entry module is swapped. Regenerated on
/// every call — nothing here is a cache the source and its compiled output
/// could ever drift out of sync on. Files that aren't Markdown pass through
/// unchanged (no copy, no `.ulexite/generated` write).
fn resolve_entry(file: &Path) -> Result<PathBuf, String> {
    let is_markdown = file
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("markdown"));
    if !is_markdown {
        return Ok(file.to_path_buf());
    }

    let src = std::fs::read_to_string(file)
        .map_err(|e| format!("could not read {}: {e}", file.display()))?;
    let conv = crate::md::parse_md(&src).map_err(|e| format!("{}: {e}", file.display()))?;
    let ulx = crate::md::render_ulx(&conv);
    if let Err(errors) = ulx_syntax::parse_source(&ulx) {
        let mut msg = format!(
            "internal error: the .ulx generated from {} does not parse:\n",
            file.display()
        );
        for e in &errors {
            msg.push_str(&format!("  {e:?}\n"));
        }
        return Err(msg);
    }

    let dir = crate::manifest::generated_dir();
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("md");
    let hash = ulx_runtime::value::hash_bytes(file.display().to_string().as_bytes());
    let generated = dir.join(format!("{stem}-{}.ulx", &hash[..12]));
    std::fs::write(&generated, &ulx)
        .map_err(|e| format!("could not write {}: {e}", generated.display()))?;
    Ok(generated)
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
    let deps = match dependency_paths(file) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error: {e}");
            return false;
        }
    };
    let entry = match resolve_entry(file) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("error: {e}");
            return false;
        }
    };
    let ws = match ulx_sema::analyze_file_with_deps(&entry, known_providers.as_ref(), &deps) {
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
            diagnostics::report_module_diagnostic(&module_name, &src, d);
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
    let deps = match dependency_paths(file) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error: {e}");
            return None;
        }
    };
    let entry = match resolve_entry(file) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("error: {e}");
            return None;
        }
    };
    let ws = match ulx_sema::analyze_file_with_deps(&entry, known_providers.as_ref(), &deps) {
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
            diagnostics::report_module_diagnostic(&module_name, &src, d);
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

    // `ulx-ir::lower_program` resolves a bare-identifier call (e.g. a
    // benchmark's `run: Translate(...)`, or one conversation calling
    // another) to a `ConversationCall` only when the callee's name appears
    // among the *same* `Program`'s own top-level decls (see its
    // `known_conversations` set) — otherwise it falls back to an
    // `OpaqueCall`, which the runtime can't actually resolve. An imported
    // conversation/judge/validator (`import conversation Translate from
    // "translate.ulx"`) therefore wouldn't be callable at all if only the
    // entry file's own `Program` were lowered. Flattening every workspace
    // module's decls into one merged `Program` before lowering (name
    // collisions resolved entry-first) fixes that — a blunt, single-
    // namespace merge rather than real per-module IR linking, but real
    // enough to make cross-file calls (like `examples/eval_translate.ulx`
    // calling `Translate` from `translate.ulx`) actually execute.
    let merged = merge_workspace_program(&ws);
    let ir = match ulx_ir::lower_program(&merged) {
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

/// Flattens every module in `ws` into one `Program` for lowering — see the
/// comment at `load()`'s call site for why. Entry-first, then the rest in a
/// deterministic (path-sorted) order; a decl name already seen is skipped,
/// so the entry module's own declarations always win over an imported
/// module's same-named one.
fn merge_workspace_program(ws: &ulx_sema::Workspace) -> Program {
    let mut modules: Vec<&ulx_sema::AnalyzedModule> = ws.modules.values().collect();
    modules.sort_by(|a, b| {
        let a_is_entry = a.path == ws.entry;
        let b_is_entry = b.path == ws.entry;
        b_is_entry
            .cmp(&a_is_entry)
            .then_with(|| a.path.cmp(&b.path))
    });

    let mut seen: HashSet<String> = HashSet::new();
    let mut decls = Vec::new();
    for module in modules {
        for (decl, span) in &module.program.decls {
            if seen.insert(pipeline_decl_name(decl).to_string()) {
                decls.push((decl.clone(), span.clone()));
            }
        }
    }
    Program {
        imports: Vec::new(),
        decls,
        doc_comments: std::collections::BTreeMap::new(),
    }
}

fn pipeline_decl_name(decl: &TopDecl) -> &str {
    match decl {
        TopDecl::Conversation(c) => &c.name,
        TopDecl::Judge(r) | TopDecl::Validator(r) => &r.name,
        TopDecl::Dataset(d) => &d.name,
        TopDecl::Type(t) => &t.name,
        TopDecl::Benchmark(b) => &b.name,
        TopDecl::Provider(p) => &p.name,
    }
}

#[cfg(test)]
mod tests {
    use super::dependency_paths;

    /// `dependency_paths` should turn `ulexite.toml`'s `[dependencies]`
    /// table into `ulx-sema`'s `DependencyPaths`: `path` entries resolved
    /// relative to the manifest's own directory, and a bare version string
    /// (no registry to resolve it against) recorded as `unresolvable` so
    /// import resolution can reject it with a clear error instead of
    /// mishandling it. `git` dependencies are covered separately below,
    /// since they need a real (if local-only, offline) git repo to
    /// resolve against.
    #[test]
    fn builds_path_and_unresolvable_entries_from_manifest() {
        let dir = std::env::temp_dir().join(format!(
            "ulexite-cli-dependency-paths-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("ulexite.toml"),
            r#"
            [package]
            name = "tiny"
            version = "0.1.0"
            ulexite = "^0.1"

            [dependencies]
            local-thing = { path = "../local-thing" }
            versioned-thing = "^2.1"
            "#,
        )
        .unwrap();
        let file = dir.join("main.ulx");
        std::fs::write(&file, "conversation Foo() -> text { \"hi\" }").unwrap();

        let deps = dependency_paths(&file).expect("must load manifest");
        assert_eq!(
            deps.path_deps.get("local-thing"),
            Some(&dir.join("../local-thing"))
        );
        assert!(deps.unresolvable.contains("versioned-thing"));
        assert!(!deps.path_deps.contains_key("versioned-thing"));

        std::fs::remove_dir_all(&dir).ok();
    }

    /// A `git` dependency is now really cloned (§14.2) — proved here
    /// against a throwaway local upstream repo (a local path is a
    /// legitimate `git clone` source, so this stays fully offline) rather
    /// than a real network host. The resolved path ends up in
    /// `path_deps`, exactly like an ordinary `path` dependency, with the
    /// tagged commit's actual content on disk.
    #[test]
    fn git_dependency_is_really_cloned_and_lands_in_path_deps() {
        let dir = std::env::temp_dir().join(format!(
            "ulexite-cli-git-dependency-test-{}",
            std::process::id()
        ));
        let upstream = dir.join("upstream");
        std::fs::create_dir_all(&upstream).unwrap();
        let run = |args: &[&str]| {
            let status = std::process::Command::new("git")
                .args(args)
                .current_dir(&upstream)
                .status()
                .expect("git must be installed to run this test");
            assert!(status.success(), "git {args:?} failed");
        };
        run(&["init", "--quiet", "-b", "main"]);
        run(&["config", "user.email", "test@example.com"]);
        run(&["config", "user.name", "Test"]);
        std::fs::write(
            upstream.join("lib.ulx"),
            "conversation Lib() -> text { \"hi\" }",
        )
        .unwrap();
        run(&["add", "."]);
        run(&["commit", "--quiet", "-m", "initial"]);
        run(&["tag", "v1.0.0"]);

        let project = dir.join("project");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            project.join("ulexite.toml"),
            format!(
                r#"
                [package]
                name = "tiny"
                version = "0.1.0"
                ulexite = "^0.1"

                [dependencies]
                git-thing = {{ git = "{}", tag = "v1.0.0" }}
                "#,
                upstream.display()
            ),
        )
        .unwrap();
        let file = project.join("main.ulx");
        std::fs::write(&file, "conversation Foo() -> text { \"hi\" }").unwrap();

        let deps = dependency_paths(&file).expect("git dependency should resolve");
        let resolved = deps
            .path_deps
            .get("git-thing")
            .expect("git-thing should be in path_deps, not unresolvable");
        let content = std::fs::read_to_string(resolved.join("lib.ulx")).unwrap();
        assert!(content.contains("conversation Lib"));

        std::fs::remove_dir_all(&dir).ok();
    }

    /// No `ulexite.toml` next to `file` at all — same "nothing to resolve
    /// against" convention `known_manifest_providers` follows: empty, not
    /// an error.
    #[test]
    fn no_manifest_means_no_dependencies() {
        let dir = std::env::temp_dir().join(format!(
            "ulexite-cli-dependency-paths-no-manifest-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("main.ulx");
        std::fs::write(&file, "conversation Foo() -> text { \"hi\" }").unwrap();

        let deps = dependency_paths(&file).expect("must not error");
        assert!(deps.path_deps.is_empty());
        assert!(deps.unresolvable.is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }
}
