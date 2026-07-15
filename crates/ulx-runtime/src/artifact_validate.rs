//! Content validation for CLI-supplied artifact arguments (§9.2's
//! `pdf`/`image`/`audio`/`video`): a declared parameter type like `doc:
//! pdf` used to accept literally any string (§24.10's "no artifact/blob
//! system" — a `--arg doc=photo.png` became a bare `Value::Text` nothing
//! ever inspected). This module sniffs the actual bytes at the given path
//! against the declared type's known magic-byte signatures, so a
//! `pdf`-typed argument is rejected up front if it isn't actually a PDF,
//! instead of silently running and failing (or worse, silently
//! "succeeding") much later at the provider boundary — the same signatures
//! `provider::artifact` already special-cases for PDF are reused here, not
//! duplicated with different rules.
//!
//! `http(s)://` and `data:` URIs are passed through unchecked, same as
//! `provider::artifact::resolve_image`/`resolve_document` — there's no
//! local file to sniff, and fetching one just to validate would be a real
//! network call this path deliberately doesn't make.

use ulx_ast::ArtifactType;

/// The artifact types this module actually validates — every other
/// `ArtifactType` (free-form text, or one with no fixed binary signature
/// like `json`/`csv`) is intentionally left unchecked, same as today.
fn expected_signatures(kind: ArtifactType) -> Option<&'static [&'static str]> {
    match kind {
        ArtifactType::Pdf => Some(&["pdf"]),
        ArtifactType::Image => Some(&["png", "jpeg", "gif", "webp"]),
        ArtifactType::Audio => Some(&["wav", "mp3", "ogg", "flac"]),
        ArtifactType::Video => Some(&["mp4", "webm", "avi"]),
        _ => None,
    }
}

/// Sniffs a well-known magic-byte signature out of a file's leading bytes.
/// Returns `None` for anything unrecognized.
fn sniff(bytes: &[u8]) -> Option<&'static str> {
    let riff_form =
        |form: &[u8; 4]| bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == form;

    if bytes.starts_with(b"%PDF-") {
        Some("pdf")
    } else if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some("png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("gif")
    } else if riff_form(b"WEBP") {
        Some("webp")
    } else if riff_form(b"WAVE") {
        Some("wav")
    } else if riff_form(b"AVI ") {
        Some("avi")
    } else if bytes.starts_with(b"ID3")
        || (bytes.len() >= 2 && bytes[0] == 0xFF && (bytes[1] & 0xE0) == 0xE0)
    {
        Some("mp3")
    } else if bytes.starts_with(b"OggS") {
        Some("ogg")
    } else if bytes.starts_with(b"fLaC") {
        Some("flac")
    } else if bytes.len() >= 8 && &bytes[4..8] == b"ftyp" {
        Some("mp4")
    } else if bytes.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        Some("webm")
    } else {
        None
    }
}

fn data_uri_family(mime: &str) -> Option<&'static str> {
    if mime.starts_with("application/pdf") {
        Some("pdf")
    } else if mime.starts_with("image/") {
        Some("image")
    } else if mime.starts_with("audio/") {
        Some("audio")
    } else if mime.starts_with("video/") {
        Some("video")
    } else {
        None
    }
}

/// Validates that `value` — the raw string a `--arg name=value` supplied
/// for a parameter declared with artifact type `kind` — actually looks
/// like that type of content. `Ok(())` covers both "genuinely matches" and
/// "not a type this module checks" (free-form text types, remote
/// references). Every error message names the path, what was declared,
/// and what was actually found/expected, so a caller can fix the argument
/// without needing to read this module's source.
pub fn validate_artifact_arg(kind: ArtifactType, value: &str) -> Result<(), String> {
    let Some(expected) = expected_signatures(kind) else {
        return Ok(());
    };

    if let Some(mime) = value.strip_prefix("data:") {
        return match data_uri_family(mime) {
            Some(family) if kind_matches_family(kind, family) => Ok(()),
            _ => Err(format!(
                "`{value}` is a data: URI whose declared content type doesn't look like `{}` — check the MIME type before the base64 payload",
                kind.keyword()
            )),
        };
    }
    if value.starts_with("http://") || value.starts_with("https://") {
        // No local bytes to sniff, and fetching one here would be a real
        // network call this validation path doesn't make; the vendor
        // adapter still enforces its own rules when it actually fetches.
        return Ok(());
    }

    let bytes = std::fs::read(value).map_err(|e| {
        format!(
            "could not read `{value}` (declared `{}`): {e}",
            kind.keyword()
        )
    })?;
    let head = &bytes[..bytes.len().min(64)];
    match sniff(head) {
        Some(found) if expected.contains(&found) => Ok(()),
        Some(found) => Err(format!(
            "`{value}` looks like {found} content, but the parameter is declared `{}` — expected one of: {}",
            kind.keyword(),
            expected.join(", ")
        )),
        None => Err(format!(
            "`{value}` doesn't look like a recognized `{}` file (checked magic bytes) — expected one of: {}",
            kind.keyword(),
            expected.join(", ")
        )),
    }
}

fn kind_matches_family(kind: ArtifactType, family: &str) -> bool {
    match kind {
        ArtifactType::Pdf => family == "pdf",
        ArtifactType::Image => family == "image",
        ArtifactType::Audio => family == "audio",
        ArtifactType::Video => family == "video",
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_temp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir();
        let path = dir.join(format!(
            "ulexite-artifact-validate-test-{}-{name}",
            std::process::id()
        ));
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn real_pdf_passes_for_pdf_param() {
        let path = write_temp("real.pdf", b"%PDF-1.4\n...");
        let result = validate_artifact_arg(ArtifactType::Pdf, path.to_str().unwrap());
        std::fs::remove_file(&path).ok();
        assert!(result.is_ok(), "{result:?}");
    }

    #[test]
    fn png_rejected_for_pdf_param() {
        let path = write_temp(
            "fake.png",
            &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A],
        );
        let result = validate_artifact_arg(ArtifactType::Pdf, path.to_str().unwrap());
        std::fs::remove_file(&path).ok();
        let err = result.unwrap_err();
        assert!(err.contains("png"), "{err}");
        assert!(err.contains("pdf"), "{err}");
    }

    #[test]
    fn png_passes_for_image_param() {
        let path = write_temp(
            "real.png",
            &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A],
        );
        let result = validate_artifact_arg(ArtifactType::Image, path.to_str().unwrap());
        std::fs::remove_file(&path).ok();
        assert!(result.is_ok(), "{result:?}");
    }

    #[test]
    fn wav_passes_for_audio_param() {
        let mut bytes = b"RIFF".to_vec();
        bytes.extend_from_slice(&[0, 0, 0, 0]);
        bytes.extend_from_slice(b"WAVE");
        let path = write_temp("real.wav", &bytes);
        let result = validate_artifact_arg(ArtifactType::Audio, path.to_str().unwrap());
        std::fs::remove_file(&path).ok();
        assert!(result.is_ok(), "{result:?}");
    }

    #[test]
    fn unrecognized_bytes_are_rejected() {
        let path = write_temp("garbage.bin", b"not a real file format at all");
        let result = validate_artifact_arg(ArtifactType::Pdf, path.to_str().unwrap());
        std::fs::remove_file(&path).ok();
        assert!(result.is_err());
    }

    #[test]
    fn missing_file_is_a_clear_error() {
        let result = validate_artifact_arg(ArtifactType::Pdf, "/does/not/exist.pdf");
        let err = result.unwrap_err();
        assert!(err.contains("could not read"), "{err}");
    }

    #[test]
    fn urls_and_data_uris_pass_through_or_check_mime_only() {
        assert!(validate_artifact_arg(ArtifactType::Pdf, "https://example.com/a.pdf").is_ok());
        assert!(
            validate_artifact_arg(ArtifactType::Image, "data:image/png;base64,iVBORw0KGgo=")
                .is_ok()
        );
        assert!(
            validate_artifact_arg(ArtifactType::Pdf, "data:image/png;base64,iVBORw0KGgo=").is_err()
        );
    }

    #[test]
    fn non_artifact_types_are_never_checked() {
        assert!(validate_artifact_arg(ArtifactType::Text, "anything at all").is_ok());
        assert!(validate_artifact_arg(ArtifactType::Json, "{not even valid json").is_ok());
    }
}
