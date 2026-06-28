use serde::ser::SerializeStruct;
use serde::Serialize;

/// Unified error type for the Meyatu Code application.
#[derive(Debug, Clone)]
pub enum AppError {
    /// OS keyring / keychain failures
    Keychain(String),
    /// HTTP request failures with optional status code
    Http(String, Option<u16>),
    /// LLM provider API errors with optional status code
    Provider(String, Option<u16>),
    /// serde / JSON serialization or deserialization errors
    Serialization(String),
    /// Resource not found
    NotFound(String),
    /// Tool execution blocked by permission policy
    PermissionDenied(String),
    /// Rate-limited by LLM provider (HTTP 429). Contains the
    /// `Retry-After` value (seconds) if present.
    RateLimited(String, Option<u64>),
    /// Catch-all for internal/unexpected failures with no more specific variant.
    Internal(String),
}

impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let message = match self {
            Self::Keychain(m)
            | Self::Http(m, _)
            | Self::Provider(m, _)
            | Self::Serialization(m)
            | Self::NotFound(m)
            | Self::PermissionDenied(m)
            | Self::RateLimited(m, _)
            | Self::Internal(m) => m.clone(),
        };
        let mut state = serializer.serialize_struct("AppError", 4)?;
        state.serialize_field("code", self.code())?;
        state.serialize_field("message", &message)?;
        state.serialize_field("retryable", &self.is_retryable())?;
        state.serialize_field("retryAfter", &self.retry_after_seconds())?;
        state.end()
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Keychain(msg) => write!(f, "Keychain error: {msg}"),
            Self::Http(msg, status) => {
                if let Some(code) = status {
                    write!(f, "HTTP error ({code}): {msg}")
                } else {
                    write!(f, "HTTP error: {msg}")
                }
            }
            Self::Provider(msg, status) => {
                if let Some(code) = status {
                    write!(f, "Provider error ({code}): {msg}")
                } else {
                    write!(f, "Provider error: {msg}")
                }
            }
            Self::Serialization(msg) => write!(f, "Serialization error: {msg}"),
            Self::NotFound(msg) => write!(f, "Not found: {msg}"),
            Self::PermissionDenied(msg) => write!(f, "Permission denied: {msg}"),
            Self::RateLimited(msg, retry_after) => {
                if let Some(secs) = retry_after {
                    write!(f, "Rate limited: {msg} (retry after {secs}s)")
                } else {
                    write!(f, "Rate limited: {msg}")
                }
            }
            Self::Internal(msg) => write!(f, "Internal error: {msg}"),
        }
    }
}

impl std::error::Error for AppError {}

impl From<serde_json::Error> for AppError {
    fn from(err: serde_json::Error) -> Self {
        Self::Serialization(err.to_string())
    }
}

impl From<reqwest::Error> for AppError {
    fn from(err: reqwest::Error) -> Self {
        let msg = err.to_string();
        let status = err.status().map(|s| s.as_u16());
        Self::Http(msg, status)
    }
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        use std::io::ErrorKind;
        let msg = err.to_string();
        match err.kind() {
            ErrorKind::NotFound => Self::NotFound(msg),
            ErrorKind::PermissionDenied => Self::PermissionDenied(msg),
            _ => Self::Internal(msg),
        }
    }
}

/// Convenience type alias for functions that return `AppError`.
impl AppError {
    /// Stable machine-readable code (matches the serialized `code` field).
    pub fn code(&self) -> &'static str {
        match self {
            Self::Keychain(_) => "keychain",
            Self::Http(..) => "http",
            Self::Provider(..) => "provider",
            Self::Serialization(_) => "serialization",
            Self::NotFound(_) => "not_found",
            Self::PermissionDenied(_) => "permission_denied",
            Self::RateLimited(..) => "rate_limited",
            Self::Internal(_) => "internal",
        }
    }

    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::RateLimited(..))
            || match self {
                Self::Provider(_, Some(status)) if *status >= 500 => true,
                Self::Http(_, Some(status)) if *status >= 500 => true,
                Self::Http(_, None) => true, // connection errors
                _ => false,
            }
    }

    /// HTTP status code if this error carries one. Part of the public AppError
    /// surface; not yet consumed internally.
    #[allow(dead_code)]
    pub fn status_code(&self) -> Option<u16> {
        match self {
            Self::RateLimited(..) => Some(429),
            Self::Provider(_, s) | Self::Http(_, s) => *s,
            _ => None,
        }
    }

    pub fn retry_after_seconds(&self) -> Option<u64> {
        match self {
            Self::RateLimited(_, ra) => *ra,
            _ => None,
        }
    }
}

pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_formatting() {
        let err = AppError::Keychain("test error".into());
        let display = format!("{err}");
        assert!(display.contains("Keychain"));
        assert!(display.contains("test error"));
    }

    #[test]
    fn test_display_formatting_http_with_status() {
        let err = AppError::Http("connection refused".into(), Some(503));
        let display = format!("{err}");
        assert!(display.contains("HTTP error (503)"));
        assert!(display.contains("connection refused"));
    }

    #[test]
    fn test_display_formatting_http_no_status() {
        let err = AppError::Http("timeout".into(), None);
        let display = format!("{err}");
        assert!(display.contains("HTTP error"));
        assert!(display.contains("timeout"));
        assert!(!display.contains('('));
    }

    #[test]
    fn test_display_formatting_provider() {
        let err = AppError::Provider("rate limited".into(), Some(429));
        let display = format!("{err}");
        assert!(display.contains("Provider error (429)"));
        assert!(display.contains("rate limited"));
    }

    #[test]
    fn test_display_formatting_not_found() {
        let err = AppError::NotFound("config.toml".into());
        let display = format!("{err}");
        assert!(display.contains("Not found"));
        assert!(display.contains("config.toml"));
    }

    #[test]
    fn test_display_formatting_permission_denied() {
        let err = AppError::PermissionDenied("shell.execute".into());
        let display = format!("{err}");
        assert!(display.contains("Permission denied"));
        assert!(display.contains("shell.execute"));
    }

    #[test]
    fn test_display_formatting_serialization() {
        let err = AppError::Serialization("invalid field".into());
        let display = format!("{err}");
        assert!(display.contains("Serialization error"));
        assert!(display.contains("invalid field"));
    }

    #[test]
    fn test_serde_roundtrip() {
        let err = AppError::NotFound("file not found".into());
        let json = serde_json::to_string(&err).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["code"], "not_found");
        assert!(value["message"].as_str().unwrap().contains("file not found"));
    }

    #[test]
    fn test_serde_keychain_shape() {
        let err = AppError::Keychain("keyring access denied".into());
        let json = serde_json::to_string(&err).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["code"], "keychain");
        assert_eq!(value["message"], "keyring access denied");
    }

    #[test]
    fn test_serde_provider_shape() {
        let err = AppError::Provider("model overloaded".into(), Some(503));
        let json = serde_json::to_string(&err).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["code"], "provider");
        assert_eq!(value["message"], "model overloaded");
    }

    #[test]
    fn test_serde_permission_denied_shape() {
        let err = AppError::PermissionDenied("tool not allowed".into());
        let json = serde_json::to_string(&err).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["code"], "permission_denied");
        assert!(value["message"].as_str().unwrap().contains("tool not allowed"));
    }

    #[test]
    fn test_serde_json_properties() {
        let err = AppError::Http("bad gateway".into(), Some(502));
        let json = serde_json::to_string(&err).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        let obj = value.as_object().unwrap();
        assert_eq!(obj.len(), 4);
        assert!(obj.contains_key("code"));
        assert!(obj.contains_key("message"));
        assert!(obj.contains_key("retryable"));
        assert!(obj.contains_key("retryAfter"));
        // 502 >= 500 → retryable
        assert_eq!(value["retryable"], true);
    }

    #[test]
    fn test_serde_rate_limited_retry_after() {
        let err = AppError::RateLimited("slow down".into(), Some(30));
        let value: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&err).unwrap()).unwrap();
        assert_eq!(value["code"], "rate_limited");
        assert_eq!(value["retryable"], true);
        assert_eq!(value["retryAfter"], 30);
    }

    #[test]
    fn test_serde_not_retryable() {
        let err = AppError::NotFound("x".into());
        let value: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&err).unwrap()).unwrap();
        assert_eq!(value["retryable"], false);
        assert!(value["retryAfter"].is_null());
    }

    #[test]
    fn test_from_serde_json_error() {
        let json_err = serde_json::from_str::<serde_json::Value>("invalid json").unwrap_err();
        let app_err: AppError = json_err.into();
        assert!(matches!(app_err, AppError::Serialization(_)));
    }

    #[test]
    fn test_error_trait() {
        let err = AppError::NotFound("missing".into());
        let _: &dyn std::error::Error = &err;
    }

    #[test]
    fn test_debug_formatting() {
        let err = AppError::Keychain("debug test".into());
        let debug = format!("{err:?}");
        assert!(debug.contains("Keychain"));
        assert!(debug.contains("debug test"));
    }

    #[test]
    fn test_clone() {
        let err = AppError::NotFound("original".into());
        let cloned = err.clone();
        assert_eq!(format!("{err}"), format!("{cloned}"));
    }

    #[test]
    fn test_from_io_error_not_found() {
        let io = std::io::Error::new(std::io::ErrorKind::NotFound, "missing");
        let app: AppError = io.into();
        assert_eq!(app.code(), "not_found");
    }

    #[test]
    fn test_from_io_error_permission() {
        let io = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "nope");
        let app: AppError = io.into();
        assert_eq!(app.code(), "permission_denied");
    }

    #[test]
    fn test_from_io_error_other() {
        let io = std::io::Error::other("boom");
        let app: AppError = io.into();
        assert_eq!(app.code(), "internal");
    }

    #[test]
    fn test_display_formatting_internal() {
        let err = AppError::Internal("disk full".into());
        let display = format!("{err}");
        assert!(display.contains("Internal error"));
        assert!(display.contains("disk full"));
    }

    #[test]
    fn test_serde_internal_shape() {
        let err = AppError::Internal("something broke".into());
        let value: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&err).unwrap()).unwrap();
        assert_eq!(value["code"], "internal");
        assert_eq!(value["message"], "something broke");
        assert_eq!(value["retryable"], false);
        assert!(value["retryAfter"].is_null());
    }
}

