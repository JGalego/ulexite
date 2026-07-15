//! Ollama's native API (`/api/chat`, `/api/embeddings`) rather than routing
//! it through `openai_compat` — this works out of the box against a plain
//! `ollama serve`, without the user needing to enable Ollama's separate
//! OpenAI-compatibility mode. No auth; `base_url` defaults to
//! `http://localhost:11434`.

use std::collections::BTreeMap;

use serde_json::json;

use crate::value::Value;

use super::artifact::{self, ImageSource};
use super::transport::{send_json_with_retry, Transport};
use super::{resolve_f64, resolve_i64, resolve_model, Invocation, Provider, ProviderError};

pub const DEFAULT_BASE_URL: &str = "http://localhost:11434";

pub struct OllamaProvider {
    capability: String,
    base_url: String,
    model: String,
    default_params: BTreeMap<String, Value>,
    transport: Box<dyn Transport>,
}

impl OllamaProvider {
    pub fn with_transport(
        capability: impl Into<String>,
        base_url: impl Into<String>,
        model: impl Into<String>,
        default_params: BTreeMap<String, Value>,
        transport: Box<dyn Transport>,
    ) -> Self {
        OllamaProvider {
            capability: capability.into(),
            base_url: base_url.into().trim_end_matches('/').to_string(),
            model: model.into(),
            default_params,
            transport,
        }
    }

    fn chat(&self, request: &Invocation) -> Result<Value, ProviderError> {
        self.chat_or_vision(request, None)
    }

    /// Ollama's `/api/chat` takes an `images` array of raw base64 strings
    /// on a message (for a multimodal-capable model like `llava`) — no
    /// URL fetching, so only a local file resolves here.
    fn vision(&self, request: &Invocation) -> Result<Value, ProviderError> {
        let image_ref = artifact::first_artifact_arg(&request.args).ok_or_else(|| {
            ProviderError::Failed(
                "vision call has no image argument (expected e.g. `ask vision(doc)`)".to_string(),
            )
        })?;
        let data_b64 = match artifact::resolve_image(image_ref)? {
            ImageSource::Inline { data_b64, .. } => data_b64,
            ImageSource::Url(_) => {
                return Err(ProviderError::Failed(
                    "ollama vision only supports local image files today, not URLs".to_string(),
                ))
            }
        };
        self.chat_or_vision(request, Some(data_b64))
    }

    fn chat_or_vision(
        &self,
        request: &Invocation,
        image_b64: Option<String>,
    ) -> Result<Value, ProviderError> {
        let model = resolve_model(&request.args, &self.model);
        let mut messages: Vec<serde_json::Value> = request
            .messages
            .iter()
            .map(|m| json!({"role": ollama_role(&m.role), "content": m.text}))
            .collect();
        if let Some(data_b64) = image_b64 {
            if let Some(last) = messages.last_mut() {
                last["images"] = json!([data_b64]);
            } else {
                messages.push(json!({"role": "user", "content": "", "images": [data_b64]}));
            }
        }
        let mut body = json!({"model": model, "messages": messages, "stream": false});
        let mut options = serde_json::Map::new();
        if let Some(t) = resolve_f64(&request.args, &self.default_params, "temperature") {
            options.insert("temperature".to_string(), json!(t));
        }
        if let Some(m) = resolve_i64(&request.args, &self.default_params, "max_tokens") {
            options.insert("num_predict".to_string(), json!(m));
        }
        if !options.is_empty() {
            body["options"] = serde_json::Value::Object(options);
        }

        let url = format!("{}/api/chat", self.base_url);
        let resp = send_json_with_retry(self.transport.as_ref(), &url, &[], &body)?;

        let text = resp
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .ok_or_else(|| ProviderError::Failed("response had no message content".to_string()))?;
        Ok(Value::Text(text.to_string()))
    }

    fn embed(&self, request: &Invocation) -> Result<Value, ProviderError> {
        let model = resolve_model(&request.args, &self.model);
        let prompt = request
            .messages
            .first()
            .map(|m| m.text.as_str())
            .unwrap_or("");
        let body = json!({"model": model, "prompt": prompt});

        let url = format!("{}/api/embeddings", self.base_url);
        let resp = send_json_with_retry(self.transport.as_ref(), &url, &[], &body)?;

        let embedding = resp
            .get("embedding")
            .and_then(|e| e.as_array())
            .ok_or_else(|| ProviderError::Failed("response had no embedding".to_string()))?;
        let values = embedding
            .iter()
            .filter_map(|v| v.as_f64())
            .map(Value::Float)
            .collect();
        Ok(Value::List(values))
    }
}

impl Provider for OllamaProvider {
    fn id(&self) -> &str {
        "ollama"
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

fn ollama_role(role: &str) -> &str {
    match role {
        "system" => "system",
        "assistant" => "assistant",
        _ => "user",
    }
}

#[cfg(test)]
mod tests {
    use super::super::transport::ScriptedTransport;
    use super::super::Message;
    use super::*;

    fn provider(transport: ScriptedTransport) -> OllamaProvider {
        OllamaProvider::with_transport(
            "chat",
            DEFAULT_BASE_URL,
            "llama3",
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
            json!({"message": {"role": "assistant", "content": "hi"}, "done": true}),
        )]);
        let p = provider(transport);
        let result = p.invoke("chat", &invocation()).unwrap();
        assert_eq!(result, Value::Text("hi".to_string()));
    }

    #[test]
    fn embed_happy_path_returns_list() {
        let transport = ScriptedTransport::new(vec![ScriptedTransport::ok(
            200,
            json!({"embedding": [0.4, 0.6]}),
        )]);
        let p = provider(transport);
        let result = p.invoke("embed", &invocation()).unwrap();
        assert_eq!(
            result,
            Value::List(vec![Value::Float(0.4), Value::Float(0.6)])
        );
    }

    #[test]
    fn judge_happy_path_returns_a_verdict() {
        let transport = ScriptedTransport::new(vec![ScriptedTransport::ok(
            200,
            json!({"message": {"role": "assistant", "content": "PASS"}, "done": true}),
        )]);
        let p = OllamaProvider::with_transport(
            "judge",
            DEFAULT_BASE_URL,
            "llama3",
            BTreeMap::new(),
            Box::new(transport),
        );
        let result = p.invoke("judge", &invocation()).unwrap();
        assert_eq!(result, Value::Verdict(crate::value::Verdict::Pass));
    }

    #[test]
    fn vision_with_local_file_returns_text() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("ulexite-ollama-test-{}.png", std::process::id()));
        std::fs::write(&path, [0x89, 0x50, 0x4e, 0x47]).unwrap();

        let transport = ScriptedTransport::new(vec![ScriptedTransport::ok(
            200,
            json!({"message": {"role": "assistant", "content": "a cat"}, "done": true}),
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
}
