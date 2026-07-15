//! Minimal local-file/URL resolution for binary inputs to real vendor
//! calls (`vision`'s image, `transcribe`'s audio file), plus `write_artifact`
//! for binary outputs (`speak`'s audio, `generate_image`'s image), which
//! writes into the real content-addressed `crate::cache::ArtifactStore`
//! (§11.2/§12.7). There is no artifact-*loading* mechanism anywhere else in
//! the runtime today — a CLI `--arg doc=photo.jpg` becomes a bare
//! `Value::Text("photo.jpg")` (`crates/ulx-cli/src/main.rs`) that nothing
//! reads or decodes — so the input side stays deliberately narrow rather
//! than a general artifact/blob system: image formats (jpg/png/gif/webp)
//! plus, for Anthropic only, PDF (`resolve_document`, consumed by
//! `anthropic.rs`'s `document` content block) — read directly off disk at
//! the HTTP-call boundary, with no new `Value` variant and no change to the
//! `Provider` trait's signature. Every other vendor still rejects a `.pdf`
//! extension via `resolve_image`/`guess_image_mime`. Video passthrough
//! remains future work — see
//! `docs/spec/24-limitations.md`.

use base64::Engine;

use super::ProviderError;

#[derive(Debug)]
pub enum ImageSource {
    /// An `http(s)://` or `data:` URI, passed straight through — most
    /// vendors accept these directly, no local read needed.
    Url(String),
    /// A local file, read and base64-encoded for an inline content block.
    Inline {
        mime: &'static str,
        data_b64: String,
    },
}

pub fn resolve_image(reference: &str) -> Result<ImageSource, ProviderError> {
    if reference.starts_with("http://")
        || reference.starts_with("https://")
        || reference.starts_with("data:")
    {
        return Ok(ImageSource::Url(reference.to_string()));
    }
    let bytes = std::fs::read(reference).map_err(|e| {
        ProviderError::Failed(format!("could not read image file `{reference}`: {e}"))
    })?;
    let mime = guess_image_mime(reference)?;
    Ok(ImageSource::Inline {
        mime,
        data_b64: base64::engine::general_purpose::STANDARD.encode(bytes),
    })
}

/// True if `reference` names a PDF — by extension for a local path or
/// plain URL, or by declared content type for a `data:` URI. Anthropic's
/// `vision` call uses this to decide whether to build a `document`
/// content block instead of an `image` one (see `resolve_document`);
/// every other vendor still routes a `.pdf` reference through
/// `resolve_image`, which rejects it with a vendor-agnostic error.
pub fn is_pdf_reference(reference: &str) -> bool {
    if let Some(rest) = reference.strip_prefix("data:") {
        return rest.starts_with("application/pdf");
    }
    std::path::Path::new(reference)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
}

/// Resolves a PDF reference the same way `resolve_image` resolves an
/// image: an `http(s)://`/`data:` URI passes straight through, a local
/// file is read and base64-encoded with a fixed `application/pdf` media
/// type. Only Anthropic's Messages API `document` content block consumes
/// this today (§24 Limitations) — callers should check `is_pdf_reference`
/// first and only call this for a PDF input.
pub fn resolve_document(reference: &str) -> Result<ImageSource, ProviderError> {
    if reference.starts_with("http://")
        || reference.starts_with("https://")
        || reference.starts_with("data:")
    {
        return Ok(ImageSource::Url(reference.to_string()));
    }
    let bytes = std::fs::read(reference).map_err(|e| {
        ProviderError::Failed(format!("could not read document file `{reference}`: {e}"))
    })?;
    Ok(ImageSource::Inline {
        mime: "application/pdf",
        data_b64: base64::engine::general_purpose::STANDARD.encode(bytes),
    })
}

/// Reads a local audio file for `transcribe`'s multipart upload, returning
/// `(filename, bytes)`. Unlike `resolve_image`, there's no URL-passthrough
/// case — every OpenAI-compatible transcription endpoint requires the raw
/// file bytes in the request body.
pub fn read_audio_file(reference: &str) -> Result<(String, Vec<u8>), ProviderError> {
    let bytes = std::fs::read(reference).map_err(|e| {
        ProviderError::Failed(format!("could not read audio file `{reference}`: {e}"))
    })?;
    let filename = std::path::Path::new(reference)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("audio")
        .to_string();
    Ok((filename, bytes))
}

fn guess_image_mime(path: &str) -> Result<&'static str, ProviderError> {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    match ext.as_deref() {
        Some("jpg") | Some("jpeg") => Ok("image/jpeg"),
        Some("png") => Ok("image/png"),
        Some("gif") => Ok("image/gif"),
        Some("webp") => Ok("image/webp"),
        Some("pdf") => Err(ProviderError::Failed(format!(
            "PDF not supported for this vendor's vision call in `{path}` — only Anthropic supports PDF input today (§24 Limitations); jpg/png/gif/webp work everywhere"
        ))),
        other => Err(ProviderError::Failed(format!(
            "unsupported image extension {other:?} in `{path}` — only jpg/png/gif/webp are supported against real vendors today"
        ))),
    }
}

/// Writes generated binary output (`speak`'s audio, `generate_image`'s
/// image) into the given `ArtifactStore` — §11.2's real, project-local,
/// content-addressed artifact store (`crate::cache::ArtifactStore`), not
/// the OS temp directory — and returns the resulting path as a
/// `Value::Text`. Same bytes in, same path out, and idempotent: a repeat
/// call with identical bytes is a no-op write (`ArtifactStore::put`).
pub fn write_artifact(
    store: &crate::cache::ArtifactStore,
    bytes: &[u8],
    extension: &str,
) -> Result<crate::value::Value, ProviderError> {
    let hash = crate::value::hash_bytes(bytes);
    let path = store
        .put(&hash[..16], extension, bytes)
        .map_err(|e| ProviderError::Failed(format!("could not write artifact: {e}")))?;
    Ok(crate::value::Value::Text(
        path.to_string_lossy().to_string(),
    ))
}

/// The first `Invocation.args` entry that plausibly names an artifact
/// input: the positional `ask vision(doc)`/`ask transcribe(audio)` form
/// keys to `"_"` (§9.2's grammar, `crates/ulx-runtime/src/interp.rs`); a
/// handful of common names cover the named form too.
pub fn first_artifact_arg(
    args: &std::collections::BTreeMap<String, crate::value::Value>,
) -> Option<&str> {
    ["_", "file", "path", "image", "audio", "document"]
        .iter()
        .find_map(|key| args.get(*key).and_then(crate::value::Value::as_text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_reference_passes_through() {
        match resolve_image("https://example.com/cat.png").unwrap() {
            ImageSource::Url(u) => assert_eq!(u, "https://example.com/cat.png"),
            ImageSource::Inline { .. } => panic!("expected a URL passthrough"),
        }
    }

    #[test]
    fn local_file_is_read_and_base64_encoded() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("ulexite-test-{}.png", std::process::id()));
        std::fs::write(&path, [0x89, 0x50, 0x4e, 0x47]).unwrap();
        let result = resolve_image(path.to_str().unwrap()).unwrap();
        std::fs::remove_file(&path).ok();
        match result {
            ImageSource::Inline { mime, data_b64 } => {
                assert_eq!(mime, "image/png");
                assert_eq!(
                    data_b64,
                    base64::engine::general_purpose::STANDARD.encode([0x89, 0x50, 0x4e, 0x47])
                );
            }
            ImageSource::Url(_) => panic!("expected an inline-encoded file"),
        }
    }

    #[test]
    fn unsupported_extension_is_rejected() {
        let err = resolve_image("document.pdf").unwrap_err();
        assert!(matches!(err, ProviderError::Failed(_)));
    }

    #[test]
    fn pdf_extension_is_rejected_by_resolve_image_with_a_clear_message() {
        let dir = std::env::temp_dir();
        // A name distinct from `resolve_document_reads_and_base64_encodes_a_local_pdf`'s
        // below -- both used the same `ulexite-test-{pid}.pdf` path, and
        // since `cargo test` runs test functions concurrently on separate
        // threads within one process (same pid), the two raced on the
        // same file (one's `remove_file` could fire mid-read of the
        // other's `write`), flaking under `--workspace`/full-suite runs.
        let path = dir.join(format!(
            "ulexite-test-resolve-image-rejects-pdf-{}.pdf",
            std::process::id()
        ));
        std::fs::write(&path, b"%PDF-1.4").unwrap();
        let err = resolve_image(path.to_str().unwrap()).unwrap_err();
        std::fs::remove_file(&path).ok();
        if let ProviderError::Failed(msg) = &err {
            assert!(msg.contains("PDF not supported"), "message was: {msg}");
            assert!(msg.contains("Anthropic"), "message was: {msg}");
        } else {
            panic!("expected ProviderError::Failed, got {err:?}");
        }
    }

    #[test]
    fn is_pdf_reference_detects_extension_and_data_uri() {
        assert!(is_pdf_reference("doc.pdf"));
        assert!(is_pdf_reference("DOC.PDF"));
        assert!(is_pdf_reference("data:application/pdf;base64,AAAA"));
        assert!(!is_pdf_reference("photo.png"));
        assert!(!is_pdf_reference("data:image/png;base64,AAAA"));
        // A `.pdf`-suffixed URL is detected too — Anthropic's vision call
        // routes it to a `document` block with a URL source, same as an
        // `.png` URL routes to an `image` block with a URL source.
        assert!(is_pdf_reference("https://example.com/report.pdf"));
        assert!(!is_pdf_reference("https://example.com/cat.png"));
    }

    #[test]
    fn resolve_document_reads_and_base64_encodes_a_local_pdf() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!(
            "ulexite-test-resolve-document-pdf-{}.pdf",
            std::process::id()
        ));
        std::fs::write(&path, b"%PDF-1.4 fake").unwrap();
        let result = resolve_document(path.to_str().unwrap()).unwrap();
        std::fs::remove_file(&path).ok();
        match result {
            ImageSource::Inline { mime, data_b64 } => {
                assert_eq!(mime, "application/pdf");
                assert_eq!(
                    data_b64,
                    base64::engine::general_purpose::STANDARD.encode(b"%PDF-1.4 fake")
                );
            }
            ImageSource::Url(_) => panic!("expected an inline-encoded file"),
        }
    }

    #[test]
    fn resolve_document_passes_through_a_url() {
        match resolve_document("https://example.com/report.pdf").unwrap() {
            ImageSource::Url(u) => assert_eq!(u, "https://example.com/report.pdf"),
            ImageSource::Inline { .. } => panic!("expected a URL passthrough"),
        }
    }

    #[test]
    fn missing_file_is_a_clear_error() {
        let err = resolve_image("/does/not/exist.png").unwrap_err();
        assert!(matches!(err, ProviderError::Failed(_)));
    }
}
