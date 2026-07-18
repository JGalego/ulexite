//! Stand-in `Provider` for the in-browser driver (`ulx-wasm`): registered
//! purely so `ProviderRegistry::resolve`/`resolve_named` have a candidate
//! for `chat`/`judge` and so `id()`/`model()` metadata flows into cache
//! keys and trace records, exactly like `MockProvider` does for the
//! parser-only story. `invoke()` itself is structurally unreachable —
//! `RunContext::suspend_on_provider_miss` makes `invoke_cached` (`interp.rs`)
//! suspend on every cache miss before it would ever call this closure, since
//! this provider can never answer synchronously: the actual answer comes
//! from an async, in-browser model call driven entirely from JS.

use crate::value::Value;

use super::{Invocation, Provider, ProviderError};

pub struct BrowserLocalProvider {
    name: String,
}

impl BrowserLocalProvider {
    pub fn new(name: impl Into<String>) -> Self {
        BrowserLocalProvider { name: name.into() }
    }
}

impl Provider for BrowserLocalProvider {
    fn id(&self) -> &str {
        &self.name
    }

    fn supports(&self, capability: &str) -> bool {
        matches!(capability, "chat" | "judge")
    }

    fn invoke(&self, _capability: &str, _request: &Invocation) -> Result<Value, ProviderError> {
        Err(ProviderError::Failed(
            "BrowserLocalProvider.invoke should never be called directly — \
             RunContext::suspend_on_provider_miss should have suspended before this point"
                .to_string(),
        ))
    }
}
