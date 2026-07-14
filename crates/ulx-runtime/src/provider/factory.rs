//! The one place vendor name strings from `ulexite.toml`'s `[providers.*]`
//! table (`vendor = "..."`) resolve to a concrete `Provider` — default
//! `base_url`/`api_key_env` presets per vendor, secrets read from the named
//! environment variable (never a literal key in the manifest). Adding a new
//! *named* vendor preset is one match arm here; a vendor that already
//! speaks an OpenAI-shaped API needs no new arm at all — set
//! `vendor = "openai_compatible"` with a custom `base_url`.

use std::collections::BTreeMap;

use crate::value::Value;

use super::anthropic::AnthropicProvider;
use super::cohere::{self, CohereProvider};
use super::gemini::{self, GeminiProvider};
use super::mock::MockProvider;
use super::ollama::{self, OllamaProvider};
use super::openai_compat::OpenAiCompatibleProvider;
use super::transport::UreqTransport;
use super::Provider;

#[derive(Debug, Clone, Default)]
pub struct ProviderSpec {
    pub vendor: String,
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub api_key_env: Option<String>,
    pub params: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProviderBuildError {
    /// The named environment variable isn't set.
    MissingApiKey(String),
    UnknownVendor(String),
    InvalidConfig(String),
}

impl std::fmt::Display for ProviderBuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProviderBuildError::MissingApiKey(env) => {
                write!(f, "environment variable `{env}` is not set")
            }
            ProviderBuildError::UnknownVendor(v) => write!(f, "unknown provider vendor `{v}`"),
            ProviderBuildError::InvalidConfig(msg) => write!(f, "invalid provider config: {msg}"),
        }
    }
}

pub fn build_provider(spec: &ProviderSpec) -> Result<Box<dyn Provider>, ProviderBuildError> {
    match spec.vendor.as_str() {
        "mock" => Ok(Box::new(MockProvider::new())),
        "openai" => build_openai_family(
            spec,
            "openai",
            "https://api.openai.com/v1",
            Some("OPENAI_API_KEY"),
            "gpt-4o-mini",
        ),
        "groq" => build_openai_family(
            spec,
            "groq",
            "https://api.groq.com/openai/v1",
            Some("GROQ_API_KEY"),
            "llama-3.3-70b-versatile",
        ),
        "openai_compatible" => {
            let base_url = spec.base_url.clone().ok_or_else(|| {
                ProviderBuildError::InvalidConfig(
                    "vendor `openai_compatible` requires `base_url`".to_string(),
                )
            })?;
            build_openai_family(spec, "openai_compatible", &base_url, None, "")
        }
        "anthropic" => {
            let base_url = spec
                .base_url
                .clone()
                .unwrap_or_else(|| "https://api.anthropic.com/v1".to_string());
            let api_key = require_api_key(spec, "ANTHROPIC_API_KEY")?;
            let model = spec
                .model
                .clone()
                .unwrap_or_else(|| "claude-3-5-sonnet-20241022".to_string());
            Ok(Box::new(AnthropicProvider::with_transport(
                base_url,
                api_key,
                model,
                spec.params.clone(),
                Box::new(UreqTransport::default()),
            )))
        }
        "gemini" => {
            let base_url = spec
                .base_url
                .clone()
                .unwrap_or_else(|| gemini::DEFAULT_BASE_URL.to_string());
            let api_key = require_api_key(spec, "GEMINI_API_KEY")?;
            let model = spec
                .model
                .clone()
                .unwrap_or_else(|| "gemini-1.5-flash".to_string());
            Ok(Box::new(GeminiProvider::with_transport(
                base_url,
                api_key,
                model,
                spec.params.clone(),
                Box::new(UreqTransport::default()),
            )))
        }
        "cohere" => {
            let base_url = spec
                .base_url
                .clone()
                .unwrap_or_else(|| cohere::DEFAULT_BASE_URL.to_string());
            let api_key = require_api_key(spec, "COHERE_API_KEY")?;
            let model = spec
                .model
                .clone()
                .unwrap_or_else(|| "command-r".to_string());
            Ok(Box::new(CohereProvider::with_transport(
                base_url,
                api_key,
                model,
                spec.params.clone(),
                Box::new(UreqTransport::default()),
            )))
        }
        "ollama" => {
            let base_url = spec
                .base_url
                .clone()
                .unwrap_or_else(|| ollama::DEFAULT_BASE_URL.to_string());
            let model = spec.model.clone().unwrap_or_else(|| "llama3".to_string());
            Ok(Box::new(OllamaProvider::with_transport(
                base_url,
                model,
                spec.params.clone(),
                Box::new(UreqTransport::default()),
            )))
        }
        other => Err(ProviderBuildError::UnknownVendor(other.to_string())),
    }
}

fn build_openai_family(
    spec: &ProviderSpec,
    id: &str,
    default_base_url: &str,
    default_env: Option<&str>,
    default_model: &str,
) -> Result<Box<dyn Provider>, ProviderBuildError> {
    let base_url = spec
        .base_url
        .clone()
        .unwrap_or_else(|| default_base_url.to_string());
    let env_name = spec
        .api_key_env
        .clone()
        .or_else(|| default_env.map(str::to_string));
    let api_key = match env_name {
        Some(name) => Some(
            std::env::var(&name).map_err(|_| ProviderBuildError::MissingApiKey(name.clone()))?,
        ),
        None => None,
    };
    let model = spec
        .model
        .clone()
        .unwrap_or_else(|| default_model.to_string());
    Ok(Box::new(OpenAiCompatibleProvider::with_transport(
        id.to_string(),
        base_url,
        api_key,
        model,
        spec.params.clone(),
        Box::new(UreqTransport::default()),
    )))
}

fn require_api_key(spec: &ProviderSpec, default_env: &str) -> Result<String, ProviderBuildError> {
    let env_name = spec
        .api_key_env
        .clone()
        .unwrap_or_else(|| default_env.to_string());
    std::env::var(&env_name).map_err(|_| ProviderBuildError::MissingApiKey(env_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_vendor_builds_without_any_env() {
        let spec = ProviderSpec {
            vendor: "mock".to_string(),
            ..Default::default()
        };
        assert!(build_provider(&spec).is_ok());
    }

    #[test]
    fn unknown_vendor_is_rejected() {
        let spec = ProviderSpec {
            vendor: "not-a-real-vendor".to_string(),
            ..Default::default()
        };
        assert_eq!(
            build_provider(&spec).err().unwrap(),
            ProviderBuildError::UnknownVendor("not-a-real-vendor".to_string())
        );
    }

    #[test]
    fn openai_compatible_without_base_url_is_rejected() {
        let spec = ProviderSpec {
            vendor: "openai_compatible".to_string(),
            ..Default::default()
        };
        assert!(matches!(
            build_provider(&spec).err().unwrap(),
            ProviderBuildError::InvalidConfig(_)
        ));
    }

    #[test]
    fn openai_compatible_with_base_url_and_no_key_env_needs_no_auth() {
        let spec = ProviderSpec {
            vendor: "openai_compatible".to_string(),
            base_url: Some("http://localhost:8000/v1".to_string()),
            ..Default::default()
        };
        assert!(build_provider(&spec).is_ok());
    }

    #[test]
    fn anthropic_without_api_key_env_set_is_rejected() {
        let env_name = "ULEXITE_TEST_MISSING_ANTHROPIC_KEY_XYZ";
        std::env::remove_var(env_name);
        let spec = ProviderSpec {
            vendor: "anthropic".to_string(),
            api_key_env: Some(env_name.to_string()),
            ..Default::default()
        };
        assert_eq!(
            build_provider(&spec).err().unwrap(),
            ProviderBuildError::MissingApiKey(env_name.to_string())
        );
    }

    #[test]
    fn ollama_uses_default_base_url_with_no_key_needed() {
        let spec = ProviderSpec {
            vendor: "ollama".to_string(),
            ..Default::default()
        };
        assert!(build_provider(&spec).is_ok());
    }
}
