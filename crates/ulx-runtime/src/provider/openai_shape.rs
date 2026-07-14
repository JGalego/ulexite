//! Request/response shape shared by every vendor speaking the OpenAI Chat
//! Completions / Embeddings JSON format — today `openai_compat` (OpenAI,
//! Groq, vLLM, LM Studio, ...) and `azure_openai`. Only the URL and auth
//! differ between them (Azure's deployment-based path, `api-version`
//! query param, and `api-key` header vs. a plain `base_url` and
//! `Authorization: Bearer`); this module is the part that doesn't.

use serde_json::json;

use crate::value::Value;

use super::{Message, ProviderError};

pub(super) fn openai_role(role: &str) -> &str {
    match role {
        "system" => "system",
        "assistant" => "assistant",
        _ => "user",
    }
}

pub(super) fn build_messages(messages: &[Message]) -> Vec<serde_json::Value> {
    messages
        .iter()
        .map(|m| json!({"role": openai_role(&m.role), "content": m.text}))
        .collect()
}

/// Attaches an image to the last message's content as an `image_url`
/// block (or creates a new user message, if there are none yet).
pub(super) fn attach_image(messages: &mut Vec<serde_json::Value>, image_url: String) {
    let image_block = json!({"type": "image_url", "image_url": {"url": image_url}});
    if let Some(last) = messages.last_mut() {
        let role = last.get("role").cloned().unwrap_or_else(|| json!("user"));
        let text = last.get("content").cloned().unwrap_or_else(|| json!(""));
        *last = json!({"role": role, "content": [{"type": "text", "text": text}, image_block]});
    } else {
        messages.push(json!({"role": "user", "content": [image_block]}));
    }
}

pub(super) fn parse_chat_response(resp: &serde_json::Value) -> Result<Value, ProviderError> {
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

pub(super) fn parse_embed_response(resp: &serde_json::Value) -> Result<Value, ProviderError> {
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
