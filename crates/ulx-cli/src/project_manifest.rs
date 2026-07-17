//! `ulexite.toml` (§14.1): package metadata, dependencies, provider
//! config, and runtime config — parsing/validation only, this module never
//! touches disk beyond reading the manifest itself. `git`/`path`
//! dependency *resolution* is real (see `crate::git_dep`/`pipeline.rs`'s
//! `dependency_paths`) — a `path` entry is joined against the manifest's
//! directory, and a `git` entry is actually cloned/checked-out via the
//! system `git` binary into a local vendored directory. What's still not
//! built: a central registry (§14.3's `packages.ulexite.dev` — real server
//! infrastructure this repo doesn't have, so a bare version string can't
//! resolve to anything), a lockfile pinning transitive dependency content
//! hashes (§14.2), and semver-contract checking at publish time (§14.4).
//! See `docs/spec/24-limitations.md`.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Manifest {
    pub package: Package,
    #[serde(default)]
    pub dependencies: BTreeMap<String, Dependency>,
    #[serde(default)]
    pub providers: BTreeMap<String, ProviderEntry>,
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

/// One `[providers.<name>]` table: a single vendor account/deployment,
/// covering however many capabilities it serves. The table name is just a
/// label (so e.g. two distinct `openai_compatible` servers can each get
/// their own entry) — `vendor` is mandatory and never inferred from it.
#[derive(Debug, Deserialize)]
pub struct ProviderEntry {
    /// Which adapter to build (§12.4): `openai`, `anthropic`, `gemini`,
    /// `groq`, `cohere`, `ollama`, `openai_compatible`, `azure_openai`, or
    /// `mock`.
    pub vendor: String,
    /// Required for `openai_compatible` (e.g. a local vLLM/LM Studio
    /// server) and `azure_openai` (your resource endpoint); optional
    /// override elsewhere.
    #[serde(default)]
    pub base_url: Option<String>,
    /// Name of the environment variable holding the API key — never a
    /// literal secret in this file.
    #[serde(default)]
    pub api_key_env: Option<String>,
    /// `azure_openai` only: the mandatory `api-version` query parameter
    /// (defaults to a recent stable version if omitted).
    #[serde(default)]
    pub api_version: Option<String>,
    /// Every other key in the table is a capability name (`chat`, `vision`,
    /// `embed`, `transcribe`, `speak`, `generate_image`, ...) — a bare
    /// model-name string, or a `{ model = "...", ... }` table for
    /// per-capability parameter overrides (`ask chat(temperature: 0.7)`
    /// still wins over this at call time).
    #[serde(flatten)]
    pub capabilities: BTreeMap<String, CapabilityConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum CapabilityConfig {
    Model(String),
    Detailed {
        #[serde(default)]
        model: Option<String>,
        #[serde(default)]
        params: BTreeMap<String, toml::Value>,
    },
}

impl CapabilityConfig {
    pub fn model(&self) -> Option<&str> {
        match self {
            CapabilityConfig::Model(m) => Some(m.as_str()),
            CapabilityConfig::Detailed { model, .. } => model.as_deref(),
        }
    }

    pub fn params(&self) -> BTreeMap<String, toml::Value> {
        match self {
            CapabilityConfig::Model(_) => BTreeMap::new(),
            CapabilityConfig::Detailed { params, .. } => params.clone(),
        }
    }
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

/// Looks for `ulexite.toml` in `dir` (the convention every provider-config
/// consumer shares: `ulx run`'s `providers::resolve_providers`, and now
/// `ulx check`'s manifest-aware `provider` validation) — `Ok(None)` if it's
/// simply not there, `Err` only if it exists but fails to parse.
pub fn discover(dir: &Path) -> Result<Option<Manifest>, ManifestError> {
    let path = dir.join("ulexite.toml");
    if !path.exists() {
        return Ok(None);
    }
    load(&path).map(Some)
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
    for (name, entry) in &m.providers {
        if entry.capabilities.is_empty() {
            return Err(ManifestError::Invalid(format!(
                "provider `{name}` (vendor `{}`) declares no capabilities — add at least one, e.g. `chat = \"...\"`",
                entry.vendor
            )));
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

            [providers.openai]
            vendor = "openai"
            chat = "gpt-4o-mini"

            [runtime]
            concurrency = 8
            cache_backend = "local"
            "#,
        )
        .expect("should parse");
        assert_eq!(m.package.name, "acme-support-bot");
        assert_eq!(m.runtime.concurrency, 8);
        assert_eq!(m.dependencies.len(), 2);
        assert_eq!(m.providers["openai"].vendor, "openai");
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
    fn bare_string_capability_is_just_a_model_name() {
        let m = parse(
            r#"
            [package]
            name = "tiny"
            version = "0.1.0"
            ulexite = "^0.1"

            [providers.anthropic]
            vendor = "anthropic"
            api_key_env = "ANTHROPIC_API_KEY"
            chat = "claude-3-5-sonnet-20241022"
            vision = "claude-3-5-sonnet-20241022"
            "#,
        )
        .expect("should parse");
        let entry = &m.providers["anthropic"];
        assert_eq!(entry.vendor, "anthropic");
        assert_eq!(entry.api_key_env.as_deref(), Some("ANTHROPIC_API_KEY"));
        assert_eq!(
            entry.capabilities["chat"].model(),
            Some("claude-3-5-sonnet-20241022")
        );
        assert!(entry.capabilities["chat"].params().is_empty());
        assert_eq!(entry.capabilities.len(), 2);
    }

    #[test]
    fn detailed_capability_table_carries_params() {
        let m = parse(
            r#"
            [package]
            name = "tiny"
            version = "0.1.0"
            ulexite = "^0.1"

            [providers.openai]
            vendor = "openai"
            api_key_env = "OPENAI_API_KEY"

            [providers.openai.chat]
            model = "gpt-4o-mini"

            [providers.openai.chat.params]
            temperature = 0.2
            max_tokens = 512
            "#,
        )
        .expect("should parse");
        let chat = &m.providers["openai"].capabilities["chat"];
        assert_eq!(chat.model(), Some("gpt-4o-mini"));
        assert_eq!(
            chat.params().get("temperature").and_then(|v| v.as_float()),
            Some(0.2)
        );
    }

    #[test]
    fn distinct_entries_for_the_same_vendor_are_fine() {
        let m = parse(
            r#"
            [package]
            name = "tiny"
            version = "0.1.0"
            ulexite = "^0.1"

            [providers.vllm_local]
            vendor = "openai_compatible"
            base_url = "http://localhost:8000/v1"
            chat = "meta-llama/Llama-3-8b"

            [providers.vllm_remote]
            vendor = "openai_compatible"
            base_url = "http://gpu-box:8000/v1"
            chat = "meta-llama/Llama-3-70b"
            "#,
        )
        .expect("should parse");
        assert_eq!(
            m.providers["vllm_local"].base_url.as_deref(),
            Some("http://localhost:8000/v1")
        );
        assert_eq!(
            m.providers["vllm_remote"].base_url.as_deref(),
            Some("http://gpu-box:8000/v1")
        );
    }

    #[test]
    fn provider_without_vendor_is_rejected() {
        let err = parse(
            r#"
            [package]
            name = "tiny"
            version = "0.1.0"
            ulexite = "^0.1"

            [providers.default]
            chat = "gpt-4o-mini"
            "#,
        )
        .unwrap_err();
        assert!(matches!(err, ManifestError::Parse(_)));
    }

    #[test]
    fn provider_with_no_capabilities_is_rejected() {
        let err = parse(
            r#"
            [package]
            name = "tiny"
            version = "0.1.0"
            ulexite = "^0.1"

            [providers.default]
            vendor = "openai"
            "#,
        )
        .unwrap_err();
        assert!(matches!(err, ManifestError::Invalid(_)));
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
