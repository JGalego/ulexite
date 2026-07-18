use crate::provider::{Message, ProviderError, ResolveError};

#[derive(Debug, Clone)]
pub enum RuntimeError {
    UndefinedName(String),
    /// Resolving a capability (optionally by name) against the provider
    /// registry failed — no provider at all, an ambiguous multi-match with
    /// no `provider:`/`--provider` disambiguation, or a named provider that
    /// doesn't exist or doesn't serve this capability.
    ProviderResolution(ResolveError),
    UnknownJudgeOrValidator(String),
    UnknownConversation(String),
    UnknownDataset(String),
    UnknownBenchmark(String),
    Provider(ProviderError),
    /// A conversation reached `escalate(...)` with no recorded human
    /// decision (§7.3, §10.7) — the caller should report `cache_key` to the
    /// user so `ulx approve`/`ulx deny` can record a decision and resume
    /// (see `interp.rs`'s module docs for how resume works in v0.1).
    Suspended {
        cache_key: String,
        reason: String,
        target: String,
        /// The chat-ready `{role, text}` messages an out-of-band resolver
        /// (a human, for `escalate`; an in-browser model, for a suspended
        /// `ask`/`judge` call) needs to actually answer this suspend point —
        /// empty for `escalate`'s own single `{target, reason}` message,
        /// which is carried in `reason`/`target` instead.
        messages: Vec<Message>,
    },
    RetriesExhausted,
    TypeError(String),
    NotImplemented(String),
    Io(String),
    ReplayMiss(String),
    /// A `with`-block branch (`eval_parallel`) panicked on its own worker
    /// thread — surfaced as an ordinary error to the caller instead of
    /// tearing down the whole process via `.join().unwrap()`/`.expect()`,
    /// per this crate's own claim that a failure "surfaces as an unsettled
    /// `Draft<T>`, not a crash" (README's "Configuring providers").
    Panicked(String),
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RuntimeError::UndefinedName(n) => write!(f, "undefined name `{n}`"),
            RuntimeError::ProviderResolution(e) => write!(f, "{e}"),
            RuntimeError::UnknownJudgeOrValidator(n) => {
                write!(f, "no `judge`/`validator` named `{n}`")
            }
            RuntimeError::UnknownConversation(n) => write!(f, "no `conversation` named `{n}`"),
            RuntimeError::UnknownDataset(n) => write!(f, "no `dataset` named `{n}`"),
            RuntimeError::UnknownBenchmark(n) => write!(f, "no `benchmark` named `{n}`"),
            RuntimeError::Provider(e) => write!(f, "{e}"),
            RuntimeError::Suspended { reason, target, .. } => {
                write!(f, "suspended waiting on `{target}`: {reason}")
            }
            RuntimeError::RetriesExhausted => {
                write!(f, "retries exhausted with no `else` fallback")
            }
            RuntimeError::TypeError(msg) => write!(f, "type error: {msg}"),
            RuntimeError::NotImplemented(msg) => write!(f, "not implemented: {msg}"),
            RuntimeError::Io(msg) => write!(f, "I/O error: {msg}"),
            RuntimeError::ReplayMiss(msg) => write!(f, "replay error: {msg}"),
            RuntimeError::Panicked(msg) => write!(f, "internal error (with-block panic): {msg}"),
        }
    }
}

impl std::error::Error for RuntimeError {}
