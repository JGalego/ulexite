//! Runtime values (§11's artifact system, simplified for v0.1): every
//! artifact type in §9.2 collapses to one of these variants rather than a
//! fully-typed lattice enforced at runtime — the static side of that
//! checking already happened in `ulx-sema` (best-effort though it is);
//! this is the dynamic representation the interpreter actually pushes
//! around.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value")]
pub enum Value {
    Text(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    List(Vec<Value>),
    Record(BTreeMap<String, Value>),
    Verdict(Verdict),
    /// The result of a `Draft<T>` that didn't settle (§9.3) — a program
    /// that doesn't `match` on it and instead uses it as a plain value hits
    /// this at runtime, which is the dynamic backstop for the static
    /// exhaustiveness check `ulx-sema` only partially enforces today.
    Unsettled(DraftOutcome),
    Unit,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Verdict {
    Pass,
    Fail(String),
    Score(f64),
    Escalate,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DraftOutcome {
    Refused(String),
    RateLimited,
    Timeout,
}

impl Value {
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Value::Text(s) => Some(s),
            _ => None,
        }
    }

    pub fn truthy(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            Value::Int(i) => *i != 0,
            Value::Float(f) => *f != 0.0,
            Value::Text(s) => !s.is_empty(),
            Value::List(l) => !l.is_empty(),
            Value::Unit => false,
            _ => true,
        }
    }

    /// Deterministic content hash (§11.1, §10.3): identical values hash
    /// identically, which is what makes cache keys and artifact addressing
    /// sound.
    pub fn content_hash(&self) -> String {
        let bytes = serde_json::to_vec(self).expect("Value always serializes");
        hash_bytes(&bytes)
    }
}

pub fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Text(s) => write!(f, "{s}"),
            Value::Int(i) => write!(f, "{i}"),
            Value::Float(x) => write!(f, "{x}"),
            Value::Bool(b) => write!(f, "{b}"),
            Value::List(items) => {
                write!(f, "[")?;
                for (i, v) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{v}")?;
                }
                write!(f, "]")
            }
            Value::Record(fields) => {
                write!(f, "{{")?;
                for (i, (k, v)) in fields.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{k}: {v}")?;
                }
                write!(f, "}}")
            }
            Value::Verdict(v) => write!(f, "{v:?}"),
            Value::Unsettled(d) => write!(f, "<unsettled: {d:?}>"),
            Value::Unit => write!(f, "()"),
        }
    }
}
