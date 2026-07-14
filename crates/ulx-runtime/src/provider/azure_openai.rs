//! Azure OpenAI: the same Chat Completions / Embeddings JSON shape as
//! OpenAI itself (`super::openai_shape`), but different URL and auth
//! conventions — a per-customer resource endpoint (no sensible fixed
//! default `base_url`), the deployment name as a path segment rather than
//! a `model` field in the body, a mandatory `api-version` query
//! parameter, and an `api-key` header instead of `Authorization: Bearer`.
//! Only `chat`/`vision`/`embed` are implemented — Azure also offers
//! Whisper/TTS/DALL-E deployments, unimplemented here (§24 Limitations).

use std::collections::BTreeMap;

use serde_json::json;

use crate::value::Value;

use super::artifact::{self, ImageSource};
use super::openai_shape;
use super::transport::{send_json_with_retry, Transport};
use super::{resolve_f64, resolve_i64, Invocation, Provider, ProviderError};

pub const DEFAULT_API_VERSION: &str = "2024-06-01";

pub struct AzureOpenAiProvider {
    capability: String,
    base_url: String,
    /// Reuses the manifest's `model` slot — for Azure this is the
    /// *deployment name*, which is what actually selects the underlying
    /// model server-side; the request body never repeats a `model` field.
    deployment: String,
    api_key: String,
    api_version: String,
    default_params: BTreeMap<String, Value>,
    transport: Box<dyn Transport>,
}

impl AzureOpenAiProvider {
    #[allow(clippy::too_many_arguments)]
    pub fn with_transport(
        capability: impl Into<String>,
        base_url: impl Into<String>,
        deployment: impl Into<String>,
        api_key: impl Into<String>,
        api_version: impl Into<String>,
        default_params: BTreeMap<String, Value>,
        transport: Box<dyn Transport>,
    ) -> Self {
        AzureOpenAiProvider {
            capability: capability.into(),
            base_url: base_url.into().trim_end_matches('/').to_string(),
            deployment: deployment.into(),
            api_key: api_key.into(),
            api_version: api_version.into(),
            default_params,
            transport,
        }
    }

    fn headers(&self) -> Vec<(String, String)> {
        vec![
            ("Content-Type".to_string(), "application/json".to_string()),
            ("api-key".to_string(), self.api_key.clone()),
        ]
    }

    fn url(&self, resource: &str) -> String {
        format!(
            "{}/openai/deployments/{}/{resource}?api-version={}",
            self.base_url, self.deployment, self.api_version
        )
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
        let mut messages = openai_shape::build_messages(&request.messages);
        if let Some(image_url) = image_url {
            openai_shape::attach_image(&mut messages, image_url);
        }

        let mut body = json!({"messages": messages});
        if let Some(t) = resolve_f64(&request.args, &self.default_params, "temperature") {
            body["temperature"] = json!(t);
        }
        if let Some(m) = resolve_i64(&request.args, &self.default_params, "max_tokens") {
            body["max_tokens"] = json!(m);
        }
        if let Some(t) = resolve_f64(&request.args, &self.default_params, "top_p") {
            body["top_p"] = json!(t);
        }

        let url = self.url("chat/completions");
        let resp = send_json_with_retry(self.transport.as_ref(), &url, &self.headers(), &body)?;
        openai_shape::parse_chat_response(&resp)
    }

    fn embed(&self, request: &Invocation) -> Result<Value, ProviderError> {
        let input = request
            .messages
            .first()
            .map(|m| m.text.as_str())
            .unwrap_or("");
        let body = json!({"input": input});

        let url = self.url("embeddings");
        let resp = send_json_with_retry(self.transport.as_ref(), &url, &self.headers(), &body)?;
        openai_shape::parse_embed_response(&resp)
    }
}

impl Provider for AzureOpenAiProvider {
    fn id(&self) -> &str {
        "azure_openai"
    }

    fn supports(&self, capability: &str) -> bool {
        capability == self.capability
    }

    fn invoke(&self, capability: &str, request: &Invocation) -> Result<Value, ProviderError> {
        match capability {
            "chat" => self.chat(request),
            "vision" => self.vision(request),
            "embed" => self.embed(request),
            other => Err(ProviderError::UnsupportedCapability(other.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::transport::ScriptedTransport;
    use super::super::Message;
    use super::*;

    fn provider(transport: ScriptedTransport) -> AzureOpenAiProvider {
        AzureOpenAiProvider::with_transport(
            "chat",
            "https://my-resource.openai.azure.com",
            "my-gpt4o-deployment",
            "test-key",
            "2024-06-01",
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
    fn url_includes_deployment_path_and_api_version() {
        let p = provider(ScriptedTransport::new(vec![]));
        assert_eq!(
            p.url("chat/completions"),
            "https://my-resource.openai.azure.com/openai/deployments/my-gpt4o-deployment/chat/completions?api-version=2024-06-01"
        );
    }

    #[test]
    fn chat_happy_path_returns_text() {
        let transport = ScriptedTransport::new(vec![ScriptedTransport::ok(
            200,
            json!({"choices": [{"message": {"content": "hi"}, "finish_reason": "stop"}]}),
        )]);
        let p = provider(transport);
        let result = p.invoke("chat", &invocation()).unwrap();
        assert_eq!(result, Value::Text("hi".to_string()));
    }

    #[test]
    fn content_filter_finish_reason_is_refused() {
        let transport = ScriptedTransport::new(vec![ScriptedTransport::ok(
            200,
            json!({"choices": [{"message": {"content": ""}, "finish_reason": "content_filter"}]}),
        )]);
        let p = provider(transport);
        let err = p.invoke("chat", &invocation()).unwrap_err();
        assert_eq!(
            err,
            ProviderError::Refused("content filtered by provider".to_string())
        );
    }

    #[test]
    fn embed_happy_path_returns_list() {
        let transport = ScriptedTransport::new(vec![ScriptedTransport::ok(
            200,
            json!({"data": [{"embedding": [0.1, 0.2]}]}),
        )]);
        let p = provider(transport);
        let result = p.invoke("embed", &invocation()).unwrap();
        assert_eq!(
            result,
            Value::List(vec![Value::Float(0.1), Value::Float(0.2)])
        );
    }

    #[test]
    fn transcribe_is_unsupported() {
        let transport = ScriptedTransport::new(vec![]);
        let p = provider(transport);
        let err = p.invoke("transcribe", &invocation()).unwrap_err();
        assert_eq!(
            err,
            ProviderError::UnsupportedCapability("transcribe".to_string())
        );
    }

    #[test]
    fn supports_only_its_declared_capability() {
        let transport = ScriptedTransport::new(vec![]);
        let p = provider(transport);
        assert!(p.supports("chat"));
        assert!(!p.supports("embed"));
    }
}
