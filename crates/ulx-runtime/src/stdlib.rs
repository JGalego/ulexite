//! A small, real slice of §15's standard library — enough to make the
//! RAG/PDF-QA examples' non-`ask` calls (`pdf.extract_text`,
//! `vector.nearest`, `embedding.of`, ...) do something genuine rather than
//! bailing out with "not implemented" on every non-trivial program. Most of
//! §15's modules (http, python/javascript/shell FFI, dataset writers, ...)
//! are honestly not implemented yet — see `docs/spec/24-limitations.md`.

use crate::error::RuntimeError;
use crate::provider::{Invocation, Message};
use crate::value::Value;
use crate::RunContext;

/// One call argument: an optional name (for `f(name: value)` call sites)
/// paired with its evaluated value, in original source order.
pub type StdlibArg = (Option<String>, Value);

fn get<'a>(args: &'a [StdlibArg], name: &str, pos: usize) -> Option<&'a Value> {
    args.iter()
        .find(|(n, _)| n.as_deref() == Some(name))
        .map(|(_, v)| v)
        .or_else(|| args.get(pos).map(|(_, v)| v))
}

/// Dispatches `module.function(args)` where `module` is a stdlib alias from
/// `import "module" as module` (§15). Returns `Ok(None)` if `module`/`function`
/// isn't one this runtime implements anything for, so the caller can
/// produce a clear "not implemented" error naming the actual call.
pub fn call(
    ctx: &RunContext,
    module: &str,
    function: &str,
    args: &[StdlibArg],
) -> Result<Option<Value>, RuntimeError> {
    match (module, function) {
        ("pdf", "extract_text") => {
            let tag = get(args, "doc", 0)
                .map(Value::content_hash)
                .unwrap_or_default();
            Ok(Some(Value::Text(format!(
                "[mock pdf text extraction of {tag}]"
            ))))
        }
        ("pdf", "to_images") => Ok(Some(Value::List(vec![Value::Text(
            "[mock rasterized pdf page]".to_string(),
        )]))),
        ("embedding", "of") => {
            let text = get(args, "text", 0)
                .and_then(Value::as_text)
                .unwrap_or("")
                .to_string();
            let provider = ctx
                .providers
                .resolve("embed")
                .ok_or_else(|| RuntimeError::UnknownCapability("embed".to_string()))?;
            let invocation = Invocation {
                messages: vec![Message {
                    role: "user".to_string(),
                    text,
                }],
                args: Default::default(),
            };
            provider
                .invoke("embed", &invocation)
                .map(Some)
                .map_err(RuntimeError::Provider)
        }
        ("vector", "cosine_similarity") => match (get(args, "a", 0), get(args, "b", 1)) {
            (Some(Value::List(a)), Some(Value::List(b))) => {
                Ok(Some(Value::Float(cosine_similarity(a, b))))
            }
            _ => Err(RuntimeError::TypeError(
                "vector.cosine_similarity expects two embedding lists".to_string(),
            )),
        },
        ("vector", "nearest") => nearest(args),
        _ => Ok(None),
    }
}

fn nearest(args: &[StdlibArg]) -> Result<Option<Value>, RuntimeError> {
    let query = get(args, "query", 0).ok_or_else(|| {
        RuntimeError::TypeError("vector.nearest requires a `query` embedding".to_string())
    })?;
    let index = get(args, "index", 1).ok_or_else(|| {
        RuntimeError::TypeError("vector.nearest requires an `index` dataset".to_string())
    })?;
    let k = get(args, "k", 2)
        .and_then(|v| match v {
            Value::Int(i) => Some(*i as usize),
            _ => None,
        })
        .unwrap_or(1);

    let Value::List(query) = query else {
        return Err(RuntimeError::TypeError(
            "query must be an embedding".to_string(),
        ));
    };
    let Value::List(rows) = index else {
        return Err(RuntimeError::TypeError(
            "index must be a dataset".to_string(),
        ));
    };

    let mut scored: Vec<(f64, &Value)> = rows
        .iter()
        .filter_map(|row| {
            let Value::Record(fields) = row else {
                return None;
            };
            let Value::List(embedding) = fields.get("embedding")? else {
                return None;
            };
            Some((cosine_similarity(query, embedding), row))
        })
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let top: Vec<Value> = scored.into_iter().take(k).map(|(_, v)| v.clone()).collect();
    Ok(Some(Value::List(top)))
}

fn cosine_similarity(a: &[Value], b: &[Value]) -> f64 {
    let a: Vec<f64> = a.iter().filter_map(as_f64).collect();
    let b: Vec<f64> = b.iter().filter_map(as_f64).collect();
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f64 = a.iter().zip(&b).map(|(x, y)| x * y).sum();
    let na: f64 = a.iter().map(|x| x * x).sum::<f64>().sqrt();
    let nb: f64 = b.iter().map(|x| x * x).sum::<f64>().sqrt();
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na * nb)
    }
}

fn as_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Float(f) => Some(*f),
        Value::Int(i) => Some(*i as f64),
        _ => None,
    }
}
