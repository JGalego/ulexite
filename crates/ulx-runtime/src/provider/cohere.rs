//! Cohere's Chat v2 API (`/v2/chat`) and Embed v1 API (`/v1/embed`). Unlike
//! OpenAI/Anthropic/Gemini, Cohere's API doesn't expose an explicit
//! safety-refusal signal in the chat response, so `Refused` is never
//! produced here — only `Failed` for a genuine `finish_reason: "ERROR"`.

use std::collections::BTreeMap;

use serde_json::json;

use crate::value::Value;

use super::transport::{send_json_with_retry, Transport};
use super::{resolve_f64, resolve_i64, resolve_model, Invocation, Provider, ProviderError};

pub(crate) const DEFAULT_BASE_URL: &str = "https://api.cohere.com";

pub struct CohereProvider {
    capability: String,
    base_url: String,
    api_key: String,
    model: String,
    default_params: BTreeMap<String, Value>,
    transport: Box<dyn Transport>,
}

impl CohereProvider {
    pub fn with_transport(
        capability: impl Into<String>,
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
        default_params: BTreeMap<String, Value>,
        transport: Box<dyn Transport>,
    ) -> Self {
        CohereProvider {
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
            (
                "Authorization".to_string(),
                format!("Bearer {}", self.api_key),
            ),
        ]
    }

    fn chat(&self, request: &Invocation) -> Result<Value, ProviderError> {
        let model = resolve_model(&request.args, &self.model);
        let messages: Vec<_> = request
            .messages
            .iter()
            .map(|m| json!({"role": cohere_role(&m.role), "content": m.text}))
            .collect();
        let mut body = json!({"model": model, "messages": messages});
        if let Some(t) = resolve_f64(&request.args, &self.default_params, "temperature") {
            body["temperature"] = json!(t);
        }
        if let Some(m) = resolve_i64(&request.args, &self.default_params, "max_tokens") {
            body["max_tokens"] = json!(m);
        }

        let url = format!("{}/v2/chat", self.base_url);
        let resp = send_json_with_retry(self.transport.as_ref(), &url, &self.headers(), &body)?;

        if resp.get("finish_reason").and_then(|r| r.as_str()) == Some("ERROR") {
            return Err(ProviderError::Failed(
                "provider reported finish_reason=ERROR".to_string(),
            ));
        }
        let text = resp
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("text"))
            .and_then(|t| t.as_str())
            .ok_or_else(|| ProviderError::Failed("response had no message content".to_string()))?;
        Ok(Value::Text(text.to_string()))
    }

    fn embed(&self, request: &Invocation) -> Result<Value, ProviderError> {
        let model = resolve_model(&request.args, &self.model);
        let text = request
            .messages
            .first()
            .map(|m| m.text.as_str())
            .unwrap_or("");
        let body = json!({"model": model, "texts": [text], "input_type": "search_document"});

        let url = format!("{}/v1/embed", self.base_url);
        let resp = send_json_with_retry(self.transport.as_ref(), &url, &self.headers(), &body)?;

        let embedding = resp
            .get("embeddings")
            .and_then(|e| e.get(0))
            .and_then(|e| e.as_array())
            .ok_or_else(|| ProviderError::Failed("response had no embeddings".to_string()))?;
        let values = embedding
            .iter()
            .filter_map(|v| v.as_f64())
            .map(Value::Float)
            .collect();
        Ok(Value::List(values))
    }
}

impl Provider for CohereProvider {
    fn id(&self) -> &str {
        "cohere"
    }

    fn supports(&self, capability: &str) -> bool {
        capability == self.capability
    }

    fn invoke(&self, capability: &str, request: &Invocation) -> Result<Value, ProviderError> {
        match capability {
            "chat" => self.chat(request),
            "embed" => self.embed(request),
            "judge" => super::judge::judge_via_chat(request, |req| self.chat(req)),
            other => Err(ProviderError::UnsupportedCapability(other.to_string())),
        }
    }
}

fn cohere_role(role: &str) -> &str {
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

    fn provider(transport: ScriptedTransport) -> CohereProvider {
        CohereProvider::with_transport(
            "chat",
            DEFAULT_BASE_URL,
            "test-key",
            "command-r",
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
            json!({"message": {"content": [{"type": "text", "text": "hi"}]}, "finish_reason": "COMPLETE"}),
        )]);
        let p = provider(transport);
        let result = p.invoke("chat", &invocation()).unwrap();
        assert_eq!(result, Value::Text("hi".to_string()));
    }

    #[test]
    fn embed_happy_path_returns_list() {
        let transport = ScriptedTransport::new(vec![ScriptedTransport::ok(
            200,
            json!({"embeddings": [[0.1, 0.2]]}),
        )]);
        let p = provider(transport);
        let result = p.invoke("embed", &invocation()).unwrap();
        assert_eq!(
            result,
            Value::List(vec![Value::Float(0.1), Value::Float(0.2)])
        );
    }

    #[test]
    fn judge_happy_path_returns_a_verdict() {
        let transport = ScriptedTransport::new(vec![ScriptedTransport::ok(
            200,
            json!({"message": {"content": [{"type": "text", "text": "PASS"}]}, "finish_reason": "COMPLETE"}),
        )]);
        let p = CohereProvider::with_transport(
            "judge",
            DEFAULT_BASE_URL,
            "test-key",
            "command-r",
            BTreeMap::new(),
            Box::new(transport),
        );
        let result = p.invoke("judge", &invocation()).unwrap();
        assert_eq!(result, Value::Verdict(crate::value::Verdict::Pass));
    }
}
