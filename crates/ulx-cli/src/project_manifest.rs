//! `ulexite.toml` (§14.1): package metadata, dependencies, provider policy,
//! and runtime config. Parsing/validation only — dependency *resolution*
//! (a registry, a lockfile, semver-contract checks at publish time, §14.2–
//! §14.4) is real, sizable infrastructure this v0.1 doesn't build; only
//! `git`/`path` dependencies make sense to even parse without a registry
//! to resolve named versions against. See `docs/spec/24-limitations.md`.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Manifest {
    pub package: Package,
    #[serde(default)]
    pub dependencies: BTreeMap<String, Dependency>,
    #[serde(default)]
    pub providers: BTreeMap<String, ProviderPolicy>,
    #[serde(default)]
    pub runtime: RuntimeConfig,
}

#[derive(Debug, Deserialize)]
pub struct Package {
    pub name: String,
    pub version: String,
    pub ulexite: String,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum Dependency {
    Version(String),
    Detailed {
        #[serde(default)]
        git: Option<String>,
        #[serde(default)]
        path: Option<String>,
        #[serde(default)]
        tag: Option<String>,
    },
}

#[derive(Debug, Deserialize)]
pub struct ProviderPolicy {
    pub capability: String,
    #[serde(default = "default_policy")]
    pub policy: String,
}

fn default_policy() -> String {
    "balanced".to_string()
}

#[derive(Debug, Deserialize)]
pub struct RuntimeConfig {
    #[serde(default = "default_concurrency")]
    pub concurrency: usize,
    #[serde(default = "default_cache_backend")]
    pub cache_backend: String,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        RuntimeConfig {
            concurrency: default_concurrency(),
            cache_backend: default_cache_backend(),
        }
    }
}

fn default_concurrency() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
}

fn default_cache_backend() -> String {
    "local".to_string()
}

#[derive(Debug)]
pub enum ManifestError {
    Io(String),
    Parse(String),
    Invalid(String),
}

impl std::fmt::Display for ManifestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ManifestError::Io(e) => write!(f, "{e}"),
            ManifestError::Parse(e) => write!(f, "{e}"),
            ManifestError::Invalid(e) => write!(f, "{e}"),
        }
    }
}

pub fn load(path: &Path) -> Result<Manifest, ManifestError> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| ManifestError::Io(format!("could not read {}: {e}", path.display())))?;
    parse(&text)
}

pub fn parse(text: &str) -> Result<Manifest, ManifestError> {
    let manifest: Manifest =
        toml::from_str(text).map_err(|e| ManifestError::Parse(e.to_string()))?;
    validate(&manifest)?;
    Ok(manifest)
}

fn validate(m: &Manifest) -> Result<(), ManifestError> {
    if m.package.name.is_empty() {
        return Err(ManifestError::Invalid(
            "package.name must not be empty".to_string(),
        ));
    }
    if !m
        .package
        .name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(ManifestError::Invalid(
            "package.name may only contain letters, digits, `-`, `_`".to_string(),
        ));
    }
    if semver_lite::parse(&m.package.version).is_none() {
        return Err(ManifestError::Invalid(format!(
            "package.version `{}` is not a valid semver (x.y.z)",
            m.package.version
        )));
    }
    for (name, dep) in &m.dependencies {
        if let Dependency::Detailed { git, path, .. } = dep {
            if git.is_none() && path.is_none() {
                return Err(ManifestError::Invalid(format!(
                    "dependency `{name}` needs a version string, or a `git`/`path` table"
                )));
            }
        }
    }
    Ok(())
}

/// Just enough semver to validate a `package.version`/`package.ulexite`
/// string shape — not a real requirement-matching resolver (there's
/// nothing to resolve against yet without a registry).
mod semver_lite {
    pub fn parse(s: &str) -> Option<(u64, u64, u64)> {
        let s = s.trim_start_matches(['^', '~', '=']);
        let mut parts = s.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next().unwrap_or("0").parse().ok()?;
        let patch = parts.next().unwrap_or("0").parse().ok()?;
        Some((major, minor, patch))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_manifest_parses() {
        let m = parse(
            r#"
            [package]
            name = "acme-support-bot"
            version = "0.3.0"
            ulexite = "^1.0"

            [dependencies]
            translation-judges = "^2.1"
            rag-toolkit = { git = "https://example.com/rag-toolkit.git", tag = "v0.9.0" }

            [providers]
            default_chat = { capability = "chat", policy = "cheapest" }

            [runtime]
            concurrency = 8
            cache_backend = "local"
            "#,
        )
        .expect("should parse");
        assert_eq!(m.package.name, "acme-support-bot");
        assert_eq!(m.runtime.concurrency, 8);
        assert_eq!(m.dependencies.len(), 2);
        assert_eq!(m.providers["default_chat"].capability, "chat");
    }

    #[test]
    fn minimal_manifest_uses_defaults() {
        let m = parse(
            r#"
            [package]
            name = "tiny"
            version = "0.1.0"
            ulexite = "^0.1"
            "#,
        )
        .expect("should parse");
        assert_eq!(m.runtime.cache_backend, "local");
        assert!(m.dependencies.is_empty());
    }

    #[test]
    fn invalid_version_is_rejected() {
        let err = parse(
            r#"
            [package]
            name = "tiny"
            version = "not-a-version"
            ulexite = "^0.1"
            "#,
        )
        .unwrap_err();
        assert!(matches!(err, ManifestError::Invalid(_)));
    }

    #[test]
    fn invalid_package_name_is_rejected() {
        let err = parse(
            r#"
            [package]
            name = "not a valid name!"
            version = "0.1.0"
            ulexite = "^0.1"
            "#,
        )
        .unwrap_err();
        assert!(matches!(err, ManifestError::Invalid(_)));
    }

    #[test]
    fn dependency_table_without_git_or_path_is_rejected() {
        let err = parse(
            r#"
            [package]
            name = "tiny"
            version = "0.1.0"
            ulexite = "^0.1"

            [dependencies]
            broken = { tag = "v1.0.0" }
            "#,
        )
        .unwrap_err();
        assert!(matches!(err, ManifestError::Invalid(_)));
    }
}
