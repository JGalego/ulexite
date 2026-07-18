//! Trace log (§18): one append-only JSONL file per run, serving replay
//! (§18.3), debugging, and audit from a single source of truth rather than
//! three unrelated systems (§18.1's stated goal, at v0.1 fidelity).

use std::cell::RefCell;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
// `std::time::SystemTime::now()` panics at runtime on
// `wasm32-unknown-unknown` — `web-time` is a drop-in replacement that uses
// `Date.now()` there and is a plain passthrough to `std::time` everywhere
// else.
use web_time::{SystemTime, UNIX_EPOCH};

use crate::provider::Message;
use crate::value::Value;

thread_local! {
    /// The chain of enclosing nested-conversation "call" frame ids, root to
    /// leaf, for *this* thread — empty at the top level of a run. §18.2's
    /// full design gives every nested conversation invocation its own
    /// `run_id`, with a `parent_run_id` linking a child run back to its
    /// parent; this interpreter runs every nested conversation call in the
    /// same flat trace file under the one top-level `run_id` (there's no
    /// separate replay-capable sub-run per call), so a frame id here is a
    /// synthetic `"{run_id}:{call_record_seq}"` string identifying one
    /// specific "call" trace record rather than a wholly separate run —
    /// enough to reconstruct a call stack (§19.4) without the much larger
    /// work of giving every nested call a genuinely separate trace file.
    static CALL_STACK: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Enters a nested-conversation call frame — pushed by `interp.rs`'s
/// `eval_conversation_call` right after it records that call's own "call"
/// trace record (using that record's `seq`, so the frame id points back at
/// an actual, findable record), popped again once that conversation's body
/// finishes (success or error alike).
pub(crate) fn push_call_frame(frame_id: String) {
    CALL_STACK.with(|s| s.borrow_mut().push(frame_id));
}

pub(crate) fn pop_call_frame() {
    CALL_STACK.with(|s| {
        s.borrow_mut().pop();
    });
}

/// The innermost enclosing call frame's id on *this* thread, if any — read
/// by `TraceWriter::record` to fill a fresh record's `parent_run_id`.
pub(crate) fn current_call_frame() -> Option<String> {
    CALL_STACK.with(|s| s.borrow().last().cloned())
}

/// The full call stack on *this* thread, root to leaf — read by
/// `eval_parallel` before spawning a `with`-block branch, so `seed_call_stack`
/// can hand the *entire* chain (not just the innermost frame) to the new
/// thread; a branch that itself calls a nested conversation and later
/// returns needs the whole chain to pop back to correctly.
pub(crate) fn current_stack() -> Vec<String> {
    CALL_STACK.with(|s| s.borrow().clone())
}

/// Mirrors `interp.rs`'s `ESCALATE_PATH`/`ESCALATE_LOCAL_SEQ` reset: called
/// at the top of `run_conversation`/`run_benchmark` so a second same-thread
/// call (both ends of `--interactive`'s suspend/resume loop, this crate's
/// own tests) doesn't inherit a stale call stack left over from a previous
/// run on the same thread.
pub(crate) fn reset_call_stack() {
    CALL_STACK.with(|s| s.borrow_mut().clear());
}

/// A freshly spawned `with`-block branch is a genuine new OS thread (see
/// `interp.rs`'s `eval_parallel`), so its `CALL_STACK` starts back at empty
/// and must be explicitly seeded with the spawning thread's stack at spawn
/// time — it is never inherited automatically. Unlike `ESCALATE_PATH`,
/// every sibling branch seeds from the exact same parent stack (no
/// per-branch index appended): the call stack describes which
/// conversations enclose this point, which is identical for every branch
/// of the same `with` block.
pub(crate) fn seed_call_stack(stack: Vec<String>) {
    CALL_STACK.with(|s| *s.borrow_mut() = stack);
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceRecord {
    pub run_id: String,
    pub seq: u64,
    pub kind: String,
    pub capability: Option<String>,
    pub cache_key: Option<String>,
    pub cache_hit: bool,
    /// The request side of an `ask`/`judge`/`escalate` effect — the actual
    /// `system`/`user` messages sent for a `chat`/`vision` call, a
    /// `subject`/`rubric` pair for a `judge` call, or a single
    /// `{role: target, text: reason}` entry for an `escalate` call.
    /// `#[serde(default)]` so a trace file written before this field
    /// existed still deserializes (as empty).
    #[serde(default)]
    pub input: Vec<Message>,
    /// Which registered provider (`Provider::id()`) actually served this
    /// effect, and which model/deployment it was configured with
    /// (`Provider::model()`) — `None` for both on a `mock`-served call, a
    /// `validator`/`call`/`escalate` record (none of those resolve to a
    /// vendor provider), or a trace file written before these fields
    /// existed (`#[serde(default)]`).
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    pub output: Option<Value>,
    pub error: Option<String>,
    pub timestamp_ms: u128,
    /// §18.2/§19.4: identifies the enclosing nested-conversation "call"
    /// record, for reconstructing a call stack across nested conversations
    /// — `"{run_id}:{seq}"` of that "call" record, `None` at the top
    /// level. `#[serde(default)]` so a trace file written before this
    /// field existed still deserializes (as `None`, i.e. top-level).
    #[serde(default)]
    pub parent_run_id: Option<String>,
}

/// `run_id` always ends up as one path component under `traces_dir` —
/// `traces_dir.join(format!("{run_id}.jsonl"))`. Without this check, a
/// `run_id` containing `/`, `..`, or an absolute path (`Path::join`
/// replaces the whole path when the joined component is itself absolute)
/// lets a caller write or read a file anywhere on disk instead of under
/// the intended trace directory.
fn validate_run_id(run_id: &str) -> std::io::Result<()> {
    if run_id.is_empty()
        || run_id == "."
        || run_id == ".."
        || run_id.contains('/')
        || run_id.contains('\\')
        || run_id.contains('\0')
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "invalid run id `{run_id}` — must be a single path component (no `/`, `\\`, `..`, or empty)"
            ),
        ));
    }
    Ok(())
}

/// Fires once per trace record, right after it's durably appended — lets a
/// caller (`ulx-cli`'s `--output text`/`plain`) stream a run's dialogue as
/// it actually happens instead of only being able to read it back after
/// `run_conversation` returns. A `with` block's branches call `record`
/// concurrently from separate OS threads, so this must be safe to call
/// from any of them.
pub type RecordCallback = Box<dyn Fn(&TraceRecord) + Send + Sync>;

/// Fires right before a capability call actually blocks on the real
/// provider (`interp.rs`'s `invoke_cached`, before either the cache check
/// or the call itself) — lets a caller print a `chat`/`vision` call's
/// `system`/`user` messages the moment they're sent, rather than only once
/// the whole call (including the real network latency the messages
/// themselves have nothing to do with) has completed and `RecordCallback`
/// fires. Same cross-thread-safety requirement as `RecordCallback`.
pub type SendCallback = Box<dyn Fn(&str, &[Message]) + Send + Sync>;

pub struct TraceWriter {
    run_id: String,
    path: PathBuf,
    file: Mutex<Box<dyn Write + Send>>,
    seq: Mutex<u64>,
    on_record: Option<RecordCallback>,
    on_send: Option<SendCallback>,
}

impl TraceWriter {
    pub fn create(traces_dir: impl AsRef<Path>, run_id: &str) -> std::io::Result<Self> {
        validate_run_id(run_id)?;
        std::fs::create_dir_all(&traces_dir)?;
        let path = traces_dir.as_ref().join(format!("{run_id}.jsonl"));
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        Ok(TraceWriter {
            run_id: run_id.to_string(),
            path,
            file: Mutex::new(Box::new(file)),
            seq: Mutex::new(0),
            on_record: None,
            on_send: None,
        })
    }

    /// No filesystem exists on `wasm32-unknown-unknown` — the in-browser
    /// driver has no use for a durable JSONL log anyway (there's no second
    /// process to replay it later), so records are simply discarded after
    /// `on_record`/`on_send` callbacks fire. Use `with_on_record` to observe
    /// a run live instead.
    pub fn in_memory(run_id: &str) -> Self {
        TraceWriter {
            run_id: run_id.to_string(),
            path: PathBuf::new(),
            file: Mutex::new(Box::new(std::io::sink())),
            seq: Mutex::new(0),
            on_record: None,
            on_send: None,
        }
    }

    /// Attaches a live callback, fired with each newly-written record (see
    /// `RecordCallback`) — builder-style, like `RunContext::replaying`/
    /// `without_cache`, so a caller that doesn't want streaming (`ulx
    /// trace`, `--output json`/`jsonl`/`mermaid`/`html`, a resume's
    /// discovery-only probe context) pays nothing extra.
    pub fn with_on_record(mut self, cb: RecordCallback) -> Self {
        self.on_record = Some(cb);
        self
    }

    /// Attaches a live pre-call callback (see `SendCallback`) — same
    /// builder style and same opt-in reasoning as `with_on_record`.
    pub fn with_on_send(mut self, cb: SendCallback) -> Self {
        self.on_send = Some(cb);
        self
    }

    /// Notifies the `on_send` callback, if any, that `capability` is about
    /// to be invoked with `input` — called by `interp.rs`'s `invoke_cached`
    /// before it knows yet whether this will be a cache hit or a real,
    /// slow provider call. Not itself a trace record (nothing is persisted
    /// here); purely a live notification.
    pub fn notify_send(&self, capability: &str, input: &[Message]) {
        if let Some(cb) = &self.on_send {
            cb(capability, input);
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record(
        &self,
        kind: &str,
        capability: Option<&str>,
        cache_key: Option<&str>,
        cache_hit: bool,
        input: &[Message],
        provider: Option<&str>,
        model: Option<&str>,
        output: Option<&Value>,
        error: Option<&str>,
    ) -> u64 {
        // `unwrap_or_else(PoisonError::into_inner)` rather than `.unwrap()`:
        // a `with`-block branch that panics while holding this lock (or
        // the file lock below) must not poison it for every other
        // branch/subsequent call in the process — `eval_parallel` already
        // converts that branch's own panic into an ordinary
        // `RuntimeError::Panicked` instead of aborting, so tracing for the
        // *other* branches (and any later run in this same process, e.g.
        // `--interactive`'s loop) must keep working.
        let mut seq_guard = self.seq.lock().unwrap_or_else(|p| p.into_inner());
        let seq = *seq_guard;
        *seq_guard += 1;
        drop(seq_guard);

        let record = TraceRecord {
            run_id: self.run_id.clone(),
            seq,
            kind: kind.to_string(),
            capability: capability.map(str::to_string),
            cache_key: cache_key.map(str::to_string),
            cache_hit,
            input: input.to_vec(),
            provider: provider.map(str::to_string),
            model: model.map(str::to_string),
            output: output.cloned(),
            error: error.map(str::to_string),
            timestamp_ms: now_ms(),
            parent_run_id: current_call_frame(),
        };
        let line = serde_json::to_string(&record).expect("TraceRecord always serializes");
        let mut file = self.file.lock().unwrap_or_else(|p| p.into_inner());
        let _ = writeln!(file, "{line}");
        let _ = file.flush();
        drop(file);

        if let Some(cb) = &self.on_record {
            cb(&record);
        }
        seq
    }
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

pub fn read_trace(traces_dir: impl AsRef<Path>, run_id: &str) -> std::io::Result<Vec<TraceRecord>> {
    validate_run_id(run_id)?;
    let path = traces_dir.as_ref().join(format!("{run_id}.jsonl"));
    let file = std::fs::File::open(path)?;
    let reader = std::io::BufReader::new(file);
    let mut out = Vec::new();
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(record) = serde_json::from_str::<TraceRecord>(&line) {
            out.push(record);
        }
    }
    Ok(out)
}
