//! `ulx` — the Ulexite CLI (§20.12).
//!
//! Implemented: `parse`, `check` (§20.7's diagnostics — also exposed live
//! through `ulx-lsp`, §20.2's language server), `run`, `bench` (§16 —
//! dataset-parametrized `benchmark` execution, `--run-id`-resumable when a
//! row suspends on a real `escalate(...)`, same mechanism as `run`; see
//! `ulx-runtime::run_benchmark` for the narrower-than-spec scope that's
//! still real: `snapshot` compares against a persisted golden baseline
//! (`--update-snapshots` accepts a new one) via exact value equality
//! rather than §16.5's semantic diff; `expect` resamples a failing judge
//! verdict up to a fixed (not yet per-statement-configurable) 3-attempt
//! budget; there's still no `metrics.*` aggregation or JUnit/JSON
//! report — a plain-text per-row pass/fail/suspended report), `approve`/`deny`
//! (§10.7's human-approval resume, v0.1-style, for both a conversation and
//! a benchmark run — see `ulx-runtime`'s docs for how that actually
//! works), `replay` (§18.3), `trace` (§20.6 — no viewer webview, but
//! `--output mermaid`/`html` render a shareable diagram/page in its place;
//! see `output.rs`).
//!
//! Also implemented: `plan` (§10.5 — a static, execution-free capability ->
//! provider resolution plus a rough token/cost estimate off a small
//! illustrative pricing table; see `plan.rs`'s module docs for exactly how
//! approximate that estimate is), `fmt` (§20.10 — an AST-based
//! pretty-printer; see `ulx_syntax::fmt` for the important caveat that it
//! does not preserve comments), `debug` (§19 — an interactive stepper over
//! an already-recorded trace: forward/backward stepping, breakpoints by
//! record seq, full inspection, and a call-stack view; see `debug.rs`'s
//! module docs for the narrower-than-spec scope — no `breakpoint()`
//! language keyword, no `ulx fork`/re-run-with-edits, no `ulx attach` to a
//! live process), `eval calibrate` (§17.1 — runs a judge against a
//! human-labeled dataset and reports agreement; see
//! `ulx_runtime::calibrate`'s module docs for why the labeled-dataset row
//! shape is narrower than §17.1's own example). `eval`'s other
//! subcommands (`shadow`/`trend`/`sweep`) aren't implemented.
//!
//! Also implemented, outside the spec's own command list: `from-md` —
//! compiles the simplified Markdown authoring format `docs/simple-format.md`
//! describes (see `md.rs`) into `.ulx` source. Every other command that
//! takes a `.ulx` file (`run`, `check`, `bench`, `plan`, `fmt`, `replay`,
//! `approve`/`deny`) also accepts a `.md` source directly — see
//! `pipeline::resolve_entry` — compiling it to a generated `.ulx` under
//! `.ulexite/generated/` on the fly, so `from-md` is only needed when you
//! actually want the compiled `.ulx` file itself (to read, commit, or
//! hand-edit from there).
//!
//! Not implemented: `doc`/`repl` (§20), `ulx fork`, `ulx attach`. See
//! `docs/spec/25-future-directions.md`.

mod debug;
mod diagnostics;
mod git_dep;
mod manifest;
mod md;
mod output;
mod pipeline;
mod plan;
mod project_manifest;
mod providers;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use output::{OutputFormat, RunOutcome};
use ulx_runtime::{
    Cache, ProviderRegistry, RecordCallback, RunContext, RuntimeError, SendCallback, TraceRecord,
    TraceWriter, Value,
};

#[derive(Parser)]
#[command(name = "ulx", about = "Ulexite language CLI", version)]
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
        /// Reuse a specific run id — the only way to get a *stable*,
        /// reproducible one across separate invocations (e.g. to `ulx
        /// approve`/`ulx trace` a suspended run later). Without this, a
        /// fresh, unique run id is generated every time, even for the
        /// exact same file/conversation/args, so two unrelated `ulx run`
        /// invocations never collide on one trace file/manifest/escalate
        /// cache key.
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
        /// Output format: `text` (default, a colorized/emoji dialogue
        /// transcript when stdout is a terminal), `plain` (the same
        /// transcript with no color/emoji/spacing), `json`, `jsonl`,
        /// `mermaid`, or `html`. The last four render the run's full
        /// trace, not just its final value. Ignored when `--interactive`
        /// is set, which always reports the way `plain` does.
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
        /// Skip the cache for `ask`/`judge` calls, forcing a fresh live
        /// call every time instead of reusing a prior identical result —
        /// useful when iterating on a prompt/rubric under the same
        /// `--run-id`/args, where a stale cache hit would otherwise hide
        /// the change. Never bypasses `escalate`'s own cache entry, since
        /// that's the persistence mechanism `ulx approve`/`ulx
        /// deny`/`--interactive` rely on, not a cost optimization.
        #[arg(long)]
        no_cache: bool,
    },
    /// Run a `benchmark` declaration to completion (§16): loads its
    /// `dataset:`, runs the benchmark body once per row, and prints a
    /// pass/fail report. Errors if no provider is configured — pass
    /// --mock to run against the deterministic mock provider instead.
    Bench {
        file: PathBuf,
        benchmark: String,
        /// Reuse a specific run id — needed to resume a row that suspends
        /// on a human-approval `escalate(...)` (`ulx approve <id>`/`ulx
        /// deny <id>`, same as `ulx run`'s `--run-id`). Without this, a
        /// fresh id is generated every time, so a suspended row can still
        /// be inspected via the printed report but not resumed by id.
        #[arg(long)]
        run_id: Option<String>,
        #[arg(long = "provider", value_name = "NAME")]
        providers: Vec<String>,
        #[arg(long, conflicts_with = "providers")]
        mock: bool,
        /// Unconditionally overwrite every `snapshot` statement's stored
        /// golden baseline with this run's freshly-evaluated value, instead
        /// of comparing against it (§16.5) — for accepting an intentional
        /// change to a benchmark's expected output.
        #[arg(long)]
        update_snapshots: bool,
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
    /// Interactively step through a completed or suspended run's trace
    /// (§19 — a deliberately narrower slice: see `debug.rs`'s module docs
    /// for exactly what this does and doesn't cover).
    Debug { run_id: String },
    /// Evaluation-methodology commands built on top of the same runtime
    /// `ulx bench` uses (§17). Only `calibrate` is implemented — §17's
    /// `shadow`/`trend`/`sweep` aren't (see `docs/spec/17-evaluation-framework.md`).
    Eval {
        #[command(subcommand)]
        command: EvalCommand,
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
    /// Compile a simplified Markdown conversation (see `docs/simple-format.md`)
    /// into `.ulx` source — a title and a paragraph is enough; `## System`/
    /// `## Judge` sections and a ```ulx-meta` block are opt-in escape
    /// hatches. Prints to stdout unless `--output` is given.
    #[command(name = "from-md")]
    FromMd {
        file: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum EvalCommand {
    /// Runs a judge against a human-labeled dataset and reports how often
    /// its pass/fail verdict agrees with the human label (§17.1). The
    /// dataset's row type must be `{subject: <any>, human_pass: bool}` —
    /// see `ulx_runtime::calibrate`'s module docs for why this is a
    /// simpler, honest scoping of §17.1's full `{subject, human_verdict:
    /// Verdict}` example (the dataset loader can't produce a `Verdict`
    /// value from JSONL today).
    Calibrate {
        file: PathBuf,
        dataset: String,
        judge: String,
        /// Minimum agreement rate (0.0-1.0) for this command to succeed —
        /// defaults to 0.8, the same default `expect ... satisfies judge
        /// ...` uses when no `with threshold(...)` is written.
        #[arg(long)]
        threshold: Option<f64>,
        #[arg(long = "provider", value_name = "NAME")]
        providers: Vec<String>,
        #[arg(long, conflicts_with = "providers")]
        mock: bool,
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
            no_cache,
        } => cmd_run(
            &file,
            &conversation,
            &args,
            run_id,
            &providers,
            mock,
            output,
            interactive,
            no_cache,
        ),
        Command::Bench {
            file,
            benchmark,
            run_id,
            providers,
            mock,
            update_snapshots,
        } => cmd_bench(
            &file,
            &benchmark,
            run_id,
            &providers,
            mock,
            update_snapshots,
        ),
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
        Command::Debug { run_id } => cmd_debug(&run_id),
        Command::Eval { command } => match command {
            EvalCommand::Calibrate {
                file,
                dataset,
                judge,
                threshold,
                providers,
                mock,
            } => cmd_eval_calibrate(&file, &dataset, &judge, threshold, &providers, mock),
        },
        Command::Init { name, dir } => cmd_init(&name, &dir),
        Command::Manifest { file } => cmd_manifest(&file),
        Command::Fmt { file, check } => cmd_fmt(&file, check),
        Command::FromMd { file, output } => cmd_from_md(&file, output.as_deref()),
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

/// `ulx from-md`: parses the Markdown dialect `md.rs` documents and prints
/// (or writes) the `.ulx` it compiles to. Mirrors `cmd_fmt`'s safety net —
/// the generated source is always re-parsed before it's handed back, so a
/// bug in the generator surfaces as a clear internal error here rather than
/// as a confusing parse failure the next time someone runs `ulx check` on
/// the output.
fn cmd_from_md(file: &Path, output: Option<&Path>) -> bool {
    let src = match std::fs::read_to_string(file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: could not read {}: {e}", file.display());
            return false;
        }
    };
    let conv = match md::parse_md(&src) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {}: {e}", file.display());
            return false;
        }
    };
    let ulx = md::render_ulx(&conv);

    if let Err(errors) = ulx_syntax::parse_source(&ulx) {
        eprintln!(
            "error: internal error: the .ulx generated from {} does not parse",
            file.display()
        );
        for e in &errors {
            diagnostics::report_parse_error("(generated)", &ulx, e);
        }
        return false;
    }

    match output {
        Some(path) => {
            if let Err(e) = std::fs::write(path, &ulx) {
                eprintln!("error: could not write {}: {e}", path.display());
                return false;
            }
            println!("wrote {}", path.display());
        }
        None => print!("{ulx}"),
    }
    true
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

/// Checks every `--arg` bound to a parameter declared with a content-sniffable
/// artifact type (`pdf`/`image`/`audio`/`video`, §9.2) against the file it
/// actually names, so e.g. a `doc: pdf` parameter fed a PNG fails fast with a
/// clear message instead of running to completion (or failing much later at
/// the provider boundary). Unknown conversation names are left for the
/// runtime's own error to report — this only validates params it can find.
fn validate_run_args(
    ir: &ulx_ir::IrProgram,
    conversation: &str,
    args: &BTreeMap<String, String>,
) -> Result<(), String> {
    let Some(conv) = ir.conversations.iter().find(|c| c.name == conversation) else {
        return Ok(());
    };
    for (name, ty) in &conv.params {
        let ulx_ast::TypeExpr::Artifact(kind) = ty else {
            continue;
        };
        if let Some(value) = args.get(name) {
            ulx_runtime::validate_artifact_arg(*kind, value)
                .map_err(|e| format!("--arg {name}: {e}"))?;
        }
    }
    Ok(())
}

/// Whether the dialogue transcript (`Text` output) should include ANSI
/// color: only when stdout is an actual terminal, and the user hasn't set
/// `NO_COLOR` (https://no-color.org) — piping into `jq`, a file, or
/// another program must always get plain text, never raw escape codes.
/// Role emoji aren't gated on this: they're plain UTF-8, not control
/// codes, so they're harmless in a pipe/file the way ANSI color isn't.
fn use_color() -> bool {
    use std::io::IsTerminal;
    std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none()
}

/// A per-process, per-instant nonce mixed into every auto-derived run id
/// (`default_run_id`/`default_bench_run_id`) below, so two separate
/// invocations of the exact same command never collide on one run id —
/// and therefore one trace file, one persisted `RunManifest`, and (for a
/// program using `escalate`) one cache key for a human decision. Before
/// this, `default_run_id` was a pure hash of file+conversation+args, so
/// e.g. two people (or two CI jobs) running `ulx run translate.ulx
/// Translate --arg source=hello --arg target_lang=fr --mock` at different
/// times would silently share a run id, appending to each other's trace
/// log and manifest. `ask`/`judge` caching is unaffected either way — it's
/// content-addressed, independent of run_id (see `cache_key` in
/// `ulx-runtime`). Nanosecond time plus pid isn't cryptographically
/// unique, but is far more than enough to avoid a practical collision
/// between `ulx` invocations; anyone who wants a *stable*, reproducible
/// run id across reruns (e.g. the README's suspend/resume demo) already
/// passes `--run-id` explicitly, which skips this entirely.
fn run_nonce() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}-{}", now.as_nanos(), std::process::id())
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
    input.push('|');
    input.push_str(&run_nonce());
    ulx_runtime::value::hash_bytes(input.as_bytes())[..16].to_string()
}

/// Builds a `RunContext` from an already-resolved `ProviderRegistry` — the
/// registry resolution itself (`providers::resolve_providers`, needing the
/// CLI's `--provider`/`--mock` flags and the loaded `provider_decls`) is the
/// caller's job, so `cmd_replay` can special-case its own fallback path.
///
/// `printers`, when given, stream the run's dialogue live (see
/// `dialogue_printers`) — `None` for callers that must not print anything
/// (`cmd_bench`'s own PASS/FAIL report, `resume`'s discovery-only probe
/// context) or that render some other way (`--output json`/`jsonl`/
/// `mermaid`/`html`, which only ever read the whole trace back after the
/// fact).
fn build_context<'a>(
    ir: &'a ulx_ir::IrProgram,
    providers: ProviderRegistry,
    file: &std::path::Path,
    run_id: &str,
    no_cache: bool,
    printers: Option<(SendCallback, RecordCallback)>,
) -> std::io::Result<RunContext<'a>> {
    let cache = Cache::new(manifest::cache_dir())?;
    let mut trace = TraceWriter::create(manifest::traces_dir(), run_id)?;
    if let Some((on_send, on_record)) = printers {
        trace = trace.with_on_send(on_send).with_on_record(on_record);
    }
    let ctx = RunContext::new(
        ir,
        providers,
        cache,
        trace,
        run_id.to_string(),
        manifest::base_dir_of(file),
    );
    Ok(if no_cache { ctx.without_cache() } else { ctx })
}

/// Builds `build_context`'s live-streaming pair for `--output text`/
/// `plain`: `on_send` prints a `chat`/`vision` call's `system`/`user`
/// messages (`output::render_sent`) the instant the call is made, and
/// `on_record` prints each record's remaining turn(s) (`output::
/// render_record_live`) the instant it completes — together, a run's
/// dialogue streams turn by turn as it actually happens, rather than
/// `execute` only being able to render the transcript after
/// `run_conversation` returns in full, and rather than a `chat`/`vision`
/// call's `system`/`user`/`assistant` turns all landing at once the moment
/// the (potentially seconds-long) call finishes. `None` for every other
/// format, which reads the trace back whole instead. `Text`'s blank line
/// between turns is reproduced by printing each block with a trailing
/// blank line; `Plain` prints with no spacing at all, matching
/// `render_dialogue_plain`'s fully-joined form (`execute` adds back the
/// one separator blank line `Plain` needs before the final value/status,
/// since none of the per-record prints put one there).
fn dialogue_printers(output: OutputFormat) -> Option<(SendCallback, RecordCallback)> {
    let plain = match output {
        OutputFormat::Text => false,
        OutputFormat::Plain => true,
        _ => return None,
    };
    let color = !plain && use_color();
    let print = move |s: String| {
        if s.is_empty() {
            return;
        }
        if plain {
            println!("{s}");
        } else {
            println!("{s}\n");
        }
    };
    let on_send: SendCallback = Box::new(
        move |capability: &str, input: &[ulx_runtime::provider::Message]| {
            print(output::render_sent(capability, input, plain, color));
        },
    );
    let on_record: RecordCallback = Box::new(move |r: &TraceRecord| {
        print(output::render_record_live(r, plain, color));
    });
    Some((on_send, on_record))
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
    no_cache: bool,
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
    if let Err(e) = validate_run_args(&loaded.ir, conversation, &args) {
        eprintln!("error: {e}");
        return false;
    }
    let run_id = run_id.unwrap_or_else(|| default_run_id(file, conversation, &args));

    if let Err(e) = manifest::save(
        &run_id,
        &manifest::RunManifest {
            file: file.to_path_buf(),
            conversation: conversation.to_string(),
            benchmark: None,
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
            no_cache,
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
    let ctx = match build_context(
        &loaded.ir,
        providers,
        file,
        &run_id,
        no_cache,
        dialogue_printers(output),
    ) {
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
    no_cache: bool,
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
        let ctx = match build_context(
            &loaded.ir,
            providers,
            file,
            run_id,
            no_cache,
            dialogue_printers(OutputFormat::Plain),
        ) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("error: could not set up run context: {e}");
                return false;
            }
        };
        match ulx_runtime::run_conversation(&ctx, conversation, args.clone()) {
            Ok(value) => {
                let records =
                    ulx_runtime::read_trace(manifest::traces_dir(), run_id).unwrap_or_default();
                if !output::dialogue_turns(&records).is_empty() {
                    println!();
                }
                println!("{value}");
                eprintln!(
                    "{}",
                    output::render_metadata_text(run_id, "ok", &records, false)
                );
                return true;
            }
            Err(RuntimeError::Suspended {
                cache_key,
                reason,
                target,
                ..
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

/// `Text` (the default) and `Plain` both print the run's dialogue
/// transcript (the `system`/`user`/`assistant`/`judge`/`escalate` turns
/// `output::dialogue_turns`/`render_record_live` reconstruct from the
/// trace `run_conversation` writes — a translate call reads back as a
/// conversation, not a bare final value) followed by the same final-value/
/// suspended/error lines `ulx` printed before `--output`/the dialogue
/// transcript existed. The transcript itself is streamed live, turn by
/// turn — a `chat`/`vision` call's `system`/`user` messages print the
/// instant the call is *made* (`on_send`), and every call's remaining
/// turn(s) print the instant it *completes* (`on_record`) — via
/// `dialogue_printers`, attached to the `RunContext`'s `TraceWriter`
/// before this function is ever called — rather than read back and
/// printed in one block after `run_conversation` returns; `execute` only
/// re-reads the trace file afterward for the metadata footer, which needs
/// the whole run to compute (every capability/provider it touched).
/// `Plain` differs from `Text` only in what `dialogue_printers` streamed
/// (no emoji, no color, no blank-line spacing) — see its doc comment for
/// why only `Plain` needs one blank line added back here. Every other
/// format is rendered by `output.rs`: `Json` embeds the same dialogue as a
/// `messages` array alongside `run_id`/`status`/`value`; `Jsonl`/
/// `Mermaid`/`Html` render the whole trace — all read back from the trace
/// file after the fact, same as before.
fn execute(
    ctx: &RunContext,
    conversation: &str,
    args: BTreeMap<String, Value>,
    run_id: &str,
    output: OutputFormat,
) -> bool {
    let result = ulx_runtime::run_conversation(ctx, conversation, args);
    let records = ulx_runtime::read_trace(manifest::traces_dir(), run_id).unwrap_or_default();

    if let OutputFormat::Text | OutputFormat::Plain = output {
        let color = output == OutputFormat::Text && use_color();
        if output == OutputFormat::Plain && !output::dialogue_turns(&records).is_empty() {
            println!();
        }
        return match result {
            Ok(value) => {
                println!("{value}");
                eprintln!(
                    "{}",
                    output::render_metadata_text(run_id, "ok", &records, color)
                );
                true
            }
            Err(RuntimeError::Suspended { reason, target, .. }) => {
                println!("suspended: waiting on `{target}` — {reason}");
                println!(
                    "{}",
                    output::render_metadata_text(run_id, "suspended", &records, color)
                );
                println!(
                    "resume with: ulx approve {run_id} --value <text>   (or: ulx deny {run_id})"
                );
                false
            }
            Err(e) => {
                eprintln!("error: {e}");
                eprintln!(
                    "{}",
                    output::render_metadata_text(run_id, "error", &records, color)
                );
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
        OutputFormat::Json => println!("{}", output::render_run_json(&outcome, &records)),
        OutputFormat::Jsonl | OutputFormat::Mermaid | OutputFormat::Html => {
            println!("{}", output::render_trace(output, &records));
        }
        OutputFormat::Text | OutputFormat::Plain => unreachable!("returned above"),
    }
    ok
}

fn default_bench_run_id(file: &std::path::Path, benchmark: &str) -> String {
    let input = format!("{}::bench::{benchmark}::{}", file.display(), run_nonce());
    ulx_runtime::value::hash_bytes(input.as_bytes())[..16].to_string()
}

/// Shared by `cmd_bench` and `resume_benchmark` so a freshly-run report and
/// a just-resumed one print identically. A suspended row prints its own
/// resume instructions (mirroring `execute`'s `suspended: ...`/`resume
/// with: ulx approve ...` lines for an ordinary conversation run) rather
/// than a PASS/FAIL verdict, since it hasn't reached one yet.
fn print_benchmark_report(report: &ulx_runtime::BenchmarkReport, run_id: &str) {
    for row in &report.rows {
        match &row.outcome {
            ulx_runtime::BenchmarkRowOutcome::Suspended { reason, target, .. } => {
                println!(
                    "row {}: SUSPENDED — waiting on `{target}` — {reason}",
                    row.row_index
                );
            }
            ulx_runtime::BenchmarkRowOutcome::Completed { .. } => {
                let status = if row.passed() { "PASS" } else { "FAIL" };
                println!("row {}: {status}", row.row_index);
                for check in row.checks() {
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
        }
    }
    println!(
        "{}: {}/{} row(s) passed{}",
        report.name,
        report.passed_count(),
        report.total(),
        if report.has_suspended() {
            format!(
                ", {} suspended",
                report.rows.iter().filter(|r| r.is_suspended()).count()
            )
        } else {
            String::new()
        }
    );
    if let Some(row) = report.suspended_rows().next() {
        println!(
            "resume with: ulx approve {run_id} --value <text>   (or: ulx deny {run_id})   -- resolves row {}",
            row.row_index
        );
    }
}

fn cmd_bench(
    file: &Path,
    benchmark: &str,
    run_id: Option<String>,
    selected_providers: &[String],
    force_mock: bool,
    update_snapshots: bool,
) -> bool {
    let Some(loaded) = pipeline::load(file) else {
        return false;
    };
    let run_id = run_id.unwrap_or_else(|| default_bench_run_id(file, benchmark));

    if let Err(e) = manifest::save(
        &run_id,
        &manifest::RunManifest {
            file: file.to_path_buf(),
            conversation: String::new(),
            benchmark: Some(benchmark.to_string()),
            args: BTreeMap::new(),
            selected_providers: selected_providers.to_vec(),
            force_mock,
        },
    ) {
        eprintln!("warning: could not persist run manifest: {e}");
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
    let ctx = match build_context(&loaded.ir, providers, file, &run_id, false, None) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: could not set up run context: {e}");
            return false;
        }
    };
    let ctx = if update_snapshots {
        ctx.with_update_snapshots()
    } else {
        ctx
    };

    match ulx_runtime::run_benchmark(&ctx, benchmark) {
        Ok(report) => {
            print_benchmark_report(&report, &run_id);
            report.all_passed() && !report.has_suspended()
        }
        Err(e) => {
            eprintln!("error: {e}");
            false
        }
    }
}

fn default_calibrate_run_id(file: &std::path::Path, judge: &str) -> String {
    let input = format!("{}::calibrate::{judge}::{}", file.display(), run_nonce());
    ulx_runtime::value::hash_bytes(input.as_bytes())[..16].to_string()
}

/// `ulx eval calibrate` (§17.1): no resume/manifest persistence — a
/// calibration run doesn't suspend on `escalate` the way a benchmark row
/// can (a judge invoked directly via `invoke_judge_with_subject` never
/// reaches conversation-level control flow), so there's nothing to
/// `ulx approve`/`ulx deny` here and no reason to write a `RunManifest`.
fn cmd_eval_calibrate(
    file: &Path,
    dataset: &str,
    judge: &str,
    threshold: Option<f64>,
    selected_providers: &[String],
    force_mock: bool,
) -> bool {
    let Some(loaded) = pipeline::load(file) else {
        return false;
    };
    let run_id = default_calibrate_run_id(file, judge);

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
    let ctx = match build_context(&loaded.ir, providers, file, &run_id, false, None) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: could not set up run context: {e}");
            return false;
        }
    };

    let gate = threshold.unwrap_or(0.8);
    match ulx_runtime::run_calibration(&ctx, dataset, judge, None) {
        Ok(report) => {
            for row in &report.rows {
                let status = if row.agrees() { "AGREE " } else { "DISAGREE" };
                println!(
                    "row {}: {status} human={} judge={}",
                    row.row_index, row.human_pass, row.judge_verdict
                );
            }
            let rate = report.agreement_rate();
            let verdict = if report.passes_threshold(gate) {
                "PASS"
            } else {
                "FAIL"
            };
            println!(
                "{judge} calibrated against {dataset}: {}/{} agree ({:.1}%) — {verdict} threshold {:.1}%",
                report.rows.len() - report.disagreements().count(),
                report.rows.len(),
                rate * 100.0,
                gate * 100.0
            );
            report.passes_threshold(gate)
        }
        Err(e) => {
            eprintln!("error: {e}");
            false
        }
    }
}

/// Entry point for `cmd_approve`/`cmd_deny`: loads the manifest once to
/// tell a benchmark run (`manifest.benchmark: Some(_)`, from `ulx bench
/// --run-id`) apart from an ordinary conversation run, and dispatches to
/// the matching resume path. `output` only applies to the conversation
/// path — a benchmark's report has its own fixed text format regardless of
/// `--output`, the same way `cmd_bench` doesn't take an `--output` flag
/// today.
fn resume_dispatch(
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
    if manifest.benchmark.is_some() {
        resume_benchmark(run_id, &manifest, decision, selected_providers, force_mock)
    } else {
        resume(run_id, decision, selected_providers, force_mock, output)
    }
}

/// `ulx bench --run-id`'s resume path: the same probe-then-inject-then-
/// rerun mechanism `resume` uses for a conversation (see its doc comment
/// for exactly why two separate `RunContext`s are needed), but targeting
/// `run_benchmark` and resolving the *first* still-suspended row's cache
/// key rather than a single conversation-wide one — a benchmark can have
/// more than one row pending, so this only ever resolves one per call;
/// call `ulx approve`/`ulx deny` again with the same `run_id` for the next.
fn resume_benchmark(
    run_id: &str,
    manifest: &manifest::RunManifest,
    decision: Value,
    selected_providers: &[String],
    force_mock: bool,
) -> bool {
    let benchmark = manifest
        .benchmark
        .as_deref()
        .expect("resume_benchmark only called when manifest.benchmark is Some");
    let Some(loaded) = pipeline::load(&manifest.file) else {
        return false;
    };

    let (selected_providers, force_mock): (&[String], bool) =
        if selected_providers.is_empty() && !force_mock {
            (manifest.selected_providers.as_slice(), manifest.force_mock)
        } else {
            (selected_providers, force_mock)
        };

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
    let probe_ctx = match build_context(
        &loaded.ir,
        probe_providers,
        &manifest.file,
        run_id,
        false,
        None,
    ) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: could not set up run context: {e}");
            return false;
        }
    };
    let probe_report = match ulx_runtime::run_benchmark(&probe_ctx, benchmark) {
        Ok(report) => report,
        Err(e) => {
            eprintln!("error: {e}");
            return false;
        }
    };
    let Some(row) = probe_report.suspended_rows().next() else {
        eprintln!(
            "benchmark run `{run_id}` has no suspended row to resolve \
             (it already completed, or every pending row was already resolved)"
        );
        return false;
    };
    let ulx_runtime::BenchmarkRowOutcome::Suspended { cache_key, .. } = &row.outcome else {
        unreachable!("suspended_rows() only yields Suspended rows");
    };
    if let Err(e) = probe_ctx.cache.put(cache_key, &decision) {
        eprintln!("error: could not record decision: {e}");
        return false;
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
    let ctx = match build_context(&loaded.ir, providers, &manifest.file, run_id, false, None) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: could not set up run context: {e}");
            return false;
        }
    };
    match ulx_runtime::run_benchmark(&ctx, benchmark) {
        Ok(report) => {
            print_benchmark_report(&report, run_id);
            report.all_passed() && !report.has_suspended()
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
    let probe_ctx = match build_context(
        &loaded.ir,
        probe_providers,
        &manifest.file,
        run_id,
        false,
        None,
    ) {
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
    let ctx = match build_context(
        &loaded.ir,
        providers,
        &manifest.file,
        run_id,
        false,
        dialogue_printers(output),
    ) {
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
    resume_dispatch(
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
    // Only for `text`: every other `--output` format promises "one JSON
    // object, always on stdout" (README's "Output formats") or a full
    // trace rendering — these two lines would corrupt either contract
    // (e.g. break a `... --output json | jq` pipeline) if printed
    // unconditionally, the way `cmd_approve` correctly never does.
    if output == OutputFormat::Text {
        println!("run `{run_id}` denied: {reason}");
        println!("note: v0.1 does not distinguish deny-and-abort from deny-with-value at the type level (§24) —");
        println!("      the denial is recorded as the escalate's resolved value, same as an approval would be.");
    }
    resume_dispatch(
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
    let ctx = match build_context(
        &loaded.ir,
        providers,
        &manifest.file,
        run_id,
        false,
        dialogue_printers(output),
    ) {
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
                // Indented by nesting depth (§18.2/§19.4) — a nested
                // conversation's records visually fall under the "call"
                // record that invoked them, the same call stack a debugger
                // would render, straight from the trace log.
                let depths = output::call_depths(&records);
                for (r, depth) in records.iter().zip(depths) {
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
                    let indent = "  ".repeat(depth);
                    println!(
                        "#{:<3} [{status}] {indent}{:<10} {}",
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

/// `ulx debug <run_id>` (§19 — see `debug.rs`'s module docs for exactly
/// how much of the full design this covers). Reads stdin line-by-line
/// rather than pulling in a REPL crate: the command surface is small
/// enough (§19.2/§19.3-inspired stepping over an already-recorded trace,
/// not a general-purpose language shell — no `repl` subcommand exists
/// either, see the module doc comment at the top of this file) that a
/// dependency for it isn't warranted.
fn cmd_debug(run_id: &str) -> bool {
    let records = match ulx_runtime::read_trace(manifest::traces_dir(), run_id) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: could not read trace for `{run_id}`: {e}");
            return false;
        }
    };
    if records.is_empty() {
        println!("run `{run_id}` has an empty trace (nothing was ever recorded)");
        return true;
    }

    let mut session = debug::DebugSession::new(records);
    println!(
        "ulx debug: {} record(s) for run `{run_id}` — type `help` for commands",
        session.len()
    );
    if let Some((target, reason)) = session.suspend_info() {
        println!(
            "this run is SUSPENDED waiting on `{target}`: {reason}\n  resume with: ulx approve {run_id} --value <text>   (or: ulx deny {run_id})"
        );
    }

    let stdin = std::io::stdin();
    loop {
        print!("(ulx-debug) ");
        let _ = std::io::Write::flush(&mut std::io::stdout());
        let mut line = String::new();
        if stdin.read_line(&mut line).unwrap_or(0) == 0 {
            break; // EOF (piped input exhausted, or Ctrl-D)
        }
        match debug::parse_command(&line) {
            debug::DebugCommand::Next => match session.step_next() {
                Some(_) => println!("{}", session.render_current_summary()),
                None => println!("(end of trace)"),
            },
            debug::DebugCommand::Back => match session.step_back() {
                Some(_) => println!("{}", session.render_current_summary()),
                None => println!("(already at the start of the trace)"),
            },
            debug::DebugCommand::Continue => match session.continue_to_breakpoint() {
                Some(_) => println!("{}", session.render_current_summary()),
                None => println!("(end of trace)"),
            },
            debug::DebugCommand::SetBreakpoint(seq) => {
                session.set_breakpoint(seq);
                println!("breakpoint set at #{seq}");
            }
            debug::DebugCommand::Inspect => println!("{}", session.render_inspect()),
            debug::DebugCommand::Stack => println!("{}", session.render_stack()),
            debug::DebugCommand::List => println!("{}", session.render_list()),
            debug::DebugCommand::Help => println!("{}", debug::HELP_TEXT),
            debug::DebugCommand::Quit => break,
            debug::DebugCommand::Unknown(cmd) => {
                if !cmd.is_empty() {
                    println!("unrecognized command `{cmd}` — type `help` for commands");
                }
            }
        }
    }
    true
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_run_id_never_collides_across_separate_calls_with_identical_inputs() {
        // Regression test: `default_run_id` used to be a pure hash of
        // file+conversation+args, so two unrelated invocations of the
        // exact same command (a realistic case: two CI jobs, or a person
        // rerunning `ulx run` without `--run-id`) silently shared one run
        // id, and therefore one trace file/manifest/escalate cache key.
        let file = std::path::Path::new("translate.ulx");
        let mut args = BTreeMap::new();
        args.insert("source".to_string(), "hello".to_string());
        args.insert("target_lang".to_string(), "fr".to_string());

        let a = default_run_id(file, "Translate", &args);
        let b = default_run_id(file, "Translate", &args);
        assert_ne!(
            a, b,
            "two calls with identical file/conversation/args must not produce the same run id"
        );
    }

    #[test]
    fn default_bench_run_id_never_collides_across_separate_calls() {
        let file = std::path::Path::new("eval_translate.ulx");
        let a = default_bench_run_id(file, "TranslateQuality");
        let b = default_bench_run_id(file, "TranslateQuality");
        assert_ne!(
            a, b,
            "two calls with identical file/benchmark must not produce the same run id"
        );
    }
}
