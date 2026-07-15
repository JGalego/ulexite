//! The Ulexite runtime (§12): executes lowered IR (`ulx-ir`) against a
//! pluggable `Provider` (§12.4), with a content-addressed cache (§10.3), a
//! JSONL trace log (§18), and a `with`-block scheduler that genuinely runs
//! independent bindings on separate OS threads (§10.2) — real concurrency,
//! not a simulated one.
//!
//! Scope, honestly: this is v0.1. Checkpoint/resume for `escalate` works by
//! leaning on the content-addressed cache rather than true continuation
//! capture (see `interp.rs`'s module docs) — a real, working design, just
//! not the full checkpoint-log architecture §10.4/§18 describes. Most of
//! §15's standard library isn't implemented (`stdlib.rs` documents exactly
//! what is). See `docs/spec/24-limitations.md` for the rest of the honest
//! accounting.

pub mod cache;
mod dataset;
mod env;
pub mod error;
mod interp;
pub mod provider;
mod stdlib;
pub mod trace;
mod validator;
pub mod value;

pub use cache::Cache;
pub use error::RuntimeError;
pub use interp::{
    run_benchmark, run_conversation, BenchmarkReport, BenchmarkRowResult, CheckResult,
};
pub use provider::{
    build_provider, MockProvider, Provider, ProviderBuildError, ProviderRegistry, ProviderSpec,
    ResolveError,
};
pub use trace::{read_trace, TraceRecord, TraceWriter};
pub use value::Value;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use ulx_ir::IrProgram;

/// Everything one conversation run needs, shared (read-only after
/// construction, aside from the atomic effect counter) across the worker
/// threads a `with` block spawns.
pub struct RunContext<'a> {
    pub program: &'a IrProgram,
    pub providers: ProviderRegistry,
    pub cache: Cache,
    pub trace: TraceWriter,
    pub run_id: String,
    pub base_dir: PathBuf,
    /// Strict replay (§10.4, §18.3): every effect must be a cache hit; a
    /// miss is a hard error rather than a live provider call.
    pub replay_only: bool,
    /// Skips the cache *read* for `ask`/`judge` calls (`invoke_cached`,
    /// `interp.rs`), forcing a fresh live call every time instead of
    /// reusing a stale prior result — the CLI's `--no-cache`. Results are
    /// still written to the cache afterward as normal, so repeated
    /// identical calls within one run (e.g. inside a loop) stay
    /// consistent, and the trace/replay log is unaffected. Deliberately
    /// does NOT apply to `escalate`'s own cache lookup (`eval_escalate`) —
    /// that cache entry is the only persistence mechanism `ulx
    /// approve`/`ulx deny`/`--interactive` have for a human's decision,
    /// not a "don't waste an API call" optimization, so bypassing it would
    /// break resume rather than just cost more.
    pub no_cache: bool,
    seq: AtomicU64,
}

impl<'a> RunContext<'a> {
    pub fn new(
        program: &'a IrProgram,
        providers: ProviderRegistry,
        cache: Cache,
        trace: TraceWriter,
        run_id: String,
        base_dir: PathBuf,
    ) -> Self {
        RunContext {
            program,
            providers,
            cache,
            trace,
            run_id,
            base_dir,
            replay_only: false,
            no_cache: false,
            seq: AtomicU64::new(0),
        }
    }

    pub fn replaying(mut self) -> Self {
        self.replay_only = true;
        self
    }

    pub fn without_cache(mut self) -> Self {
        self.no_cache = true;
        self
    }

    pub(crate) fn next_seq(&self) -> u64 {
        self.seq.fetch_add(1, Ordering::SeqCst)
    }
}

pub type Args = BTreeMap<String, Value>;
