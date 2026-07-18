//! A small, real slice of §15's standard library — enough to make the
//! RAG/PDF-QA examples' non-`ask` calls (`pdf.extract_text`,
//! `vector.nearest`, `embedding.of`, ...) do something genuine rather than
//! bailing out with "not implemented" on every non-trivial program. Most of
//! §15's modules (http, python/javascript/shell FFI, dataset writers, ...)
//! are honestly not implemented yet — see `docs/spec/24-limitations.md`.
//!
//! `pdf.extract_text` is real text extraction (via the pure-Rust
//! `pdf-extract` crate — no system library, so it cross-compiles for every
//! `release.yml` target the same as everything else here) — a local file
//! path or a `data:application/pdf;base64,...` URI both work; an
//! `http(s)://` reference is a clear `NotImplemented` rather than a silent
//! network fetch this module doesn't otherwise make. `pdf.to_images` is
//! honestly NOT real: rasterizing a PDF page to a bitmap needs an actual
//! rendering engine (pdfium/poppler/mupdf), none of which are pure Rust —
//! every option needs a real, several-hundred-KB platform-specific binary
//! bundled per `release.yml` target, which is a packaging decision bigger
//! than swapping in a crate. Left as a clear, named `NotImplemented` error
//! instead of a fake "[mock rasterized pdf page]" string.

use crate::error::RuntimeError;
use crate::provider::{Invocation, Message};
use crate::value::Value;
use crate::RunContext;

/// Reads a `pdf`-typed reference's bytes: a local file path, or a
/// `data:application/pdf;base64,...` URI decoded in place — same two forms
/// `crate::provider::artifact::resolve_document` accepts for a real vendor
/// call, so `pdf.extract_text` understands exactly what a `doc: pdf`
/// parameter can actually contain. An `http(s)://` reference is refused
/// with a clear message rather than fetched — this module makes no network
/// calls of its own.
#[cfg(feature = "real-providers")]
fn read_pdf_bytes(reference: &str) -> Result<Vec<u8>, RuntimeError> {
    if reference.starts_with("http://") || reference.starts_with("https://") {
        return Err(RuntimeError::NotImplemented(format!(
            "pdf.extract_text: fetching a remote PDF (`{reference}`) isn't supported — \
             download it locally first, or pass a data: URI"
        )));
    }
    if let Some(rest) = reference.strip_prefix("data:") {
        let (_mime, payload) = rest.split_once(";base64,").ok_or_else(|| {
            RuntimeError::TypeError(format!(
                "pdf.extract_text: `{reference}` isn't a `;base64,`-encoded data: URI"
            ))
        })?;
        return base64::Engine::decode(&base64::engine::general_purpose::STANDARD, payload)
            .map_err(|e| RuntimeError::TypeError(format!("pdf.extract_text: bad base64: {e}")));
    }
    std::fs::read(reference)
        .map_err(|e| RuntimeError::Io(format!("could not read PDF file `{reference}`: {e}")))
}

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
        #[cfg(feature = "real-providers")]
        ("pdf", "extract_text") => {
            let reference = get(args, "doc", 0)
                .and_then(Value::as_text)
                .ok_or_else(|| {
                    RuntimeError::TypeError(
                        "pdf.extract_text requires a `doc` argument".to_string(),
                    )
                })?;
            let bytes = read_pdf_bytes(reference)?;
            let text = pdf_extract::extract_text_from_mem(&bytes).map_err(|e| {
                RuntimeError::TypeError(format!(
                    "pdf.extract_text: could not extract text from `{reference}`: {e}"
                ))
            })?;
            Ok(Some(Value::Text(text)))
        }
        #[cfg(not(feature = "real-providers"))]
        ("pdf", "extract_text") => Err(RuntimeError::NotImplemented(
            "pdf.extract_text: not available in this build (no local filesystem/`pdf-extract` — \
             e.g. the in-browser playground)"
                .to_string(),
        )),
        ("pdf", "to_images") => Err(RuntimeError::NotImplemented(
            "pdf.to_images: rasterizing a PDF page to a bitmap needs a real rendering engine \
             (pdfium/poppler/mupdf) — none are pure Rust, so this isn't implemented rather than \
             faked; pdf.extract_text is real and covers PDFs that have a text layer"
                .to_string(),
        )),
        ("embedding", "of") => {
            let text = get(args, "text", 0)
                .and_then(Value::as_text)
                .unwrap_or("")
                .to_string();
            let provider = match get(args, "provider", usize::MAX).and_then(Value::as_text) {
                Some(name) => ctx
                    .providers
                    .resolve_named("embed", name)
                    .map_err(RuntimeError::ProviderResolution)?,
                None => ctx
                    .providers
                    .resolve("embed")
                    .map_err(RuntimeError::ProviderResolution)?,
            };
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

#[cfg(all(test, feature = "real-providers"))]
mod tests {
    use super::*;

    fn sample_pdf_path() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/fixtures/sample.pdf")
    }

    #[test]
    fn read_pdf_bytes_reads_a_real_local_file() {
        let path = sample_pdf_path();
        let bytes = read_pdf_bytes(path.to_str().unwrap()).expect("should read the fixture");
        assert!(
            bytes.starts_with(b"%PDF-"),
            "not a real PDF: {:?}",
            &bytes[..8.min(bytes.len())]
        );
    }

    #[test]
    fn read_pdf_bytes_decodes_a_data_uri() {
        let raw = std::fs::read(sample_pdf_path()).unwrap();
        let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &raw);
        let uri = format!("data:application/pdf;base64,{b64}");
        let decoded = read_pdf_bytes(&uri).expect("should decode the data URI");
        assert_eq!(decoded, raw);
    }

    #[test]
    fn read_pdf_bytes_refuses_a_remote_url() {
        let err = read_pdf_bytes("https://example.com/doc.pdf").unwrap_err();
        assert!(matches!(err, RuntimeError::NotImplemented(_)));
    }

    #[test]
    fn read_pdf_bytes_rejects_malformed_data_uri() {
        let err = read_pdf_bytes("data:application/pdf,not-base64-shaped").unwrap_err();
        assert!(matches!(err, RuntimeError::TypeError(_)));
    }

    #[test]
    fn extract_text_from_the_real_fixture_finds_real_story_text() {
        let bytes = std::fs::read(sample_pdf_path()).unwrap();
        let text = pdf_extract::extract_text_from_mem(&bytes).expect("should extract real text");
        assert!(
            text.contains("Little Red Riding Hood"),
            "expected real extracted text, got: {text}"
        );
    }
}
