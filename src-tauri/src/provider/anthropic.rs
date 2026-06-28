#![allow(dead_code)]
//! Anthropic Messages API adapter with SSE streaming and tool-use support.
//!
//! Converts OpenAI-style tool definitions to Anthropic's `input_schema` format
//! and maps content-block events (`text_delta`, `tool_use`, `input_json_delta`)
//! back to the unified [`StreamEvent`] model.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use futures::channel::mpsc;
use futures::StreamExt;
use log::warn;
use serde_json::json;

use crate::error::{AppError, AppResult};
use crate::provider::{send_with_retry, ChatMessage, ContentPart, MessageContent, ProviderAdapter, parse_data_url};
use crate::stream::{StreamEvent, TokenUsage};

/// Anthropic Messages API adapter that streams via SSE.
///
/// Uses the [Messages streaming API](https://docs.anthropic.com/en/api/messages-streaming).
pub struct AnthropicAdapter {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
}

impl AnthropicAdapter {
    pub fn new(api_key: String, base_url: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url,
            api_key,
        }
    }
}

/// Convert our ChatMessage list into Anthropic-shaped messages.
///
/// Returns `(system_prompt, messages)` where `system_prompt` is extracted from
/// any message with `role: "system"`. Anthropic does not allow `system` inside
/// the `messages` array.
fn to_anthropic_messages(messages: &[ChatMessage]) -> (Option<String>, Vec<serde_json::Value>) {
    let mut system = None;
    let mut out = Vec::new();

    for msg in messages {
        if msg.role == "system" {
            system = msg.content.as_ref().and_then(|c| c.as_text());
            continue;
        }

        let mut entry = if let Some(ref content) = msg.content {
            match content {
                MessageContent::Parts(parts) => {
                    let blocks: Vec<serde_json::Value> = parts
                        .iter()
                        .map(|part| match part {
                            ContentPart::Text { text } => json!({
                                "type": "text",
                                "text": text,
                            }),
                            ContentPart::ImageUrl { image_url } => {
                                if let Some((media_type, data)) = parse_data_url(&image_url.url) {
                                    json!({
                                        "type": "image",
                                        "source": {
                                            "type": "base64",
                                            "media_type": media_type,
                                            "data": data,
                                        }
                                    })
                                } else {
                                    json!({
                                        "type": "text",
                                        "text": &image_url.url,
                                    })
                                }
                            }
                        })
                        .collect();
                    json!({"role": msg.role, "content": blocks})
                }
                MessageContent::Text(_) => {
                    json!({"role": msg.role, "content": content})
                }
            }
        } else {
            json!({"role": msg.role, "content": serde_json::Value::Null})
        };

        // Attach tool call metadata if present
        if let Some(ref tcs) = msg.tool_calls {
            let anthropic_tool_calls: Vec<serde_json::Value> = tcs
                .iter()
                .map(|tc| {
                    json!({
                        "type": "tool_use",
                        "id": tc.id,
                        "name": tc.function.name,
                        "input": serde_json::from_str::<serde_json::Value>(&tc.function.arguments)
                            .unwrap_or(json!({})),
                    })
                })
                .collect();
            entry["content"] = json!(anthropic_tool_calls);
        }

        // Tool results in Anthropic use content blocks with tool_result type
        if msg.role == "tool" || (msg.role == "user" && msg.tool_call_id.is_some()) {
            entry = json!({
                "role": "user",
                "content": [{
                    "type": "tool_result",
                    "tool_use_id": msg.tool_call_id,
                    "content": msg.content.as_ref().and_then(|c| c.as_text()).unwrap_or_default(),
                }]
            });
        }

        out.push(entry);
    }

    (system, out)
}

/// Convert our tool definitions to Anthropic's format.
fn to_anthropic_tools(tools: &[serde_json::Value]) -> Vec<serde_json::Value> {
    tools
        .iter()
        .filter_map(|t| {
            let func = t.get("function")?;
            Some(json!({
                "name": func.get("name")?,
                "description": func.get("description").unwrap_or(&json!("")),
                "input_schema": func.get("parameters").unwrap_or(&json!({"type": "object"})),
            }))
        })
        .collect()
}

/// --- SSE event shapes (minimal representation) ---
#[derive(Debug, serde::Deserialize)]
struct AnthropicEvent {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(default)]
    delta: Option<AnthropicDelta>,
    #[serde(default)]
    content_block: Option<AnthropicContentBlock>,
    #[serde(default)]
    index: Option<usize>,
    #[serde(default)]
    message: Option<AnthropicMessage>,
}

#[derive(Debug, serde::Deserialize)]
struct AnthropicDelta {
    #[serde(rename = "type", default)]
    delta_type: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default, alias = "partial_json")]
    partial_json: Option<String>,
    #[serde(default)]
    stop_reason: Option<String>,
    #[serde(default)]
    usage: Option<AnthropicUsage>,
}

#[derive(Debug, serde::Deserialize)]
struct AnthropicContentBlock {
    #[serde(rename = "type")]
    block_type: String,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct AnthropicMessage {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    model: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct AnthropicUsage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
}

impl ProviderAdapter for AnthropicAdapter {
    async fn stream_chat(
        &self,
        messages: &[ChatMessage],
        model: &str,
        system_prompt: Option<&str>,
        tools: Option<&[serde_json::Value]>,
        cancel_token: Arc<AtomicBool>,
    ) -> AppResult<impl futures::Stream<Item = AppResult<StreamEvent>> + Send> {
        let url = format!(
            "{}/v1/messages",
            crate::provider::normalize_api_base(&self.base_url)
        );

        let (system_from_msgs, anthropic_messages) = to_anthropic_messages(messages);

        // System prompt: explicit arg takes priority, then extracted from messages
        let effective_system = system_prompt
            .map(|s| s.to_string())
            .or(system_from_msgs);

        let mut body = json!({
            "model": model,
            "messages": anthropic_messages,
            "stream": true,
            "max_tokens": 4096,
        });

        if let Some(sys) = effective_system {
            // system prompt as cached content block array — saves ~90% on repeated reads
            body["system"] = json!([{
                "type": "text",
                "text": sys,
                "cache_control": {"type": "ephemeral"}
            }]);
        }

        if let Some(t) = tools {
            let anthropic_tools = to_anthropic_tools(t);
            if !anthropic_tools.is_empty() {
                body["tools"] = json!(anthropic_tools);
            }
        }

        let response = send_with_retry(
            self.client
                .post(&url)
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", "2023-06-01")
                .header("content-type", "application/json")
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
            let mut last_usage: Option<TokenUsage> = None;
            // Track active tool_use content blocks: content_block_index -> (id, name)
            // Accumulate partial JSON arguments and emit on input_json_delta
            let mut tool_blocks: std::collections::BTreeMap<usize, (String, String, String)> =
                std::collections::BTreeMap::new();

            loop {
                if cancel.load(Ordering::Relaxed) {
                    if !sent_done {
                        let _ = tx.unbounded_send(Ok(StreamEvent::Done { usage: None }));
                    }
                    break;
                }

                // Extract complete SSE events (delimited by \n\n)
                while let Some(pos) = buffer.find("\n\n") {
                    let chunk = buffer[..pos].to_string();
                    buffer = buffer[pos + 2..].to_string();

                    let (events, usage) = parse_anthropic_sse(&chunk, &mut tool_blocks, &mut sent_done);
                    if let Some(u) = usage {
                        last_usage = Some(merge_usage(last_usage.take(), u));
                    }
                    for event in events {
                        let is_done = matches!(&event, Ok(StreamEvent::Done { .. }));
                        // Attach accumulated usage to the terminal Done event so the
                        // frontend can show token counts / cost (mirrors openai.rs).
                        let event = match event {
                            Ok(StreamEvent::Done { usage: None }) => {
                                Ok(StreamEvent::Done { usage: last_usage.clone() })
                            }
                            other => other,
                        };
                        if tx.unbounded_send(event).is_err() {
                            return; // receiver dropped
                        }
                        if is_done {
                            return;
                        }
                    }
                }

                match byte_stream.next().await {
                    Some(Ok(bytes)) => {
                        // Accumulate raw bytes, decode only complete UTF-8
                        // sequences. See openai.rs for detailed rationale.
                        pending_utf8.extend_from_slice(&bytes);
                        match String::from_utf8(std::mem::take(&mut pending_utf8)) {
                            Ok(s) => {
                                buffer.push_str(&s.replace("\r\n", "\n"));
                            }
                            Err(e) => {
                                let valid_up_to = e.utf8_error().valid_up_to();
                                let raw = e.into_bytes();
                                if valid_up_to > 0 {
                                    if let Ok(valid) =
                                        String::from_utf8(raw[..valid_up_to].to_vec())
                                    {
                                        buffer.push_str(&valid.replace("\r\n", "\n"));
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

/// Parse a single SSE event chunk from the Anthropic streaming API into
/// zero or more `StreamEvent` values.
fn parse_anthropic_sse(
    chunk: &str,
    tool_blocks: &mut std::collections::BTreeMap<usize, (String, String, String)>,
    sent_done: &mut bool,
) -> (Vec<AppResult<StreamEvent>>, Option<TokenUsage>) {
    let mut events = Vec::new();
    let mut usage: Option<TokenUsage> = None;
    let chunk = chunk.trim();
    if chunk.is_empty() {
        return (events, usage);
    }

    // Anthropic SSE: each event may have "event:" and "data:" lines
    let mut event_type = String::new();
    let mut data = String::new();

    for line in chunk.lines() {
        if let Some(rest) = line.strip_prefix("event: ") {
            event_type = rest.to_string();
        } else if let Some(rest) = line.strip_prefix("data: ") {
            data = rest.to_string();
        }
    }

    if data.is_empty() {
        return (events, usage);
    }

    let parsed: serde_json::Value = match serde_json::from_str(&data) {
        Ok(v) => v,
        Err(e) => {
            warn!("Failed to parse Anthropic SSE data: {e}");
            return (events, usage);
        }
    };

    let etype = parsed
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or(&event_type);

    match etype {
        "content_block_delta" => {
            let index = parsed
                .get("index")
                .and_then(|v| v.as_u64())
                .map(|i| i as usize)
                .unwrap_or(0);

            let empty_obj = serde_json::Value::Object(Default::default());
            let delta = parsed.get("delta").unwrap_or(&empty_obj);
            let delta_type = delta.get("type").and_then(|v| v.as_str()).unwrap_or("");

            match delta_type {
                "text_delta" => {
                    if let Some(text) = delta.get("text").and_then(|v| v.as_str()) {
                        if !text.is_empty() {
                            events.push(Ok(StreamEvent::ContentDelta {
                                content: text.to_string(),
                            }));
                        }
                    }
                }
                "input_json_delta" => {
                    if let Some(json_frag) = delta.get("partial_json").and_then(|v| v.as_str()) {
                        let entry = tool_blocks
                            .entry(index)
                            .or_insert_with(|| (String::new(), String::new(), String::new()));
                        entry.2.push_str(json_frag);

                        // Only emit if we have id and name from content_block_start
                        if !entry.0.is_empty() && !entry.1.is_empty() {
                            events.push(Ok(StreamEvent::ToolCall {
                                id: entry.0.clone(),
                                name: entry.1.clone(),
                                arguments: entry.2.clone(),
                            }));
                        }
                    }
                }
                _ => {}
            }
        }
        "content_block_start" => {
            let empty_obj = serde_json::Value::Object(Default::default());
            let block = parsed
                .get("content_block")
                .unwrap_or(&empty_obj);
            let block_type = block.get("type").and_then(|v| v.as_str()).unwrap_or("");

            if block_type == "tool_use" {
                let index = parsed
                    .get("index")
                    .and_then(|v| v.as_u64())
                    .map(|i| i as usize)
                    .unwrap_or(0);

                if let (Some(id), Some(name)) = (
                    block.get("id").and_then(|v| v.as_str()),
                    block.get("name").and_then(|v| v.as_str()),
                ) {
                    tool_blocks.insert(index, (id.to_string(), name.to_string(), String::new()));
                }
            }
        }
        "content_block_stop" => {
            let index = parsed
                .get("index")
                .and_then(|v| v.as_u64())
                .map(|i| i as usize)
                .unwrap_or(0);

            // Emit final accumulated tool call if present
            if let Some((id, name, args)) = tool_blocks.remove(&index) {
                if !id.is_empty() && !name.is_empty() {
                    events.push(Ok(StreamEvent::ToolCall {
                        id,
                        name,
                        arguments: args,
                    }));
                }
            }
        }
        "message_start" => {
            // Anthropic reports input + cache token counts here. Cache reads and
            // cache creations are billed separately but still occupy the context
            // window, so fold all three into prompt_tokens for an accurate count.
            if let Some(u) = parsed.get("message").and_then(|m| m.get("usage")) {
                let field = |k: &str| u.get(k).and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                let prompt = field("input_tokens")
                    + field("cache_creation_input_tokens")
                    + field("cache_read_input_tokens");
                usage = Some(TokenUsage {
                    prompt_tokens: prompt,
                    completion_tokens: 0,
                    total_tokens: prompt,
                });
            }
        }
        "message_delta" => {
            // message_delta carries the cumulative output_tokens for the turn.
            if let Some(out) = parsed
                .get("usage")
                .and_then(|u| u.get("output_tokens"))
                .and_then(|v| v.as_u64())
            {
                usage = Some(TokenUsage {
                    prompt_tokens: 0,
                    completion_tokens: out as u32,
                    total_tokens: out as u32,
                });
            }
            if let Some(delta) = parsed.get("delta") {
                if let Some(reason) = delta.get("stop_reason").and_then(|v| v.as_str()) {
                    if (reason == "end_turn" || reason == "stop_sequence")
                        && !*sent_done
                    {
                        *sent_done = true;
                        events.push(Ok(StreamEvent::Done { usage: None }));
                    }
                }
            }
        }
        "ping" => {
            // No action needed for ping events
        }
        _ => {
            // message_stop, etc. — no action needed
        }
    }

    (events, usage)
}

/// Merge token-usage fields across SSE chunks. Anthropic reports `input_tokens`
/// (plus cache counts) in `message_start` and the cumulative `output_tokens` in
/// `message_delta`, so we take the max of each field as fragments arrive.
fn merge_usage(acc: Option<TokenUsage>, new: TokenUsage) -> TokenUsage {
    let mut a = acc.unwrap_or_default();
    a.prompt_tokens = a.prompt_tokens.max(new.prompt_tokens);
    a.completion_tokens = a.completion_tokens.max(new.completion_tokens);
    a.total_tokens = a.prompt_tokens + a.completion_tokens;
    a
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ImageUrlSource;

    // Helpers
    fn test_adapter(base_url: String) -> AnthropicAdapter {
        AnthropicAdapter {
            client: reqwest::Client::new(),
            base_url,
            api_key: "sk-ant-test".to_string(),
        }
    }

    // -------------------------------------------------------------------
    // to_anthropic_messages
    // -------------------------------------------------------------------

    #[test]
    fn extracts_system_prompt() {
        let msgs = vec![
            ChatMessage { name: None, reasoning_content: None, 
                role: "system".into(),
                content: Some("You are helpful.".into()),
                tool_calls: None,
                tool_call_id: None,
            },
            ChatMessage { name: None, reasoning_content: None, 
                role: "user".into(),
                content: Some("Hello".into()),
                tool_calls: None,
                tool_call_id: None,
            },
        ];
        let (sys, anth) = to_anthropic_messages(&msgs);
        assert_eq!(sys, Some("You are helpful.".into()));
        assert_eq!(anth.len(), 1);
        assert_eq!(anth[0]["role"], "user");
    }

    #[test]
    fn converts_tool_result_messages() {
        let msgs = vec![ChatMessage { name: None, reasoning_content: None, 
            role: "tool".into(),
            content: Some("file contents here".into()),
            tool_calls: None,
            tool_call_id: Some("toolu_123".into()),
        }];
        let (_sys, anth) = to_anthropic_messages(&msgs);
        assert_eq!(anth.len(), 1);
        assert_eq!(anth[0]["role"], "user");
        let content = anth[0]["content"].as_array().unwrap();
        assert_eq!(content[0]["type"], "tool_result");
        assert_eq!(content[0]["tool_use_id"], "toolu_123");
    }

    #[test]
    fn vision_message_converts_to_image_block() {
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
        let (_sys, anth) = to_anthropic_messages(&msgs);
        assert_eq!(anth.len(), 1);
        let content = anth[0]["content"].as_array().unwrap();
        assert_eq!(content.len(), 2);
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[0]["text"], "Describe this");
        assert_eq!(content[1]["type"], "image");
        assert_eq!(content[1]["source"]["type"], "base64");
        assert_eq!(content[1]["source"]["media_type"], "image/png");
        assert_eq!(content[1]["source"]["data"], "abc123");
    }

    // -------------------------------------------------------------------
    // parse_anthropic_sse
    // -------------------------------------------------------------------

    fn fresh_blocks() -> std::collections::BTreeMap<usize, (String, String, String)> {
        std::collections::BTreeMap::new()
    }

    #[test]
    fn text_delta_parsing() {
        let chunk =
            r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}"#;
        let (result, _usage) = parse_anthropic_sse(chunk, &mut fresh_blocks(), &mut false);
        assert_eq!(result.len(), 1);
        match &result[0] {
            Ok(StreamEvent::ContentDelta { content }) => assert_eq!(content, "Hello"),
            other => panic!("expected ContentDelta, got {other:?}"),
        }
    }

    #[test]
    fn empty_text_delta_skipped() {
        let chunk =
            r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":""}}"#;
        let (result, _usage) = parse_anthropic_sse(chunk, &mut fresh_blocks(), &mut false);
        assert!(result.is_empty());
    }

    #[test]
    fn tool_use_lifecycle() {
        let mut blocks = fresh_blocks();
        let mut sent = false;

        // content_block_start: tool_use
        let chunk1 = r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_01","name":"read_file"}}"#;
        let (r1, _u1) = parse_anthropic_sse(chunk1, &mut blocks, &mut sent);
        assert!(r1.is_empty(), "start should not emit yet");
        assert!(blocks.contains_key(&0));

        // content_block_delta: input_json_delta
        let chunk2 = r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"path\":\"/tmp/foo\"}"}}"#;
        let (r2, _u2) = parse_anthropic_sse(chunk2, &mut blocks, &mut sent);
        assert_eq!(r2.len(), 1);
        match &r2[0] {
            Ok(StreamEvent::ToolCall { id, name, arguments }) => {
                assert_eq!(id, "toolu_01");
                assert_eq!(name, "read_file");
                assert!(arguments.contains("/tmp/foo"));
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }

        // content_block_stop
        let chunk3 = r#"event: content_block_stop
data: {"type":"content_block_stop","index":0}"#;
        let (r3, _u3) = parse_anthropic_sse(chunk3, &mut blocks, &mut sent);
        assert!(!r3.is_empty(), "stop should emit final ToolCall");
        assert!(!blocks.contains_key(&0), "block should be removed");
    }

    #[test]
    fn message_delta_end_turn() {
        let mut sent = false;
        let chunk = r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":15}}"#;
        let (result, _usage) = parse_anthropic_sse(chunk, &mut fresh_blocks(), &mut sent);
        assert_eq!(result.len(), 1);
        assert!(matches!(&result[0], Ok(StreamEvent::Done { .. })));
        assert!(sent);
    }

    #[test]
    fn ping_ignored() {
        let chunk =
            r#"event: ping
data: {"type":"ping"}"#;
        let (result, _usage) = parse_anthropic_sse(chunk, &mut fresh_blocks(), &mut false);
        assert!(result.is_empty());
    }

    #[test]
    fn empty_chunk_skipped() {
        assert!(parse_anthropic_sse("", &mut fresh_blocks(), &mut false).0.is_empty());
        assert!(parse_anthropic_sse("   ", &mut fresh_blocks(), &mut false).0.is_empty());
    }

    #[test]
    fn input_json_delta_without_start_ignored() {
        // Should not emit ToolCall without prior content_block_start
        let chunk = r#"event: content_block_delta
data: {"type":"content_block_delta","index":5,"delta":{"type":"input_json_delta","partial_json":"test"}}"#;
        let (result, _usage) = parse_anthropic_sse(chunk, &mut fresh_blocks(), &mut false);
        assert!(result.is_empty(), "no id/name accumulated yet, should be empty");
    }

    // -------------------------------------------------------------------
    // token usage
    // -------------------------------------------------------------------

    #[test]
    fn message_start_reports_input_usage() {
        // input_tokens + cache_creation + cache_read all count toward prompt size
        let chunk = r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_1","usage":{"input_tokens":100,"cache_creation_input_tokens":20,"cache_read_input_tokens":30,"output_tokens":1}}}"#;
        let (_events, usage) = parse_anthropic_sse(chunk, &mut fresh_blocks(), &mut false);
        let u = usage.expect("message_start should yield usage");
        assert_eq!(u.prompt_tokens, 150);
    }

    #[test]
    fn message_delta_reports_output_usage() {
        let mut sent = false;
        let chunk = r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":42}}"#;
        let (events, usage) = parse_anthropic_sse(chunk, &mut fresh_blocks(), &mut sent);
        assert!(matches!(&events[0], Ok(StreamEvent::Done { .. })));
        let u = usage.expect("message_delta should yield usage");
        assert_eq!(u.completion_tokens, 42);
    }

    #[test]
    fn merge_usage_combines_input_and_output() {
        let start = TokenUsage { prompt_tokens: 150, completion_tokens: 0, total_tokens: 150 };
        let delta = TokenUsage { prompt_tokens: 0, completion_tokens: 42, total_tokens: 42 };
        let merged = merge_usage(Some(start), delta);
        assert_eq!(merged.prompt_tokens, 150);
        assert_eq!(merged.completion_tokens, 42);
        assert_eq!(merged.total_tokens, 192);
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
            .stream_chat(&messages, "claude-sonnet-4-20250514", None, None, cancel)
            .await;

        assert!(result.is_err());
    }
}
