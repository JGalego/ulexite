//! Golden-baseline storage for `benchmark`'s `snapshot expr as key`
//! statement (§16.5). Each distinct key gets its own JSON file under
//! `<package-dir>/snapshots/<benchmark>/` — committed alongside the
//! source, unlike `.ulexite/`'s cache/traces (local, ephemeral, and
//! gitignored) — a snapshot is a deliberately versioned baseline, the
//! same role a `.snap` file plays for `insta` or `__snapshots__/` plays
//! for Jest.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::value::{hash_bytes, Value};

/// A key can be any string a `.ulx` program computes at runtime, so it
/// isn't necessarily a safe filename on its own — sanitized to
/// alphanumeric/`-`/`_` and capped at 60 characters for readability, with
/// a short content hash of the *original* key appended so two different
/// keys that happen to sanitize to the same string still get distinct
/// files instead of silently colliding.
fn sanitize(key: &str) -> String {
    let cleaned: String = key
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(60)
        .collect();
    let cleaned = if cleaned.is_empty() {
        "_".to_string()
    } else {
        cleaned
    };
    format!("{cleaned}-{}", &hash_bytes(key.as_bytes())[..8])
}

fn path_for(base_dir: &Path, benchmark: &str, key: &str) -> PathBuf {
    base_dir
        .join("snapshots")
        .join(benchmark)
        .join(format!("{}.json", sanitize(key)))
}

/// The stored file's shape — keeps the original, human-readable `key`
/// alongside the value, since the filename itself is sanitized/hashed and
/// not necessarily legible on its own.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Stored {
    key: String,
    value: Value,
}

/// `Ok(None)` when no baseline exists yet for this key (first time this
/// snapshot statement has ever run) — the caller should treat that as
/// "record it," not a failure.
pub fn load(base_dir: &Path, benchmark: &str, key: &str) -> std::io::Result<Option<Value>> {
    let path = path_for(base_dir, benchmark, key);
    match std::fs::read(&path) {
        Ok(bytes) => {
            let stored: Stored = serde_json::from_slice(&bytes)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            Ok(Some(stored.value))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

pub fn save(base_dir: &Path, benchmark: &str, key: &str, value: &Value) -> std::io::Result<()> {
    let path = path_for(base_dir, benchmark, key);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let stored = Stored {
        key: key.to_string(),
        value: value.clone(),
    };
    let bytes = serde_json::to_vec_pretty(&stored).expect("Stored always serializes");
    std::fs::write(path, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_a_value() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(load(tmp.path(), "Bench", "fr").unwrap(), None);

        save(
            tmp.path(),
            "Bench",
            "fr",
            &Value::Text("Bonjour".to_string()),
        )
        .unwrap();
        assert_eq!(
            load(tmp.path(), "Bench", "fr").unwrap(),
            Some(Value::Text("Bonjour".to_string()))
        );

        // Overwriting (what --update-snapshots does) replaces the baseline.
        save(tmp.path(), "Bench", "fr", &Value::Text("Salut".to_string())).unwrap();
        assert_eq!(
            load(tmp.path(), "Bench", "fr").unwrap(),
            Some(Value::Text("Salut".to_string()))
        );
    }

    #[test]
    fn different_keys_never_collide_even_after_sanitizing_the_same() {
        let tmp = tempfile::tempdir().unwrap();
        save(tmp.path(), "Bench", "a/b", &Value::Text("one".to_string())).unwrap();
        save(tmp.path(), "Bench", "a_b", &Value::Text("two".to_string())).unwrap();
        assert_eq!(
            load(tmp.path(), "Bench", "a/b").unwrap(),
            Some(Value::Text("one".to_string()))
        );
        assert_eq!(
            load(tmp.path(), "Bench", "a_b").unwrap(),
            Some(Value::Text("two".to_string()))
        );
    }
}
