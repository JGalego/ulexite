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

    fn vision(&self, request: &Invocation) -> Result<Value, ProviderError> {
        let image_ref = artifact::first_artifact_arg(&request.args).ok_or_else(|| {
            ProviderError::Failed(
                "vision call has no image argument (expected e.g. `ask vision(doc)`)".to_string(),
            )
        })?;
        let image = artifact::resolve_image(image_ref)?;
        self.chat_or_vision(request, Some(image))
    }

    /// Anthropic's Messages API has one endpoint for both text-only and
    /// multimodal chat — an image is just another content block on the
    /// last message, native support since Claude 3 (no separate `vision`
    /// endpoint the way `transcribe`/`speak`/`generate_image` are separate
    /// REST resources on other vendors).
    fn chat_or_vision(
        &self,
        request: &Invocation,
        image: Option<ImageSource>,
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

        if let Some(image) = image {
            let image_block = match image {
                ImageSource::Inline { mime, data_b64 } => json!({
                    "type": "image",
                    "source": {"type": "base64", "media_type": mime, "data": data_b64},
                }),
                ImageSource::Url(url) => json!({
                    "type": "image",
                    "source": {"type": "url", "url": url},
                }),
            };
            if let Some(last) = messages.last_mut() {
                let role = last.get("role").cloned().unwrap_or_else(|| json!("user"));
                let text = last.get("content").cloned().unwrap_or_else(|| json!(""));
                *last =
                    json!({"role": role, "content": [{"type": "text", "text": text}, image_block]});
            } else {
                messages.push(json!({"role": "user", "content": [image_block]}));
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
