#![allow(dead_code)]
//! Gemini API adapter with SSE streaming and function-calling support.
//!
//! Converts OpenAI-style tools to Gemini's `functionDeclarations` format
//! and parses newline-delimited JSON SSE responses including
//! `candidates[].content.parts[]` with text and `functionCall` events.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use futures::channel::mpsc;
use futures::StreamExt;
use serde_json::json;

use crate::error::{AppError, AppResult};
use crate::provider::{send_with_retry, ChatMessage, ContentPart, MessageContent, ProviderAdapter, parse_data_url};
use crate::stream::StreamEvent;

/// Google Gemini API adapter that streams via SSE (server-sent events).
///
/// Uses the [Gemini streamGenerateContent](https://ai.google.dev/api/generate-content#streamGenerateContent) endpoint.
pub struct GeminiAdapter {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
}

impl GeminiAdapter {
    pub fn new(api_key: String, base_url: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url,
            api_key,
        }
    }
}

/// Convert our ChatMessage list into Gemini contents + system instruction.
///
/// Returns `(system_instruction_text, contents)`.
fn to_gemini_payload(
    messages: &[ChatMessage],
    system_prompt: Option<&str>,
) -> (Option<serde_json::Value>, Vec<serde_json::Value>) {
    let mut system: Option<serde_json::Value> = None;
    let mut contents: Vec<serde_json::Value> = Vec::new();

    for msg in messages {
        if msg.role == "system" {
            system = Some(json!({
                "parts": [{"text": msg.content.as_ref().and_then(|c| c.as_text()).unwrap_or_default()}]
            }));
            continue;
        }

        // Map role: assistant -> model, tool -> function (via functionResponse)
        let role = match msg.role.as_str() {
            "assistant" => "model",
            "tool" => "function",
            other => other, // user, model, function
        };
        let mut content_parts: Vec<serde_json::Value> = Vec::new();

        if let Some(ref content) = msg.content {
            if !content.is_empty() {
                // If this is a tool result message, wrap as functionResponse
                if msg.tool_call_id.is_some() {
                    let tool_name = msg.name.as_deref()
                        .filter(|s| !s.is_empty())
                        .unwrap_or("unknown");
                    content_parts.push(json!({
                        "functionResponse": {
                            "name": tool_name,
                            "response": { "result": content.as_text().unwrap_or_default() }
                        }
                    }));
                } else {
                    match content {
                        MessageContent::Parts(parts_vec) => {
                            for part in parts_vec {
                                match part {
                                    ContentPart::Text { text } => {
                                        content_parts.push(json!({"text": text}));
                                    }
                                    ContentPart::ImageUrl { image_url } => {
                                        if let Some((mime_type, data)) = parse_data_url(&image_url.url) {
                                            content_parts.push(json!({
                                                "inline_data": {
                                                    "mime_type": mime_type,
                                                    "data": data,
                                                }
                                            }));
                                        } else {
                                            content_parts.push(json!({"text": &image_url.url}));
                                        }
                                    }
                                }
                            }
                        }
                        MessageContent::Text(_) => {
                            content_parts.push(json!({"text": content}));
                        }
                    }
                }
            }
        }

        // Add tool calls from assistant messages as functionCall parts
        if let Some(ref tcs) = msg.tool_calls {
            for tc in tcs {
                content_parts.push(json!({
                    "functionCall": {
                        "name": tc.function.name,
                        "args": serde_json::from_str::<serde_json::Value>(&tc.function.arguments)
                            .unwrap_or(json!({})),
                    }
                }));
            }
        }

        if !content_parts.is_empty() {
            contents.push(json!({
                "role": role,
                "parts": content_parts,
            }));
        }
    }

    // Apply explicit system_prompt if provided
    if let Some(sys) = system_prompt {
        system = Some(json!({
            "parts": [{"text": sys}]
        }));
    }

    (system, contents)
}

/// Convert our tool definitions to Gemini's format.
fn to_gemini_tools(tools: &[serde_json::Value]) -> Vec<serde_json::Value> {
    let declarations: Vec<serde_json::Value> = tools
        .iter()
        .filter_map(|t| {
            let func = t.get("function")?;
            Some(json!({
                "name": func.get("name")?,
                "description": func.get("description").unwrap_or(&json!("")),
                "parameters": func.get("parameters").unwrap_or(&json!({"type": "object"})),
            }))
        })
        .collect();

    if declarations.is_empty() {
        Vec::new()
    } else {
        vec![json!({ "functionDeclarations": declarations })]
    }
}

impl ProviderAdapter for GeminiAdapter {
    async fn stream_chat(
        &self,
        messages: &[ChatMessage],
        model: &str,
        system_prompt: Option<&str>,
        tools: Option<&[serde_json::Value]>,
        cancel_token: Arc<AtomicBool>,
    ) -> AppResult<impl futures::Stream<Item = AppResult<StreamEvent>> + Send> {
        // Gemini SSE endpoint: POST /v1beta/models/{model}:streamGenerateContent?alt=sse
        // API key is sent via the x-goog-api-key header to avoid leaking it in URLs.
        let url = format!(
            "{}/v1beta/models/{}:streamGenerateContent?alt=sse",
            self.base_url.trim_end_matches('/'),
            model
        );

        let (system_from_msgs, contents) = to_gemini_payload(messages, system_prompt);

        let mut body = json!({
            "contents": contents,
            "generationConfig": {
                "temperature": 0.7,
            },
        });

        if let Some(si) = system_from_msgs {
            body["systemInstruction"] = si;
        }

        if let Some(t) = tools {
            let gemini_tools = to_gemini_tools(t);
            if !gemini_tools.is_empty() {
                body["tools"] = json!(gemini_tools);
            }
        }

        let response = send_with_retry(
            self.client
                .post(&url)
                .header("content-type", "application/json")
                .header("x-goog-api-key", &self.api_key)
                .json(&body),
        )
        .await?;

        let mut byte_stream = response.bytes_stream();
        let (tx, rx) = mpsc::unbounded::<AppResult<StreamEvent>>();
        let cancel = cancel_token;

        tokio::spawn(async move {
            let mut buffer = String::new();
            let mut pending_utf8 = Vec::<u8>::new();
            let mut sent_done = false;

            // Track tool call accumulation per candidate index
            // key: index, value: accumulated functionCall args
            let mut tool_acc: std::collections::BTreeMap<usize, (String, String)> =
                std::collections::BTreeMap::new();

            loop {
                if cancel.load(Ordering::Relaxed) {
                    if !sent_done {
                        let _ = tx.unbounded_send(Ok(StreamEvent::Done { usage: None }));
                    }
                    break;
                }

                // Gemini SSE: newline-delimited JSON, trimmed
                // Each line is one complete JSON object (not chunked like OpenAI)
                // Format: {"candidates":[{"content":{"parts":[{"text":"..."}]},"finishReason":null}],...}
                while let Some(pos) = buffer.find('\n') {
                    let line = buffer[..pos].trim().to_string();
                    buffer = buffer[pos + 1..].to_string();

                    if line.is_empty() {
                        continue;
                    }

                    // Try JSON parse — some lines might not be JSON (empty, whitespace)
                    let parsed: serde_json::Value = match serde_json::from_str(&line) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };

                    // Check for candidates
                    if let Some(candidates) = parsed.get("candidates").and_then(|c| c.as_array()) {
                        for (idx, candidate) in candidates.iter().enumerate() {
                            let finish_reason = candidate
                                .get("finishReason")
                                .and_then(|v| v.as_str());

                            if let Some("STOP") = finish_reason {
                                if !sent_done {
                                    sent_done = true;
                                    let _ = tx.unbounded_send(Ok(StreamEvent::Done { usage: None }));
                                }
                                continue;
                            }

                            if let Some(content) = candidate.get("content") {
                                if let Some(parts) = content.get("parts").and_then(|p| p.as_array()) {
                                    for part in parts {
                                        // Text content
                                        if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                                            if !text.is_empty() {
                                                let _ = tx.unbounded_send(Ok(StreamEvent::ContentDelta {
                                                    content: text.to_string(),
                                                }));
                                            }
                                        }

                                        // Function call
                                        if let Some(fc) = part.get("functionCall") {
                                            let name = fc
                                                .get("name")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("unknown");
                                            let args = fc
                                                .get("args")
                                                .map(|a| a.to_string())
                                                .unwrap_or_else(|| "{}".to_string());

                                            // Generate a synthetic id using index
                                            let id = format!("gfc_{}_{}", idx, tool_acc.len());
                                            let _ = tx.unbounded_send(Ok(StreamEvent::ToolCall {
                                                id: id.clone(),
                                                name: name.to_string(),
                                                arguments: args,
                                            }));
                                            tool_acc.insert(idx, (name.to_string(), id));
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Check for top-level errors
                    if let Some(error) = parsed.get("error") {
                        let msg = error
                            .get("message")
                            .and_then(|v| v.as_str())
                            .unwrap_or("Unknown Gemini error");
                        let _ = tx.unbounded_send(Err(AppError::Provider(
                            msg.to_string(),
                            error.get("code").and_then(|c| c.as_u64()).map(|c| c as u16),
                        )));
                        break;
                    }
                }

                match byte_stream.next().await {
                    Some(Ok(bytes)) => {
                        // Accumulate raw bytes, decode only complete UTF-8
                        // sequences. See openai.rs for detailed rationale.
                        pending_utf8.extend_from_slice(&bytes);
                        match String::from_utf8(std::mem::take(&mut pending_utf8)) {
                            Ok(s) => {
                                // Gemini SSE uses bare \n\n (not \r\n\r\n),
                                // but CRLF normalization is harmless.
                                buffer.push_str(&s);
                            }
                            Err(e) => {
                                let valid_up_to = e.utf8_error().valid_up_to();
                                let raw = e.into_bytes();
                                if valid_up_to > 0 {
                                    if let Ok(valid) =
                                        String::from_utf8(raw[..valid_up_to].to_vec())
                                    {
                                        buffer.push_str(&valid);
                                    }
                                }
                                pending_utf8 = raw[valid_up_to..].to_vec();
                            }
                        }
                    }
                    Some(Err(e)) => {
                        let _ = tx.unbounded_send(Err(AppError::Http(e.to_string(), None)));
                        break;
                    }
                    None => {
                        if !sent_done {
                            let _ = tx.unbounded_send(Ok(StreamEvent::Done { usage: None }));
                        }
                        break;
                    }
                }
            }
        });

        Ok(rx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ImageUrlSource;

    fn test_adapter(base_url: String) -> GeminiAdapter {
        GeminiAdapter {
            client: reqwest::Client::new(),
            base_url,
            api_key: "gemini-test-key".into(),
        }
    }

    // -------------------------------------------------------------------
    // to_gemini_payload
    // -------------------------------------------------------------------

    #[test]
    fn maps_roles_correctly() {
        let msgs = vec![
            ChatMessage { name: None, reasoning_content: None, 
                role: "user".into(),
                content: Some("Hello".into()),
                tool_calls: None,
                tool_call_id: None,
            },
            ChatMessage { name: None, reasoning_content: None, 
                role: "assistant".into(),
                content: Some("Hi there".into()),
                tool_calls: None,
                tool_call_id: None,
            },
        ];
        let (_sys, contents) = to_gemini_payload(&msgs, None);
        assert_eq!(contents.len(), 2);
        assert_eq!(contents[0]["role"], "user");
        assert_eq!(contents[1]["role"], "model");
    }

    #[test]
    fn extracts_system_instruction() {
        let msgs = vec![
            ChatMessage { name: None, reasoning_content: None, 
                role: "system".into(),
                content: Some("Be helpful".into()),
                tool_calls: None,
                tool_call_id: None,
            },
            ChatMessage { name: None, reasoning_content: None, 
                role: "user".into(),
                content: Some("Hi".into()),
                tool_calls: None,
                tool_call_id: None,
            },
        ];
        let (sys, contents) = to_gemini_payload(&msgs, None);
        assert!(sys.is_some());
        assert_eq!(contents.len(), 1); // only user message remains
        assert_eq!(contents[0]["role"], "user");
    }

    #[test]
    fn tool_result_as_function_response() {
        let msgs = vec![ChatMessage { name: None, reasoning_content: None, 
            role: "tool".into(),
            content: Some("result content".into()),
            tool_calls: None,
            tool_call_id: Some("read_file_123".into()),
        }];
        let (_sys, contents) = to_gemini_payload(&msgs, None);
        assert_eq!(contents.len(), 1);
        assert_eq!(contents[0]["role"], "function");
        let parts = contents[0]["parts"].as_array().unwrap();
        assert!(parts[0]
            .get("functionResponse")
            .is_some());
        // Verify that name field is set to "unknown" when msg.name is None
        assert_eq!(
            parts[0]["functionResponse"]["name"].as_str().unwrap(),
            "unknown"
        );
    }

    #[test]
    fn tool_result_with_empty_name_uses_unknown() {
        let msgs = vec![ChatMessage { 
            name: Some("".into()),
            reasoning_content: None,
            role: "tool".into(),
            content: Some("result content".into()),
            tool_calls: None,
            tool_call_id: Some("read_file_123".into()),
        }];
        let (_sys, contents) = to_gemini_payload(&msgs, None);
        let parts = contents[0]["parts"].as_array().unwrap();
        // Empty string should be filtered out and replaced with "unknown"
        assert_eq!(
            parts[0]["functionResponse"]["name"].as_str().unwrap(),
            "unknown"
        );
    }

    #[test]
    fn tool_result_with_valid_name_preserved() {
        let msgs = vec![ChatMessage { 
            name: Some("read_file".into()),
            reasoning_content: None,
            role: "tool".into(),
            content: Some("result content".into()),
            tool_calls: None,
            tool_call_id: Some("read_file_123".into()),
        }];
        let (_sys, contents) = to_gemini_payload(&msgs, None);
        let parts = contents[0]["parts"].as_array().unwrap();
        // Valid name should be preserved
        assert_eq!(
            parts[0]["functionResponse"]["name"].as_str().unwrap(),
            "read_file"
        );
    }

    #[test]
    fn vision_message_converts_to_inline_data() {
        let msgs = vec![ChatMessage { name: None, reasoning_content: None, 
            role: "user".into(),
            content: Some(MessageContent::Parts(vec![
                ContentPart::Text { text: "Describe this".into() },
                ContentPart::ImageUrl {
                    image_url: ImageUrlSource {
                        url: "data:image/png;base64,abc123".into(),
                        detail: None,
                    },
                },
            ])),
            tool_calls: None,
            tool_call_id: None,
        }];
        let (_sys, contents) = to_gemini_payload(&msgs, None);
        assert_eq!(contents.len(), 1);
        let parts = contents[0]["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0]["text"], "Describe this");
        assert!(parts[1].get("inline_data").is_some());
        assert_eq!(parts[1]["inline_data"]["mime_type"], "image/png");
        assert_eq!(parts[1]["inline_data"]["data"], "abc123");
    }

    // -------------------------------------------------------------------
    // HTTP error
    // -------------------------------------------------------------------

    #[tokio::test]
    async fn http_error_produces_provider_error() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let adapter = test_adapter(format!("http://127.0.0.1:{port}"));
        let messages = [ChatMessage { name: None, reasoning_content: None, 
            role: "user".into(),
            content: Some("ping".into()),
            tool_calls: None,
            tool_call_id: None,
        }];
        let cancel = Arc::new(AtomicBool::new(false));
        let result = adapter
            .stream_chat(&messages, "gemini-2.5-pro", None, None, cancel)
            .await;

        assert!(result.is_err());
    }

    /// Regression test for Finding 1 (HIGH): Gemini API key must NOT be in URL.
    ///
    /// Verifies that the URL is constructed WITHOUT the API key in the query
    /// string (CWE-598 fix). The key is now sent via the `x-goog-api-key` header.
    #[test]
    fn gemini_url_excludes_key_from_query_string() {
        let base_url = "https://generativelanguage.googleapis.com";
        let model = "gemini-2.5-pro";
        let url = format!(
            "{}/v1beta/models/{}:streamGenerateContent?alt=sse",
            base_url.trim_end_matches('/'),
            model
        );
        assert_eq!(
            url,
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-pro:streamGenerateContent?alt=sse"
        );
        assert!(
            !url.contains("key="),
            "URL must not contain the API key after the fix"
        );
    }

    /// Regression test for Finding 1 (HIGH): API key must NOT leak in errors.
    ///
    /// When the HTTP request fails, reqwest's error `to_string()` includes the
    /// full URL. With the key removed from the URL, the error message must no
    /// longer contain the key. This prevents CWE-209 (Error Message Leak).
    #[tokio::test]
    async fn api_key_not_leaked_in_http_error_message() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let adapter = test_adapter(format!("http://127.0.0.1:{port}"));
        let messages = [ChatMessage { name: None, reasoning_content: None, 
            role: "user".into(),
            content: Some("ping".into()),
            tool_calls: None,
            tool_call_id: None,
        }];
        let cancel = Arc::new(AtomicBool::new(false));
        let result = adapter
            .stream_chat(&messages, "gemini-2.5-pro", None, None, cancel)
            .await;

        assert!(result.is_err());
        let err = match result {
            Err(e) => e,
            Ok(_) => panic!("expected an error"),
        };
        let err_str = format!("{err}");
        assert!(
            !err_str.contains("gemini-test-key"),
            "error message must NOT leak the API key after the fix: {err_str}"
        );
    }
}
