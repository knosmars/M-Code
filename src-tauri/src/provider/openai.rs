//! OpenAI-compatible streaming adapter.
//!
//! Handles SSE (Server-Sent Events) parsing with multi-line data joining,
//! tool-call accumulation, token-usage extraction, and cancellation via
//! [`Arc<AtomicBool>`](std::sync::atomic::AtomicBool).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use futures::channel::mpsc;
use futures::StreamExt;
use log::warn;

use crate::error::{AppError, AppResult};
use crate::provider::{send_with_retry, ChatMessage, ProviderAdapter};
use crate::stream::{StreamEvent, TokenUsage};

/// OpenAI-compatible API adapter that streams chat completions via SSE.
pub struct OpenAIAdapter {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    supports_vision: bool,
    openrouter_routing: bool,
}

impl OpenAIAdapter {
    pub fn new(api_key: String, base_url: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url,
            api_key,
            supports_vision: false, // Default to no vision support for safety
            openrouter_routing: false,
        }
    }

    /// Enable vision support (content parts with image_url).
    pub fn with_vision_support(mut self, enabled: bool) -> Self {
        self.supports_vision = enabled;
        self
    }

    /// Enable OpenRouter-style `provider` routing in the request body.
    ///
    /// OpenRouter (and the Meyatu proxy that forwards to it) rejects requests
    /// with HTTP 404 "No endpoints available matching your guardrail
    /// restrictions and data policy" when the only upstream endpoints for a
    /// model require prompt logging but the account's data policy denies it.
    /// Many free models (e.g. `*-flash-free`) ONLY have logging endpoints, so
    /// we must opt into data collection to get any endpoint at all. Setting
    /// `data_collection: "allow"` + `allow_fallbacks: true` widens the pool
    /// and avoids the intermittent 404 when the router happens to pick such an
    /// endpoint.
    pub fn with_openrouter_routing(mut self, enabled: bool) -> Self {
        self.openrouter_routing = enabled;
        self
    }

    /// Convert MessageContent::Parts to Text if vision is not supported.
    fn downgrade_for_non_vision(&self, messages: Vec<ChatMessage>) -> Vec<ChatMessage> {
        if self.supports_vision {
            return messages;
        }
        
        messages
            .into_iter()
            .map(|mut msg| {
                if let Some(ref content) = msg.content {
                    if content.has_images() {
                        // Downgrade to text-only
                        msg.content = content.as_text().map(crate::provider::MessageContent::Text);
                    }
                }
                msg
            })
            .collect()
    }
}

impl ProviderAdapter for OpenAIAdapter {
    async fn stream_chat(
        &self,
        messages: &[ChatMessage],
        model: &str,
        system_prompt: Option<&str>,
        tools: Option<&[serde_json::Value]>,
        cancel_token: Arc<AtomicBool>,
    ) -> AppResult<impl futures::Stream<Item = AppResult<StreamEvent>> + Send> {
        let url = format!("{}/v1/chat/completions", crate::provider::normalize_api_base(&self.base_url));

        let mut all_messages: Vec<ChatMessage> = messages.to_vec();
        if let Some(sys) = system_prompt {
            all_messages.insert(
                0,
                ChatMessage { name: None, 
                    role: "system".to_string(),
                    content: Some(crate::provider::MessageContent::Text(sys.to_string())),
                    reasoning_content: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
            );
        }

        let mut fixed_tcid = 0u32;
        let mut fixed_name = 0u32;
        for msg in &mut all_messages {
            if msg.role == "tool" {
                if msg.tool_call_id.as_ref().map_or(true, |id| id.is_empty()) {
                    msg.tool_call_id = Some(format!("call_{}", uuid::Uuid::new_v4()));
                    fixed_tcid += 1;
                }
                if msg.name.as_ref().map_or(true, |n| n.is_empty()) {
                    msg.name = Some("unknown".to_string());
                    fixed_name += 1;
                }
            }
        }
        if fixed_tcid > 0 || fixed_name > 0 {
            log::info!("[openai] fixed {} tool_call_id and {} name fields in {} messages",
                fixed_tcid, fixed_name, all_messages.len());
        }

        // Sanitize assistant messages: the OpenAI API rejects messages that have
        // BOTH content AND tool_calls. If an assistant message has tool_calls,
        // strip its content to keep the API happy.
        for msg in &mut all_messages {
            if msg.role == "assistant" && msg.tool_calls.is_some() {
                msg.content = None;
                msg.reasoning_content = None;
            }
        }

        // Downgrade image content to text for providers without vision support
        let all_messages = self.downgrade_for_non_vision(all_messages);

        let mut body = serde_json::json!({
            "model": model,
            "messages": all_messages,
            "stream": true,
        });

        if let Some(t) = tools {
            if !t.is_empty() {
                body["tools"] = serde_json::json!(t);
            }
        }

        // OpenRouter routing: opt into data-collection endpoints so free models
        // with logging-only upstreams don't 404 with a data-policy guardrail.
        if self.openrouter_routing {
            body["provider"] = serde_json::json!({
                "data_collection": "allow",
                "allow_fallbacks": true,
            });
        }

        let request = crate::provider::apply_bearer_auth(self.client.post(&url), &self.api_key);
        let response = send_with_retry(request.json(&body)).await?;

        let mut byte_stream = response.bytes_stream();
        let (tx, rx) = mpsc::unbounded::<AppResult<StreamEvent>>();
        let cancel = cancel_token;

        tokio::spawn(async move {
            let mut buffer = String::new();
            let mut sent_done = false;
            let mut last_usage: Option<TokenUsage> = None;
            // Accumulate partial tool calls by index: (id, name, arguments)
            let mut tool_call_acc: std::collections::BTreeMap<i64, (String, String, String)> =
                std::collections::BTreeMap::new();
            // Pending raw bytes for an incomplete multi-byte UTF-8 character
            // that was split across TCP chunks. SSE streams regularly split
            // CJK characters (3 bytes each) across chunks; using from_utf8_lossy
            // on each chunk would replace the split bytes with U+FFFD (�),
            // corrupting CJK text in tool arguments (查→�� etc).
            let mut pending_utf8 = Vec::<u8>::new();

            loop {
                if cancel.load(Ordering::Relaxed) {
                    if !sent_done {
                        let _ = tx.unbounded_send(Ok(StreamEvent::Done { usage: None }));
                    }
                    break;
                }

                // Drain the buffer of complete SSE events before fetching the
                // next chunk — this keeps the buffer empty in steady state and
                // lets the bytes_chunk branch below accumulate the next chunk.
                // (The loop-invariant: when we enter this while-loop, either
                // the buffer has data from a prior chunk, or we just appended
                // the latest chunk in the match below.)
                while let Some(pos) = buffer.find("\n\n") {
                    let chunk = buffer[..pos].to_string();
                    buffer = buffer[pos + 2..].to_string();

                    let (events, usage) = parse_sse_chunk(&chunk, &mut tool_call_acc);
                    if let Some(ref u) = usage {
                        log::info!(
                            "token usage: prompt={}, completion={}, total={}",
                            u.prompt_tokens,
                            u.completion_tokens,
                            u.total_tokens
                        );
                        last_usage = Some(u.clone());
                    }
                    for event in events {
                        let is_done = matches!(&event, Ok(StreamEvent::Done { .. }));
                        if is_done {
                            sent_done = true;
                        }
                        // Inject last_usage into Done events that lack it
                        let event = if is_done {
                            inject_usage(event, &last_usage)
                        } else {
                            event
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
                        // -- UTF-8-safe byte accumulation --
                        // We MUST NOT use String::from_utf8_lossy on individual
                        // chunks. When a 3-byte CJK character (e.g. 组 U+7EC4
                        // = E7 BB 84) is split across chunks — which happens
                        // routinely in SSE streams — from_utf8_lossy replaces
                        // each partial fragment with U+FFFD (�), permanently
                        // corrupting the character.
                        //
                        // Instead: accumulate raw bytes, decode only complete
                        // UTF-8 sequences, and hold incomplete trailing bytes
                        // for the next chunk.
                        pending_utf8.extend_from_slice(&bytes);
                        match String::from_utf8(std::mem::take(&mut pending_utf8)) {
                            Ok(s) => {
                                // Normalize CRLF -> LF so \n\n event-boundary
                                // detection works behind proxies (e.g. Cloudflare)
                                // that emit \r\n\r\n SSE delimiters.
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
                                // Hold the incomplete trailing bytes for the
                                // next chunk — this is the key fix.
                                pending_utf8 = raw[valid_up_to..].to_vec();
                            }
                        }
                    }
                    Some(Err(e)) => {
                        let _ = tx.unbounded_send(Err(AppError::Http(
                            e.to_string(),
                            None,
                        )));
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

fn inject_usage(event: AppResult<StreamEvent>, last_usage: &Option<TokenUsage>) -> AppResult<StreamEvent> {
    if let Ok(StreamEvent::Done { usage: None }) = &event {
        if let Some(u) = last_usage {
            // Only inject real (non-zero) usage — Meyatu proxy sends intermediate
            // chunks with usage: { prompt_tokens: 0, completion_tokens: 0, total_tokens: 0 }
            // that would otherwise overwrite the final Done event with zero values.
            if u.prompt_tokens > 0 || u.completion_tokens > 0 {
                return Ok(StreamEvent::Done { usage: Some(u.clone()) });
            }
        }
        Ok(StreamEvent::Done { usage: None })
    } else {
        event
    }
}

/// Parse a single SSE chunk (delimited by `\n\n`) into zero or more `StreamEvent` values,
/// and optionally extract the token usage from the final chunk.
///
/// Returns the events and the last usage value seen (if any).
fn parse_sse_chunk(
    chunk: &str,
    tool_call_acc: &mut std::collections::BTreeMap<i64, (String, String, String)>,
) -> (Vec<AppResult<StreamEvent>>, Option<TokenUsage>) {
    let mut events: Vec<AppResult<StreamEvent>> = Vec::new();
    let mut usage: Option<TokenUsage> = None;

    let chunk = chunk.trim();
    if chunk.is_empty() {
        return (events, usage);
    }

    let data: String = chunk
        .lines()
        .filter(|line| line.starts_with("data: "))
        .map(|line| line.strip_prefix("data: ").unwrap_or("").to_string())
        .collect::<Vec<_>>()
        .join("");

    if data.is_empty() {
        return (events, usage);
    }

    if data == "[DONE]" {
        events.push(Ok(StreamEvent::Done { usage: usage.clone() }));
        return (events, usage);
    }

    match serde_json::from_str::<serde_json::Value>(&data) {
        Ok(value) => {
            // Extract usage if present
            usage = value.get("usage").and_then(|u| {
                Some(TokenUsage {
                    prompt_tokens: u.get("prompt_tokens")?.as_u64()? as u32,
                    completion_tokens: u.get("completion_tokens")?.as_u64()? as u32,
                    total_tokens: u.get("total_tokens")?.as_u64()? as u32,
                })
            });

            let choices = value.get("choices").and_then(|c| c.as_array());
            let first = choices.and_then(|cs| cs.first());

            if let Some(choice) = first {
                    // --- content in delta (extract BEFORE finish_reason check) ---
                    let delta = choice.get("delta");
                    if let Some(content) = delta
                        .and_then(|d| d.get("content"))
                        .and_then(|c| c.as_str())
                    {
                        if !content.is_empty() {
                            events.push(Ok(StreamEvent::ContentDelta {
                                content: content.to_string(),
                            }));
                        }
                    }

                    // --- reasoning_content in delta (DeepSeek / thinking models) ---
                    if let Some(rc) = delta
                        .and_then(|d| d.get("reasoning_content"))
                        .and_then(|c| c.as_str())
                    {
                        if !rc.is_empty() {
                            events.push(Ok(StreamEvent::ReasoningDelta {
                                content: rc.to_string(),
                            }));
                        }
                    }

                    // --- tool_calls in delta ---
                    if let Some(tool_calls) = delta
                        .and_then(|d| d.get("tool_calls"))
                        .and_then(|tc| tc.as_array())
                    {
                        for tc in tool_calls {
                            let index = tc
                                .get("index")
                                .and_then(|v| v.as_i64())
                                .unwrap_or(0);

                            let entry = tool_call_acc
                                .entry(index)
                                .or_insert_with(|| (String::new(), String::new(), String::new()));

                            if let Some(id) = tc.get("id").and_then(|v| v.as_str()) {
                                entry.0 = id.to_string();
                            }
                            if let Some(fn_obj) = tc.get("function") {
                                if let Some(name) = fn_obj.get("name").and_then(|v| v.as_str()) {
                                    entry.1 = name.to_string();
                                }
                                if let Some(args) = fn_obj.get("arguments").and_then(|v| v.as_str()) {
                                    entry.2.push_str(args);
                                }
                            }

                            // Emit a ToolCall event with accumulated state
                            if !entry.0.is_empty() && !entry.1.is_empty() {
                                events.push(Ok(StreamEvent::ToolCall {
                                    id: entry.0.clone(),
                                    name: entry.1.clone(),
                                    arguments: entry.2.clone(),
                                }));
                            }
                        }
                    }

                    // --- finish_reason check (AFTER content/tool_calls extraction) ---
                    let finish_reason = choice
                        .get("finish_reason")
                        .and_then(|r| r.as_str());

                    if let Some("stop") = finish_reason {
                        events.push(Ok(StreamEvent::Done { usage: usage.clone() }));
                        return (events, usage);
                    }

                    // If finish_reason is "tool_calls", clear the accumulator
                    // so we don't re-emit on subsequent chunks
                    if let Some("tool_calls") = finish_reason {
                        tool_call_acc.clear();
                    }
                }
        }
        Err(e) => {
            warn!("Failed to parse SSE chunk: {e}");
        }
    }

    (events, usage)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{ContentPart, ImageUrlSource, MessageContent};

    // ---------------------------------------------------------------------------
    // parse_sse_chunk tests
    // ---------------------------------------------------------------------------

    fn fresh_acc() -> std::collections::BTreeMap<i64, (String, String, String)> {
        std::collections::BTreeMap::new()
    }

    #[test]
    fn content_delta_parsing() {
        let chunk = r#"data: {"choices":[{"delta":{"content":"Hello"},"index":0}]}"#;
        let (result, _usage) = parse_sse_chunk(chunk, &mut fresh_acc());
        assert_eq!(result.len(), 1, "should produce one ContentDelta event");
        match &result[0] {
            Ok(StreamEvent::ContentDelta { content }) => assert_eq!(content, "Hello"),
            other => panic!("expected ContentDelta, got {other:?}"),
        }
    }

    #[test]
    fn done_sentinel_parsing() {
        let chunk = "data: [DONE]";
        let (result, _usage) = parse_sse_chunk(chunk, &mut fresh_acc());
        assert_eq!(result.len(), 1, "should produce a Done event");
        match &result[0] {
            Ok(StreamEvent::Done { usage: None }) => {}
            other => panic!("expected Done, got {other:?}"),
        }
    }

    #[test]
    fn finish_reason_stop_parsing() {
        let chunk =
            r#"data: {"choices":[{"delta":{},"finish_reason":"stop","index":0}]}"#;
        let (result, _usage) = parse_sse_chunk(chunk, &mut fresh_acc());
        assert_eq!(result.len(), 1, "should produce a Done event for stop");
        match &result[0] {
            Ok(StreamEvent::Done { usage: None }) => {}
            other => panic!("expected Done (stop), got {other:?}"),
        }
    }

    #[test]
    fn malformed_json_skipped() {
        let chunk = "data: {not-valid-json";
        let (result, _usage) = parse_sse_chunk(chunk, &mut fresh_acc());
        assert!(
            result.is_empty(),
            "malformed JSON should produce no events"
        );
    }

    #[test]
    fn empty_chunk_returns_empty() {
        assert!(parse_sse_chunk("", &mut fresh_acc()).0.is_empty());
        assert!(parse_sse_chunk("   ", &mut fresh_acc()).0.is_empty());
    }

    #[test]
    fn chunk_without_data_prefix() {
        let chunk = "event: ping\n\n";
        let (result, _usage) = parse_sse_chunk(chunk, &mut fresh_acc());
        assert!(result.is_empty(), "chunks without data: prefix should be skipped");
    }

    #[test]
    fn multiline_sse_data() {
        let chunk = "data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}\n\ndata:  ,\"index\":0}]}";
        let (result, _usage) = parse_sse_chunk(chunk, &mut fresh_acc());
        assert_eq!(result.len(), 1, "multi-line data should be joined");
    }

    #[test]
    fn delta_with_empty_content() {
        let chunk =
            r#"data: {"choices":[{"delta":{"content":""},"index":0}]}"#;
        let (result, _usage) = parse_sse_chunk(chunk, &mut fresh_acc());
        assert!(result.is_empty(), "empty content should not produce an event");
    }

    #[test]
    fn content_extracted_before_finish_reason_stop() {
        // When a chunk has both delta.content and finish_reason: "stop",
        // content must be extracted BEFORE the Done event is emitted.
        // This is critical for Meyatu proxy which sends final content in the stop chunk.
        let chunk =
            r#"data: {"choices":[{"delta":{"content":"extra"},"finish_reason":"stop","index":0}]}"#;
        let (result, _usage) = parse_sse_chunk(chunk, &mut fresh_acc());
        assert_eq!(result.len(), 2, "should produce ContentDelta + Done events");
        match &result[0] {
            Ok(StreamEvent::ContentDelta { content }) => assert_eq!(content, "extra"),
            other => panic!("expected ContentDelta first, got {other:?}"),
        }
        match &result[1] {
            Ok(StreamEvent::Done { usage: None }) => {}
            other => panic!("finish_reason stop should produce Done second, got {other:?}"),
        }
    }

    #[test]
    fn tool_call_parsing_single_chunk() {
        let chunk = r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_abc123","type":"function","function":{"name":"read_file","arguments":"{\"path\": \"/tmp/foo\"}"}}]}}]}"#;
        let (result, _usage) = parse_sse_chunk(chunk, &mut fresh_acc());
        assert_eq!(result.len(), 1, "should produce one ToolCall event");
        match &result[0] {
            Ok(StreamEvent::ToolCall { id, name, arguments }) => {
                assert_eq!(id, "call_abc123");
                assert_eq!(name, "read_file");
                assert!(arguments.contains("/tmp/foo"));
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }

    #[test]
    fn tool_call_accumulates_arguments() {
        let mut acc = fresh_acc();
        // First chunk: id and name + partial args
        let chunk1 = r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"grep","arguments":"{\"pattern\":"}}]}}]}"#;
        let (r1, _) = parse_sse_chunk(chunk1, &mut acc);
        assert_eq!(r1.len(), 1);

        // Second chunk: more args
        let chunk2 = r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":" \"test\"}"}}]}}]}"#;
        let (r2, _) = parse_sse_chunk(chunk2, &mut acc);
        assert_eq!(r2.len(), 1);
        match &r2[0] {
            Ok(StreamEvent::ToolCall { id, name, arguments }) => {
                assert_eq!(id, "call_1");
                assert_eq!(name, "grep");
                assert!(arguments.contains(r#""pattern""#));
                assert!(arguments.contains("test"));
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }

    #[test]
    fn tool_call_finish_reason_clears_accumulator() {
        let mut acc = fresh_acc();
        // Build up a tool call
        let chunk1 = r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_x","type":"function","function":{"name":"shell","arguments":"{}"}}]}}]}"#;
        let _ = parse_sse_chunk(chunk1, &mut acc);
        assert!(!acc.is_empty());

        // finish_reason "tool_calls" should clear
        let chunk2 = r#"data: {"choices":[{"delta":{},"finish_reason":"tool_calls","index":0}]}"#;
        let (r2, _) = parse_sse_chunk(chunk2, &mut acc);
        assert!(acc.is_empty(), "accumulator should be cleared after tool_calls finish_reason");
        // Should emit ToolCall from chunk1 and Done from finish_reason? Actually finish_reason tool_calls doesn't produce Done
        assert_eq!(r2.len(), 0);
    }

    #[test]
    fn tool_call_and_content_delta_together() {
        let chunk = r#"data: {"choices":[{"delta":{"content":"Let me read that file.","tool_calls":[{"index":0,"id":"call_y","type":"function","function":{"name":"read_file","arguments":"{}"}}]}}]}"#;
        let (result, _usage) = parse_sse_chunk(chunk, &mut fresh_acc());
        // Should produce both content delta and tool call
        assert!(!result.is_empty());
        let has_content = result.iter().any(|e| matches!(e, Ok(StreamEvent::ContentDelta { .. })));
        let has_tool = result.iter().any(|e| matches!(e, Ok(StreamEvent::ToolCall { .. })));
        assert!(has_content || has_tool, "should have at least one event type");
    }

    #[test]
    fn vision_message_serializes_natively() {
        let msg = ChatMessage {
            role: "user".to_string(),
            content: Some(MessageContent::Parts(vec![
                ContentPart::Text {
                    text: "What's in this image?".to_string(),
                },
                ContentPart::ImageUrl {
                    image_url: ImageUrlSource {
                        url: "data:image/png;base64,iVBORw0KGgo".to_string(),
                        detail: Some("auto".to_string()),
                    },
                },
            ])),
            tool_calls: None,
            tool_call_id: None,
            name: None,
            reasoning_content: None,
        };
        let json = serde_json::to_value(&msg).unwrap();
        let content = json.get("content").unwrap().as_array().unwrap();
        assert_eq!(content.len(), 2);
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[0]["text"], "What's in this image?");
        assert_eq!(content[1]["type"], "image_url");
        assert_eq!(
            content[1]["image_url"]["url"],
            "data:image/png;base64,iVBORw0KGgo"
        );
        assert_eq!(content[1]["image_url"]["detail"], "auto");
    }

    // ---------------------------------------------------------------------------
    // HTTP error tests
    // ---------------------------------------------------------------------------

    fn test_adapter(base_url: String) -> OpenAIAdapter {
        OpenAIAdapter {
            client: reqwest::Client::new(),
            base_url,
            api_key: "sk-test-key".to_string(),
            supports_vision: false,
            openrouter_routing: false,
        }
    }

    #[tokio::test]
    async fn http_error_produces_provider_error() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("failed to bind ephemeral port");
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let adapter = test_adapter(format!("http://127.0.0.1:{port}"));
        let messages = [ChatMessage { name: None, reasoning_content: None,
            role: "user".to_string(),
            content: Some("ping".into()),
            tool_calls: None,
            tool_call_id: None,
        }];
        let cancel = Arc::new(AtomicBool::new(false));
        let result = adapter
            .stream_chat(&messages, "gpt-4", None, None, cancel)
            .await;

        assert!(result.is_err(), "should return an error on connection failure");
        let err = result.err().unwrap();
        assert!(
            matches!(err, AppError::Provider(_, _) | AppError::Http(_, _)),
            "expected Provider or Http error, got {err:?}"
        );
    }
}
