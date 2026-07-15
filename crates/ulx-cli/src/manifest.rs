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
    pub conversation: String,
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

pub fn save(run_id: &str, manifest: &RunManifest) -> std::io::Result<()> {
    let dir = runs_dir();
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{run_id}.json"));
    let bytes = serde_json::to_vec_pretty(manifest).expect("RunManifest always serializes");
    std::fs::write(path, bytes)
}

pub fn load(run_id: &str) -> std::io::Result<RunManifest> {
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
