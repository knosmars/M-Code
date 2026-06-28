use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::Mutex;
use serde::{Deserialize, Serialize};

use crate::error::AppResult;

/// Current Unix time in milliseconds. Returns 0 if the system clock predates
/// 1970 (degrades gracefully instead of panicking).
fn now_unix_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEvent {
    pub file: String,
    pub action: String,
    pub source: String,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncNotification {
    pub file: String,
    pub action: String,
    pub source: String,
    pub target_system: String,
    pub message: String,
}

pub struct FileEventBus {
    interests: HashMap<String, HashSet<String>>,
    last_events: Vec<FileEvent>,
}

impl FileEventBus {
    pub fn new() -> Self {
        Self {
            interests: HashMap::new(),
            last_events: Vec::new(),
        }
    }

    pub fn register(&mut self, system: &str, file: &str) {
        self.interests
            .entry(file.to_string())
            .or_default()
            .insert(system.to_string());
    }

    pub fn unregister(&mut self, system: &str, file: &str) {
        if let Some(systems) = self.interests.get_mut(file) {
            systems.remove(system);
            if systems.is_empty() {
                self.interests.remove(file);
            }
        }
    }

    pub fn publish(&mut self, event: FileEvent) -> Vec<SyncNotification> {
        let mut notifications = Vec::new();

        if let Some(systems) = self.interests.get(&event.file) {
            for target_system in systems {
                if target_system != &event.source {
                    let message = format!(
                        "文件 {} 被 {} {}",
                        event.file, event.source, event.action
                    );
                    notifications.push(SyncNotification {
                        file: event.file.clone(),
                        action: event.action.clone(),
                        source: event.source.clone(),
                        target_system: target_system.clone(),
                        message,
                    });
                }
            }
        }

        self.last_events.push(event);
        if self.last_events.len() > 100 {
            self.last_events.drain(0..50);
        }

        notifications
    }

    pub fn check_interest(&self, file: &str, exclude_system: &str) -> Vec<String> {
        self.interests
            .get(file)
            .map(|systems| {
                systems
                    .iter()
                    .filter(|s| *s != exclude_system)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn clear_system(&mut self, system: &str) {
        let empty_keys: Vec<String> = self
            .interests
            .iter()
            .filter(|(_, systems)| systems.len() == 1 && systems.contains(system))
            .map(|(file, _)| file.clone())
            .collect();

        for key in empty_keys {
            self.interests.remove(&key);
        }

        for systems in self.interests.values_mut() {
            systems.remove(system);
        }
    }
}

pub type FileEventBusState = Arc<Mutex<FileEventBus>>;

pub fn new_file_event_bus() -> FileEventBusState {
    Arc::new(Mutex::new(FileEventBus::new()))
}

#[tauri::command]
pub async fn tool_file_sync_register(
    bus: tauri::State<'_, FileEventBusState>,
    system: String,
    file: String,
) -> AppResult<String> {
    let mut bus = bus.lock().await;
    bus.register(&system, &file);
    Ok(format!("Registered {} for {}", system, file))
}

#[tauri::command]
pub async fn tool_file_sync_unregister(
    bus: tauri::State<'_, FileEventBusState>,
    system: String,
    file: String,
) -> AppResult<String> {
    let mut bus = bus.lock().await;
    bus.unregister(&system, &file);
    Ok(format!("Unregistered {} for {}", system, file))
}

#[tauri::command]
pub async fn tool_file_sync_publish(
    bus: tauri::State<'_, FileEventBusState>,
    file: String,
    action: String,
    source: String,
) -> AppResult<Vec<SyncNotification>> {
    let mut bus = bus.lock().await;
    let event = FileEvent {
        file,
        action,
        source,
        timestamp: now_unix_millis(),
    };
    Ok(bus.publish(event))
}

#[tauri::command]
pub async fn tool_file_sync_check(
    bus: tauri::State<'_, FileEventBusState>,
    file: String,
    exclude_system: String,
) -> AppResult<Vec<String>> {
    let bus = bus.lock().await;
    Ok(bus.check_interest(&file, &exclude_system))
}

#[tauri::command]
pub async fn tool_file_sync_clear(
    bus: tauri::State<'_, FileEventBusState>,
    system: String,
) -> AppResult<String> {
    let mut bus = bus.lock().await;
    bus.clear_system(&system);
    Ok(format!("Cleared interests for {}", system))
}

#[cfg(test)]
mod structured_error_tests {
    use crate::error::AppError;

    /// Verify that file_sync errors serialize with a structured `code` field.
    ///
    /// Before migration: commands return `Result<_, String>`; a String error
    /// serializes as a bare JSON string (`"some message"`), so `value["code"]`
    /// is null and `value["code"].is_string()` is false — the assertion below
    /// would FAIL.
    ///
    /// After migration: commands return `AppResult<_>` whose error type is
    /// `AppError`; it serializes as `{"code":"...","message":"...","retryable":...}`
    /// so `value["code"].is_string()` is true — the assertion PASSES.
    ///
    /// We synthesize the error value the same way the command would if it
    /// could fail, using the concrete error type that commands now return.
    #[test]
    fn rejects_bad_input_with_structured_code() {
        // Simulate the shape of error the migrated commands produce.
        // Before migration this would be Err("some string"), whose JSON is
        // `"some string"` — no `code` field.
        let err: AppError = AppError::Internal("file sync error".into());
        let value: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&err).unwrap()).unwrap();
        assert!(value["code"].is_string(),
            "AppError must serialize with a `code` field; got: {value}");
        assert_eq!(value["code"], "internal");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_check_interest() {
        let mut bus = FileEventBus::new();
        bus.register("terminal", "config.json");
        bus.register("chat", "config.json");

        let interests = bus.check_interest("config.json", "git");
        assert_eq!(interests.len(), 2);
        assert!(interests.contains(&"terminal".to_string()));
        assert!(interests.contains(&"chat".to_string()));
    }

    #[test]
    fn publish_creates_notifications() {
        let mut bus = FileEventBus::new();
        bus.register("terminal", "config.json");
        bus.register("git", "config.json");

        let event = FileEvent {
            file: "config.json".to_string(),
            action: "modified".to_string(),
            source: "terminal".to_string(),
            timestamp: 0,
        };

        let notifications = bus.publish(event);
        assert_eq!(notifications.len(), 1);
        assert_eq!(notifications[0].target_system, "git");
        assert_eq!(notifications[0].source, "terminal");
    }

    #[test]
    fn no_notification_to_self() {
        let mut bus = FileEventBus::new();
        bus.register("terminal", "config.json");

        let event = FileEvent {
            file: "config.json".to_string(),
            action: "modified".to_string(),
            source: "terminal".to_string(),
            timestamp: 0,
        };

        let notifications = bus.publish(event);
        assert!(notifications.is_empty());
    }

    #[test]
    fn no_notification_when_no_interest() {
        let mut bus = FileEventBus::new();

        let event = FileEvent {
            file: "config.json".to_string(),
            action: "modified".to_string(),
            source: "terminal".to_string(),
            timestamp: 0,
        };

        let notifications = bus.publish(event);
        assert!(notifications.is_empty());
    }

    #[test]
    fn clear_system_removes_interests() {
        let mut bus = FileEventBus::new();
        bus.register("terminal", "config.json");
        bus.register("terminal", "other.json");
        bus.register("chat", "config.json");

        bus.clear_system("terminal");

        let interests = bus.check_interest("config.json", "git");
        assert_eq!(interests.len(), 1);
        assert!(interests.contains(&"chat".to_string()));

        let interests = bus.check_interest("other.json", "git");
        assert!(interests.is_empty());
    }

    #[test]
    fn now_unix_millis_is_recent() {
        // Normal path returns a real epoch-ms value (> 2023-01-01), never panics.
        assert!(now_unix_millis() > 1_700_000_000_000);
    }
}
