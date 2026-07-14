//! Builds a `ProviderRegistry` for `ulx run` from `ulexite.toml`'s
//! `[providers]` table (§14.1), translating the manifest's toml-shaped
//! `ProviderPolicy` into `ulx-runtime`'s vendor-neutral `ProviderSpec` so
//! the runtime crate never needs to know about `toml::Value`. Absent a
//! manifest (or an empty `[providers]` table), behavior is unchanged from
//! before this existed: `ProviderRegistry::with_mock()`.

use std::collections::BTreeMap;
use std::path::Path;

use ulx_runtime::{build_provider, ProviderRegistry, ProviderSpec, Value};

use crate::project_manifest::{self, ProviderPolicy};

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
    for (name, policy) in &manifest.providers {
        let spec = to_provider_spec(policy);
        let provider = build_provider(&spec)
            .map_err(|e| format!("provider `{name}` (vendor `{}`): {e}", spec.vendor))?;
        registry.register(provider);
    }
    Ok(registry)
}

fn to_provider_spec(policy: &ProviderPolicy) -> ProviderSpec {
    ProviderSpec {
        vendor: policy.vendor.clone().unwrap_or_else(|| "mock".to_string()),
        capability: policy.capability.clone(),
        model: policy.model.clone(),
        base_url: policy.base_url.clone(),
        api_key_env: policy.api_key_env.clone(),
        params: policy
            .params
            .iter()
            .map(|(k, v)| (k.clone(), toml_to_value(v)))
            .collect::<BTreeMap<_, _>>(),
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
