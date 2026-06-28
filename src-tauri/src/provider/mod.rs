pub mod anthropic;
pub mod gemini;
pub mod openai;
#[allow(unused_imports)]
pub use anthropic::AnthropicAdapter;
#[allow(unused_imports)]
pub use gemini::GeminiAdapter;
pub use openai::OpenAIAdapter;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use log::warn;
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::stream::StreamEvent;

const MAX_RETRIES: u32 = 3;
const BASE_DELAY_MS: u64 = 1000;

/// Attach a Bearer token only when non-empty.
///
/// Keyless local providers (Ollama, llama.cpp, LM Studio) reject — or at best
/// ignore — an empty `Authorization: Bearer ` header. Omitting it entirely is
/// the clean keyless path shared by the chat adapter and `list_models`.
pub fn apply_bearer_auth(
    req: reqwest::RequestBuilder,
    api_key: &str,
) -> reqwest::RequestBuilder {
    if api_key.is_empty() {
        req
    } else {
        req.header("Authorization", format!("Bearer {api_key}"))
    }
}

pub(crate) async fn send_with_retry(
    req: reqwest::RequestBuilder,
) -> Result<reqwest::Response, AppError> {
    let mut attempt = 0u32;

    loop {
        // Clone the request builder for each attempt (it borrows the body bytes)
        let request = match req.try_clone() {
            Some(r) => r,
            None => return Err(AppError::Http(
                "request body is not cloneable".into(),
                None,
            )),
        };

        match request.send().await {
            Ok(response) => {
                let status = response.status();
                if status.is_success() {
                    return Ok(response);
                }

                let code = status.as_u16();

                // Read headers before consuming the body with .text()
                let retry_after_header = response
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok());

                let body_text = response.text().await.unwrap_or_default();

                if code == 429 {
                    let delay = retry_after_header
                        .map(Duration::from_secs)
                        .unwrap_or_else(|| backoff_delay(attempt));
                    let err = AppError::RateLimited(body_text.clone(), retry_after_header);
                    attempt += 1;
                    if attempt >= MAX_RETRIES {
                        return Err(err);
                    }
                    warn!(
                        "Rate limited (429), retrying in {:.1}s (attempt {}/{})",
                        delay.as_secs_f64(), attempt, MAX_RETRIES
                    );
                    tokio::time::sleep(delay).await;
                } else if code >= 500 {
                    let delay = backoff_delay(attempt);
                    attempt += 1;
                    if attempt >= MAX_RETRIES {
                        return Err(AppError::Provider(body_text, Some(code)));
                    }
                    warn!(
                        "Provider error ({code}), retrying in {:.1}s (attempt {}/{})",
                        delay.as_secs_f64(), attempt, MAX_RETRIES
                    );
                    tokio::time::sleep(delay).await;
                } else {
                    return Err(AppError::Provider(body_text, Some(code)));
                }
            }
            Err(e) => {
                let code = e.status().map(|s| s.as_u16());
                let msg = e.to_string();
                if code == Some(429) {
                    let delay = backoff_delay(attempt);
                    attempt += 1;
                    if attempt >= MAX_RETRIES {
                        return Err(AppError::RateLimited(msg, None));
                    }
                    warn!(
                        "Rate limited (429 connection), retrying in {:.1}s (attempt {}/{})",
                        delay.as_secs_f64(), attempt, MAX_RETRIES
                    );
                    tokio::time::sleep(delay).await;
                } else if code.map_or(true, |c| c >= 500) {
                    let delay = backoff_delay(attempt);
                    attempt += 1;
                    if attempt >= MAX_RETRIES {
                        return Err(AppError::Http(msg, code));
                    }
                    warn!(
                        "HTTP error ({code:?}), retrying in {:.1}s (attempt {}/{})",
                        delay.as_secs_f64(), attempt, MAX_RETRIES
                    );
                    tokio::time::sleep(delay).await;
                } else {
                    return Err(AppError::Http(msg, code));
                }
            }
        }
    }
}

fn backoff_delay(attempt: u32) -> Duration {
    Duration::from_millis(BASE_DELAY_MS * 2u64.pow(attempt))
}

/// A tool call requested by the model, as returned by the API.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: ToolCallFunction,
}

/// The function specification within a tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallFunction {
    pub name: String,
    pub arguments: String,
}

/// Multimodal message content — either plain text or an array of content parts.
///
/// Supports OpenAI vision format where Parts contain `[{type: "text", text: "..."},
/// {type: "image_url", image_url: {url: "data:image/..."}}]`.
/// Anthropic format is handled by adapter-level transformation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    /// Plain text content (legacy format, backward compatible)
    Text(String),
    /// Array of content parts (multimodal: text + images)
    Parts(Vec<ContentPart>),
}

impl MessageContent {
    /// Extract text content as a single string.
    /// For Text variant, returns the string directly.
    /// For Parts variant, concatenates all text parts.
    pub fn as_text(&self) -> Option<String> {
        match self {
            MessageContent::Text(s) => Some(s.clone()),
            MessageContent::Parts(parts) => {
                let texts: Vec<&str> = parts
                    .iter()
                    .filter_map(|p| match p {
                        ContentPart::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect();
                if texts.is_empty() {
                    None
                } else {
                    Some(texts.join("\n"))
                }
            }
        }
    }

    /// Total character length of all text content (for validation).
    pub fn text_len(&self) -> usize {
        match self {
            MessageContent::Text(s) => s.len(),
            MessageContent::Parts(parts) => parts
                .iter()
                .map(|p| match p {
                    ContentPart::Text { text } => text.len(),
                    ContentPart::ImageUrl { image_url } => image_url.url.len(),
                })
                .sum(),
        }
    }

    /// Check if content is empty (no text, no images).
    pub fn is_empty(&self) -> bool {
        match self {
            MessageContent::Text(s) => s.is_empty(),
            MessageContent::Parts(parts) => parts.is_empty(),
        }
    }

    /// Check if content contains any image parts.
    pub fn has_images(&self) -> bool {
        matches!(
            self,
            MessageContent::Parts(parts)
                if parts.iter().any(|p| matches!(p, ContentPart::ImageUrl { .. }))
        )
    }
}

impl From<String> for MessageContent {
    fn from(s: String) -> Self {
        MessageContent::Text(s)
    }
}

impl From<&str> for MessageContent {
    fn from(s: &str) -> Self {
        MessageContent::Text(s.to_string())
    }
}

/// Individual content part within a multimodal message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentPart {
    /// Text content
    #[serde(rename = "text")]
    Text { text: String },
    /// Image content (URL or base64 data URI)
    #[serde(rename = "image_url")]
    ImageUrl { image_url: ImageUrlSource },
}

/// Image source for content parts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageUrlSource {
    /// Data URI (e.g., "data:image/png;base64,...") or HTTP URL
    pub url: String,
    /// Optional detail level for vision models ("low", "high", "auto")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Parse a data URL and extract the media type and base64 data.
///
/// Expected format: `data:image/TYPE;base64,DATA`
/// Returns `Some((media_type, base64_data))` or `None` if not a valid data URL.
pub fn parse_data_url(url: &str) -> Option<(String, String)> {
    let url = url.strip_prefix("data:")?;
    let comma_idx = url.find(',')?;
    let meta = &url[..comma_idx];
    let data = &url[comma_idx + 1..];
    let media_type = meta.split(';').next()?;
    if !media_type.starts_with("image/") {
        return None;
    }
    Some((media_type.to_string(), data.to_string()))
}

/// A single chat message with a role and optional content.
///
/// This struct is used in BOTH directions, which need different field casing:
/// - **Deserialize** from the frontend `ChatRequest` → camelCase (`toolCalls`,
///   `toolCallId`), matching what `buildRequest` in `loop.ts` emits.
/// - **Serialize** into the OpenAI request body → snake_case (`tool_calls`,
///   `tool_call_id`), which the OpenAI-compatible API requires. Emitting
///   `toolCalls` makes the API ignore the assistant's tool calls, which then
///   orphans the following `tool` message → HTTP 400 "Messages with role 'tool'
///   must be a response to a preceding message with 'tool_calls'".
///
/// `content` supports multimodal input: either a plain string or an array
/// of content parts (text + images) for vision-capable models.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all(serialize = "snake_case", deserialize = "camelCase"))]
pub struct ChatMessage {
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<MessageContent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Normalize a user-supplied base URL so adapters can safely append versioned
/// paths. Strips trailing `/v1` and trailing `/` so the adapter always controls
/// the API version suffix, avoiding accidental double `/v1/v1/chat/completions`.
pub fn normalize_api_base(base_url: &str) -> &str {
    let trimmed = base_url.trim_end_matches('/');
    if let Some(stripped) = trimmed.strip_suffix("/v1") {
        stripped
    } else {
        trimmed
    }
}

/// Adapter trait for LLM providers that support streaming chat completions.
pub trait ProviderAdapter: Send + Sync {
    /// Stream a chat completion from the provider.
    ///
    /// Returns a [`Stream`](futures::Stream) of [`StreamEvent`] items, or an [`AppError`]
    /// if the initial request fails.
    ///
    /// `cancel_token` — when set to `true`, the adapter should stop reading from
    /// the upstream API and close the stream as soon as practical.
    async fn stream_chat(
        &self,
        messages: &[ChatMessage],
        model: &str,
        system_prompt: Option<&str>,
        tools: Option<&[serde_json::Value]>,
        cancel_token: Arc<AtomicBool>,
    ) -> AppResult<impl futures::Stream<Item = AppResult<StreamEvent>> + Send>;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A ChatMessage must DESERIALIZE from the frontend's camelCase
    /// (`toolCalls`, `toolCallId`) but SERIALIZE to the OpenAI body's snake_case
    /// (`tool_calls`, `tool_call_id`). Getting this wrong orphans the `tool`
    /// message and the API returns HTTP 400.
    #[test]
    fn chat_message_camel_in_snake_out_with_preserved_ids() {
        // Assistant message exactly as `buildRequest` (loop.ts) emits it.
        let assistant_json = r#"{
            "role": "assistant",
            "content": "",
            "toolCalls": [
                {"id": "call_abc", "type": "function",
                 "function": {"name": "ssh_exec", "arguments": "{}"}}
            ]
        }"#;
        let assistant: ChatMessage = serde_json::from_str(assistant_json).unwrap();
        let tcs = assistant.tool_calls.as_ref().expect("toolCalls must deserialize");
        assert_eq!(tcs[0].id, "call_abc");

        let out = serde_json::to_value(&assistant).unwrap();
        assert!(out.get("tool_calls").is_some(), "must serialize as snake_case tool_calls");
        assert!(out.get("toolCalls").is_none(), "must NOT serialize as camelCase");

        // Tool result message, as emitted by buildRequest.
        let tool_json = r#"{
            "role": "tool",
            "content": "ok",
            "toolCallId": "call_abc",
            "name": "ssh_exec"
        }"#;
        let tool: ChatMessage = serde_json::from_str(tool_json).unwrap();
        assert_eq!(
            tool.tool_call_id.as_deref(),
            Some("call_abc"),
            "toolCallId must deserialize so it matches the assistant's tool_call id"
        );

        let out = serde_json::to_value(&tool).unwrap();
        assert_eq!(out.get("tool_call_id").and_then(|v| v.as_str()), Some("call_abc"));
        assert!(out.get("toolCallId").is_none());
    }
}

#[cfg(test)]
mod bearer_tests {
    use super::apply_bearer_auth;

    #[test]
    fn bearer_omitted_when_key_empty() {
        let client = reqwest::Client::new();
        let req = apply_bearer_auth(client.get("http://localhost:11434/v1/models"), "")
            .build()
            .unwrap();
        assert!(req.headers().get("authorization").is_none());
    }

    #[test]
    fn bearer_present_when_key_set() {
        let client = reqwest::Client::new();
        let req = apply_bearer_auth(client.get("http://api.example.com/models"), "sk-test")
            .build()
            .unwrap();
        assert_eq!(
            req.headers().get("authorization").unwrap(),
            "Bearer sk-test"
        );
    }
}
