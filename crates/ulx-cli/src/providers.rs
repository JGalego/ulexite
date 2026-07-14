//! Builds a `ProviderRegistry` for `ulx run` from `ulexite.toml`'s
//! `[providers]` table (§14.1). Each `[providers.<name>]` entry is one
//! vendor account/deployment that may serve several capabilities — this
//! module expands it into one `ulx_runtime::ProviderSpec` per populated
//! capability (so `ulx-runtime`'s `ProviderSpec`/`build_provider` stay
//! "one spec = one capability" exactly as before, and never need to know
//! about `toml::Value` or the manifest's shape at all). Absent a manifest
//! (or an empty `[providers]` table), behavior is unchanged from before
//! this existed: `ProviderRegistry::with_mock()`.

use std::path::Path;

use ulx_runtime::{build_provider, ProviderRegistry, ProviderSpec, Value};

use crate::project_manifest::{self, CapabilityConfig, ProviderEntry};

pub fn resolve_providers(file: &Path) -> Result<ProviderRegistry, String> {
    let manifest_path = crate::manifest::base_dir_of(file).join("ulexite.toml");
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

    #[test]
    fn bare_model_string_becomes_a_model_only_spec() {
        let entry = ProviderEntry {
            vendor: "anthropic".to_string(),
            base_url: None,
            api_key_env: Some("ANTHROPIC_API_KEY".to_string()),
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
