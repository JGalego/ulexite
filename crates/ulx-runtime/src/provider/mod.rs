//! The provider abstraction (§12.4): a `Provider` is a plugin satisfying
//! one or more capabilities; the runtime never names a vendor, only a
//! capability. `mock::MockProvider` remains the zero-config default —
//! deterministic and network-free, so the whole test suite (and anyone
//! trying the language without an API key) gets real, reproducible
//! behavior. Real HTTP-backed adapters (`openai_compat`, `anthropic`,
//! `gemini`, `cohere`, `ollama`) cover `chat` (and, where the vendor has a
//! simple stable endpoint, `embed`) across the mainstream hosted APIs and
//! local/self-hosted runtimes; `vision`/`transcribe`/`speak`/
//! `generate_image` stay mock-only for now — see `docs/spec/24-limitations.md`.
//!
//! Adding a brand-new provider needs no compiler/grammar/IR change (§12.4):
//! implement `Provider` in a new module and add it to `factory::build_provider`.
//! If it merely speaks an OpenAI-shaped chat/completions API, it needs no
//! new code at all — `openai_compat::OpenAiCompatibleProvider` with a
//! different `base_url` already covers it.

use std::collections::BTreeMap;

use crate::value::Value;

mod anthropic;
mod cohere;
mod factory;
mod gemini;
mod mock;
mod ollama;
mod openai_compat;
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

/// Resolves an unqualified `ask <capability>(...)` to a concrete provider
/// (§12.4, §5.5) — v0.1's "policy" is simply "first registered provider
/// that supports it."
pub struct ProviderRegistry {
    providers: Vec<Box<dyn Provider>>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        ProviderRegistry {
            providers: Vec::new(),
        }
    }

    pub fn with_mock() -> Self {
        let mut r = Self::new();
        r.register(Box::new(MockProvider::new()));
        r
    }

    pub fn register(&mut self, provider: Box<dyn Provider>) {
        self.providers.push(provider);
    }

    pub fn resolve(&self, capability: &str) -> Option<&dyn Provider> {
        self.providers
            .iter()
            .find(|p| p.supports(capability))
            .map(|p| p.as_ref())
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
