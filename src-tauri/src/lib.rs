//! MeyatuCode — Desktop AI coding assistant powered by Tauri 2.
//!
//! # Architecture
//! - **Provider layer** ([`provider`]) — multi-protocol adapters (OpenAI, Anthropic, Gemini)
//! - **Tool system** ([`tools`]) — 7 file/code tools gated by permission checks
//! - **Session store** ([`sessions`]) — SQLite-backed conversation history
//! - **Streaming IPC** — `Channel<StreamEvent>` bridge between Rust & TypeScript
//!
//! # Entry point
//! [`run()`] boots the Tauri application, registers all commands,
//! and initialises managed state.

mod commands;
mod error;
mod handlers;
mod keychain;
mod provider;
mod sessions;
mod stream;
mod tools;

use commands::AppState;
use sessions::DbState;
use tauri::Manager;

/// Open the session DB under `data_dir`. Returns an error (rather than
/// panicking) if the path is not valid UTF-8 or the DB cannot be opened.
fn open_session_db(data_dir: &std::path::Path) -> Result<DbState, String> {
    let db_path = data_dir.join("meyatu_sessions.db");
    let path_str = db_path
        .to_str()
        .ok_or_else(|| format!("db path is not valid UTF-8: {}", db_path.display()))?;
    DbState::new(path_str)
}

/// Boot the Tauri application.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
    .manage(AppState::new("com.meyatu.code"))
    .setup(|app| {
      let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("failed to resolve app data directory: {e}"))?;
      std::fs::create_dir_all(&data_dir)
        .map_err(|e| format!("failed to create app data directory: {e}"))?;
      app.handle().manage(open_session_db(&data_dir).map_err(|e| format!("failed to open session db: {e}"))?);

      app.handle().plugin(
        tauri_plugin_log::Builder::default()
          .level(log::LevelFilter::Info)
          .build(),
      )?;
      app.handle().plugin(tauri_plugin_dialog::init())?;
      app.handle().manage(tools::terminal::new_shell_store());
      app.handle().manage(tools::file_sync::new_file_event_bus());
      Ok(())
    })
    .invoke_handler(app_handler!())
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_session_db_creates_db_in_dir() {
        let dir = std::env::temp_dir().join(format!("meyatu_depanic_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let result = open_session_db(&dir);
        assert!(result.is_ok(), "expected Ok, got {result:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
