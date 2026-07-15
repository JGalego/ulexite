//! The provider abstraction (§12.4): a `Provider` is a plugin satisfying
//! one or more capabilities; the runtime never names a vendor, only a
//! capability. `mock::MockProvider` remains the zero-config default —
//! deterministic and network-free, so the whole test suite (and anyone
//! trying the language without an API key) gets real, reproducible
//! behavior. Real HTTP-backed adapters cover `chat` (every vendor),
//! `embed` (`openai_compat`, `gemini`, `cohere`, `ollama`), `vision`
//! (`openai_compat`, `anthropic`, `gemini`, `ollama`, `azure_openai`),
//! `transcribe`/`speak`/`generate_image` (`openai_compat` only), and now
//! `judge` (every vendor with a `chat` path, via the shared `judge` module
//! — a judge call is just a rubric-evaluation chat completion, parsed into
//! a `Verdict`) — see `docs/spec/24-limitations.md` for exactly what's
//! still mock-only.
//!
//! Adding a brand-new provider needs no compiler/grammar/IR change (§12.4):
//! implement `Provider` in a new module and add it to `factory::build_provider`.
//! If it merely speaks an OpenAI-shaped chat/completions API with a plain
//! `base_url` and `Authorization: Bearer` auth, it needs no new code at
//! all — `openai_compat::OpenAiCompatibleProvider` already covers it;
//! `openai_shape` factors out the request/response JSON shape itself so a
//! vendor with the same JSON shape but different URL/auth conventions
//! (`azure_openai`) only has to write the URL-building and header logic.

use std::collections::BTreeMap;

use crate::value::Value;

mod anthropic;
mod artifact;
mod azure_openai;
mod cohere;
mod factory;
mod gemini;
mod judge;
mod mock;
mod ollama;
mod openai_compat;
mod openai_shape;
mod transport;

pub use factory::{build_provider, ProviderBuildError, ProviderSpec};
pub use mock::MockProvider;

#[derive(Debug, Clone)]
pub struct Message {
    pub role: String,
    pub text: String,
}

#[derive(Debug, Clone, Default)]
pub struct Invocation {
    pub messages: Vec<Message>,
    pub args: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProviderError {
    UnsupportedCapability(String),
    Failed(String),
    /// The vendor responded with a rate-limit signal (HTTP 429). Mapped by
    /// the interpreter to `Value::Unsettled(DraftOutcome::RateLimited)`
    /// rather than propagated as a hard error (§9.3's `Draft<T>`).
    RateLimited,
    /// The request timed out client-side. Mapped to
    /// `Value::Unsettled(DraftOutcome::Timeout)`.
    Timeout,
    /// The vendor declined to answer on safety/policy grounds. Mapped to
    /// `Value::Unsettled(DraftOutcome::Refused(reason))`.
    Refused(String),
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProviderError::UnsupportedCapability(c) => {
                write!(f, "provider does not support capability `{c}`")
            }
            ProviderError::Failed(msg) => write!(f, "provider call failed: {msg}"),
            ProviderError::RateLimited => write!(f, "provider rate-limited the request"),
            ProviderError::Timeout => write!(f, "provider request timed out"),
            ProviderError::Refused(reason) => write!(f, "provider refused the request: {reason}"),
        }
    }
}

pub trait Provider: Send + Sync {
    fn id(&self) -> &str;
    fn supports(&self, capability: &str) -> bool;
    fn invoke(&self, capability: &str, request: &Invocation) -> Result<Value, ProviderError>;
}

/// Failure modes for resolving a capability (optionally by name) against
/// the registry — distinct from `ProviderError`, which is what a *found*
/// provider returns once actually invoked.
#[derive(Debug, Clone, PartialEq)]
pub enum ResolveError {
    /// No registered provider supports this capability at all.
    UnknownCapability(String),
    /// 2+ registered providers support this capability and nothing
    /// disambiguated it (§12.4 — no more silent first-match).
    Ambiguous {
        capability: String,
        candidates: Vec<String>,
    },
    /// `provider: "name"` (or `--provider name`) named something that
    /// isn't registered at all.
    UnknownProvider(String),
    /// `provider: "name"` named a real, registered provider that just
    /// doesn't support this capability.
    ProviderDoesNotSupportCapability { name: String, capability: String },
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolveError::UnknownCapability(c) => {
                write!(f, "no provider registered supports capability `{c}`")
            }
            ResolveError::Ambiguous {
                capability,
                candidates,
            } => write!(
                f,
                "capability `{capability}` is served by multiple providers {candidates:?} — add \
                 `provider: \"name\"` to this call, or pass --provider name on the CLI"
            ),
            ResolveError::UnknownProvider(name) => {
                write!(f, "no provider named `{name}` is registered")
            }
            ResolveError::ProviderDoesNotSupportCapability { name, capability } => write!(
                f,
                "provider `{name}` does not support capability `{capability}`"
            ),
        }
    }
}

/// Resolves an unqualified `ask <capability>(...)` to a concrete provider
/// (§12.4, §5.5) — every registered provider is tracked by name so a
/// `provider: "name"` arg (or `--provider name` on the CLI) can pick a
/// specific one; with no name given, 2+ providers supporting the same
/// capability is an explicit `Ambiguous` error, not a silent first-match.
pub struct ProviderRegistry {
    providers: Vec<(String, Box<dyn Provider>)>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        ProviderRegistry {
            providers: Vec::new(),
        }
    }

    pub fn with_mock() -> Self {
        let mut r = Self::new();
        r.register("mock", Box::new(MockProvider::new()));
        r
    }

    pub fn register(&mut self, name: impl Into<String>, provider: Box<dyn Provider>) {
        self.providers.push((name.into(), provider));
    }

    pub fn resolve(&self, capability: &str) -> Result<&dyn Provider, ResolveError> {
        let matching: Vec<&(String, Box<dyn Provider>)> = self
            .providers
            .iter()
            .filter(|(_, p)| p.supports(capability))
            .collect();
        match matching.as_slice() {
            [] => Err(ResolveError::UnknownCapability(capability.to_string())),
            [(_, provider)] => Ok(provider.as_ref()),
            multiple => Err(ResolveError::Ambiguous {
                capability: capability.to_string(),
                candidates: multiple.iter().map(|(name, _)| name.clone()).collect(),
            }),
        }
    }

    pub fn resolve_named(
        &self,
        capability: &str,
        name: &str,
    ) -> Result<&dyn Provider, ResolveError> {
        let Some((_, provider)) = self.providers.iter().find(|(n, _)| n == name) else {
            return Err(ResolveError::UnknownProvider(name.to_string()));
        };
        if !provider.supports(capability) {
            return Err(ResolveError::ProviderDoesNotSupportCapability {
                name: name.to_string(),
                capability: capability.to_string(),
            });
        }
        Ok(provider.as_ref())
    }
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Per-call `ask chat(model: "...", temperature: 0.2)` args (§9.2 grammar
/// already supports named args) take precedence over a provider's
/// manifest-level `params` defaults (`ulexite.toml`'s `[providers.*.params]`,
/// §14.1) — shared by every real adapter so each one only deals with
/// mapping the resolved value into its own vendor-specific JSON shape.
pub(crate) fn resolve_model(args: &BTreeMap<String, Value>, default_model: &str) -> String {
    args.get("model")
        .and_then(Value::as_text)
        .unwrap_or(default_model)
        .to_string()
}

pub(crate) fn resolve_param<'a>(
    args: &'a BTreeMap<String, Value>,
    defaults: &'a BTreeMap<String, Value>,
    key: &str,
) -> Option<&'a Value> {
    args.get(key).or_else(|| defaults.get(key))
}

pub(crate) fn resolve_f64(
    args: &BTreeMap<String, Value>,
    defaults: &BTreeMap<String, Value>,
    key: &str,
) -> Option<f64> {
    match resolve_param(args, defaults, key) {
        Some(Value::Float(f)) => Some(*f),
        Some(Value::Int(i)) => Some(*i as f64),
        _ => None,
    }
}

pub(crate) fn resolve_i64(
    args: &BTreeMap<String, Value>,
    defaults: &BTreeMap<String, Value>,
    key: &str,
) -> Option<i64> {
    match resolve_param(args, defaults, key) {
        Some(Value::Int(i)) => Some(*i),
        Some(Value::Float(f)) => Some(*f as i64),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_registered_provider_is_unknown_capability() {
        let registry = ProviderRegistry::new();
        let err = registry.resolve("chat").err().unwrap();
        assert_eq!(err, ResolveError::UnknownCapability("chat".to_string()));
    }

    #[test]
    fn single_match_resolves_directly() {
        let mut registry = ProviderRegistry::new();
        registry.register("only", Box::new(MockProvider::new()));
        assert!(registry.resolve("chat").is_ok());
    }

    #[test]
    fn two_matches_is_ambiguous_by_name_not_silent_first_match() {
        let mut registry = ProviderRegistry::new();
        registry.register("a", Box::new(MockProvider::new()));
        registry.register("b", Box::new(MockProvider::new()));
        let err = registry.resolve("chat").err().unwrap();
        assert_eq!(
            err,
            ResolveError::Ambiguous {
                capability: "chat".to_string(),
                candidates: vec!["a".to_string(), "b".to_string()],
            }
        );
    }

    #[test]
    fn resolve_named_finds_the_exact_provider_even_when_ambiguous() {
        let mut registry = ProviderRegistry::new();
        registry.register("a", Box::new(MockProvider::new()));
        registry.register("b", Box::new(MockProvider::new()));
        assert!(registry.resolve_named("chat", "b").is_ok());
    }

    #[test]
    fn resolve_named_unknown_name_is_an_error() {
        let registry = ProviderRegistry::with_mock();
        let err = registry.resolve_named("chat", "nonexistent").err().unwrap();
        assert_eq!(
            err,
            ResolveError::UnknownProvider("nonexistent".to_string())
        );
    }

    #[test]
    fn resolve_named_provider_not_supporting_capability_is_an_error() {
        let registry = ProviderRegistry::with_mock();
        let err = registry
            .resolve_named("not-a-real-capability", "mock")
            .err()
            .unwrap();
        assert_eq!(
            err,
            ResolveError::ProviderDoesNotSupportCapability {
                name: "mock".to_string(),
                capability: "not-a-real-capability".to_string(),
            }
        );
    }
}
