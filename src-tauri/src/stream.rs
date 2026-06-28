#![allow(dead_code)]
use serde::{Deserialize, Serialize};

/// Token usage data from the LLM API response.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// Events emitted during a streaming LLM response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamEvent {
    /// An incremental text chunk from the model.
    ContentDelta { content: String },
    /// Reasoning/thinking content from the model (e.g. DeepSeek reasoning).
    ReasoningDelta { content: String },
    /// A tool invocation requested by the model.
    ToolCall {
        id: String,
        name: String,
        arguments: String,
    },
    /// The result returned from executing a tool.
    ToolResult {
        id: String,
        content: String,
    },
    /// An error encountered during streaming.
    Error {
        code: String,
        message: String,
        /// Whether the client may safely retry the request (rate limit / 5xx /
        /// transient connection error).
        retryable: bool,
    },
    /// The stream has completed successfully.
    Done {
        #[serde(skip_serializing_if = "Option::is_none")]
        usage: Option<TokenUsage>,
    },
}

impl StreamEvent {
    /// Convenience constructor for a content delta event.
    pub fn content(s: impl Into<String>) -> Self {
        Self::ContentDelta { content: s.into() }
    }

    /// Convenience constructor for a reasoning content delta event.
    pub fn reasoning(s: impl Into<String>) -> Self {
        Self::ReasoningDelta { content: s.into() }
    }

    /// Convenience constructor for a non-retryable error event.
    pub fn error(code: impl Into<String>, msg: impl Into<String>) -> Self {
        Self::Error {
            code: code.into(),
            message: msg.into(),
            retryable: false,
        }
    }

    /// Build an error event from a structured [`AppError`], preserving its
    /// machine-readable code and retryability so the UI can react by code.
    pub fn from_app_error(err: &crate::error::AppError) -> Self {
        Self::Error {
            code: err.code().to_string(),
            message: err.to_string(),
            retryable: err.is_retryable(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_content_delta_serialization() {
        let evt = StreamEvent::content("hello world");
        let json = serde_json::to_string(&evt).unwrap();
        assert!(json.contains(r#""type":"content_delta""#));
        assert!(json.contains("hello world"));
    }

    #[test]
    fn test_done_serialization() {
        let json = serde_json::to_string(&StreamEvent::Done { usage: None }).unwrap();
        assert_eq!(json, r#"{"type":"done"}"#);
    }

    #[test]
    fn test_error_serialization() {
        let evt = StreamEvent::error("TIMEOUT", "request timed out");
        let json = serde_json::to_string(&evt).unwrap();
        assert!(json.contains(r#""type":"error""#));
        assert!(json.contains("TIMEOUT"));
    }

    #[test]
    fn test_convenience_constructors() {
        assert!(matches!(
            StreamEvent::content("x"),
            StreamEvent::ContentDelta { .. }
        ));
        assert!(matches!(
            StreamEvent::error("E", "m"),
            StreamEvent::Error { .. }
        ));
    }

    #[test]
    fn test_tool_call_serialization() {
        let evt = StreamEvent::ToolCall {
            id: "call_1".into(),
            name: "read_file".into(),
            arguments: r#"{"path": "/tmp/foo"}"#.into(),
        };
        let json = serde_json::to_string(&evt).unwrap();
        assert!(json.contains(r#""type":"tool_call""#));
        assert!(json.contains(r#""id":"call_1""#));
        assert!(json.contains(r#""name":"read_file""#));
        assert!(json.contains(r#""arguments":"{\"path\": \"/tmp/foo\"}"#));
    }

    #[test]
    fn test_tool_result_serialization() {
        let evt = StreamEvent::ToolResult {
            id: "call_1".into(),
            content: "file contents here".into(),
        };
        let json = serde_json::to_string(&evt).unwrap();
        assert!(json.contains(r#""type":"tool_result""#));
        assert!(json.contains(r#""id":"call_1""#));
        assert!(json.contains("file contents here"));
    }

    #[test]
    fn test_done_roundtrip() {
        let evt = StreamEvent::Done { usage: None };
        let json = serde_json::to_string(&evt).unwrap();
        let deserialized: StreamEvent = serde_json::from_str(&json).unwrap();
        assert!(matches!(deserialized, StreamEvent::Done { usage: None }));
    }

    #[test]
    fn test_content_delta_roundtrip() {
        let evt = StreamEvent::ContentDelta {
            content: "roundtrip test".into(),
        };
        let json = serde_json::to_string(&evt).unwrap();
        let deserialized: StreamEvent = serde_json::from_str(&json).unwrap();
        match deserialized {
            StreamEvent::ContentDelta { content } => assert_eq!(content, "roundtrip test"),
            other => panic!("expected ContentDelta, got {other:?}"),
        }
    }

    #[test]
    fn test_error_roundtrip() {
        let evt = StreamEvent::error("E001", "something broke");
        let json = serde_json::to_string(&evt).unwrap();
        let deserialized: StreamEvent = serde_json::from_str(&json).unwrap();
        match deserialized {
            StreamEvent::Error { code, message, retryable } => {
                assert_eq!(code, "E001");
                assert_eq!(message, "something broke");
                assert!(!retryable);
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn from_app_error_preserves_code_and_retryable() {
        use crate::error::AppError;
        let evt = StreamEvent::from_app_error(&AppError::RateLimited("slow".into(), Some(5)));
        match evt {
            StreamEvent::Error { code, retryable, .. } => {
                assert_eq!(code, "rate_limited");
                assert!(retryable);
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn test_debug_formatting() {
        let evt = StreamEvent::content("debug");
        let debug = format!("{evt:?}");
        assert!(debug.contains("ContentDelta"));
        assert!(debug.contains("debug"));
    }

    #[test]
    fn test_clone() {
        let evt = StreamEvent::content("clone me");
        let cloned = evt.clone();
        match cloned {
            StreamEvent::ContentDelta { content } => assert_eq!(content, "clone me"),
            other => panic!("expected ContentDelta, got {other:?}"),
        }
    }
}
