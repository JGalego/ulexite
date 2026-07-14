//! Content-addressed cache (§10.3): every effectful call is keyed by a hash
//! of its capability, resolved provider, and inputs, and is a cache hit on
//! any subsequent identical call — including across separate `ulx`
//! invocations, since this is a disk-backed store, which is also what
//! makes the escalate/human-approval resume flow work (see `interp.rs`).
//!
//! `ArtifactStore` below is §11.2's real artifact store: split out from
//! `Cache` now that a real provider (`openai_compat.rs`'s `speak`/
//! `generate_image`) produces genuinely large binary output. It reuses
//! `Cache`'s sharded 2-char-prefix directory layout and lives under the
//! same project-local root convention (`ulx-cli`'s `manifest::cache_dir`/
//! `manifest::artifacts_dir`), but stores raw bytes keyed by a
//! caller-supplied content hash instead of JSON-serialized `Value`s keyed
//! by `cache_key()`.

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

/// A content-addressed local blob store for provider-generated binary
/// artifacts (`speak`'s audio, `generate_image`'s image) — §11.2's
/// artifact store. Same sharded 2-char-prefix layout as `Cache`, rooted
/// under the project's `.ulexite/artifacts` (mirroring `Cache`'s
/// `.ulexite/cache`), not the OS temp directory.
pub struct ArtifactStore {
    root: PathBuf,
}

impl ArtifactStore {
    pub fn new(root: impl Into<PathBuf>) -> std::io::Result<Self> {
        let root = root.into();
        std::fs::create_dir_all(&root)?;
        Ok(ArtifactStore { root })
    }

    fn path_for(&self, hash: &str, extension: &str) -> PathBuf {
        let (prefix, rest) = hash.split_at(hash.len().min(2));
        self.root.join(prefix).join(format!("{rest}.{extension}"))
    }

    /// Writes `bytes` to `hash`'s sharded, extension-suffixed path and
    /// returns it — unless a file is already there, in which case this is
    /// a no-op: same bytes hash the same, so a repeat write of identical
    /// content never touches disk twice (idempotent-by-hash, the same
    /// pattern `Cache::put`'s callers rely on via `Cache::has`).
    pub fn put(&self, hash: &str, extension: &str, bytes: &[u8]) -> std::io::Result<PathBuf> {
        let path = self.path_for(hash, extension);
        if !path.exists() {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, bytes)?;
        }
        Ok(path)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_dir(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "ulexite-artifact-store-test-{label}-{}",
            std::process::id()
        ))
    }

    #[test]
    fn identical_bytes_write_once_and_share_a_path() {
        let root = scratch_dir("idempotent");
        let store = ArtifactStore::new(&root).unwrap();
        let bytes = b"same bytes every time";
        let hash = crate::value::hash_bytes(bytes);

        let path1 = store.put(&hash[..16], "bin", bytes).unwrap();
        let mtime1 = std::fs::metadata(&path1).unwrap().modified().unwrap();

        // Sleep briefly so a second real write (if it happened) would be
        // observable as a changed mtime.
        std::thread::sleep(std::time::Duration::from_millis(20));
        let path2 = store.put(&hash[..16], "bin", bytes).unwrap();
        let mtime2 = std::fs::metadata(&path2).unwrap().modified().unwrap();

        assert_eq!(
            path1, path2,
            "identical bytes must resolve to the same path"
        );
        assert_eq!(
            mtime1, mtime2,
            "second put of identical bytes must be a no-op, not a second write"
        );
        assert_eq!(std::fs::read(&path1).unwrap(), bytes);

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn artifacts_are_sharded_under_the_given_root_not_the_os_temp_default() {
        let root = scratch_dir("sharded");
        let store = ArtifactStore::new(&root).unwrap();
        let bytes = b"shard me";
        let hash = crate::value::hash_bytes(bytes);

        let path = store.put(&hash[..16], "png", bytes).unwrap();

        assert!(
            path.starts_with(&root),
            "artifact must live under the store's own root"
        );
        // Sharded like `Cache::path_for`: a 2-char prefix directory, then
        // the rest of the hash as the filename.
        assert!(path.starts_with(root.join(&hash[..2])));
        assert_eq!(
            path.file_name().unwrap().to_str().unwrap(),
            format!("{}.png", &hash[2..16])
        );
        // The root is whatever the caller passed in — not the old
        // hardcoded `std::env::temp_dir().join("ulexite-artifacts")`.
        assert_ne!(root, std::env::temp_dir().join("ulexite-artifacts"));

        std::fs::remove_dir_all(&root).ok();
    }
}
