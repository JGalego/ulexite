//! Anthropic's Messages API (`/v1/messages`) — a distinct shape from the
//! OpenAI family: auth via `x-api-key` + `anthropic-version`, a top-level
//! `system` string pulled out of the message list rather than a `system`
//! role inside it, and a mandatory `max_tokens`.

use std::collections::BTreeMap;

use serde_json::json;

use crate::value::Value;

use super::artifact::{self, ImageSource};
use super::transport::{send_json_with_retry, Transport};
use super::{resolve_f64, resolve_i64, resolve_model, Invocation, Provider, ProviderError};

const ANTHROPIC_VERSION: &str = "2023-06-01";
const DEFAULT_MAX_TOKENS: i64 = 1024;

/// Which Anthropic content-block shape to build for a `vision` call's
/// resolved artifact: `image` (native multimodal input since Claude 3) or
/// `document` (PDF support — Anthropic's `type: "document"` block,
/// `{"type": "base64"/"url", "media_type": "application/pdf", ...}`). The
/// underlying `ImageSource` (URL passthrough vs. local-file base64) is the
/// same shape either way; only the wrapping content-block `type` differs.
enum VisionInput {
    Image(ImageSource),
    Document(ImageSource),
}

pub struct AnthropicProvider {
    capability: String,
    base_url: String,
    api_key: String,
    model: String,
    default_params: BTreeMap<String, Value>,
    transport: Box<dyn Transport>,
}

impl AnthropicProvider {
    pub fn with_transport(
        capability: impl Into<String>,
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
        default_params: BTreeMap<String, Value>,
        transport: Box<dyn Transport>,
    ) -> Self {
        AnthropicProvider {
            capability: capability.into(),
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key: api_key.into(),
            model: model.into(),
            default_params,
            transport,
        }
    }

    fn headers(&self) -> Vec<(String, String)> {
        vec![
            ("Content-Type".to_string(), "application/json".to_string()),
            ("x-api-key".to_string(), self.api_key.clone()),
            (
                "anthropic-version".to_string(),
                ANTHROPIC_VERSION.to_string(),
            ),
        ]
    }

    fn chat(&self, request: &Invocation) -> Result<Value, ProviderError> {
        self.chat_or_vision(request, None)
    }

    /// Routes to a `document` content block for a `.pdf` reference, an
    /// `image` block otherwise — Anthropic is the only vendor adapter
    /// today that accepts PDF input for `vision` (§24 Limitations); every
    /// other vendor still rejects `.pdf` via `artifact::resolve_image`.
    fn vision(&self, request: &Invocation) -> Result<Value, ProviderError> {
        let image_ref = artifact::first_artifact_arg(&request.args).ok_or_else(|| {
            ProviderError::Failed(
                "vision call has no image argument (expected e.g. `ask vision(doc)`)".to_string(),
            )
        })?;
        let input = if artifact::is_pdf_reference(image_ref) {
            VisionInput::Document(artifact::resolve_document(image_ref)?)
        } else {
            VisionInput::Image(artifact::resolve_image(image_ref)?)
        };
        self.chat_or_vision(request, Some(input))
    }

    /// Anthropic's Messages API has one endpoint for both text-only and
    /// multimodal chat — an image (or, for a PDF, a document) is just
    /// another content block on the last message, native support since
    /// Claude 3 (no separate `vision` endpoint the way
    /// `transcribe`/`speak`/`generate_image` are separate REST resources
    /// on other vendors).
    fn chat_or_vision(
        &self,
        request: &Invocation,
        input: Option<VisionInput>,
    ) -> Result<Value, ProviderError> {
        let model = resolve_model(&request.args, &self.model);
        let system: Vec<&str> = request
            .messages
            .iter()
            .filter(|m| m.role == "system")
            .map(|m| m.text.as_str())
            .collect();
        let mut messages: Vec<serde_json::Value> = request
            .messages
            .iter()
            .filter(|m| m.role != "system")
            .map(|m| {
                json!({
                    "role": if m.role == "assistant" { "assistant" } else { "user" },
                    "content": m.text,
                })
            })
            .collect();

        if let Some(input) = input {
            let content_block = match input {
                VisionInput::Image(ImageSource::Inline { mime, data_b64 }) => json!({
                    "type": "image",
                    "source": {"type": "base64", "media_type": mime, "data": data_b64},
                }),
                VisionInput::Image(ImageSource::Url(url)) => json!({
                    "type": "image",
                    "source": {"type": "url", "url": url},
                }),
                VisionInput::Document(ImageSource::Inline { mime, data_b64 }) => json!({
                    "type": "document",
                    "source": {"type": "base64", "media_type": mime, "data": data_b64},
                }),
                VisionInput::Document(ImageSource::Url(url)) => json!({
                    "type": "document",
                    "source": {"type": "url", "url": url},
                }),
            };
            if let Some(last) = messages.last_mut() {
                let role = last.get("role").cloned().unwrap_or_else(|| json!("user"));
                let text = last.get("content").cloned().unwrap_or_else(|| json!(""));
                *last = json!({"role": role, "content": [{"type": "text", "text": text}, content_block]});
            } else {
                messages.push(json!({"role": "user", "content": [content_block]}));
            }
        }

        let max_tokens = resolve_i64(&request.args, &self.default_params, "max_tokens")
            .unwrap_or(DEFAULT_MAX_TOKENS);
        let mut body = json!({"model": model, "max_tokens": max_tokens, "messages": messages});
        if !system.is_empty() {
            body["system"] = json!(system.join("\n"));
        }
        if let Some(t) = resolve_f64(&request.args, &self.default_params, "temperature") {
            body["temperature"] = json!(t);
        }

        let url = format!("{}/messages", self.base_url);
        let resp = send_json_with_retry(self.transport.as_ref(), &url, &self.headers(), &body)?;

        if resp.get("stop_reason").and_then(|r| r.as_str()) == Some("refusal") {
            return Err(ProviderError::Refused(
                "model declined to answer".to_string(),
            ));
        }
        let text = resp
            .get("content")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("text"))
            .and_then(|t| t.as_str())
            .ok_or_else(|| ProviderError::Failed("response had no text content".to_string()))?;
        Ok(Value::Text(text.to_string()))
    }
}

impl Provider for AnthropicProvider {
    fn id(&self) -> &str {
        "anthropic"
    }

    fn supports(&self, capability: &str) -> bool {
        capability == self.capability
    }

    fn invoke(&self, capability: &str, request: &Invocation) -> Result<Value, ProviderError> {
        match capability {
            "chat" => self.chat(request),
            "vision" => self.vision(request),
            other => Err(ProviderError::UnsupportedCapability(other.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use base64::Engine;

    use super::super::transport::ScriptedTransport;
    use super::super::Message;
    use super::*;

    fn provider(transport: ScriptedTransport) -> AnthropicProvider {
        AnthropicProvider::with_transport(
            "chat",
            "https://api.anthropic.com/v1",
            "sk-ant-test",
            "claude-3-5-sonnet-20241022",
            BTreeMap::new(),
            Box::new(transport),
        )
    }

    fn invocation_with_system() -> Invocation {
        Invocation {
            messages: vec![
                Message {
                    role: "system".to_string(),
                    text: "be terse".to_string(),
                },
                Message {
                    role: "user".to_string(),
                    text: "hello".to_string(),
                },
            ],
            args: BTreeMap::new(),
        }
    }

    #[test]
    fn chat_happy_path_returns_text() {
        let transport = ScriptedTransport::new(vec![ScriptedTransport::ok(
            200,
            json!({"content": [{"type": "text", "text": "hi"}], "stop_reason": "end_turn"}),
        )]);
        let p = provider(transport);
        let result = p.invoke("chat", &invocation_with_system()).unwrap();
        assert_eq!(result, Value::Text("hi".to_string()));
    }

    #[test]
    fn refusal_stop_reason_maps_to_refused() {
        let transport = ScriptedTransport::new(vec![ScriptedTransport::ok(
            200,
            json!({"content": [], "stop_reason": "refusal"}),
        )]);
        let p = provider(transport);
        let err = p.invoke("chat", &invocation_with_system()).unwrap_err();
        assert_eq!(
            err,
            ProviderError::Refused("model declined to answer".to_string())
        );
    }

    #[test]
    fn embed_is_unsupported() {
        let transport = ScriptedTransport::new(vec![]);
        let p = provider(transport);
        let err = p.invoke("embed", &invocation_with_system()).unwrap_err();
        assert_eq!(
            err,
            ProviderError::UnsupportedCapability("embed".to_string())
        );
    }

    #[test]
    fn vision_with_url_image_returns_text() {
        let transport = ScriptedTransport::new(vec![ScriptedTransport::ok(
            200,
            json!({"content": [{"type": "text", "text": "a cat"}], "stop_reason": "end_turn"}),
        )]);
        let p = provider(transport);
        let invocation = Invocation {
            messages: vec![Message {
                role: "user".to_string(),
                text: "describe this".to_string(),
            }],
            args: BTreeMap::from([(
                "_".to_string(),
                Value::Text("https://example.com/cat.png".to_string()),
            )]),
        };
        let result = p.invoke("vision", &invocation).unwrap();
        assert_eq!(result, Value::Text("a cat".to_string()));
    }

    /// Builds a provider around an `Arc<ScriptedTransport>`, returning both
    /// so a test can inspect `sent_bodies()` after `invoke` — `provider()`
    /// moves its transport into the `Box<dyn Transport>` and can't be
    /// inspected afterwards, which the request-shape assertions below need.
    fn provider_with_shared_transport(
        transport: ScriptedTransport,
    ) -> (AnthropicProvider, std::sync::Arc<ScriptedTransport>) {
        let shared = std::sync::Arc::new(transport);
        let p = AnthropicProvider::with_transport(
            "vision",
            "https://api.anthropic.com/v1",
            "sk-ant-test",
            "claude-3-5-sonnet-20241022",
            BTreeMap::new(),
            Box::new(shared.clone()),
        );
        (p, shared)
    }

    fn vision_invocation(artifact_ref: &str) -> Invocation {
        Invocation {
            messages: vec![Message {
                role: "user".to_string(),
                text: "describe this".to_string(),
            }],
            args: BTreeMap::from([("_".to_string(), Value::Text(artifact_ref.to_string()))]),
        }
    }

    #[test]
    fn vision_with_local_pdf_builds_a_document_content_block() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!(
            "ulexite-anthropic-test-{}.pdf",
            std::process::id()
        ));
        std::fs::write(&path, b"%PDF-1.4 fake").unwrap();

        let (p, transport) = provider_with_shared_transport(ScriptedTransport::new(vec![
            ScriptedTransport::ok(
                200,
                json!({"content": [{"type": "text", "text": "the document says X"}], "stop_reason": "end_turn"}),
            ),
        ]));
        let invocation = vision_invocation(path.to_str().unwrap());
        let result = p.invoke("vision", &invocation).unwrap();
        std::fs::remove_file(&path).ok();

        assert_eq!(result, Value::Text("the document says X".to_string()));

        let sent = transport.sent_bodies();
        assert_eq!(sent.len(), 1);
        let content = &sent[0]["messages"][0]["content"];
        let blocks = content.as_array().expect("content should be an array");
        assert_eq!(blocks.len(), 2, "expected a text block and a document block");
        let doc_block = &blocks[1];
        assert_eq!(doc_block["type"], json!("document"));
        assert_eq!(doc_block["source"]["type"], json!("base64"));
        assert_eq!(doc_block["source"]["media_type"], json!("application/pdf"));
        assert_eq!(
            doc_block["source"]["data"],
            json!(base64::engine::general_purpose::STANDARD.encode(b"%PDF-1.4 fake"))
        );
    }

    #[test]
    fn vision_with_url_pdf_builds_a_document_content_block_with_a_url_source() {
        let (p, transport) = provider_with_shared_transport(ScriptedTransport::new(vec![
            ScriptedTransport::ok(
                200,
                json!({"content": [{"type": "text", "text": "ok"}], "stop_reason": "end_turn"}),
            ),
        ]));
        let invocation = vision_invocation("https://example.com/report.pdf");
        p.invoke("vision", &invocation).unwrap();

        let sent = transport.sent_bodies();
        let doc_block = &sent[0]["messages"][0]["content"][1];
        assert_eq!(doc_block["type"], json!("document"));
        assert_eq!(doc_block["source"]["type"], json!("url"));
        assert_eq!(
            doc_block["source"]["url"],
            json!("https://example.com/report.pdf")
        );
    }

    #[test]
    fn vision_with_local_jpg_still_builds_an_image_content_block() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!(
            "ulexite-anthropic-test-{}.jpg",
            std::process::id()
        ));
        std::fs::write(&path, [0xff, 0xd8, 0xff, 0xe0]).unwrap();

        let (p, transport) = provider_with_shared_transport(ScriptedTransport::new(vec![
            ScriptedTransport::ok(
                200,
                json!({"content": [{"type": "text", "text": "a cat"}], "stop_reason": "end_turn"}),
            ),
        ]));
        let invocation = vision_invocation(path.to_str().unwrap());
        let result = p.invoke("vision", &invocation).unwrap();
        std::fs::remove_file(&path).ok();

        assert_eq!(result, Value::Text("a cat".to_string()));

        let sent = transport.sent_bodies();
        let image_block = &sent[0]["messages"][0]["content"][1];
        assert_eq!(image_block["type"], json!("image"));
        assert_eq!(image_block["source"]["type"], json!("base64"));
        assert_eq!(image_block["source"]["media_type"], json!("image/jpeg"));
        assert_eq!(
            image_block["source"]["data"],
            json!(base64::engine::general_purpose::STANDARD.encode([0xff, 0xd8, 0xff, 0xe0]))
        );
    }

    #[test]
    fn vision_with_url_png_still_builds_an_image_content_block() {
        let (p, transport) = provider_with_shared_transport(ScriptedTransport::new(vec![
            ScriptedTransport::ok(
                200,
                json!({"content": [{"type": "text", "text": "a cat"}], "stop_reason": "end_turn"}),
            ),
        ]));
        let invocation = vision_invocation("https://example.com/cat.png");
        p.invoke("vision", &invocation).unwrap();

        let sent = transport.sent_bodies();
        let image_block = &sent[0]["messages"][0]["content"][1];
        assert_eq!(image_block["type"], json!("image"));
        assert_eq!(image_block["source"]["type"], json!("url"));
        assert_eq!(
            image_block["source"]["url"],
            json!("https://example.com/cat.png")
        );
    }

    #[test]
    fn vision_without_image_argument_is_a_clear_error() {
        let transport = ScriptedTransport::new(vec![]);
        let p = provider(transport);
        let err = p.invoke("vision", &invocation_with_system()).unwrap_err();
        assert!(matches!(err, ProviderError::Failed(_)));
    }

    #[test]
    fn supports_only_its_declared_capability() {
        let transport = ScriptedTransport::new(vec![]);
        let p = provider(transport);
        assert!(p.supports("chat"));
        assert!(!p.supports("vision"));
        assert!(!p.supports("embed"));
    }
}
