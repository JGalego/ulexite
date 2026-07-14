//! One adapter for every vendor that speaks an OpenAI-shaped
//! `/chat/completions` + `/embeddings` API: OpenAI itself, Groq, Together,
//! Fireworks, OpenRouter, Perplexity, DeepInfra, Anyscale, LM Studio,
//! text-generation-webui, and vLLM's OpenAI-compatible server mode — a
//! different `base_url`/`api_key_env` preset (`factory.rs`) is the only
//! thing that distinguishes them.

use std::collections::BTreeMap;

use serde_json::json;

use crate::value::Value;

use super::transport::{send_json_with_retry, Transport};
use super::{resolve_f64, resolve_i64, resolve_model, Invocation, Provider, ProviderError};

pub struct OpenAiCompatibleProvider {
    id: String,
    base_url: String,
    api_key: Option<String>,
    model: String,
    default_params: BTreeMap<String, Value>,
    transport: Box<dyn Transport>,
}

impl OpenAiCompatibleProvider {
    pub fn with_transport(
        id: impl Into<String>,
        base_url: impl Into<String>,
        api_key: Option<String>,
        model: impl Into<String>,
        default_params: BTreeMap<String, Value>,
        transport: Box<dyn Transport>,
    ) -> Self {
        OpenAiCompatibleProvider {
            id: id.into(),
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key,
            model: model.into(),
            default_params,
            transport,
        }
    }

    fn headers(&self) -> Vec<(String, String)> {
        let mut h = vec![("Content-Type".to_string(), "application/json".to_string())];
        if let Some(key) = &self.api_key {
            h.push(("Authorization".to_string(), format!("Bearer {key}")));
        }
        h
    }

    fn chat(&self, request: &Invocation) -> Result<Value, ProviderError> {
        let model = resolve_model(&request.args, &self.model);
        let messages: Vec<_> = request
            .messages
            .iter()
            .map(|m| json!({"role": openai_role(&m.role), "content": m.text}))
            .collect();
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
        let resp = send_json_with_retry(self.transport.as_ref(), &url, &self.headers(), &body)?;

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
        let resp = send_json_with_retry(self.transport.as_ref(), &url, &self.headers(), &body)?;

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
}

impl Provider for OpenAiCompatibleProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn supports(&self, capability: &str) -> bool {
        matches!(capability, "chat" | "embed")
    }

    fn invoke(&self, capability: &str, request: &Invocation) -> Result<Value, ProviderError> {
        match capability {
            "chat" => self.chat(request),
            "embed" => self.embed(request),
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
    fn rate_limit_is_not_retried_away() {
        let transport = ScriptedTransport::new(vec![
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
    fn vision_is_unsupported() {
        let transport = ScriptedTransport::new(vec![]);
        let p = provider(transport);
        let err = p.invoke("vision", &chat_invocation()).unwrap_err();
        assert_eq!(
            err,
            ProviderError::UnsupportedCapability("vision".to_string())
        );
    }
}
