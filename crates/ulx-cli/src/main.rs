//! `ulx` — the Ulexite CLI (§20.12).
//!
//! Implemented: `parse`, `check` (§20.7's diagnostics — also exposed live
//! through `ulx-lsp`, §20.2's language server), `run`, `bench` (§16 —
//! dataset-parametrized `benchmark` execution; see
//! `ulx-runtime::run_benchmark` for the narrower-than-spec scope: no
//! `expect`-polling/retry-until-converged, no golden-file `snapshot`
//! comparison, no `metrics.*` aggregation or JUnit/JSON report — a
//! plain-text per-row pass/fail report), `approve`/`deny` (§10.7's
//! human-approval resume, v0.1-style — see `ulx-runtime`'s docs for how
//! that actually works), `replay` (§18.3), `trace` (§20.6 — no viewer
//! webview, but `--output mermaid`/`html` render a shareable diagram/page
//! in its place; see `output.rs`).
//!
//! Also implemented: `plan` (§10.5 — a static, execution-free capability ->
//! provider resolution plus a rough token/cost estimate off a small
//! illustrative pricing table; see `plan.rs`'s module docs for exactly how
//! approximate that estimate is), `fmt` (§20.10 — an AST-based
//! pretty-printer; see `ulx_syntax::fmt` for the important caveat that it
//! does not preserve comments).
//!
//! Not implemented: `doc`/`repl` (§20). See
//! `docs/spec/25-future-directions.md`.

mod diagnostics;
mod manifest;
mod output;
mod pipeline;
mod plan;
mod project_manifest;
mod providers;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use output::{OutputFormat, RunOutcome};
use ulx_runtime::{Cache, ProviderRegistry, RunContext, RuntimeError, TraceWriter, Value};

#[derive(Parser)]
#[command(name = "ulx", about = "Ulexite language CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Parse a .ulx file and report success or syntax errors.
    Parse { file: PathBuf },
    /// Parse + run semantic analysis (§9, §13.3) across the file and its imports.
    Check { file: PathBuf },
    /// Run a conversation to completion (or suspension). Errors if no
    /// provider is configured — pass --mock to run against the
    /// deterministic mock provider instead.
    Run {
        file: PathBuf,
        conversation: String,
        /// Repeatable `name=value` argument, e.g. `--arg source=hello`.
        #[arg(long = "arg", value_name = "NAME=VALUE")]
        args: Vec<String>,
        /// Reuse a specific run id (default: derived from file+conversation+args).
        #[arg(long)]
        run_id: Option<String>,
        /// Select a specific configured provider by name (a `.ulx`
        /// `provider` decl or a `ulexite.toml` entry) — repeatable; only
        /// the named provider(s) are registered, so an otherwise-ambiguous
        /// capability resolves unambiguously.
        #[arg(long = "provider", value_name = "NAME")]
        providers: Vec<String>,
        /// Force the deterministic offline mock provider, ignoring any
        /// configured real providers entirely.
        #[arg(long, conflicts_with = "providers")]
        mock: bool,
        /// Output format: `text` (default), `json`, `jsonl`, `mermaid`, or
        /// `html`. The latter three render the run's full trace, not just
        /// its final value. Ignored when `--interactive` is set, which
        /// always reports in plain text.
        #[arg(long, value_enum, default_value = "text")]
        output: OutputFormat,
        /// Prompt at the terminal for each `escalate(...)` suspension
        /// instead of stopping and printing `ulx approve`/`ulx deny`
        /// instructions — the synchronous alternative for a human sitting
        /// at the terminal right now, rather than the asynchronous
        /// suspend/resume flow §7.3/§10.7 otherwise describe (a human
        /// answering later, possibly via a different interface, possibly
        /// after the process has exited).
        #[arg(long)]
        interactive: bool,
    },
    /// Run a `benchmark` declaration to completion (§16): loads its
    /// `dataset:`, runs the benchmark body once per row, and prints a
    /// pass/fail report. Errors if no provider is configured — pass
    /// --mock to run against the deterministic mock provider instead.
    Bench {
        file: PathBuf,
        benchmark: String,
        #[arg(long = "provider", value_name = "NAME")]
        providers: Vec<String>,
        #[arg(long, conflicts_with = "providers")]
        mock: bool,
    },
    /// Statically estimate which providers/models a conversation will call
    /// and a rough token/cost range, without executing anything (§10.5).
    Plan {
        file: PathBuf,
        conversation: String,
        /// Repeatable `name=value` argument — only used to sharpen the
        /// token-length estimate for a parameter a `--arg` explicitly
        /// pins, never to actually run anything.
        #[arg(long = "arg", value_name = "NAME=VALUE")]
        args: Vec<String>,
        /// Select a specific configured provider by name, same as `ulx
        /// run --provider` — repeatable. There is deliberately no `--mock`
        /// here: `plan` never calls a provider for real, so forcing the
        /// mock provider would only hide what a real `ulx run` will
        /// actually resolve to.
        #[arg(long = "provider", value_name = "NAME")]
        providers: Vec<String>,
    },
    /// Record a human decision for a suspended run and resume it.
    Approve {
        run_id: String,
        /// The value to resolve the `escalate(...)` expression to.
        #[arg(long, default_value = "approved")]
        value: String,
        #[arg(long = "provider", value_name = "NAME")]
        providers: Vec<String>,
        #[arg(long, conflicts_with = "providers")]
        mock: bool,
        #[arg(long, value_enum, default_value = "text")]
        output: OutputFormat,
    },
    /// Record a denial for a suspended run (does not resume execution).
    Deny {
        run_id: String,
        #[arg(long)]
        note: Option<String>,
        #[arg(long = "provider", value_name = "NAME")]
        providers: Vec<String>,
        #[arg(long, conflicts_with = "providers")]
        mock: bool,
        #[arg(long, value_enum, default_value = "text")]
        output: OutputFormat,
    },
    /// Strictly replay a completed run from its trace log (§18.3) — a cache
    /// miss is an error, never a live provider call.
    Replay {
        run_id: String,
        #[arg(long, value_enum, default_value = "text")]
        output: OutputFormat,
    },
    /// Print a run's trace log (§18, §20.6 without the viewer webview).
    Trace {
        run_id: String,
        /// Output format: `text` (default), `json`, `jsonl`, `mermaid`
        /// (a sequence diagram), or `html` (a self-contained page).
        #[arg(long, value_enum, default_value = "text")]
        output: OutputFormat,
    },
    /// Scaffold a new package: `ulexite.toml` + a starter conversation (§14.1).
    Init {
        name: String,
        #[arg(default_value = ".")]
        dir: PathBuf,
    },
    /// Parse, validate, and print `ulexite.toml` (§14.1).
    Manifest {
        #[arg(default_value = "ulexite.toml")]
        file: PathBuf,
    },
    /// Reformat a .ulx file to canonical style (§20.10). An AST-based
    /// pretty-printer, NOT a lossless/comment-preserving formatter —
    /// comments are dropped (see `ulx_syntax::fmt` module docs).
    Fmt {
        file: PathBuf,
        /// Don't write anything; exit non-zero if the file isn't already
        /// in canonical form (mirrors `cargo fmt --check`/`gofmt -l`).
        #[arg(long)]
        check: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    let ok = match cli.command {
        Command::Parse { file } => cmd_parse(&file),
        Command::Check { file } => pipeline::check(&file),
        Command::Run {
            file,
            conversation,
            args,
            run_id,
            providers,
            mock,
            output,
            interactive,
        } => cmd_run(
            &file,
            &conversation,
            &args,
            run_id,
            &providers,
            mock,
            output,
            interactive,
        ),
        Command::Bench {
            file,
            benchmark,
            providers,
            mock,
        } => cmd_bench(&file, &benchmark, &providers, mock),
        Command::Plan {
            file,
            conversation,
            args,
            providers,
        } => cmd_plan(&file, &conversation, &args, &providers),
        Command::Approve {
            run_id,
            value,
            providers,
            mock,
            output,
        } => cmd_approve(&run_id, &value, &providers, mock, output),
        Command::Deny {
            run_id,
            note,
            providers,
            mock,
            output,
        } => cmd_deny(&run_id, note.as_deref(), &providers, mock, output),
        Command::Replay { run_id, output } => cmd_replay(&run_id, output),
        Command::Trace { run_id, output } => cmd_trace(&run_id, output),
        Command::Init { name, dir } => cmd_init(&name, &dir),
        Command::Manifest { file } => cmd_manifest(&file),
        Command::Fmt { file, check } => cmd_fmt(&file, check),
    };
    if !ok {
        std::process::exit(1);
    }
}

fn cmd_parse(file: &PathBuf) -> bool {
    let src = match std::fs::read_to_string(file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: could not read {}: {e}", file.display());
            return false;
        }
    };
    let name = file.display().to_string();
    match ulx_syntax::parse_source(&src) {
        Ok(program) => {
            println!(
                "OK: {} import(s), {} declaration(s)",
                program.imports.len(),
                program.decls.len()
            );
            true
        }
        Err(errors) => {
            for e in &errors {
                diagnostics::report_parse_error(&name, &src, e);
            }
            false
        }
    }
}

fn cmd_fmt(file: &PathBuf, check: bool) -> bool {
    let src = match std::fs::read_to_string(file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: could not read {}: {e}", file.display());
            return false;
        }
    };
    let name = file.display().to_string();
    let program = match ulx_syntax::parse_source(&src) {
        Ok(p) => p,
        Err(errors) => {
            for e in &errors {
                diagnostics::report_parse_error(&name, &src, e);
            }
            return false;
        }
    };
    let formatted = ulx_syntax::format_program(&program);

    // Safety net: the printer should always produce reparseable output; if
    // it doesn't, that's a formatter bug, not something to silently write
    // over the user's file.
    if let Err(errors) = ulx_syntax::parse_source(&formatted) {
        eprintln!(
            "error: internal formatter error: reformatted output for {} does not parse",
            file.display()
        );
        for e in &errors {
            diagnostics::report_parse_error(&name, &formatted, e);
        }
        return false;
    }

    if check {
        if formatted == src {
            true
        } else {
            println!("would reformat {}", file.display());
            false
        }
    } else if formatted == src {
        true
    } else if let Err(e) = std::fs::write(file, &formatted) {
        eprintln!("error: could not write {}: {e}", file.display());
        false
    } else {
        println!("formatted {}", file.display());
        true
    }
}

fn parse_args(raw: &[String]) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    for a in raw {
        let (k, v) = a
            .split_once('=')
            .ok_or_else(|| format!("--arg `{a}` must be in `name=value` form"))?;
        out.insert(k.to_string(), v.to_string());
    }
    Ok(out)
}

fn default_run_id(
    file: &std::path::Path,
    conversation: &str,
    args: &BTreeMap<String, String>,
) -> String {
    let mut input = format!("{}::{conversation}", file.display());
    for (k, v) in args {
        input.push('|');
        input.push_str(k);
        input.push('=');
        input.push_str(v);
    }
    ulx_runtime::value::hash_bytes(input.as_bytes())[..16].to_string()
}

/// Builds a `RunContext` from an already-resolved `ProviderRegistry` — the
/// registry resolution itself (`providers::resolve_providers`, needing the
/// CLI's `--provider`/`--mock` flags and the loaded `provider_decls`) is the
/// caller's job, so `cmd_replay` can special-case its own fallback path.
fn build_context<'a>(
    ir: &'a ulx_ir::IrProgram,
    providers: ProviderRegistry,
    file: &std::path::Path,
    run_id: &str,
) -> std::io::Result<RunContext<'a>> {
    let cache = Cache::new(manifest::cache_dir())?;
    let trace = TraceWriter::create(manifest::traces_dir(), run_id)?;
    Ok(RunContext::new(
        ir,
        providers,
        cache,
        trace,
        run_id.to_string(),
        manifest::base_dir_of(file),
    ))
}

#[allow(clippy::too_many_arguments)]
fn cmd_run(
    file: &Path,
    conversation: &str,
    raw_args: &[String],
    run_id: Option<String>,
    selected_providers: &[String],
    force_mock: bool,
    output: OutputFormat,
    interactive: bool,
) -> bool {
    let Some(loaded) = pipeline::load(file) else {
        return false;
    };
    let args = match parse_args(raw_args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("error: {e}");
            return false;
        }
    };
    let run_id = run_id.unwrap_or_else(|| default_run_id(file, conversation, &args));

    if let Err(e) = manifest::save(
        &run_id,
        &manifest::RunManifest {
            file: file.to_path_buf(),
            conversation: conversation.to_string(),
            args: args.clone(),
            selected_providers: selected_providers.to_vec(),
            force_mock,
        },
    ) {
        eprintln!("warning: could not persist run manifest: {e}");
    }

    let value_args: BTreeMap<String, Value> =
        args.into_iter().map(|(k, v)| (k, Value::Text(v))).collect();

    if interactive {
        return run_interactive(
            &loaded,
            file,
            conversation,
            &value_args,
            &run_id,
            selected_providers,
            force_mock,
        );
    }

    let providers = match providers::resolve_providers(
        file,
        &loaded.provider_decls,
        selected_providers,
        force_mock,
    ) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            return false;
        }
    };
    let ctx = match build_context(&loaded.ir, providers, file, &run_id) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: could not set up run context: {e}");
            return false;
        }
    };

    execute(&ctx, conversation, value_args, &run_id, output)
}

/// The synchronous counterpart to `execute`/`resume`'s suspend-then-resume
/// flow (§7.3, §10.7): instead of exiting with "suspended, resume with
/// `ulx approve`", prompt right here at the terminal and keep going. Each
/// iteration rebuilds a fresh `ProviderRegistry`/`RunContext` (neither is
/// `Clone`) and replays the conversation from the top — the same
/// probe-then-inject-into-cache mechanism `resume()` uses, since a
/// suspension's cache key can only be discovered by actually running into
/// it (see `resume`'s doc comment for why two separate contexts are
/// needed even there). Always reports in plain text — `--output` is
/// ignored, since a live terminal Q&A and a machine-readable trace dump
/// don't compose cleanly, and text is the natural mode for a human
/// answering questions in real time.
#[allow(clippy::too_many_arguments)]
fn run_interactive(
    loaded: &pipeline::Loaded,
    file: &Path,
    conversation: &str,
    args: &BTreeMap<String, Value>,
    run_id: &str,
    selected_providers: &[String],
    force_mock: bool,
) -> bool {
    loop {
        let providers = match providers::resolve_providers(
            file,
            &loaded.provider_decls,
            selected_providers,
            force_mock,
        ) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("error: {e}");
                return false;
            }
        };
        let ctx = match build_context(&loaded.ir, providers, file, run_id) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("error: could not set up run context: {e}");
                return false;
            }
        };
        match ulx_runtime::run_conversation(&ctx, conversation, args.clone()) {
            Ok(value) => {
                println!("{value}");
                eprintln!("run id: {run_id}");
                return true;
            }
            Err(RuntimeError::Suspended {
                cache_key,
                reason,
                target,
            }) => match prompt_decision(&target, &reason) {
                Some(decision) => {
                    if let Err(e) = ctx.cache.put(&cache_key, &decision) {
                        eprintln!("error: could not record decision: {e}");
                        return false;
                    }
                    // Loop: rebuild fresh and replay now that the decision
                    // is cached — may hit another `escalate` further along.
                }
                None => {
                    println!("suspended: waiting on `{target}` — {reason}");
                    println!("run id: {run_id}");
                    println!(
                        "resume with: ulx approve {run_id} --value <text>   (or: ulx deny {run_id})"
                    );
                    return false;
                }
            },
            Err(e) => {
                eprintln!("error: {e}");
                return false;
            }
        }
    }
}

/// Prompts at the terminal for a suspended run's decision. `None` means
/// the user chose to leave it suspended for a later `ulx approve`/`ulx
/// deny` rather than decide now.
fn prompt_decision(target: &str, reason: &str) -> Option<Value> {
    use std::io::Write;

    println!("suspended: waiting on `{target}` — {reason}");
    loop {
        print!("[a]pprove, [d]eny, or [q]uit (leave suspended)? ");
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        if std::io::stdin().read_line(&mut line).is_err() || line.is_empty() {
            // EOF (e.g. stdin isn't a terminal) — treat like "quit".
            return None;
        }
        match line.trim().to_ascii_lowercase().as_str() {
            "a" | "approve" => {
                print!("value (default \"approved\"): ");
                let _ = std::io::stdout().flush();
                let mut value = String::new();
                let _ = std::io::stdin().read_line(&mut value);
                let value = value.trim();
                let value = if value.is_empty() {
                    "approved".to_string()
                } else {
                    value.to_string()
                };
                return Some(Value::Text(value));
            }
            "d" | "deny" => {
                print!("note (optional): ");
                let _ = std::io::stdout().flush();
                let mut note = String::new();
                let _ = std::io::stdin().read_line(&mut note);
                let note = note.trim();
                let reason = if note.is_empty() {
                    "denied by human reviewer".to_string()
                } else {
                    note.to_string()
                };
                return Some(Value::Verdict(ulx_runtime::value::Verdict::Fail(reason)));
            }
            "q" | "quit" => return None,
            other => {
                println!("please answer a/d/q (got {other:?})");
            }
        }
    }
}

/// Statically walks `conversation`'s compiled IR and reports every
/// capability/judge call it will make, the provider each resolves to under
/// current policy, and a rough token/cost estimate (§10.5) — never invokes
/// a provider, only `ProviderRegistry::resolve`/`resolve_named` (see
/// `plan.rs`'s module docs for exactly how approximate the estimate is).
fn cmd_plan(
    file: &Path,
    conversation: &str,
    raw_args: &[String],
    selected_providers: &[String],
) -> bool {
    let Some(loaded) = pipeline::load(file) else {
        return false;
    };
    let known_vars = match parse_args(raw_args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("error: {e}");
            return false;
        }
    };

    let (registry, infos) = match providers::resolve_providers_with_info(
        file,
        &loaded.provider_decls,
        selected_providers,
    ) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {e}");
            return false;
        }
    };

    let rows = match plan::build_plan(&loaded.ir, conversation, &registry, &infos, &known_vars) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {e}");
            return false;
        }
    };

    plan::print_plan(conversation, &rows)
}

/// `Text` (the default) is handled inline here, byte-for-byte what `ulx`
/// printed before `--output` existed — every other format is rendered by
/// `output.rs` instead. `Json` needs only the final outcome; `Jsonl`/
/// `Mermaid`/`Html` need the whole trace, so those re-read the trace file
/// `run_conversation` just finished writing — a cheap local read, and it
/// keeps this feature entirely CLI-side with no `ulx-runtime` changes.
fn execute(
    ctx: &RunContext,
    conversation: &str,
    args: BTreeMap<String, Value>,
    run_id: &str,
    output: OutputFormat,
) -> bool {
    let result = ulx_runtime::run_conversation(ctx, conversation, args);
    if let OutputFormat::Text = output {
        return match result {
            Ok(value) => {
                println!("{value}");
                eprintln!("run id: {run_id}");
                true
            }
            Err(RuntimeError::Suspended { reason, target, .. }) => {
                println!("suspended: waiting on `{target}` — {reason}");
                println!("run id: {run_id}");
                println!(
                    "resume with: ulx approve {run_id} --value <text>   (or: ulx deny {run_id})"
                );
                false
            }
            Err(e) => {
                eprintln!("error: {e}");
                false
            }
        };
    }

    let (ok, outcome) = match &result {
        Ok(value) => (true, RunOutcome::Value { run_id, value }),
        Err(RuntimeError::Suspended { reason, target, .. }) => (
            false,
            RunOutcome::Suspended {
                run_id,
                reason,
                target,
            },
        ),
        Err(e) => (
            false,
            RunOutcome::Error {
                run_id,
                message: e.to_string(),
            },
        ),
    };
    match output {
        OutputFormat::Json => println!("{}", output::render_run_json(&outcome)),
        OutputFormat::Jsonl | OutputFormat::Mermaid | OutputFormat::Html => {
            let records =
                ulx_runtime::read_trace(manifest::traces_dir(), run_id).unwrap_or_default();
            println!("{}", output::render_trace(output, &records));
        }
        OutputFormat::Text => unreachable!("returned above"),
    }
    ok
}

fn default_bench_run_id(file: &std::path::Path, benchmark: &str) -> String {
    let input = format!("{}::bench::{benchmark}", file.display());
    ulx_runtime::value::hash_bytes(input.as_bytes())[..16].to_string()
}

fn cmd_bench(
    file: &Path,
    benchmark: &str,
    selected_providers: &[String],
    force_mock: bool,
) -> bool {
    let Some(loaded) = pipeline::load(file) else {
        return false;
    };
    let run_id = default_bench_run_id(file, benchmark);

    let providers = match providers::resolve_providers(
        file,
        &loaded.provider_decls,
        selected_providers,
        force_mock,
    ) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            return false;
        }
    };
    let ctx = match build_context(&loaded.ir, providers, file, &run_id) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: could not set up run context: {e}");
            return false;
        }
    };

    match ulx_runtime::run_benchmark(&ctx, benchmark) {
        Ok(report) => {
            for row in &report.rows {
                let status = if row.passed() { "PASS" } else { "FAIL" };
                println!("row {}: {status}", row.row_index);
                for check in &row.checks {
                    if !check.passed {
                        let reason = check
                            .message
                            .as_deref()
                            .map(|m| format!(": {m}"))
                            .unwrap_or_default();
                        println!("  - {} failed{reason}", check.kind);
                    }
                }
            }
            println!(
                "{}: {}/{} row(s) passed",
                report.name,
                report.passed_count(),
                report.total()
            );
            report.all_passed()
        }
        Err(e) => {
            eprintln!("error: {e}");
            false
        }
    }
}

fn resume(
    run_id: &str,
    decision: Value,
    selected_providers: &[String],
    force_mock: bool,
    output: OutputFormat,
) -> bool {
    let manifest = match manifest::load(run_id) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("error: no run manifest for `{run_id}`: {e}");
            return false;
        }
    };
    let Some(loaded) = pipeline::load(&manifest.file) else {
        return false;
    };
    let args: BTreeMap<String, Value> = manifest
        .args
        .iter()
        .map(|(k, v)| (k.clone(), Value::Text(v.clone())))
        .collect();

    // Default to whatever `ulx run` originally used (persisted in the
    // manifest, §24.11) so `approve`/`deny` need no flags at all in the
    // common case — an explicit `--provider`/`--mock` on this command
    // still overrides it, for the rare case that's deliberate (e.g.
    // migrating a run to a provider config that didn't exist yet when it
    // was started).
    let (selected_providers, force_mock): (&[String], bool) =
        if selected_providers.is_empty() && !force_mock {
            (manifest.selected_providers.as_slice(), manifest.force_mock)
        } else {
            (selected_providers, force_mock)
        };

    // Re-run once (cache-miss) to discover the pending escalate's exact
    // cache key, then record the decision under it and run again in a
    // *fresh* `RunContext` — this is the resume mechanism `ulx-runtime`'s
    // docs describe. The two `run_conversation` calls deliberately use
    // separate contexts (each needs its own freshly-resolved
    // `ProviderRegistry`, since it isn't `Clone`): escalate cache keys mix
    // in a per-context sequence counter to disambiguate multiple escalate
    // call sites (see `interp.rs`), so reusing one context across both
    // calls would advance that counter and compute a *different* key on
    // the second call, missing the very decision just recorded.
    let probe_providers = match providers::resolve_providers(
        &manifest.file,
        &loaded.provider_decls,
        selected_providers,
        force_mock,
    ) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            return false;
        }
    };
    let probe_ctx = match build_context(&loaded.ir, probe_providers, &manifest.file, run_id) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: could not set up run context: {e}");
            return false;
        }
    };
    match ulx_runtime::run_conversation(&probe_ctx, &manifest.conversation, args.clone()) {
        Err(RuntimeError::Suspended { cache_key, .. }) => {
            if let Err(e) = probe_ctx.cache.put(&cache_key, &decision) {
                eprintln!("error: could not record decision: {e}");
                return false;
            }
        }
        Ok(_) => {
            eprintln!("run `{run_id}` was not suspended (it already completed)");
            return false;
        }
        Err(e) => {
            eprintln!("error: {e}");
            return false;
        }
    }

    let providers = match providers::resolve_providers(
        &manifest.file,
        &loaded.provider_decls,
        selected_providers,
        force_mock,
    ) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            return false;
        }
    };
    let ctx = match build_context(&loaded.ir, providers, &manifest.file, run_id) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: could not set up run context: {e}");
            return false;
        }
    };
    execute(&ctx, &manifest.conversation, args, run_id, output)
}

fn cmd_approve(
    run_id: &str,
    value: &str,
    selected_providers: &[String],
    force_mock: bool,
    output: OutputFormat,
) -> bool {
    resume(
        run_id,
        Value::Text(value.to_string()),
        selected_providers,
        force_mock,
        output,
    )
}

fn cmd_deny(
    run_id: &str,
    note: Option<&str>,
    selected_providers: &[String],
    force_mock: bool,
    output: OutputFormat,
) -> bool {
    let reason = note.unwrap_or("denied by human reviewer").to_string();
    println!("run `{run_id}` denied: {reason}");
    println!("note: v0.1 does not distinguish deny-and-abort from deny-with-value at the type level (§24) —");
    println!("      the denial is recorded as the escalate's resolved value, same as an approval would be.");
    resume(
        run_id,
        Value::Verdict(ulx_runtime::value::Verdict::Fail(reason)),
        selected_providers,
        force_mock,
        output,
    )
}

fn cmd_replay(run_id: &str, output: OutputFormat) -> bool {
    let manifest = match manifest::load(run_id) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("error: no run manifest for `{run_id}`: {e}");
            return false;
        }
    };
    let Some(loaded) = pipeline::load(&manifest.file) else {
        return false;
    };
    // Replay never invokes a provider for real (`invoke_cached`'s
    // `replay_only` branch short-circuits on every cache hit before any
    // `provider.invoke` call) — but cache keys are derived partly from
    // `provider.id()` (interp.rs), so replay still needs the *same*
    // provider ids the original run used to find matching cache entries.
    // Resolve using the original run's persisted `--provider`/`--mock`
    // selection (§24.11) first; only if that fails (nothing configured
    // at all, or a named provider no longer exists) fall back to the mock
    // registry, whose `id()`s match what an unconfigured/`--mock` original
    // run must have used.
    let providers = match providers::resolve_providers(
        &manifest.file,
        &loaded.provider_decls,
        &manifest.selected_providers,
        manifest.force_mock,
    ) {
        Ok(p) => p,
        Err(_) => ProviderRegistry::with_mock(),
    };
    let ctx = match build_context(&loaded.ir, providers, &manifest.file, run_id) {
        Ok(c) => c.replaying(),
        Err(e) => {
            eprintln!("error: could not set up run context: {e}");
            return false;
        }
    };
    let args: BTreeMap<String, Value> = manifest
        .args
        .iter()
        .map(|(k, v)| (k.clone(), Value::Text(v.clone())))
        .collect();
    execute(&ctx, &manifest.conversation, args, run_id, output)
}

fn cmd_trace(run_id: &str, output: OutputFormat) -> bool {
    match ulx_runtime::read_trace(manifest::traces_dir(), run_id) {
        Ok(records) => {
            if let OutputFormat::Text = output {
                for r in &records {
                    let status = if r.cache_hit {
                        "hit "
                    } else if r.error.is_some() {
                        "err "
                    } else {
                        "miss"
                    };
                    let cap = r.capability.as_deref().unwrap_or("-");
                    let out = r
                        .output
                        .as_ref()
                        .map(|v| v.to_string())
                        .or_else(|| r.error.clone())
                        .unwrap_or_default();
                    println!(
                        "#{:<3} [{status}] {:<10} {}",
                        r.seq,
                        cap,
                        output::truncate(&out, 100)
                    );
                }
            } else {
                println!("{}", output::render_trace(output, &records));
            }
            true
        }
        Err(e) => {
            eprintln!("error: could not read trace for `{run_id}`: {e}");
            false
        }
    }
}

fn cmd_manifest(file: &Path) -> bool {
    match project_manifest::load(file) {
        Ok(m) => {
            println!(
                "package: {} v{} (requires ulexite {})",
                m.package.name, m.package.version, m.package.ulexite
            );
            if m.dependencies.is_empty() {
                println!("dependencies: (none)");
            } else {
                println!("dependencies:");
                for (name, dep) in &m.dependencies {
                    match dep {
                        project_manifest::Dependency::Version(v) => println!("  {name} = \"{v}\""),
                        project_manifest::Dependency::Detailed { git, path, tag } => {
                            let src = git
                                .as_deref()
                                .map(|g| format!("git={g}"))
                                .or_else(|| path.as_deref().map(|p| format!("path={p}")))
                                .unwrap_or_default();
                            let tag = tag
                                .as_deref()
                                .map(|t| format!(", tag={t}"))
                                .unwrap_or_default();
                            println!("  {name}: {src}{tag}");
                        }
                    }
                }
            }
            if !m.providers.is_empty() {
                println!("providers:");
                for (name, p) in &m.providers {
                    let capabilities = p
                        .capabilities
                        .keys()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ");
                    println!(
                        "  {name}: vendor={}, capabilities=[{capabilities}]",
                        p.vendor
                    );
                }
            }
            println!(
                "runtime: concurrency={}, cache_backend={}",
                m.runtime.concurrency, m.runtime.cache_backend
            );
            true
        }
        Err(e) => {
            eprintln!("error: {} : {e}", file.display());
            false
        }
    }
}

fn cmd_init(name: &str, dir: &Path) -> bool {
    if let Err(e) = std::fs::create_dir_all(dir) {
        eprintln!("error: could not create {}: {e}", dir.display());
        return false;
    }
    let manifest_path = dir.join("ulexite.toml");
    if manifest_path.exists() {
        eprintln!("error: {} already exists", manifest_path.display());
        return false;
    }
    let manifest_src = format!(
        r#"[package]
name = "{name}"
version = "0.1.0"
ulexite = "^0.1"

[dependencies]

[runtime]
concurrency = 4
cache_backend = "local"
"#
    );
    if let Err(e) = std::fs::write(&manifest_path, manifest_src) {
        eprintln!("error: could not write {}: {e}", manifest_path.display());
        return false;
    }

    let main_src = r#"conversation Hello(name: text) -> text {
  system: """You are a friendly assistant."""
  user: """Say hello to {name}."""
  assistant -> greeting: text
  greeting
}
"#;
    let main_path = dir.join("main.ulx");
    if let Err(e) = std::fs::write(&main_path, main_src) {
        eprintln!("error: could not write {}: {e}", main_path.display());
        return false;
    }

    println!("created {}", manifest_path.display());
    println!("created {}", main_path.display());
    println!(
        "try: ulx run {} Hello --arg name=world --mock   (or configure a real provider — see README's \"Configuring providers\")",
        main_path.display()
    );
    true
}
