use crate::provider::ProviderError;

#[derive(Debug, Clone)]
pub enum RuntimeError {
    UndefinedName(String),
    UnknownCapability(String),
    UnknownJudgeOrValidator(String),
    UnknownConversation(String),
    UnknownDataset(String),
    Provider(ProviderError),
    /// A conversation reached `escalate(...)` with no recorded human
    /// decision (§7.3, §10.7) — the caller should report `cache_key` to the
    /// user so `ulx approve`/`ulx deny` can record a decision and resume
    /// (see `interp.rs`'s module docs for how resume works in v0.1).
    Suspended {
        cache_key: String,
        reason: String,
        target: String,
    },
    RetriesExhausted,
    TypeError(String),
    NotImplemented(String),
    Io(String),
    ReplayMiss(String),
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RuntimeError::UndefinedName(n) => write!(f, "undefined name `{n}`"),
            RuntimeError::UnknownCapability(c) => {
                write!(f, "no provider registered for capability `{c}`")
            }
            RuntimeError::UnknownJudgeOrValidator(n) => {
                write!(f, "no `judge`/`validator` named `{n}`")
            }
            RuntimeError::UnknownConversation(n) => write!(f, "no `conversation` named `{n}`"),
            RuntimeError::UnknownDataset(n) => write!(f, "no `dataset` named `{n}`"),
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
        }
    }
}

impl std::error::Error for RuntimeError {}
