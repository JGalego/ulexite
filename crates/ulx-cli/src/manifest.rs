//! Per-run bookkeeping the CLI needs that the runtime itself doesn't
//! persist: which file/conversation/arguments a `run_id` corresponds to, so
//! `ulx replay`/`ulx approve`/`ulx deny` can reconstruct and re-invoke it.
//! Lives under `.ulexite/` in the current working directory (§14's package
//! conventions, applied to run state rather than a package manifest).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct RunManifest {
    pub file: PathBuf,
    /// The conversation this run invoked — empty and unused when
    /// `benchmark` is `Some` (a benchmark run has no single conversation
    /// name; `ulx approve`/`ulx deny` check `benchmark` first to decide
    /// which of `run_conversation`/`run_benchmark` to re-invoke).
    pub conversation: String,
    /// Set instead of (never alongside) `conversation` for a `ulx bench
    /// --run-id <id>` run — the benchmark name to re-invoke via
    /// `run_benchmark` on `ulx approve`/`ulx deny`. `#[serde(default)]` so
    /// a manifest written before this field existed still deserializes as
    /// an ordinary conversation run.
    #[serde(default)]
    pub benchmark: Option<String>,
    pub args: BTreeMap<String, String>,
    /// The `--provider name` selection (if any) `ulx run` was invoked
    /// with — persisted so `ulx approve`/`ulx deny`/`ulx replay` default to
    /// re-resolving the *same* providers rather than erroring (an
    /// otherwise-ambiguous capability) or silently resolving to something
    /// different (a mismatched provider id breaks cache-key lookups and
    /// can replay a live call against the wrong vendor entirely — see
    /// `docs/spec/24-limitations.md` §24.11). `#[serde(default)]` so a
    /// manifest written before this field existed still deserializes.
    #[serde(default)]
    pub selected_providers: Vec<String>,
    /// The `--mock` flag `ulx run` was invoked with — same rationale as
    /// `selected_providers` above.
    #[serde(default)]
    pub force_mock: bool,
}

pub fn state_dir() -> PathBuf {
    PathBuf::from(".ulexite")
}

pub fn cache_dir() -> PathBuf {
    state_dir().join("cache")
}

/// Root of the content-addressed artifact store (§11.2) — `speak`'s audio
/// and `generate_image`'s image output land here, sharded the same way
/// `cache_dir()`'s entries are, instead of the OS temp directory.
pub fn artifacts_dir() -> PathBuf {
    state_dir().join("artifacts")
}

pub fn traces_dir() -> PathBuf {
    state_dir().join("traces")
}

fn runs_dir() -> PathBuf {
    state_dir().join("runs")
}

/// `run_id` (whether auto-derived or given via `--run-id`/a positional
/// `<RUN_ID>` argument) always ends up as one path component under
/// `.ulexite/` — `dir.join(format!("{run_id}.json"))` and friends. Without
/// this check, a `--run-id` containing `/`, `..`, or an absolute path
/// (`Path::join` replaces the whole path when the joined component is
/// itself absolute) lets `ulx run`/`approve`/`deny` write or read a file
/// anywhere on disk instead of under `.ulexite/runs/`.
fn validate_run_id(run_id: &str) -> Result<(), String> {
    if run_id.is_empty()
        || run_id == "."
        || run_id == ".."
        || run_id.contains('/')
        || run_id.contains('\\')
        || run_id.contains('\0')
    {
        return Err(format!(
            "invalid run id `{run_id}` — must be a single path component (no `/`, `\\`, `..`, or empty)"
        ));
    }
    Ok(())
}

pub fn save(run_id: &str, manifest: &RunManifest) -> std::io::Result<()> {
    validate_run_id(run_id)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    let dir = runs_dir();
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{run_id}.json"));
    let bytes = serde_json::to_vec_pretty(manifest).expect("RunManifest always serializes");
    std::fs::write(path, bytes)
}

pub fn load(run_id: &str) -> std::io::Result<RunManifest> {
    validate_run_id(run_id)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    let path = runs_dir().join(format!("{run_id}.json"));
    let bytes = std::fs::read(path)?;
    serde_json::from_slice(&bytes)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

pub fn base_dir_of(file: &Path) -> PathBuf {
    file.parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}
