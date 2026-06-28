//! SQLite-backed session persistence.
//!
//! Per DEVELOPMENT_GUIDE §16, session history is stored locally
//! in SQLite. Each session row holds metadata + token stats;
//! messages are stored in a child table with a foreign key.
//!
//! # Schema
//! ```sql
//! CREATE TABLE sessions (id, title, provider, model, ...tokens...);
//! CREATE TABLE messages (session_id, msg_id, role, content, ...);
//! ```
//!
//! The Tauri commands accept/return JSON strings so the frontend
//! can serialise its TypeScript `Session` objects without a Rust
//! mirror struct.

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

/// A single message row for the `messages` table.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MessageRow {
    id: String,
    role: String,
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    timestamp: i64,
}

/// Full session document as received from / sent to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionDoc {
    id: String,
    title: String,
    messages: Vec<MessageRow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    status: serde_json::Value,
    tokens: TokenRow,
    #[serde(alias = "createdAt")]
    created_at: i64,
    #[serde(alias = "updatedAt")]
    updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenRow {
    prompt_tokens: i64,
    completion_tokens: i64,
    total_tokens: i64,
}

/// Thin wrapper around a `rusqlite::Connection` that provides
/// typed CRUD for session documents.
#[derive(Debug)]
pub struct SessionStore {
    conn: Connection,
}

impl SessionStore {
    /// Open (or create) the SQLite database at `path` and run migrations.
    pub fn open(path: &str) -> Result<Self, String> {
        let conn = Connection::open(path).map_err(|e| format!("failed to open db: {e}"))?;

        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .map_err(|e| format!("pragma: {e}"))?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS sessions (
                id            TEXT PRIMARY KEY,
                title         TEXT NOT NULL DEFAULT 'New Chat',
                provider      TEXT,
                model         TEXT,
                status_json   TEXT NOT NULL DEFAULT '{\"type\":\"idle\"}',
                prompt_tokens     INTEGER NOT NULL DEFAULT 0,
                completion_tokens INTEGER NOT NULL DEFAULT 0,
                total_tokens      INTEGER NOT NULL DEFAULT 0,
                created_at    INTEGER NOT NULL,
                updated_at    INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS messages (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id   TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                msg_id       TEXT NOT NULL,
                role         TEXT NOT NULL,
                content      TEXT NOT NULL DEFAULT '',
                tool_calls   TEXT,
                tool_call_id TEXT,
                name         TEXT,
                timestamp    INTEGER NOT NULL
            );",
        )
        .map_err(|e| format!("migration: {e}"))?;

        // Migration: add name column for existing databases (ignore error if exists)
        let _ = conn.execute_batch("ALTER TABLE messages ADD COLUMN name TEXT;");

        Ok(Self { conn })
    }

    /// Persist (upsert) a single session and its messages.
    pub fn save_session(&self, json: &str) -> Result<(), String> {
        let doc: SessionDoc =
            serde_json::from_str(json).map_err(|e| format!("parse session json: {e}"))?;

        let status_json =
            serde_json::to_string(&doc.status).map_err(|e| format!("status json: {e}"))?;

        self.conn
            .execute(
                "INSERT INTO sessions (id, title, provider, model, status_json,
                 prompt_tokens, completion_tokens, total_tokens,
                 created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT(id) DO UPDATE SET
                   title=excluded.title,
                   provider=excluded.provider,
                   model=excluded.model,
                   status_json=excluded.status_json,
                   prompt_tokens=excluded.prompt_tokens,
                   completion_tokens=excluded.completion_tokens,
                   total_tokens=excluded.total_tokens,
                   updated_at=excluded.updated_at",
                params![
                    doc.id,
                    doc.title,
                    doc.provider,
                    doc.model,
                    status_json,
                    doc.tokens.prompt_tokens,
                    doc.tokens.completion_tokens,
                    doc.tokens.total_tokens,
                    doc.created_at,
                    doc.updated_at,
                ],
            )
            .map_err(|e| format!("insert session: {e}"))?;

        // Replace all messages for this session (delete + re-insert).
        self.conn
            .execute(
                "DELETE FROM messages WHERE session_id = ?1",
                params![doc.id],
            )
            .map_err(|e| format!("delete messages: {e}"))?;

        for msg in &doc.messages {
            let tc_json = msg
                .tool_calls
                .as_ref()
                .map(serde_json::to_string)
                .transpose()
                .map_err(|e| format!("tool_calls json: {e}"))?;

            self.conn
                .execute(
                    "INSERT INTO messages (session_id, msg_id, role, content,
                     tool_calls, tool_call_id, name, timestamp)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![
                        doc.id,
                        msg.id,
                        msg.role,
                        msg.content,
                        tc_json,
                        msg.tool_call_id,
                        msg.name,
                        msg.timestamp,
                    ],
                )
                .map_err(|e| format!("insert message: {e}"))?;
        }

        Ok(())
    }

    /// Load all sessions (with their messages) as a JSON array.
    pub fn load_sessions(&self) -> Result<String, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, title, provider, model, status_json,
                 prompt_tokens, completion_tokens, total_tokens,
                 created_at, updated_at
                 FROM sessions ORDER BY updated_at DESC",
            )
            .map_err(|e| format!("prepare sessions: {e}"))?;

        let session_rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,   // id
                    row.get::<_, String>(1)?,   // title
                    row.get::<_, Option<String>>(2)?, // provider
                    row.get::<_, Option<String>>(3)?, // model
                    row.get::<_, String>(4)?,   // status_json
                    row.get::<_, i64>(5)?,      // prompt_tokens
                    row.get::<_, i64>(6)?,      // completion_tokens
                    row.get::<_, i64>(7)?,      // total_tokens
                    row.get::<_, i64>(8)?,      // created_at
                    row.get::<_, i64>(9)?,      // updated_at
                ))
            })
            .map_err(|e| format!("query sessions: {e}"))?;

        let mut sessions: Vec<serde_json::Value> = Vec::new();

        for row in session_rows {
            let (id, title, provider, model, status_json, pt, ct, tt, ca, ua) =
                row.map_err(|e| format!("row: {e}"))?;

            let status: serde_json::Value =
                serde_json::from_str(&status_json).map_err(|e| format!("status parse: {e}"))?;

            // Load messages for this session.
            let mut msg_stmt = self
                .conn
                .prepare(
                    "SELECT msg_id, role, content, tool_calls, tool_call_id, name, timestamp
                     FROM messages WHERE session_id = ?1 ORDER BY id ASC",
                )
                .map_err(|e| format!("prepare messages: {e}"))?;

            let msg_rows = msg_stmt
                .query_map(params![id], |row| {
                    Ok(serde_json::json!({
                        "id": row.get::<_, String>(0)?,
                        "role": row.get::<_, String>(1)?,
                        "content": row.get::<_, String>(2)?,
                        "toolCalls": row.get::<_, Option<String>>(3)?
                            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok()),
                        "toolCallId": row.get::<_, Option<String>>(4)?,
                        "name": row.get::<_, Option<String>>(5)?,
                        "timestamp": row.get::<_, i64>(6)?,
                    }))
                })
                .map_err(|e| format!("query messages: {e}"))?;

            let messages: Vec<serde_json::Value> = msg_rows
                .filter_map(|r| r.ok())
                .collect();

            sessions.push(serde_json::json!({
                "id": id,
                "title": title,
                "messages": messages,
                "provider": provider,
                "model": model,
                "status": status,
                "tokens": {
                    "promptTokens": pt,
                    "completionTokens": ct,
                    "totalTokens": tt,
                },
                "createdAt": ca,
                "updatedAt": ua,
            }));
        }

        serde_json::to_string(&sessions).map_err(|e| format!("serialize sessions: {e}"))
    }

    /// Delete a session and its messages (cascaded).
    pub fn delete_session(&self, id: &str) -> Result<(), String> {
        self.conn
            .execute("DELETE FROM sessions WHERE id = ?1", params![id])
            .map_err(|e| format!("delete session: {e}"))?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

use std::sync::Mutex;
use tauri::State;

/// Wrapper so Tauri can manage the store as managed state.
#[derive(Debug)]
pub struct DbState {
    pub store: Mutex<SessionStore>,
}

impl DbState {
    pub fn new(path: &str) -> Result<Self, String> {
        Ok(Self {
            store: Mutex::new(SessionStore::open(path)?),
        })
    }
}

/// Persist a session document (JSON string) to SQLite.
#[tauri::command]
pub fn db_save_session(state: State<'_, DbState>, json: String) -> Result<(), String> {
    let store = state.store.lock().map_err(|e| format!("lock: {e}"))?;
    store.save_session(&json)
}

/// Load all sessions from SQLite, returned as a JSON array string.
#[tauri::command]
pub fn db_load_sessions(state: State<'_, DbState>) -> Result<String, String> {
    let store = state.store.lock().map_err(|e| format!("lock: {e}"))?;
    store.load_sessions()
}

/// Delete a session from SQLite.
#[tauri::command]
pub fn db_delete_session(state: State<'_, DbState>, id: String) -> Result<(), String> {
    let store = state.store.lock().map_err(|e| format!("lock: {e}"))?;
    store.delete_session(&id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> SessionStore {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("meyatu_test_{}.db", uuid::Uuid::new_v4()));
        let path_str = path.to_string_lossy().to_string();
        let store = SessionStore::open(&path_str).unwrap();
        // Cleanup on drop — best-effort.
        std::panic::set_hook(Box::new(move |_| {}));
        store
    }

    #[test]
    fn test_roundtrip_single_session() {
        let store = temp_store();
        let json = r#"{
            "id": "sess-1",
            "title": "Test Chat",
            "messages": [
                {"id": "msg-1", "role": "user", "content": "hello", "timestamp": 1000},
                {"id": "msg-2", "role": "assistant", "content": "hi!", "timestamp": 2000}
            ],
            "provider": "openai",
            "model": "gpt-4",
            "status": {"type": "done"},
            "tokens": {"promptTokens": 10, "completionTokens": 5, "totalTokens": 15},
            "createdAt": 1000,
            "updatedAt": 2000
        }"#;

        store.save_session(json).unwrap();
        let loaded = store.load_sessions().unwrap();
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&loaded).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0]["id"], "sess-1");
        assert_eq!(parsed[0]["title"], "Test Chat");
        assert_eq!(parsed[0]["messages"].as_array().unwrap().len(), 2);
        assert_eq!(parsed[0]["tokens"]["totalTokens"], 15);
    }

    #[test]
    fn test_update_session() {
        let store = temp_store();
        let json1 = r#"{
            "id": "sess-1", "title": "Old Title", "messages": [],
            "status": {"type":"idle"},
            "tokens": {"promptTokens":0, "completionTokens":0, "totalTokens":0},
            "createdAt": 1000, "updatedAt": 2000
        }"#;
        store.save_session(json1).unwrap();

        let json2 = r#"{
            "id": "sess-1", "title": "New Title", "messages": [
                {"id":"m1","role":"user","content":"x","timestamp":3000}
            ],
            "status": {"type":"done"},
            "tokens": {"promptTokens":5, "completionTokens":3, "totalTokens":8},
            "createdAt": 1000, "updatedAt": 4000
        }"#;
        store.save_session(json2).unwrap();

        let loaded = store.load_sessions().unwrap();
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&loaded).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0]["title"], "New Title");
        assert_eq!(parsed[0]["tokens"]["totalTokens"], 8);
        assert_eq!(parsed[0]["messages"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_delete_session() {
        let store = temp_store();
        store.save_session(r#"{"id":"a","title":"A","messages":[],"status":{"type":"idle"},"tokens":{"promptTokens":0,"completionTokens":0,"totalTokens":0},"createdAt":1,"updatedAt":1}"#).unwrap();
        store.save_session(r#"{"id":"b","title":"B","messages":[],"status":{"type":"idle"},"tokens":{"promptTokens":0,"completionTokens":0,"totalTokens":0},"createdAt":2,"updatedAt":2}"#).unwrap();

        store.delete_session("a").unwrap();

        let loaded = store.load_sessions().unwrap();
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&loaded).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0]["id"], "b");
    }

    #[test]
    fn test_load_empty() {
        let store = temp_store();
        let loaded = store.load_sessions().unwrap();
        assert_eq!(loaded, "[]");
    }

    #[test]
    fn test_tool_calls_message() {
        let store = temp_store();
        let json = r#"{
            "id": "sess-tc",
            "title": "Tool Chat",
            "messages": [
                {"id":"m1","role":"assistant","content":"","toolCalls":[{"id":"call_1","name":"read_file","arguments":"{\"path\":\"/tmp/x\"}"}],"timestamp":1000},
                {"id":"m2","role":"tool","content":"file contents","toolCallId":"call_1","timestamp":2000}
            ],
            "status": {"type":"done"},
            "tokens": {"promptTokens":20,"completionTokens":10,"totalTokens":30},
            "createdAt": 1000,
            "updatedAt": 2000
        }"#;
        store.save_session(json).unwrap();

        let loaded = store.load_sessions().unwrap();
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&loaded).unwrap();
        let msgs = parsed[0]["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 2);
        assert!(msgs[0]["toolCalls"].is_array());
        assert_eq!(msgs[0]["toolCalls"][0]["name"], "read_file");
        assert_eq!(msgs[1]["toolCallId"], "call_1");
    }

    #[test]
    fn test_multiple_sessions_ordered() {
        let store = temp_store();
        store.save_session(r#"{"id":"x","title":"X","messages":[],"status":{"type":"idle"},"tokens":{"promptTokens":0,"completionTokens":0,"totalTokens":0},"createdAt":1,"updatedAt":1}"#).unwrap();
        store.save_session(r#"{"id":"y","title":"Y","messages":[],"status":{"type":"idle"},"tokens":{"promptTokens":0,"completionTokens":0,"totalTokens":0},"createdAt":2,"updatedAt":100}"#).unwrap();
        store.save_session(r#"{"id":"z","title":"Z","messages":[],"status":{"type":"idle"},"tokens":{"promptTokens":0,"completionTokens":0,"totalTokens":0},"createdAt":3,"updatedAt":50}"#).unwrap();

        let loaded = store.load_sessions().unwrap();
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&loaded).unwrap();
        assert_eq!(parsed.len(), 3);
        // Ordered by updated_at DESC
        assert_eq!(parsed[0]["id"], "y");
        assert_eq!(parsed[1]["id"], "z");
        assert_eq!(parsed[2]["id"], "x");
    }

    #[test]
    fn test_bad_json_rejected() {
        let store = temp_store();
        let result = store.save_session("not json");
        assert!(result.is_err());
    }
}
