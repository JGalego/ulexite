//! Trace log (§18): one append-only JSONL file per run, serving replay
//! (§18.3), debugging, and audit from a single source of truth rather than
//! three unrelated systems (§18.1's stated goal, at v0.1 fidelity).

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::provider::Message;
use crate::value::Value;

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

pub struct TraceWriter {
    run_id: String,
    path: PathBuf,
    file: Mutex<std::fs::File>,
    seq: Mutex<u64>,
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
            file: Mutex::new(file),
            seq: Mutex::new(0),
        })
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
        };
        let line = serde_json::to_string(&record).expect("TraceRecord always serializes");
        let mut file = self.file.lock().unwrap_or_else(|p| p.into_inner());
        let _ = writeln!(file, "{line}");
        let _ = file.flush();
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
