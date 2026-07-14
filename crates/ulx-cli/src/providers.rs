//! Builds a `ProviderRegistry` for `ulx run` from two sources that get
//! merged into one namespace: `ulexite.toml`'s `[providers.<name>]` tables
//! (§14.1) and `.ulx` `provider <name> [from "<manifest-entry>"] { ... }`
//! decls (§12.4) — `pipeline::load` collects the latter across the whole
//! workspace (own file + imports) since providers are pure config, never
//! routed through `ulx-ir`. A `.ulx` decl with `from` inherits a manifest
//! entry's fields, overriding them field-by-field/capability-by-capability;
//! one with no `from` must be fully self-contained (checked defensively
//! here even though `ulx-sema` already validates it). Each resolved
//! provider expands into one `ulx_runtime::ProviderSpec` per capability, so
//! `ulx-runtime` stays "one spec = one capability" and never needs to know
//! about `toml::Value`/`Expr` or either config shape.
//!
//! Absent any configured provider at all, `ulx run` now errors instead of
//! silently defaulting to mock — pass `--mock` to opt into the
//! deterministic offline provider explicitly, or `--provider name` to pick
//! a specific configured one when more than one exists for a capability
//! (`ProviderRegistry::resolve`'s new ambiguity check would otherwise
//! reject an unqualified `ask` covering more than one candidate).
//!
//! Before any of that, a `.env` file next to the `.ulx` file (if one
//! exists) is loaded into the process environment — the same place
//! `api_key_env` values are read from, so this is purely a convenience so
//! `OPENAI_API_KEY=...` can live in a local, gitignored file instead of
//! being `export`ed by hand every session. Real shell-exported variables
//! always win: `dotenvy::from_path` never overrides an already-set var.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use ulx_ast::{Expr, ProviderDecl, Spanned};
use ulx_runtime::{build_provider, ProviderRegistry, ProviderSpec, Value};

use crate::project_manifest::{self, CapabilityConfig, ProviderEntry};

const PROVIDER_SCALAR_FIELDS: [&str; 4] = ["vendor", "api_key_env", "base_url", "api_version"];

/// One provider's fully-merged config, whichever combination of
/// `ulexite.toml` and `.ulx` `provider` decl it came from — the common
/// shape both sides get converted into before building real `Provider`s.
#[derive(Debug, Clone, Default)]
struct ResolvedProvider {
    vendor: String,
    base_url: Option<String>,
    api_key_env: Option<String>,
    api_version: Option<String>,
    capabilities: BTreeMap<String, ResolvedCapability>,
}

#[derive(Debug, Clone, Default)]
struct ResolvedCapability {
    model: Option<String>,
    params: BTreeMap<String, Value>,
}

impl ResolvedProvider {
    fn from_manifest_entry(entry: &ProviderEntry) -> Self {
        ResolvedProvider {
            vendor: entry.vendor.clone(),
            base_url: entry.base_url.clone(),
            api_key_env: entry.api_key_env.clone(),
            api_version: entry.api_version.clone(),
            capabilities: entry
                .capabilities
                .iter()
                .map(|(name, cfg)| (name.clone(), resolved_capability_from_toml(cfg)))
                .collect(),
        }
    }
}

fn resolved_capability_from_toml(cfg: &CapabilityConfig) -> ResolvedCapability {
    ResolvedCapability {
        model: cfg.model().map(str::to_string),
        params: cfg
            .params()
            .iter()
            .map(|(k, v)| (k.clone(), toml_to_value(v)))
            .collect(),
    }
}

pub fn resolve_providers(
    file: &Path,
    provider_decls: &[ProviderDecl],
    selected: &[String],
    force_mock: bool,
) -> Result<ProviderRegistry, String> {
    let base_dir = crate::manifest::base_dir_of(file);
    load_dotenv(&base_dir)?;

    if force_mock {
        return Ok(ProviderRegistry::with_mock());
    }

    let manifest = project_manifest::discover(&base_dir).map_err(|e| e.to_string())?;
    let manifest_exists = manifest.is_some();
    let manifest_entries = manifest.map(|m| m.providers).unwrap_or_default();

    let mut merged: BTreeMap<String, ResolvedProvider> = manifest_entries
        .iter()
        .map(|(name, entry)| (name.clone(), ResolvedProvider::from_manifest_entry(entry)))
        .collect();

    for decl in provider_decls {
        let resolved = resolve_ulx_provider(decl, &manifest_entries, manifest_exists, file)?;
        match merged.get(&decl.name) {
            Some(_) if decl.from.as_deref() != Some(decl.name.as_str()) => {
                return Err(format!(
                    "provider `{}` is declared both in ulexite.toml and in {} — rename one",
                    decl.name,
                    file.display()
                ));
            }
            _ => {
                merged.insert(decl.name.clone(), resolved);
            }
        }
    }

    if !selected.is_empty() {
        let selected_set: BTreeSet<&str> = selected.iter().map(String::as_str).collect();
        let unknown: Vec<&str> = selected_set
            .iter()
            .filter(|name| !merged.contains_key(**name))
            .copied()
            .collect();
        if !unknown.is_empty() {
            return Err(format!(
                "--provider named unknown provider(s): {unknown:?} (known: {:?})",
                merged.keys().collect::<Vec<_>>()
            ));
        }
        merged.retain(|name, _| selected_set.contains(name.as_str()));
    }

    if merged.is_empty() {
        return Err(format!(
            "no provider is configured (no ulexite.toml [providers] entries, no provider blocks \
             in {} or its imports) — pass --mock to run against the deterministic mock provider, \
             or configure a real one (see README's \"Configuring providers\")",
            file.display()
        ));
    }

    let artifact_root = crate::manifest::artifacts_dir();
    let mut registry = ProviderRegistry::new();
    for (name, resolved) in &merged {
        for (capability, cap) in &resolved.capabilities {
            let spec = to_provider_spec(resolved, capability, cap);
            let provider = build_provider(&spec, &artifact_root).map_err(|e| {
                format!(
                    "provider `{name}` capability `{capability}` (vendor `{}`): {e}",
                    resolved.vendor
                )
            })?;
            registry.register(name.clone(), provider);
        }
    }
    Ok(registry)
}

/// Resolves one `.ulx` `provider` decl into a `ResolvedProvider`: starts
/// from its `from`-inherited manifest entry (if any) and overlays the
/// block's own scalar/capability fields on top.
fn resolve_ulx_provider(
    decl: &ProviderDecl,
    manifest_entries: &BTreeMap<String, ProviderEntry>,
    manifest_exists: bool,
    file: &Path,
) -> Result<ResolvedProvider, String> {
    let mut resolved = match &decl.from {
        Some(from_name) => match manifest_entries.get(from_name) {
            Some(entry) => ResolvedProvider::from_manifest_entry(entry),
            None if manifest_exists => {
                return Err(format!(
                    "provider `{}` has `from \"{from_name}\"`, but ulexite.toml has no \
                     `[providers.{from_name}]` entry",
                    decl.name
                ));
            }
            None => {
                return Err(format!(
                    "provider `{}` has `from \"{from_name}\"`, but no ulexite.toml exists next \
                     to {}",
                    decl.name,
                    file.display()
                ));
            }
        },
        None => ResolvedProvider::default(),
    };

    if let Some(v) = scalar_field(&decl.fields, "vendor") {
        resolved.vendor = v.to_string();
    }
    if let Some(v) = scalar_field(&decl.fields, "base_url") {
        resolved.base_url = Some(v.to_string());
    }
    if let Some(v) = scalar_field(&decl.fields, "api_key_env") {
        resolved.api_key_env = Some(v.to_string());
    }
    if let Some(v) = scalar_field(&decl.fields, "api_version") {
        resolved.api_version = Some(v.to_string());
    }
    if resolved.vendor.is_empty() {
        // Defense in depth — `ulx-sema`'s `check_provider` already rejects
        // a standalone (no `from`) provider decl with no `vendor` field.
        return Err(format!(
            "provider `{}` has no `from` and no `vendor`",
            decl.name
        ));
    }

    for (name, value) in &decl.fields {
        if PROVIDER_SCALAR_FIELDS.contains(&name.as_str()) {
            continue;
        }
        resolved
            .capabilities
            .insert(name.clone(), resolved_capability_from_expr(&value.0));
    }

    Ok(resolved)
}

fn scalar_field<'a>(fields: &'a [(String, Spanned<Expr>)], name: &str) -> Option<&'a str> {
    fields
        .iter()
        .find(|(n, _)| n == name)
        .and_then(|(_, v)| match &v.0 {
            Expr::Str(s) => Some(s.as_str()),
            _ => None,
        })
}

fn resolved_capability_from_expr(value: &Expr) -> ResolvedCapability {
    match value {
        Expr::Str(model) => ResolvedCapability {
            model: Some(model.clone()),
            params: BTreeMap::new(),
        },
        Expr::RecordLit(fields) => {
            let mut model = None;
            let mut params = BTreeMap::new();
            for (name, v) in fields {
                if name == "model" {
                    if let Expr::Str(s) = &v.0 {
                        model = Some(s.clone());
                    }
                } else {
                    params.insert(name.clone(), expr_literal_to_value(&v.0));
                }
            }
            ResolvedCapability { model, params }
        }
        // `ulx-sema`'s `check_provider` already rejects any other shape.
        _ => ResolvedCapability::default(),
    }
}

fn expr_literal_to_value(e: &Expr) -> Value {
    match e {
        Expr::Str(s) => Value::Text(s.clone()),
        Expr::Int(i) => Value::Int(*i),
        Expr::Float(f) => Value::Float(*f),
        _ => Value::Unit,
    }
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
    resolved: &ResolvedProvider,
    capability: &str,
    cap: &ResolvedCapability,
) -> ProviderSpec {
    ProviderSpec {
        vendor: resolved.vendor.clone(),
        capability: capability.to_string(),
        model: cap.model.clone(),
        base_url: resolved.base_url.clone(),
        api_key_env: resolved.api_key_env.clone(),
        api_version: resolved.api_version.clone(),
        params: cap.params.clone(),
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
        let resolved = ResolvedProvider::from_manifest_entry(&entry);
        let cap = resolved_capability_from_toml(&CapabilityConfig::Model(
            "claude-3-5-sonnet-20241022".to_string(),
        ));
        let spec = to_provider_spec(&resolved, "chat", &cap);
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
        let resolved = ResolvedProvider::from_manifest_entry(&entry);
        let mut params = BTreeMap::new();
        params.insert("temperature".to_string(), toml::Value::Float(0.2));
        let cap = resolved_capability_from_toml(&CapabilityConfig::Detailed {
            model: Some("gpt-4o-mini".to_string()),
            params,
        });
        let spec = to_provider_spec(&resolved, "chat", &cap);
        assert_eq!(spec.model.as_deref(), Some("gpt-4o-mini"));
        assert_eq!(spec.params.get("temperature"), Some(&Value::Float(0.2)));
    }

    #[test]
    fn ulx_provider_bare_string_capability_is_a_model_only_config() {
        let cap = resolved_capability_from_expr(&Expr::Str("gpt-4o-mini".to_string()));
        assert_eq!(cap.model.as_deref(), Some("gpt-4o-mini"));
        assert!(cap.params.is_empty());
    }

    #[test]
    fn ulx_provider_record_capability_extracts_model_and_params() {
        let record = Expr::RecordLit(vec![
            (
                "model".to_string(),
                (Expr::Str("gpt-4o-mini".to_string()), 0..0),
            ),
            ("temperature".to_string(), (Expr::Float(0.2), 0..0)),
        ]);
        let cap = resolved_capability_from_expr(&record);
        assert_eq!(cap.model.as_deref(), Some("gpt-4o-mini"));
        assert_eq!(cap.params.get("temperature"), Some(&Value::Float(0.2)));
    }

    fn provider_decl(name: &str, from: Option<&str>, fields: Vec<(&str, Expr)>) -> ProviderDecl {
        ProviderDecl {
            doc: None,
            name: name.to_string(),
            from: from.map(str::to_string),
            fields: fields
                .into_iter()
                .map(|(k, v)| (k.to_string(), (v, 0..0)))
                .collect(),
        }
    }

    #[test]
    fn standalone_ulx_provider_needs_no_manifest() {
        let decl = provider_decl(
            "MyAnthropic",
            None,
            vec![
                ("vendor", Expr::Str("anthropic".to_string())),
                ("api_key_env", Expr::Str("ANTHROPIC_API_KEY".to_string())),
                ("chat", Expr::Str("claude-3-5-sonnet-20241022".to_string())),
            ],
        );
        let resolved =
            resolve_ulx_provider(&decl, &BTreeMap::new(), false, Path::new("main.ulx")).unwrap();
        assert_eq!(resolved.vendor, "anthropic");
        assert_eq!(
            resolved.capabilities["chat"].model.as_deref(),
            Some("claude-3-5-sonnet-20241022")
        );
    }

    #[test]
    fn from_with_no_manifest_is_a_clear_error() {
        let decl = provider_decl("MyAnthropic", Some("anthropic"), vec![]);
        let err = resolve_ulx_provider(&decl, &BTreeMap::new(), false, Path::new("main.ulx"))
            .unwrap_err();
        assert!(err.contains("no ulexite.toml exists"));
    }

    #[test]
    fn from_missing_entry_in_existing_manifest_is_a_clear_error() {
        let decl = provider_decl("MyAnthropic", Some("anthropic"), vec![]);
        let err =
            resolve_ulx_provider(&decl, &BTreeMap::new(), true, Path::new("main.ulx")).unwrap_err();
        assert!(err.contains("no `[providers.anthropic]` entry"));
    }

    #[test]
    fn from_inheriting_entry_overlays_fields() {
        let mut manifest_entries = BTreeMap::new();
        let mut capabilities = BTreeMap::new();
        capabilities.insert(
            "chat".to_string(),
            CapabilityConfig::Model("claude-3-5-sonnet-20241022".to_string()),
        );
        manifest_entries.insert(
            "anthropic".to_string(),
            ProviderEntry {
                vendor: "anthropic".to_string(),
                base_url: None,
                api_key_env: Some("ANTHROPIC_API_KEY".to_string()),
                api_version: None,
                capabilities,
            },
        );
        let decl = provider_decl(
            "MyAnthropic",
            Some("anthropic"),
            vec![(
                "vision",
                Expr::Str("claude-3-5-sonnet-20241022".to_string()),
            )],
        );
        let resolved =
            resolve_ulx_provider(&decl, &manifest_entries, true, Path::new("main.ulx")).unwrap();
        assert_eq!(resolved.vendor, "anthropic");
        assert!(resolved.capabilities.contains_key("chat"));
        assert!(resolved.capabilities.contains_key("vision"));
    }
}
