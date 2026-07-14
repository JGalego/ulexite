//! Builds a `ProviderRegistry` for `ulx run` from `ulexite.toml`'s
//! `[providers]` table (§14.1). Each `[providers.<name>]` entry is one
//! vendor account/deployment that may serve several capabilities — this
//! module expands it into one `ulx_runtime::ProviderSpec` per populated
//! capability (so `ulx-runtime`'s `ProviderSpec`/`build_provider` stay
//! "one spec = one capability" exactly as before, and never need to know
//! about `toml::Value` or the manifest's shape at all). Absent a manifest
//! (or an empty `[providers]` table), behavior is unchanged from before
//! this existed: `ProviderRegistry::with_mock()`.
//!
//! Before any of that, a `.env` file next to the `.ulx` file (if one
//! exists) is loaded into the process environment — the same place
//! `api_key_env` values are read from, so this is purely a convenience so
//! `OPENAI_API_KEY=...` can live in a local, gitignored file instead of
//! being `export`ed by hand every session. Real shell-exported variables
//! always win: `dotenvy::from_path` never overrides an already-set var.

use std::path::Path;

use ulx_runtime::{build_provider, ProviderRegistry, ProviderSpec, Value};

use crate::project_manifest::{self, CapabilityConfig, ProviderEntry};

pub fn resolve_providers(file: &Path) -> Result<ProviderRegistry, String> {
    let base_dir = crate::manifest::base_dir_of(file);
    load_dotenv(&base_dir)?;

    let manifest_path = base_dir.join("ulexite.toml");
    if !manifest_path.exists() {
        return Ok(ProviderRegistry::with_mock());
    }

    let manifest = project_manifest::load(&manifest_path)
        .map_err(|e| format!("{}: {e}", manifest_path.display()))?;
    if manifest.providers.is_empty() {
        return Ok(ProviderRegistry::with_mock());
    }

    let mut registry = ProviderRegistry::new();
    for (name, entry) in &manifest.providers {
        for (capability, cap_config) in &entry.capabilities {
            let spec = to_provider_spec(entry, capability, cap_config);
            let provider = build_provider(&spec).map_err(|e| {
                format!(
                    "provider `{name}` capability `{capability}` (vendor `{}`): {e}",
                    entry.vendor
                )
            })?;
            registry.register(provider);
        }
    }
    Ok(registry)
}

/// Loads `<dir>/.env` into the process environment, if it exists. Not
/// finding one is fine (most projects won't have one); a malformed one is
/// a real misconfiguration and surfaces as a clear error rather than
/// being silently skipped, same as a malformed `ulexite.toml`.
fn load_dotenv(dir: &Path) -> Result<(), String> {
    let dotenv_path = dir.join(".env");
    if !dotenv_path.exists() {
        return Ok(());
    }
    dotenvy::from_path(&dotenv_path).map_err(|e| format!("{}: {e}", dotenv_path.display()))
}

fn to_provider_spec(
    entry: &ProviderEntry,
    capability: &str,
    cap_config: &CapabilityConfig,
) -> ProviderSpec {
    ProviderSpec {
        vendor: entry.vendor.clone(),
        capability: capability.to_string(),
        model: cap_config.model().map(str::to_string),
        base_url: entry.base_url.clone(),
        api_key_env: entry.api_key_env.clone(),
        api_version: entry.api_version.clone(),
        params: cap_config
            .params()
            .iter()
            .map(|(k, v)| (k.clone(), toml_to_value(v)))
            .collect(),
    }
}

fn toml_to_value(v: &toml::Value) -> Value {
    match v {
        toml::Value::String(s) => Value::Text(s.clone()),
        toml::Value::Integer(i) => Value::Int(*i),
        toml::Value::Float(f) => Value::Float(*f),
        toml::Value::Boolean(b) => Value::Bool(*b),
        toml::Value::Array(items) => Value::List(items.iter().map(toml_to_value).collect()),
        toml::Value::Table(t) => Value::Record(
            t.iter()
                .map(|(k, v)| (k.clone(), toml_to_value(v)))
                .collect(),
        ),
        toml::Value::Datetime(dt) => Value::Text(dt.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn temp_test_dir(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ulexite-dotenv-test-{label}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn missing_dotenv_file_is_fine() {
        let dir = temp_test_dir("missing");
        assert!(load_dotenv(&dir).is_ok());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn dotenv_file_sets_unset_vars() {
        let dir = temp_test_dir("sets-unset");
        let var_name = "ULEXITE_TEST_DOTENV_UNSET_VAR";
        std::env::remove_var(var_name);
        std::fs::write(dir.join(".env"), format!("{var_name}=from-dotenv\n")).unwrap();

        load_dotenv(&dir).unwrap();
        assert_eq!(std::env::var(var_name).as_deref(), Ok("from-dotenv"));

        std::env::remove_var(var_name);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn dotenv_file_does_not_override_a_real_env_var() {
        let dir = temp_test_dir("no-override");
        let var_name = "ULEXITE_TEST_DOTENV_ALREADY_SET_VAR";
        std::env::set_var(var_name, "from-shell");
        std::fs::write(dir.join(".env"), format!("{var_name}=from-dotenv\n")).unwrap();

        load_dotenv(&dir).unwrap();
        assert_eq!(std::env::var(var_name).as_deref(), Ok("from-shell"));

        std::env::remove_var(var_name);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn bare_model_string_becomes_a_model_only_spec() {
        let entry = ProviderEntry {
            vendor: "anthropic".to_string(),
            base_url: None,
            api_key_env: Some("ANTHROPIC_API_KEY".to_string()),
            api_version: None,
            capabilities: BTreeMap::new(),
        };
        let cap = CapabilityConfig::Model("claude-3-5-sonnet-20241022".to_string());
        let spec = to_provider_spec(&entry, "chat", &cap);
        assert_eq!(spec.vendor, "anthropic");
        assert_eq!(spec.capability, "chat");
        assert_eq!(spec.model.as_deref(), Some("claude-3-5-sonnet-20241022"));
        assert!(spec.params.is_empty());
    }

    #[test]
    fn detailed_config_carries_params_through() {
        let entry = ProviderEntry {
            vendor: "openai".to_string(),
            base_url: None,
            api_key_env: None,
            api_version: None,
            capabilities: BTreeMap::new(),
        };
        let mut params = BTreeMap::new();
        params.insert("temperature".to_string(), toml::Value::Float(0.2));
        let cap = CapabilityConfig::Detailed {
            model: Some("gpt-4o-mini".to_string()),
            params,
        };
        let spec = to_provider_spec(&entry, "chat", &cap);
        assert_eq!(spec.model.as_deref(), Some("gpt-4o-mini"));
        assert_eq!(spec.params.get("temperature"), Some(&Value::Float(0.2)));
    }
}
