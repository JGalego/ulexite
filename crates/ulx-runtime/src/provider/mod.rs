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

use serde::{Deserialize, Serialize};

use crate::value::Value;

// Real HTTP-backed vendor adapters all go through `ureq` (blocking HTTP),
// which doesn't target `wasm32-unknown-unknown` — gated behind
// `real-providers` (on by default, off for the in-browser `ulx-wasm` build)
// so the crate compiles for wasm at all. `judge`/`mock`/`browser` are pure
// logic with no I/O, so they're always available.
#[cfg(feature = "real-providers")]
mod anthropic;
#[cfg(feature = "real-providers")]
mod artifact;
#[cfg(feature = "real-providers")]
mod azure_openai;
mod browser;
#[cfg(feature = "real-providers")]
mod cohere;
#[cfg(feature = "real-providers")]
mod factory;
#[cfg(feature = "real-providers")]
mod gemini;
mod judge;
mod mock;
#[cfg(feature = "real-providers")]
mod ollama;
#[cfg(feature = "real-providers")]
mod openai_compat;
#[cfg(feature = "real-providers")]
mod openai_shape;
#[cfg(feature = "real-providers")]
mod transport;

pub use browser::BrowserLocalProvider;
#[cfg(feature = "real-providers")]
pub use factory::{build_provider, ProviderBuildError, ProviderSpec};
pub use mock::MockProvider;

/// The chat-ready `{role, text}` prompt for a rubric-evaluation ("judge")
/// call, built from the same shared `judge::build_prompt` every real
/// vendor's `judge` match arm feeds into `judge_via_chat` — exposed here so
/// `interp.rs` can hand it to an out-of-band resolver (an in-browser model,
/// via `RuntimeError::Suspended`) without duplicating the prompt format or
/// reaching into a private module.
pub(crate) fn judge_prompt_messages(request: &Invocation) -> Vec<Message> {
    judge::build_prompt(request).messages
}

/// Parses a suspended `judge` call's raw model reply into the same
/// `Value::Verdict` shape a real vendor's `invoke("judge", ...)` would have
/// produced and cached. Public (unlike `judge_prompt_messages`) because
/// `ulx-wasm`'s in-browser driver needs it from outside this crate: once a
/// `RuntimeError::Suspended { target: "judge", .. }` hands the driver a
/// chat-ready prompt and it gets raw completion text back from its local
/// model, that text has to be parsed into a `Verdict` before being written
/// into the cache — a `Value::Text` there would break the interpreter's own
/// `match judge Name(x) { Pass => ..., Fail(reason) => ... }` handling,
/// which expects `Value::Verdict`.
pub fn judge_reply_to_value(text: &str) -> Value {
    Value::Verdict(judge::parse_verdict(text))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// The configured model/deployment name this provider calls, when it
    /// has just one fixed answer for that (every real adapter does — the
    /// `model`/`deployment` field it was built with). `None` for
    /// `MockProvider`, which accepts (and ignores) any model name.
    /// Surfaced in `TraceRecord`/`ulx run`'s dialogue metadata so a
    /// transcript shows not just *that* a provider answered, but which
    /// model it actually was.
    fn model(&self) -> Option<&str> {
        None
    }
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

    /// Like `with_mock`, but registers a separate `MockProvider` under each
    /// given name instead of one anonymous `"mock"` entry — lets `--mock`
    /// still resolve an explicitly named provider (e.g. a `.ulx` `provider
    /// LocalAssistant { ... }` decl referenced via `ask
    /// chat(provider: "LocalAssistant")`) instead of failing with "no
    /// provider named `LocalAssistant` is registered" just because `--mock`
    /// would otherwise replace the whole registry with a single unnamed
    /// entry. Falls back to `with_mock`'s single anonymous entry when
    /// `names` is empty, which is the common case for every other example
    /// (no declared providers at all).
    pub fn with_mock_named(names: impl IntoIterator<Item = String>) -> Self {
        let mut r = Self::new();
        let mut any = false;
        for name in names {
            r.register(name, Box::new(MockProvider::new()));
            any = true;
        }
        if !any {
            r.register("mock", Box::new(MockProvider::new()));
        }
        r
    }

    /// Same "override whatever's declared" trick as `with_mock_named`, for
    /// the in-browser driver: every provider name a loaded `.ulx` source
    /// declares (regardless of that decl's own `vendor:`) resolves to the
    /// same `BrowserLocalProvider` — no `ulexite.toml`/`ProviderSpec`
    /// parsing involved at all.
    pub fn with_browser_local_named(names: impl IntoIterator<Item = String>) -> Self {
        let mut r = Self::new();
        let mut any = false;
        for name in names {
            r.register(name.clone(), Box::new(BrowserLocalProvider::new(name)));
            any = true;
        }
        if !any {
            r.register("local", Box::new(BrowserLocalProvider::new("local")));
        }
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
        // A single `ulexite.toml`/`.ulx provider` entry declaring several
        // capabilities (e.g. groq's `chat` + `transcribe`) registers one
        // real `Provider` instance *per capability* (`build_registry`),
        // all sharing this same name — so `name` alone doesn't uniquely
        // pick one entry, and stopping at the first name match (as this
        // used to) could silently hand back a same-named instance that
        // supports a different capability entirely.
        let mut same_name = self.providers.iter().filter(|(n, _)| n == name).peekable();
        if same_name.peek().is_none() {
            return Err(ResolveError::UnknownProvider(name.to_string()));
        }
        same_name
            .find(|(_, p)| p.supports(capability))
            .map(|(_, p)| p.as_ref())
            .ok_or_else(|| ResolveError::ProviderDoesNotSupportCapability {
                name: name.to_string(),
                capability: capability.to_string(),
            })
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
#[cfg(feature = "real-providers")]
pub(crate) fn resolve_model(args: &BTreeMap<String, Value>, default_model: &str) -> String {
    args.get("model")
        .and_then(Value::as_text)
        .unwrap_or(default_model)
        .to_string()
}

#[cfg(feature = "real-providers")]
pub(crate) fn resolve_param<'a>(
    args: &'a BTreeMap<String, Value>,
    defaults: &'a BTreeMap<String, Value>,
    key: &str,
) -> Option<&'a Value> {
    args.get(key).or_else(|| defaults.get(key))
}

#[cfg(feature = "real-providers")]
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

#[cfg(feature = "real-providers")]
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

    /// A stub that supports exactly one capability — real adapters built by
    /// `ulx-cli`'s `build_registry` are like this: `ProviderSpec.capability`
    /// (§`provider/factory.rs`) is a single string, so a `ulexite.toml`
    /// entry declaring N capabilities becomes N separate `Provider`
    /// instances, all registered under that entry's one name.
    struct SingleCapabilityProvider(&'static str);

    impl Provider for SingleCapabilityProvider {
        fn id(&self) -> &str {
            "single-capability-stub"
        }
        fn supports(&self, capability: &str) -> bool {
            capability == self.0
        }
        fn invoke(&self, _capability: &str, _request: &Invocation) -> Result<Value, ProviderError> {
            Ok(Value::Text("stub".to_string()))
        }
    }

    #[test]
    fn resolve_named_finds_the_right_capability_instance_among_same_named_entries() {
        // Mirrors `ulx-cli`'s `build_registry`: one `ulexite.toml` entry
        // ("groq") declaring both `chat` and `transcribe` registers two
        // `Provider` instances under the identical name "groq", each
        // supporting only its own capability. `resolve_named` must not
        // stop at the first "groq" match — that used to be the "chat"
        // instance (registration order here mirrors the real BTreeMap
        // iteration order, `chat` before `transcribe`), silently reporting
        // "groq doesn't support transcribe" even though a same-named
        // instance genuinely does.
        let mut registry = ProviderRegistry::new();
        registry.register("groq", Box::new(SingleCapabilityProvider("chat")));
        registry.register("groq", Box::new(SingleCapabilityProvider("transcribe")));

        assert!(registry.resolve_named("chat", "groq").is_ok());
        assert!(registry.resolve_named("transcribe", "groq").is_ok());
        let err = registry.resolve_named("speak", "groq").err().unwrap();
        assert_eq!(
            err,
            ResolveError::ProviderDoesNotSupportCapability {
                name: "groq".to_string(),
                capability: "speak".to_string(),
            }
        );
    }
}
