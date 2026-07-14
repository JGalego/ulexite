//! The provider abstraction (§12.4): a `Provider` is a plugin satisfying
//! one or more capabilities; the runtime never names a vendor, only a
//! capability. `MockProvider` is the one shipped implementation for v0 —
//! deterministic and network-free, so the whole test suite (and anyone
//! trying the language without an API key) gets real, reproducible
//! behavior. A real HTTP-backed provider is real, sizable work (auth,
//! retries, per-vendor request/response shapes) that's explicitly out of
//! scope here — see `docs/spec/24-limitations.md`.

use std::collections::BTreeMap;

use crate::value::{Value, Verdict};

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
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProviderError::UnsupportedCapability(c) => {
                write!(f, "provider does not support capability `{c}`")
            }
            ProviderError::Failed(msg) => write!(f, "provider call failed: {msg}"),
        }
    }
}

pub trait Provider: Send + Sync {
    fn id(&self) -> &str;
    fn supports(&self, capability: &str) -> bool;
    fn invoke(&self, capability: &str, request: &Invocation) -> Result<Value, ProviderError>;
}

/// A deterministic, offline stand-in for a real model/tool provider.
/// Behavior is intentionally simple and documented per-capability so tests
/// (and users kicking the tyres without an API key) get reproducible
/// results, not a simulation of "real" model quality.
pub struct MockProvider;

impl MockProvider {
    pub fn new() -> Self {
        MockProvider
    }
}

impl Default for MockProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for MockProvider {
    fn id(&self) -> &str {
        "mock"
    }

    fn supports(&self, capability: &str) -> bool {
        matches!(
            capability,
            "chat" | "vision" | "embed" | "transcribe" | "speak" | "generate_image" | "judge"
        )
    }

    fn invoke(&self, capability: &str, request: &Invocation) -> Result<Value, ProviderError> {
        match capability {
            "chat" | "vision" => {
                let combined: String = request
                    .messages
                    .iter()
                    .map(|m| format!("{}: {}", m.role, m.text))
                    .collect::<Vec<_>>()
                    .join(" | ");
                Ok(Value::Text(format!(
                    "[mock:{}] response to -> {}",
                    capability,
                    truncate(&combined, 200)
                )))
            }
            "transcribe" => Ok(Value::Text("[mock transcription]".to_string())),
            "speak" => Ok(Value::Text("[mock audio artifact]".to_string())),
            "generate_image" => Ok(Value::Text("[mock image artifact]".to_string())),
            "embed" => {
                let text = request
                    .messages
                    .first()
                    .map(|m| m.text.as_str())
                    .unwrap_or("");
                Ok(Value::List(deterministic_embedding(text, 8)))
            }
            "judge" => {
                let subject = request
                    .args
                    .get("subject")
                    .and_then(Value::as_text)
                    .unwrap_or("");
                Ok(Value::Verdict(mock_judge(subject)))
            }
            other => Err(ProviderError::UnsupportedCapability(other.to_string())),
        }
    }
}

/// Deterministic mock judging (documented, not a real quality signal):
/// empty subjects fail, subjects containing the literal marker
/// `MOCK_JUDGE_FAIL` fail with that reason, subjects containing
/// `MOCK_JUDGE_ESCALATE` escalate, everything else passes.
fn mock_judge(subject: &str) -> Verdict {
    if subject.is_empty() {
        Verdict::Fail("subject is empty".to_string())
    } else if subject.contains("MOCK_JUDGE_FAIL") {
        Verdict::Fail("subject contained the MOCK_JUDGE_FAIL marker".to_string())
    } else if subject.contains("MOCK_JUDGE_ESCALATE") {
        Verdict::Escalate
    } else {
        Verdict::Pass
    }
}

fn deterministic_embedding(text: &str, dims: usize) -> Vec<Value> {
    let hash = crate::value::hash_bytes(text.as_bytes());
    let bytes = hash.as_bytes();
    (0..dims)
        .map(|i| {
            let b = bytes[i % bytes.len()] as f64;
            Value::Float((b / 255.0) * 2.0 - 1.0)
        })
        .collect()
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max])
    }
}

/// Resolves an unqualified `ask <capability>(...)` to a concrete provider
/// (§12.4, §5.5) — v0.1's "policy" is simply "first registered provider
/// that supports it," since `MockProvider` is the only one shipped.
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
