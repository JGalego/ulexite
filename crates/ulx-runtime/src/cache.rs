//! Content-addressed cache (§10.3): every effectful call is keyed by a hash
//! of its capability, resolved provider, and inputs, and is a cache hit on
//! any subsequent identical call — including across separate `ulx`
//! invocations, since this is a disk-backed store, which is also what
//! makes the escalate/human-approval resume flow work (see `interp.rs`).
//!
//! This also stands in for §11.2's artifact store in v0.1: there's no
//! separate large-binary-blob store yet (the mock provider and stdlib only
//! ever produce small JSON-serializable values), so one content-addressed
//! store serves both roles. Splitting them out is future work once a real
//! provider produces genuinely large binary artifacts.

use std::path::{Path, PathBuf};

use crate::value::Value;

pub struct Cache {
    root: PathBuf,
}

impl Cache {
    pub fn new(root: impl Into<PathBuf>) -> std::io::Result<Self> {
        let root = root.into();
        std::fs::create_dir_all(&root)?;
        Ok(Cache { root })
    }

    fn path_for(&self, key: &str) -> PathBuf {
        let (prefix, rest) = key.split_at(key.len().min(2));
        self.root.join(prefix).join(rest)
    }

    pub fn get(&self, key: &str) -> Option<Value> {
        let path = self.path_for(key);
        let bytes = std::fs::read(path).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    pub fn put(&self, key: &str, value: &Value) -> std::io::Result<()> {
        let path = self.path_for(key);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let bytes = serde_json::to_vec_pretty(value).expect("Value always serializes");
        std::fs::write(path, bytes)
    }

    pub fn has(&self, key: &str) -> bool {
        self.path_for(key).exists()
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

/// Hashes a capability invocation's identity (§10.3's cache-key
/// definition): capability name, resolved provider id, and every input's
/// content hash, plus any extra deterministic parameters the caller mixes
/// in (e.g. a rubric string, a regex pattern).
pub fn cache_key(capability: &str, provider_id: &str, inputs: &[&Value], extra: &[&str]) -> String {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(capability.as_bytes());
    hasher.update(provider_id.as_bytes());
    for v in inputs {
        hasher.update(v.content_hash().as_bytes());
    }
    for e in extra {
        hasher.update(e.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}
