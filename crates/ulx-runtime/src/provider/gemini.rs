//! Google Gemini's REST API — `generateContent`/`embedContent`, with the
//! API key passed as a query parameter rather than a header, `role: "model"`
//! instead of `"assistant"`, and safety blocks surfaced via
//! `promptFeedback`/`finishReason` rather than an HTTP error.

use std::collections::BTreeMap;

use serde_json::json;

use crate::value::Value;

use super::artifact::{self, ImageSource};
use super::transport::{send_json_with_retry, Transport};
use super::{resolve_f64, resolve_i64, resolve_model, Invocation, Provider, ProviderError};

pub(crate) const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta";

pub struct GeminiProvider {
    capability: String,
    base_url: String,
    api_key: String,
    model: String,
    default_params: BTreeMap<String, Value>,
    transport: Box<dyn Transport>,
}

impl GeminiProvider {
    pub fn with_transport(
        capability: impl Into<String>,
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
        default_params: BTreeMap<String, Value>,
        transport: Box<dyn Transport>,
    ) -> Self {
        GeminiProvider {
            capability: capability.into(),
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key: api_key.into(),
            model: model.into(),
            default_params,
            transport,
        }
    }

    fn chat(&self, request: &Invocation) -> Result<Value, ProviderError> {
        self.chat_or_vision(request, None)
    }

    /// Gemini's `inline_data` image part only takes raw base64 bytes, not
    /// an arbitrary URL — fetching a remote image would need the separate
    /// File API (upload, then reference by `file_uri`), which is out of
    /// scope here (§24 Limitations); a local file is the supported path.
    fn vision(&self, request: &Invocation) -> Result<Value, ProviderError> {
        let image_ref = artifact::first_artifact_arg(&request.args).ok_or_else(|| {
            ProviderError::Failed(
                "vision call has no image argument (expected e.g. `ask vision(doc)`)".to_string(),
            )
        })?;
        let image_part = match artifact::resolve_image(image_ref)? {
            ImageSource::Inline { mime, data_b64 } => {
                json!({"inline_data": {"mime_type": mime, "data": data_b64}})
            }
            ImageSource::Url(_) => {
                return Err(ProviderError::Failed(
                    "gemini vision only supports local image files today, not URLs (no File API upload support yet)"
                        .to_string(),
                ))
            }
        };
        self.chat_or_vision(request, Some(image_part))
    }

    fn chat_or_vision(
        &self,
        request: &Invocation,
        image_part: Option<serde_json::Value>,
    ) -> Result<Value, ProviderError> {
        let model = resolve_model(&request.args, &self.model);
        let system: Vec<&str> = request
            .messages
            .iter()
            .filter(|m| m.role == "system")
            .map(|m| m.text.as_str())
            .collect();
        let mut contents: Vec<serde_json::Value> = request
            .messages
            .iter()
            .filter(|m| m.role != "system")
            .map(|m| {
                let role = if m.role == "assistant" {
                    "model"
                } else {
                    "user"
                };
                json!({"role": role, "parts": [{"text": m.text}]})
            })
            .collect();

        if let Some(image_part) = image_part {
            if let Some(last) = contents.last_mut() {
                last["parts"]
                    .as_array_mut()
                    .expect("contents entries always have a `parts` array")
                    .push(image_part);
            } else {
                contents.push(json!({"role": "user", "parts": [image_part]}));
            }
        }

        let mut body = json!({"contents": contents});
        if !system.is_empty() {
            body["systemInstruction"] = json!({"parts": [{"text": system.join("\n")}]});
        }
        let mut generation_config = serde_json::Map::new();
        if let Some(t) = resolve_f64(&request.args, &self.default_params, "temperature") {
            generation_config.insert("temperature".to_string(), json!(t));
        }
        if let Some(m) = resolve_i64(&request.args, &self.default_params, "max_tokens") {
            generation_config.insert("maxOutputTokens".to_string(), json!(m));
        }
        if !generation_config.is_empty() {
            body["generationConfig"] = serde_json::Value::Object(generation_config);
        }

        let url = format!(
            "{}/models/{model}:generateContent?key={}",
            self.base_url, self.api_key
        );
        let resp = send_json_with_retry(self.transport.as_ref(), &url, &[], &body)?;

        if let Some(reason) = resp
            .get("promptFeedback")
            .and_then(|f| f.get("blockReason"))
            .and_then(|r| r.as_str())
        {
            return Err(ProviderError::Refused(format!("blocked: {reason}")));
        }
        let candidate = resp
            .get("candidates")
            .and_then(|c| c.get(0))
            .ok_or_else(|| ProviderError::Failed("response had no candidates".to_string()))?;
        if candidate.get("finishReason").and_then(|r| r.as_str()) == Some("SAFETY") {
            return Err(ProviderError::Refused(
                "response withheld for safety".to_string(),
            ));
        }
        let text = candidate
            .get("content")
            .and_then(|c| c.get("parts"))
            .and_then(|p| p.get(0))
            .and_then(|p| p.get("text"))
            .and_then(|t| t.as_str())
            .ok_or_else(|| ProviderError::Failed("response had no text part".to_string()))?;
        Ok(Value::Text(text.to_string()))
    }

    fn embed(&self, request: &Invocation) -> Result<Value, ProviderError> {
        let model = resolve_model(&request.args, &self.model);
        let text = request
            .messages
            .first()
            .map(|m| m.text.as_str())
            .unwrap_or("");
        let body = json!({"content": {"parts": [{"text": text}]}});

        let url = format!(
            "{}/models/{model}:embedContent?key={}",
            self.base_url, self.api_key
        );
        let resp = send_json_with_retry(self.transport.as_ref(), &url, &[], &body)?;

        let values = resp
            .get("embedding")
            .and_then(|e| e.get("values"))
            .and_then(|v| v.as_array())
            .ok_or_else(|| ProviderError::Failed("response had no embedding values".to_string()))?;
        let values = values
            .iter()
            .filter_map(|v| v.as_f64())
            .map(Value::Float)
            .collect();
        Ok(Value::List(values))
    }
}

impl Provider for GeminiProvider {
    fn id(&self) -> &str {
        "gemini"
    }

    fn supports(&self, capability: &str) -> bool {
        capability == self.capability
    }

    fn invoke(&self, capability: &str, request: &Invocation) -> Result<Value, ProviderError> {
        match capability {
            "chat" => self.chat(request),
            "vision" => self.vision(request),
            "embed" => self.embed(request),
            "judge" => super::judge::judge_via_chat(request, |req| self.chat(req)),
            other => Err(ProviderError::UnsupportedCapability(other.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::transport::ScriptedTransport;
    use super::super::Message;
    use super::*;

    fn provider(transport: ScriptedTransport) -> GeminiProvider {
        GeminiProvider::with_transport(
            "chat",
            DEFAULT_BASE_URL,
            "test-key",
            "gemini-1.5-flash",
            BTreeMap::new(),
            Box::new(transport),
        )
    }

    fn invocation() -> Invocation {
        Invocation {
            messages: vec![Message {
                role: "user".to_string(),
                text: "hello".to_string(),
            }],
            args: BTreeMap::new(),
        }
    }

    #[test]
    fn chat_happy_path_returns_text() {
        let transport = ScriptedTransport::new(vec![ScriptedTransport::ok(
            200,
            json!({"candidates": [{"content": {"parts": [{"text": "hi"}]}, "finishReason": "STOP"}]}),
        )]);
        let p = provider(transport);
        let result = p.invoke("chat", &invocation()).unwrap();
        assert_eq!(result, Value::Text("hi".to_string()));
    }

    #[test]
    fn blocked_prompt_feedback_is_refused() {
        let transport = ScriptedTransport::new(vec![ScriptedTransport::ok(
            200,
            json!({"promptFeedback": {"blockReason": "SAFETY"}, "candidates": []}),
        )]);
        let p = provider(transport);
        let err = p.invoke("chat", &invocation()).unwrap_err();
        assert_eq!(err, ProviderError::Refused("blocked: SAFETY".to_string()));
    }

    #[test]
    fn embed_happy_path_returns_list() {
        let transport = ScriptedTransport::new(vec![ScriptedTransport::ok(
            200,
            json!({"embedding": {"values": [0.5, -0.5]}}),
        )]);
        let p = provider(transport);
        let result = p.invoke("embed", &invocation()).unwrap();
        assert_eq!(
            result,
            Value::List(vec![Value::Float(0.5), Value::Float(-0.5)])
        );
    }

    #[test]
    fn vision_with_local_file_returns_text() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("ulexite-gemini-test-{}.png", std::process::id()));
        std::fs::write(&path, [0x89, 0x50, 0x4e, 0x47]).unwrap();

        let transport = ScriptedTransport::new(vec![ScriptedTransport::ok(
            200,
            json!({"candidates": [{"content": {"parts": [{"text": "a cat"}]}, "finishReason": "STOP"}]}),
        )]);
        let p = provider(transport);
        let invocation = Invocation {
            messages: vec![Message {
                role: "user".to_string(),
                text: "describe this".to_string(),
            }],
            args: BTreeMap::from([(
                "_".to_string(),
                Value::Text(path.to_string_lossy().to_string()),
            )]),
        };
        let result = p.invoke("vision", &invocation).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(result, Value::Text("a cat".to_string()));
    }

    #[test]
    fn judge_happy_path_returns_a_verdict() {
        let transport = ScriptedTransport::new(vec![ScriptedTransport::ok(
            200,
            json!({"candidates": [{"content": {"parts": [{"text": "SCORE: 0.9"}]}, "finishReason": "STOP"}]}),
        )]);
        let p = GeminiProvider::with_transport(
            "judge",
            DEFAULT_BASE_URL,
            "test-key",
            "gemini-1.5-flash",
            BTreeMap::new(),
            Box::new(transport),
        );
        let result = p.invoke("judge", &invocation()).unwrap();
        assert_eq!(result, Value::Verdict(crate::value::Verdict::Score(0.9)));
    }

    #[test]
    fn vision_with_url_is_a_clear_error() {
        let transport = ScriptedTransport::new(vec![]);
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
        let err = p.invoke("vision", &invocation).unwrap_err();
        assert!(matches!(err, ProviderError::Failed(_)));
    }
}
