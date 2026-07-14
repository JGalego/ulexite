//! Trace log (§18): one append-only JSONL file per run, serving replay
//! (§18.3), debugging, and audit from a single source of truth rather than
//! three unrelated systems (§18.1's stated goal, at v0.1 fidelity).

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::value::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceRecord {
    pub run_id: String,
    pub seq: u64,
    pub kind: String,
    pub capability: Option<String>,
    pub cache_key: Option<String>,
    pub cache_hit: bool,
    pub output: Option<Value>,
    pub error: Option<String>,
    pub timestamp_ms: u128,
}

pub struct TraceWriter {
    run_id: String,
    path: PathBuf,
    file: Mutex<std::fs::File>,
    seq: Mutex<u64>,
}

impl TraceWriter {
    pub fn create(traces_dir: impl AsRef<Path>, run_id: &str) -> std::io::Result<Self> {
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
        output: Option<&Value>,
        error: Option<&str>,
    ) -> u64 {
        let mut seq_guard = self.seq.lock().unwrap();
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
            output: output.cloned(),
            error: error.map(str::to_string),
            timestamp_ms: now_ms(),
        };
        let line = serde_json::to_string(&record).expect("TraceRecord always serializes");
        let mut file = self.file.lock().unwrap();
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
