use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub mod pick_folder;

use futures::StreamExt;
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use tauri::{ipc::Channel, AppHandle, State};
use uuid::Uuid;

use crate::error::AppResult;
use crate::keychain::Keychain;
use crate::provider::{AnthropicAdapter, ChatMessage, GeminiAdapter, OpenAIAdapter, ProviderAdapter};
use crate::stream::StreamEvent;

const MAX_MESSAGES: usize = 200;
const MAX_TEXT_CONTENT_LEN: usize = 32_000;
const MAX_IMAGE_CONTENT_LEN: usize = 4_000_000; // 4MB — covers base64 images within API limits

/// True when two URLs share the same host and effective port. Used to decide
/// whether a stored API key may be sent to a (possibly frontend-supplied) URL.
fn same_host(a: &str, b: &str) -> bool {
    match (url::Url::parse(a), url::Url::parse(b)) {
        (Ok(x), Ok(y)) => {
            x.host_str() == y.host_str() && x.port_or_known_default() == y.port_or_known_default()
        }
        _ => false,
    }
}

const OPENAI_BASE_URL: &str = "https://api.openai.com";
const ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com";
const GEMINI_BASE_URL: &str = "https://generativelanguage.googleapis.com";
const MEYATU_BASE_URL: &str = "https://api.meyatu.io";

/// Holds all managed state for the Tauri application.
///
/// Contains a [`Keychain`](crate::keychain::Keychain) for API key storage.
/// Adapters are created per-request based on `provider_id`.
pub struct AppState {
    pub keychain: Arc<Keychain>,
}

impl AppState {
    /// Create new [`AppState`] with a keychain instance.
    pub fn new(keychain_service_name: &str) -> Self {
        Self {
            keychain: Arc::new(Keychain::new(keychain_service_name)),
        }
    }
}

/// Incoming chat request from the frontend.
///
/// Sent via Tauri IPC and validated by [`validate_request`] before being
/// dispatched to the provider adapter.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatRequest {
    pub session_id: String,
    pub messages: Vec<ChatMessage>,
    pub provider_id: Option<String>,
    pub model: String,
    pub system_prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<serde_json::Value>>,
    /// Optional provider base URL override (e.g. a local Ollama endpoint or a
    /// custom OpenAI-compatible gateway). Falls back to the per-provider
    /// default constant when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
}

fn validate_request(request: &ChatRequest) -> Result<(), (&'static str, String)> {
    if request.messages.is_empty() {
        return Err(("VALIDATION_ERROR", "messages must not be empty".into()));
    }
    if request.messages.len() > MAX_MESSAGES {
        return Err((
            "SESSION_LIMIT",
            format!(
                "此会话消息过多（{} 条，上限 {} 条），请新建会话继续",
                request.messages.len(),
                MAX_MESSAGES
            ),
        ));
    }
    if request.model.is_empty() {
        return Err(("VALIDATION_ERROR", "model must not be empty".into()));
    }
    for (i, msg) in request.messages.iter().enumerate() {
        if msg.role.is_empty() {
            return Err((
                "VALIDATION_ERROR",
                format!("message {}: role must not be empty", i),
            ));
        }
        if let Some(ref content) = msg.content {
            let len = content.text_len();
            let limit = if content.has_images() {
                MAX_IMAGE_CONTENT_LEN
            } else {
                MAX_TEXT_CONTENT_LEN
            };
            if len > limit {
                return Err((
                    "VALIDATION_ERROR",
                    format!("message {}: content too long ({} > {})", i, len, limit),
                ));
            }
        }
    }
    Ok(())
}

/// Streaming chat endpoint registered as a Tauri command.
///
/// Validates the request, selects the provider adapter based on
/// `provider_id`, and bridges the adapter's stream to the frontend
/// via a Tauri `Channel<StreamEvent>`.
#[tauri::command]
pub async fn stream_chat(
    _app: AppHandle,
    state: State<'_, AppState>,
    on_event: Channel<StreamEvent>,
    request: ChatRequest,
) -> Result<(), String> {
    let request_id = Uuid::new_v4();
    let provider_id = request.provider_id.as_deref().unwrap_or("openai-compatible");
    info!(
        "[{}] stream_chat: provider={}, session={}, model={}, {} messages",
        request_id,
        provider_id,
        request.session_id,
        request.model,
        request.messages.len()
    );

    if let Err((code, msg)) = validate_request(&request) {
        warn!("[{}] validation failed: {}", request_id, msg);
        let _ = on_event.send(StreamEvent::error(code, &msg));
        return Err(msg);
    }

    // Resolve the effective base URL + whether the stored key may be attached.
    // The stored key is ONLY sent when the request targets the provider's
    // canonical host. A frontend-supplied base_url pointing anywhere else
    // (local Ollama, a custom gateway, or an attacker host) is treated as
    // keyless — this prevents a compromised renderer from redirecting a cloud
    // provider's API key off-host (credential exfiltration). Keys are optional
    // anyway: local providers need none, and the frontend gates the rest.
    let default_url = match provider_id {
        "anthropic" => ANTHROPIC_BASE_URL,
        "google" | "gemini" => GEMINI_BASE_URL,
        "meyatu" => MEYATU_BASE_URL,
        _ => OPENAI_BASE_URL,
    };
    let effective_url = request.base_url.clone().unwrap_or_else(|| default_url.to_string());
    let api_key = if same_host(&effective_url, default_url) {
        state.keychain.get_key(provider_id).unwrap_or_default()
    } else {
        info!("[{}] base_url host differs from default — proceeding keyless", request_id);
        String::new()
    };

    let cancel_token = Arc::new(AtomicBool::new(false));
    let mut messages = request.messages;
    let model = request.model;
    let system_prompt = request.system_prompt.as_deref();
    let tools = request.tools.as_deref();

    for msg in &mut messages {
        if msg.role == "tool" {
            if msg.tool_call_id.as_ref().map_or(true, |id| id.is_empty()) {
                msg.tool_call_id = Some(format!("call_{}", Uuid::new_v4()));
            }
            if msg.name.as_ref().map_or(true, |n| n.is_empty()) {
                msg.name = Some("unknown".to_string());
            }
        }
    }

    match provider_id {
        "anthropic" => {
            let adapter = AnthropicAdapter::new(api_key, effective_url.clone());
            let stream = adapter
                .stream_chat(&messages, &model, system_prompt, tools, Arc::clone(&cancel_token))
                .await;
            bridge_stream(request_id, &on_event, stream, &cancel_token).await
        }
        "google" | "gemini" => {
            let adapter = GeminiAdapter::new(api_key, effective_url.clone());
            let stream = adapter
                .stream_chat(&messages, &model, system_prompt, tools, Arc::clone(&cancel_token))
                .await;
            bridge_stream(request_id, &on_event, stream, &cancel_token).await
        }
        "meyatu" => {
            let adapter = OpenAIAdapter::new(api_key, effective_url.clone())
                .with_vision_support(true)
                .with_openrouter_routing(true);
            let stream = adapter
                .stream_chat(&messages, &model, system_prompt, tools, Arc::clone(&cancel_token))
                .await;
            bridge_stream(request_id, &on_event, stream, &cancel_token).await
        }
        _ => {
            let adapter = OpenAIAdapter::new(api_key, effective_url.clone())
                .with_vision_support(true);
            let stream = adapter
                .stream_chat(&messages, &model, system_prompt, tools, Arc::clone(&cancel_token))
                .await;
            bridge_stream(request_id, &on_event, stream, &cancel_token).await
        }
    }
}

async fn bridge_stream<S>(
    request_id: Uuid,
    on_event: &Channel<StreamEvent>,
    stream_result: AppResult<S>,
    cancel_token: &Arc<AtomicBool>,
) -> Result<(), String>
where
    S: futures::Stream<Item = AppResult<StreamEvent>> + Send + Unpin,
{
    match stream_result {
        Ok(mut stream) => {
            while let Some(event_result) = stream.next().await {
                if cancel_token.load(Ordering::Relaxed) {
                    break;
                }
                match event_result {
                    Ok(evt) => {
                        if let Err(e) = on_event.send(evt) {
                            error!("[{}] channel send error: {}", request_id, e);
                            return Err(e.to_string());
                        }
                    }
                    Err(e) => {
                        error!("[{}] stream error: {}", request_id, e);
                        let _ = on_event.send(StreamEvent::from_app_error(&e));
                        return Err(e.to_string());
                    }
                }
            }

            if !cancel_token.load(Ordering::Relaxed) {
                let _ = on_event.send(StreamEvent::Done { usage: None });
            }

            info!("[{}] stream_chat completed", request_id);
            Ok(())
        }
        Err(e) => {
            error!("[{}] adapter error: {}", request_id, e);
            let _ = on_event.send(StreamEvent::from_app_error(&e));
            Err(e.to_string())
        }
    }
}

// -----------------------------------------------------------------------------
// Keychain management commands
// -----------------------------------------------------------------------------

#[tauri::command]
pub fn get_api_key(
    state: State<'_, AppState>,
    provider: String,
) -> Result<Option<String>, crate::error::AppError> {
    match state.keychain.get_key(&provider) {
        Ok(key) => Ok(Some(key)),
        Err(crate::error::AppError::NotFound(_)) => Ok(None),
        Err(e) => Err(e),
    }
}

#[tauri::command]
pub fn set_api_key(
    state: State<'_, AppState>,
    provider: String,
    api_key: String,
) -> Result<(), crate::error::AppError> {
    state.keychain.set_key(&provider, &api_key)
}

#[tauri::command]
pub fn delete_api_key(
    state: State<'_, AppState>,
    provider: String,
) -> Result<(), crate::error::AppError> {
    state.keychain.delete_key(&provider)
}

/// Build the models-listing endpoint URL for a given provider.
///
/// Each provider exposes models at a different path:
///   - Anthropic: `{base}/v1/models`
///   - Google Gemini: `{base}/v1beta/models`
///   - OpenAI-compatible / Meyatu / custom: `{base}/models`
///     (the `base` is expected to already include the API version, e.g. `.../v1`)
fn models_endpoint(provider_id: &str, base_url: &str) -> String {
    let base = base_url.trim_end_matches('/');
    match provider_id {
        "anthropic" => format!("{base}/v1/models"),
        "google" | "gemini" => format!("{base}/v1beta/models"),
        _ => format!("{base}/models"),
    }
}

/// Fetch available models from a provider's models endpoint.
///
/// Handles the per-provider differences in URL path, authentication header,
/// and response shape (OpenAI/Anthropic use `data[].id`; Gemini uses
/// `models[].name`). Returns a normalized JSON string `{"models":[...]}` so
/// the frontend parses a single shape regardless of provider.
/// A 10-second timeout prevents hanging on unreachable endpoints.
#[tauri::command]
pub async fn list_models(
    provider_id: String,
    base_url: String,
    api_key: String,
) -> Result<String, String> {
    use std::time::Duration;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {e}"))?;

    let endpoint = models_endpoint(&provider_id, &base_url);

    // Build the request with provider-appropriate authentication.
    let request = match provider_id.as_str() {
        "anthropic" => client
            .get(&endpoint)
            .header("x-api-key", &api_key)
            .header("anthropic-version", "2023-06-01"),
        "google" | "gemini" => {
            // Send the key via header (not the URL query string) to avoid
            // leaking it in logs/error messages — consistent with the Gemini
            // chat adapter's CWE-598 fix.
            client.get(&endpoint).header("x-goog-api-key", &api_key)
        }
        // OpenAI-compatible, Meyatu, and custom providers use a Bearer token.
        // Empty key (local Ollama/llama.cpp/LM Studio) omits the header.
        _ => crate::provider::apply_bearer_auth(client.get(&endpoint), &api_key),
    };

    let resp = request
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;

    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| format!("Read response failed: {e}"))?;

    if !status.is_success() {
        return Err(format!("HTTP {}: {}", status.as_u16(), body));
    }

    let models = parse_models(&provider_id, &body)?;
    serde_json::to_string(&serde_json::json!({ "models": models }))
        .map_err(|e| format!("Failed to serialize models: {e}"))
}

/// Parse a provider's models response into a flat list of model IDs.
fn parse_models(provider_id: &str, body: &str) -> Result<Vec<String>, String> {
    let json: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("Invalid models response: {e}"))?;

    let ids: Vec<String> = match provider_id {
        "google" | "gemini" => json["models"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| m["name"].as_str())
                    // Gemini names look like "models/gemini-2.5-pro" — strip the prefix.
                    .map(|n| n.strip_prefix("models/").unwrap_or(n).to_string())
                    .collect()
            })
            .unwrap_or_default(),
        // OpenAI, Anthropic, Meyatu, custom: `{ "data": [ { "id": ... } ] }`
        _ => json["data"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| m["id"].as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default(),
    };

    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_state_new() {
        let state = AppState::new("com.meyatu.code.test");
        assert!(state.keychain.get_key("nonexistent").is_err());
    }

    #[test]
    fn test_models_endpoint_per_provider() {
        assert_eq!(
            models_endpoint("anthropic", "https://api.anthropic.com"),
            "https://api.anthropic.com/v1/models"
        );
        assert_eq!(
            models_endpoint("google", "https://generativelanguage.googleapis.com"),
            "https://generativelanguage.googleapis.com/v1beta/models"
        );
        // OpenAI-compatible base already includes the version segment.
        assert_eq!(
            models_endpoint("openai-compatible", "https://api.openai.com/v1"),
            "https://api.openai.com/v1/models"
        );
        // Trailing slash is normalized away.
        assert_eq!(
            models_endpoint("custom", "http://localhost:1234/v1/"),
            "http://localhost:1234/v1/models"
        );
    }

    #[test]
    fn test_parse_models_openai_shape() {
        let body = r#"{"data":[{"id":"gpt-4o"},{"id":"gpt-4o-mini"}]}"#;
        let ids = parse_models("openai-compatible", body).unwrap();
        assert_eq!(ids, vec!["gpt-4o", "gpt-4o-mini"]);
    }

    #[test]
    fn test_parse_models_gemini_shape() {
        let body = r#"{"models":[{"name":"models/gemini-2.5-pro"},{"name":"models/gemini-2.5-flash"}]}"#;
        let ids = parse_models("google", body).unwrap();
        assert_eq!(ids, vec!["gemini-2.5-pro", "gemini-2.5-flash"]);
    }

    #[test]
    fn test_parse_models_empty_is_ok() {
        assert!(parse_models("openai-compatible", r#"{"data":[]}"#)
            .unwrap()
            .is_empty());
        assert!(parse_models("anthropic", r#"{"other":1}"#).unwrap().is_empty());
    }

    #[test]
    fn same_host_matches_ignoring_path_and_scheme_port() {
        // Canonical host with a differing path (e.g. meyatu's /v1) still matches.
        assert!(same_host("https://api.meyatu.io/v1", "https://api.meyatu.io"));
        assert!(same_host("https://api.openai.com/v1", "https://api.openai.com"));
        assert!(same_host("https://api.anthropic.com", "https://api.anthropic.com"));
    }

    #[test]
    fn same_host_rejects_off_host_urls() {
        // A frontend-supplied URL pointing elsewhere must NOT match the default,
        // so the stored key is withheld (no credential exfiltration).
        assert!(!same_host("https://attacker.example", "https://api.anthropic.com"));
        assert!(!same_host("http://localhost:11434/v1", "https://api.openai.com"));
        assert!(!same_host("not a url", "https://api.openai.com"));
    }

    #[test]
    fn test_chat_request_serialization() {
        let req = ChatRequest {
            session_id: "sess-1".into(),
            messages: vec![ChatMessage { name: None, 
                reasoning_content: None,
                role: "user".into(),
                content: Some("hello".into()),
                tool_calls: None,
                tool_call_id: None,
            }],
            provider_id: None,
            model: "gpt-4".into(),
            system_prompt: Some("You are helpful.".into()),
            tools: None,
            base_url: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains(r#""sessionId":"sess-1""#));
        assert!(json.contains(r#""model":"gpt-4""#));
        assert!(json.contains(r#""systemPrompt":"You are helpful.""#));
    }

    #[test]
    fn test_chat_request_default_system_prompt() {
        let req = ChatRequest {
            session_id: "sess-1".into(),
            messages: vec![],
            provider_id: None,
            model: "gpt-4".into(),
            system_prompt: None,
            tools: None,
            base_url: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(value["systemPrompt"].is_null());
    }

    #[test]
    fn validate_rejects_empty_messages() {
        let req = ChatRequest {
            session_id: "sess-1".into(),
            messages: vec![],
            provider_id: None,
            model: "gpt-4".into(),
            system_prompt: None,
            tools: None,
            base_url: None,
        };
        match validate_request(&req) {
            Err((code, _)) => assert_eq!(code, "VALIDATION_ERROR"),
            Ok(()) => panic!("expected validation error"),
        }
    }

    #[test]
    fn validate_rejects_empty_model() {
        let req = ChatRequest {
            session_id: "sess-1".into(),
            messages: vec![ChatMessage { name: None,
                reasoning_content: None,
                role: "user".into(),
                content: Some("hi".into()),
                tool_calls: None,
                tool_call_id: None,
            }],
            provider_id: None,
            model: "".into(),
            system_prompt: None,
            tools: None,
            base_url: None,
        };
        match validate_request(&req) {
            Err((code, _)) => assert_eq!(code, "VALIDATION_ERROR"),
            Ok(()) => panic!("expected validation error"),
        }
    }

    #[test]
    fn validate_rejects_empty_role() {
        let req = ChatRequest {
            session_id: "sess-1".into(),
            messages: vec![ChatMessage { name: None,
                reasoning_content: None,
                role: "".into(),
                content: Some("hi".into()),
                tool_calls: None,
                tool_call_id: None,
            }],
            provider_id: None,
            model: "gpt-4".into(),
            system_prompt: None,
            tools: None,
            base_url: None,
        };
        match validate_request(&req) {
            Err((code, _)) => assert_eq!(code, "VALIDATION_ERROR"),
            Ok(()) => panic!("expected validation error"),
        }
    }

    #[test]
    fn validate_accepts_valid_request() {
        let req = ChatRequest {
            session_id: "sess-1".into(),
            messages: vec![ChatMessage { name: None,
                reasoning_content: None,
                role: "user".into(),
                content: Some("hi".into()),
                tool_calls: None,
                tool_call_id: None,
            }],
            provider_id: None,
            model: "gpt-4".into(),
            system_prompt: None,
            tools: None,
            base_url: None,
        };
        assert!(validate_request(&req).is_ok());
    }

    #[test]
    fn test_chat_request_with_tools() {
        let req = ChatRequest {
            session_id: "sess-1".into(),
            messages: vec![ChatMessage { name: None, 
                reasoning_content: None,
                role: "user".into(),
                content: Some("read my file".into()),
                tool_calls: None,
                tool_call_id: None,
            }],
            provider_id: None,
            model: "gpt-4".into(),
            system_prompt: None,
            tools: Some(vec![serde_json::json!({
                "type": "function",
                "function": {
                    "name": "read_file",
                    "description": "Read a file",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "path": {"type": "string"}
                        }
                    }
                }
            })]),
            base_url: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains(r#""tools""#));
        assert!(json.contains("read_file"));
    }

    #[test]
    fn test_chat_request_with_tool_calls_message() {
        let req = ChatRequest {
            session_id: "sess-1".into(),
            messages: vec![
                ChatMessage { name: None, 
                    reasoning_content: None,
                    role: "user".into(),
                    content: Some("hello".into()),
                    tool_calls: None,
                    tool_call_id: None,
                },
                ChatMessage { name: None, 
                    reasoning_content: None,
                    role: "assistant".into(),
                    content: None,
                    tool_calls: Some(vec![crate::provider::ToolCall {
                        id: "call_1".into(),
                        call_type: "function".into(),
                        function: crate::provider::ToolCallFunction {
                            name: "read_file".into(),
                            arguments: r#"{"path":"/tmp/foo"}"#.into(),
                        },
                    }]),
                    tool_call_id: None,
                },
                ChatMessage { name: None, 
                    reasoning_content: None,
                    role: "tool".into(),
                    content: Some("file contents".into()),
                    tool_calls: None,
                    tool_call_id: Some("call_1".into()),
                },
            ],
            provider_id: None,
            model: "gpt-4".into(),
            system_prompt: None,
            tools: None,
            base_url: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains(r#""tool_calls""#));
        assert!(json.contains(r#""tool_call_id":"call_1""#));
    }

    #[test]
    fn validate_accepts_null_content_for_tool_calls() {
        let req = ChatRequest {
            session_id: "sess-1".into(),
            messages: vec![ChatMessage { name: None,
                reasoning_content: None,
                role: "assistant".into(),
                content: None,
                tool_calls: Some(vec![crate::provider::ToolCall {
                    id: "call_1".into(),
                    call_type: "function".into(),
                    function: crate::provider::ToolCallFunction {
                        name: "read_file".into(),
                        arguments: "{}".into(),
                    },
                }]),
                tool_call_id: None,
            }],
            provider_id: None,
            model: "gpt-4".into(),
            system_prompt: None,
            tools: None,
            base_url: None,
        };
        // null content is valid for tool_calls messages
        assert!(validate_request(&req).is_ok());
    }

    #[test]
    fn validate_session_limit_has_distinct_code() {
        let msg = ChatMessage {
            name: None,
            reasoning_content: None,
            role: "user".into(),
            content: Some("hi".into()),
            tool_calls: None,
            tool_call_id: None,
        };
        let req = ChatRequest {
            session_id: "sess-1".into(),
            messages: vec![msg; MAX_MESSAGES + 1],
            provider_id: None,
            model: "gpt-4".into(),
            system_prompt: None,
            tools: None,
            base_url: None,
        };
        match validate_request(&req) {
            Err((code, m)) => {
                assert_eq!(code, "SESSION_LIMIT");
                assert!(m.contains("新建会话"));
            }
            Ok(()) => panic!("expected session-limit error"),
        }
    }

    #[test]
    fn validate_empty_messages_keeps_validation_code() {
        let req = ChatRequest {
            session_id: "sess-1".into(),
            messages: vec![],
            provider_id: None,
            model: "gpt-4".into(),
            system_prompt: None,
            tools: None,
            base_url: None,
        };
        match validate_request(&req) {
            Err((code, _)) => assert_eq!(code, "VALIDATION_ERROR"),
            Ok(()) => panic!("expected validation error"),
        }
    }
}

/// Open a URL in the system's default browser.
/// Works cross-platform: `cmd /C start` on Windows, `open` on macOS, `xdg-open` on Linux.
#[tauri::command]
pub fn tool_open_url(url: String) -> Result<(), String> {
    let status = if cfg!(target_os = "windows") {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", &url])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open")
            .arg(&url)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
    } else {
        std::process::Command::new("xdg-open")
            .arg(&url)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
    };

    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(format!("Failed to open URL: exit code {:?}", s.code())),
        Err(e) => Err(format!("Failed to open URL: {e}")),
    }
}
