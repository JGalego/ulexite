//! Minimal local-file/URL resolution for binary inputs to real vendor
//! calls (`vision`'s image, `transcribe`'s audio file), plus `write_artifact`
//! for binary outputs (`speak`'s audio, `generate_image`'s image), which
//! writes into the real content-addressed `crate::cache::ArtifactStore`
//! (§11.2/§12.7). There is no artifact-*loading* mechanism anywhere else in
//! the runtime today — a CLI `--arg doc=photo.jpg` becomes a bare
//! `Value::Text("photo.jpg")` (`crates/ulx-cli/src/main.rs`) that nothing
//! reads or decodes — so the input side stays deliberately narrow rather
//! than a general artifact/blob system: image formats only (jpg/png/gif/
//! webp), read directly off disk at the HTTP-call boundary, with no new
//! `Value` variant and no change to the `Provider` trait's signature.
//! PDF/video passthrough remains future work — see
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
    fn missing_file_is_a_clear_error() {
        let err = resolve_image("/does/not/exist.png").unwrap_err();
        assert!(matches!(err, ProviderError::Failed(_)));
    }
}
