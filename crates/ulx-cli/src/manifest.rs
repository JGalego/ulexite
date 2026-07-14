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
}

pub fn state_dir() -> PathBuf {
    PathBuf::from(".ulexite")
}

pub fn cache_dir() -> PathBuf {
    state_dir().join("cache")
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
