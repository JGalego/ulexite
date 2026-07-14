//! One adapter for every vendor that speaks an OpenAI-shaped
//! `/chat/completions` + `/embeddings` API: OpenAI itself, Groq, Together,
//! Fireworks, OpenRouter, Perplexity, DeepInfra, Anyscale, LM Studio,
//! text-generation-webui, and vLLM's OpenAI-compatible server mode — a
//! different `base_url`/`api_key_env` preset (`factory.rs`) is the only
//! thing that distinguishes them. Also covers `/audio/transcriptions`
//! (multipart upload), `/audio/speech` (binary response, written to a temp
//! file), and `/images/generations` (base64 response, written to a temp
//! file) — real endpoints on OpenAI and (for transcription) Groq; another
//! OpenAI-compatible server that doesn't implement these just surfaces a
//! normal HTTP error when called.

use std::collections::BTreeMap;

use base64::Engine;
use serde_json::json;

use crate::value::Value;

use super::artifact::{self, ImageSource};
use super::transport::{
    send_json_expect_bytes_with_retry, send_json_with_retry, send_multipart_with_retry, Transport,
};
use super::{resolve_f64, resolve_i64, resolve_model, Invocation, Provider, ProviderError};

pub struct OpenAiCompatibleProvider {
    id: String,
    capability: String,
    base_url: String,
    api_key: Option<String>,
    model: String,
    default_params: BTreeMap<String, Value>,
    transport: Box<dyn Transport>,
}

impl OpenAiCompatibleProvider {
    #[allow(clippy::too_many_arguments)]
    pub fn with_transport(
        id: impl Into<String>,
        capability: impl Into<String>,
        base_url: impl Into<String>,
        api_key: Option<String>,
        model: impl Into<String>,
        default_params: BTreeMap<String, Value>,
        transport: Box<dyn Transport>,
    ) -> Self {
        OpenAiCompatibleProvider {
            id: id.into(),
            capability: capability.into(),
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key,
            model: model.into(),
            default_params,
            transport,
        }
    }

    fn json_headers(&self) -> Vec<(String, String)> {
        let mut h = vec![("Content-Type".to_string(), "application/json".to_string())];
        if let Some(key) = &self.api_key {
            h.push(("Authorization".to_string(), format!("Bearer {key}")));
        }
        h
    }

    /// Multipart requests set their own `Content-Type` (with a boundary) —
    /// don't also force `application/json`.
    fn bearer_only_headers(&self) -> Vec<(String, String)> {
        match &self.api_key {
            Some(key) => vec![("Authorization".to_string(), format!("Bearer {key}"))],
            None => vec![],
        }
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
        let image_url = match artifact::resolve_image(image_ref)? {
            ImageSource::Url(u) => u,
            ImageSource::Inline { mime, data_b64 } => format!("data:{mime};base64,{data_b64}"),
        };
        self.chat_or_vision(request, Some(image_url))
    }

    fn chat_or_vision(
        &self,
        request: &Invocation,
        image_url: Option<String>,
    ) -> Result<Value, ProviderError> {
        let model = resolve_model(&request.args, &self.model);
        let mut messages: Vec<serde_json::Value> = request
            .messages
            .iter()
            .map(|m| json!({"role": openai_role(&m.role), "content": m.text}))
            .collect();

        if let Some(image_url) = image_url {
            let image_block = json!({"type": "image_url", "image_url": {"url": image_url}});
            if let Some(last) = messages.last_mut() {
                let role = last.get("role").cloned().unwrap_or_else(|| json!("user"));
                let text = last.get("content").cloned().unwrap_or_else(|| json!(""));
                *last =
                    json!({"role": role, "content": [{"type": "text", "text": text}, image_block]});
            } else {
                messages.push(json!({"role": "user", "content": [image_block]}));
            }
        }

        let mut body = json!({"model": model, "messages": messages});
        if let Some(t) = resolve_f64(&request.args, &self.default_params, "temperature") {
            body["temperature"] = json!(t);
        }
        if let Some(m) = resolve_i64(&request.args, &self.default_params, "max_tokens") {
            body["max_tokens"] = json!(m);
        }
        if let Some(t) = resolve_f64(&request.args, &self.default_params, "top_p") {
            body["top_p"] = json!(t);
        }

        let url = format!("{}/chat/completions", self.base_url);
        let resp =
            send_json_with_retry(self.transport.as_ref(), &url, &self.json_headers(), &body)?;

        let choice = resp
            .get("choices")
            .and_then(|c| c.get(0))
            .ok_or_else(|| ProviderError::Failed("response had no `choices`".to_string()))?;
        if choice.get("finish_reason").and_then(|r| r.as_str()) == Some("content_filter") {
            return Err(ProviderError::Refused(
                "content filtered by provider".to_string(),
            ));
        }
        let text = choice
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .ok_or_else(|| ProviderError::Failed("response had no message content".to_string()))?;
        Ok(Value::Text(text.to_string()))
    }

    fn embed(&self, request: &Invocation) -> Result<Value, ProviderError> {
        let model = resolve_model(&request.args, &self.model);
        let input = request
            .messages
            .first()
            .map(|m| m.text.as_str())
            .unwrap_or("");
        let body = json!({"model": model, "input": input});

        let url = format!("{}/embeddings", self.base_url);
        let resp =
            send_json_with_retry(self.transport.as_ref(), &url, &self.json_headers(), &body)?;

        let embedding = resp
            .get("data")
            .and_then(|d| d.get(0))
            .and_then(|d| d.get("embedding"))
            .and_then(|e| e.as_array())
            .ok_or_else(|| ProviderError::Failed("response had no embedding".to_string()))?;
        let values = embedding
            .iter()
            .filter_map(|v| v.as_f64())
            .map(Value::Float)
            .collect();
        Ok(Value::List(values))
    }

    /// `/audio/transcriptions` (Whisper-compatible): a multipart upload of
    /// the audio file plus the model name.
    fn transcribe(&self, request: &Invocation) -> Result<Value, ProviderError> {
        let audio_ref = artifact::first_artifact_arg(&request.args).ok_or_else(|| {
            ProviderError::Failed(
                "transcribe call has no audio file argument (expected e.g. `ask transcribe(recording)`)"
                    .to_string(),
            )
        })?;
        let (filename, bytes) = artifact::read_audio_file(audio_ref)?;
        let model = resolve_model(&request.args, &self.model);

        let url = format!("{}/audio/transcriptions", self.base_url);
        let resp = send_multipart_with_retry(
            self.transport.as_ref(),
            &url,
            &self.bearer_only_headers(),
            vec![("model".to_string(), model)],
            ("file".to_string(), filename, bytes),
        )?;

        let text = resp
            .get("text")
            .and_then(|t| t.as_str())
            .ok_or_else(|| ProviderError::Failed("response had no `text`".to_string()))?;
        Ok(Value::Text(text.to_string()))
    }

    /// `/audio/speech` (TTS): JSON request, raw audio bytes back — written
    /// to a content-addressed temp file (no real artifact/blob store yet,
    /// §12.7, §24 Limitations), with the path returned as the result.
    fn speak(&self, request: &Invocation) -> Result<Value, ProviderError> {
        let text = request
            .messages
            .iter()
            .map(|m| m.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let model = resolve_model(&request.args, &self.model);
        let voice = request
            .args
            .get("voice")
            .and_then(Value::as_text)
            .unwrap_or("alloy");
        let body = json!({"model": model, "input": text, "voice": voice});

        let url = format!("{}/audio/speech", self.base_url);
        let bytes = send_json_expect_bytes_with_retry(
            self.transport.as_ref(),
            &url,
            &self.json_headers(),
            &body,
        )?;
        artifact::write_artifact(&bytes, "mp3")
    }

    /// `/images/generations` (DALL-E-compatible): JSON in, base64 image
    /// out — written to a temp file, path returned (same caveat as `speak`).
    fn generate_image(&self, request: &Invocation) -> Result<Value, ProviderError> {
        let prompt = request
            .messages
            .iter()
            .map(|m| m.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let model = resolve_model(&request.args, &self.model);
        let body = json!({"model": model, "prompt": prompt, "response_format": "b64_json", "n": 1});

        let url = format!("{}/images/generations", self.base_url);
        let resp =
            send_json_with_retry(self.transport.as_ref(), &url, &self.json_headers(), &body)?;

        let b64 = resp
            .get("data")
            .and_then(|d| d.get(0))
            .and_then(|d| d.get("b64_json"))
            .and_then(|b| b.as_str())
            .ok_or_else(|| ProviderError::Failed("response had no `b64_json`".to_string()))?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| ProviderError::Failed(format!("invalid base64 image data: {e}")))?;
        artifact::write_artifact(&bytes, "png")
    }
}

impl Provider for OpenAiCompatibleProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn supports(&self, capability: &str) -> bool {
        capability == self.capability
    }

    fn invoke(&self, capability: &str, request: &Invocation) -> Result<Value, ProviderError> {
        match capability {
            "chat" => self.chat(request),
            "vision" => self.vision(request),
            "embed" => self.embed(request),
            "transcribe" => self.transcribe(request),
            "speak" => self.speak(request),
            "generate_image" => self.generate_image(request),
            other => Err(ProviderError::UnsupportedCapability(other.to_string())),
        }
    }
}

fn openai_role(role: &str) -> &str {
    match role {
        "system" => "system",
        "assistant" => "assistant",
        _ => "user",
    }
}

#[cfg(test)]
mod tests {
    use super::super::transport::ScriptedTransport;
    use super::*;

    fn provider(transport: ScriptedTransport) -> OpenAiCompatibleProvider {
        OpenAiCompatibleProvider::with_transport(
            "openai",
            "chat",
            "https://api.openai.com/v1",
            Some("sk-test".to_string()),
            "gpt-4o-mini",
            BTreeMap::new(),
            Box::new(transport),
        )
    }

    fn chat_invocation() -> Invocation {
        Invocation {
            messages: vec![super::super::Message {
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
            json!({"choices": [{"message": {"content": "hi there"}, "finish_reason": "stop"}]}),
        )]);
        let p = provider(transport);
        let result = p.invoke("chat", &chat_invocation()).unwrap();
        assert_eq!(result, Value::Text("hi there".to_string()));
    }

    #[test]
    fn persistent_rate_limit_still_fails_after_retries_exhausted() {
        let transport = ScriptedTransport::new(vec![
            ScriptedTransport::ok(429, json!({"error": {"message": "rate limited"}})),
            ScriptedTransport::ok(429, json!({"error": {"message": "rate limited"}})),
            ScriptedTransport::ok(429, json!({"error": {"message": "rate limited"}})),
            ScriptedTransport::ok(429, json!({"error": {"message": "rate limited"}})),
        ]);
        let p = provider(transport);
        let err = p.invoke("chat", &chat_invocation()).unwrap_err();
        assert_eq!(err, ProviderError::RateLimited);
    }

    #[test]
    fn five_hundred_then_success_retries_once() {
        let transport = ScriptedTransport::new(vec![
            ScriptedTransport::ok(500, json!({"error": "boom"})),
            ScriptedTransport::ok(
                200,
                json!({"choices": [{"message": {"content": "recovered"}}]}),
            ),
        ]);
        let p = provider(transport);
        let result = p.invoke("chat", &chat_invocation()).unwrap();
        assert_eq!(result, Value::Text("recovered".to_string()));
    }

    #[test]
    fn content_filter_finish_reason_is_refused() {
        let transport = ScriptedTransport::new(vec![ScriptedTransport::ok(
            200,
            json!({"choices": [{"message": {"content": ""}, "finish_reason": "content_filter"}]}),
        )]);
        let p = provider(transport);
        let err = p.invoke("chat", &chat_invocation()).unwrap_err();
        assert_eq!(
            err,
            ProviderError::Refused("content filtered by provider".to_string())
        );
    }

    #[test]
    fn embed_happy_path_returns_list() {
        let transport = ScriptedTransport::new(vec![ScriptedTransport::ok(
            200,
            json!({"data": [{"embedding": [0.1, 0.2, 0.3]}]}),
        )]);
        let p = provider(transport);
        let result = p.invoke("embed", &chat_invocation()).unwrap();
        assert_eq!(
            result,
            Value::List(vec![
                Value::Float(0.1),
                Value::Float(0.2),
                Value::Float(0.3)
            ])
        );
    }

    #[test]
    fn vision_with_url_image_returns_text() {
        let transport = ScriptedTransport::new(vec![ScriptedTransport::ok(
            200,
            json!({"choices": [{"message": {"content": "a cat"}, "finish_reason": "stop"}]}),
        )]);
        let p = provider(transport);
        let invocation = Invocation {
            messages: vec![super::super::Message {
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
    fn transcribe_happy_path_returns_text() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!(
            "ulexite-openai-audio-test-{}.wav",
            std::process::id()
        ));
        std::fs::write(&path, [0u8, 1, 2, 3]).unwrap();

        let transport = ScriptedTransport::new(vec![ScriptedTransport::ok(
            200,
            json!({"text": "hello world"}),
        )]);
        let p = provider(transport);
        let invocation = Invocation {
            messages: vec![],
            args: BTreeMap::from([(
                "_".to_string(),
                Value::Text(path.to_string_lossy().to_string()),
            )]),
        };
        let result = p.invoke("transcribe", &invocation).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(result, Value::Text("hello world".to_string()));
    }

    #[test]
    fn speak_writes_bytes_to_an_artifact_file() {
        let transport = ScriptedTransport::new(vec![ScriptedTransport::ok_bytes(
            200,
            vec![0xff, 0xfb, 0x90, 0x00],
        )]);
        let p = provider(transport);
        let invocation = Invocation {
            messages: vec![super::super::Message {
                role: "user".to_string(),
                text: "hello world".to_string(),
            }],
            args: BTreeMap::new(),
        };
        let result = p.invoke("speak", &invocation).unwrap();
        let path = match result {
            Value::Text(p) => p,
            other => panic!("expected a Value::Text path, got {other:?}"),
        };
        assert!(path.ends_with(".mp3"));
        assert_eq!(std::fs::read(&path).unwrap(), vec![0xff, 0xfb, 0x90, 0x00]);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn generate_image_decodes_base64_to_an_artifact_file() {
        let png_bytes = [0x89u8, 0x50, 0x4e, 0x47];
        let b64 = base64::engine::general_purpose::STANDARD.encode(png_bytes);
        let transport = ScriptedTransport::new(vec![ScriptedTransport::ok(
            200,
            json!({"data": [{"b64_json": b64}]}),
        )]);
        let p = provider(transport);
        let invocation = Invocation {
            messages: vec![super::super::Message {
                role: "user".to_string(),
                text: "a cat".to_string(),
            }],
            args: BTreeMap::new(),
        };
        let result = p.invoke("generate_image", &invocation).unwrap();
        let path = match result {
            Value::Text(p) => p,
            other => panic!("expected a Value::Text path, got {other:?}"),
        };
        assert!(path.ends_with(".png"));
        assert_eq!(std::fs::read(&path).unwrap(), png_bytes);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn supports_only_its_declared_capability() {
        let transport = ScriptedTransport::new(vec![]);
        let p = provider(transport);
        assert!(p.supports("chat"));
        assert!(!p.supports("embed"));
        assert!(!p.supports("transcribe"));
    }
}
