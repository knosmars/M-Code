//! Minimal MCP (Model Context Protocol) client over stdio.
//!
//! Spawns configured local MCP servers, performs the initialize handshake,
//! lists their tools, and proxies `tools/call`. Tools are surfaced to the
//! agent loop namespaced as `mcp__<server>__<tool>`.

use crate::error::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Mutex, OnceLock};

const PROTOCOL_VERSION: &str = "2024-11-05";
/// Cap on lines read while waiting for a matching JSON-RPC response, so a
/// chatty or misbehaving server can't loop forever.
const MAX_RESPONSE_LINES: usize = 2000;

// ── Config ──────────────────────────────────────────────────────────────

#[derive(Deserialize, Serialize, Clone)]
struct ServerSpec {
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: HashMap<String, String>,
    #[serde(default)]
    disabled: bool,
}

#[derive(Deserialize, Serialize, Default)]
struct McpConfig {
    #[serde(rename = "mcpServers", default)]
    servers: HashMap<String, ServerSpec>,
}

fn config_path() -> Option<PathBuf> {
    let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).ok()?;
    Some(PathBuf::from(home).join(".config/meyatu-code/mcp.json"))
}

fn read_config() -> McpConfig {
    config_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Read counterpart to `write_config_at`, used by the roundtrip test.
#[cfg(test)]
fn read_config_at(path: &Path) -> McpConfig {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write_config_at(path: &Path, cfg: &McpConfig) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(AppError::from)?;
    }
    let json = serde_json::to_string_pretty(cfg).map_err(AppError::from)?;
    std::fs::write(path, json).map_err(AppError::from)
}

// ── JSON-RPC framing (pure, unit-tested) ────────────────────────────────

fn build_request(id: i64, method: &str, params: Value) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }).to_string()
}

fn build_notification(method: &str, params: Value) -> String {
    json!({ "jsonrpc": "2.0", "method": method, "params": params }).to_string()
}

/// Parse a JSON-RPC line. Returns `(id, Ok(result) | Err(message))`, or `None`
/// when the line is not a response (e.g. a notification with no `id`).
fn parse_response(line: &str) -> Option<(i64, Result<Value, String>)> {
    let v: Value = serde_json::from_str(line).ok()?;
    let id = v.get("id")?.as_i64()?;
    if let Some(err) = v.get("error") {
        let msg = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown error")
            .to_string();
        return Some((id, Err(msg)));
    }
    Some((id, Ok(v.get("result").cloned().unwrap_or(Value::Null))))
}

// ── Server connection ───────────────────────────────────────────────────

struct McpTool {
    /// Namespaced name exposed to the agent (`mcp__<server>__<tool>`).
    name: String,
    description: String,
    parameters: Value,
}

struct McpServer {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: i64,
    tools: Vec<McpTool>,
}

impl McpServer {
    fn write_line(&mut self, line: &str) -> Result<(), String> {
        self.stdin
            .write_all(line.as_bytes())
            .and_then(|_| self.stdin.write_all(b"\n"))
            .and_then(|_| self.stdin.flush())
            .map_err(|e| format!("write to mcp server failed: {e}"))
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<(), String> {
        let line = build_notification(method, params);
        self.write_line(&line)
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.next_id += 1;
        let id = self.next_id;
        self.write_line(&build_request(id, method, params))?;
        for _ in 0..MAX_RESPONSE_LINES {
            let mut line = String::new();
            let n = self
                .stdout
                .read_line(&mut line)
                .map_err(|e| format!("read from mcp server failed: {e}"))?;
            if n == 0 {
                return Err("mcp server closed its output".to_string());
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Some((rid, res)) = parse_response(trimmed) {
                if rid == id {
                    return res;
                }
            }
            // Otherwise a notification or unrelated message — skip it.
        }
        Err("no matching mcp response".to_string())
    }

    fn call_tool(&mut self, raw_name: &str, arguments: Value) -> Result<String, String> {
        let result = self.request("tools/call", json!({ "name": raw_name, "arguments": arguments }))?;
        // MCP returns `content: [{ type: "text", text }]`.
        let text = result
            .get("content")
            .and_then(|c| c.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| item.get("text").and_then(|t| t.as_str()))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        if text.is_empty() {
            Ok(serde_json::to_string(&result).unwrap_or_default())
        } else {
            Ok(text)
        }
    }
}

fn spawn_and_init(server_name: &str, spec: &ServerSpec) -> Result<McpServer, String> {
    let mut cmd = Command::new(&spec.command);
    cmd.args(&spec.args);
    for (k, v) in &spec.env {
        cmd.env(k, v);
    }
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("failed to spawn mcp server '{}' ({}): {e}", server_name, spec.command))?;
    let stdin = child.stdin.take().ok_or("mcp server has no stdin")?;
    let stdout = BufReader::new(child.stdout.take().ok_or("mcp server has no stdout")?);
    let mut server = McpServer { child, stdin, stdout, next_id: 0, tools: Vec::new() };

    server.request(
        "initialize",
        json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": { "name": "meyatu-code", "version": env!("CARGO_PKG_VERSION") }
        }),
    )?;
    server.notify("notifications/initialized", json!({}))?;

    let result = server.request("tools/list", json!({}))?;
    let raw_tools = result.get("tools").and_then(|t| t.as_array()).cloned().unwrap_or_default();
    server.tools = raw_tools
        .iter()
        .filter_map(|t| {
            let raw_name = t.get("name")?.as_str()?;
            Some(McpTool {
                name: format!("mcp__{server_name}__{raw_name}"),
                description: t.get("description").and_then(|d| d.as_str()).unwrap_or("").to_string(),
                parameters: t.get("inputSchema").cloned().unwrap_or(json!({ "type": "object" })),
            })
        })
        .collect();
    Ok(server)
}

fn registry() -> &'static Mutex<HashMap<String, McpServer>> {
    static REGISTRY: OnceLock<Mutex<HashMap<String, McpServer>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Ensure the named server is connected, then run `f` against it.
fn with_server<T>(
    reg: &mut HashMap<String, McpServer>,
    name: &str,
    config: &McpConfig,
    f: impl FnOnce(&mut McpServer) -> Result<T, String>,
) -> Result<T, String> {
    if !reg.contains_key(name) {
        let spec = config
            .servers
            .get(name)
            .ok_or_else(|| format!("unknown mcp server '{name}'"))?;
        let server = spawn_and_init(name, spec)?;
        reg.insert(name.to_string(), server);
    }
    f(reg.get_mut(name).ok_or("mcp server missing after connect")?)
}

/// Kill and drop a live server connection, if present.
fn disconnect(reg: &mut HashMap<String, McpServer>, name: &str) {
    if let Some(mut server) = reg.remove(name) {
        let _ = server.child.kill();
    }
}

// ── Config mutators (pure, unit-tested) ─────────────────────────────────

fn validate_new_name(name: &str, cfg: &McpConfig) -> AppResult<()> {
    let n = name.trim();
    if n.is_empty() {
        return Err(AppError::Internal("server name must not be empty".into()));
    }
    if n.contains("__") {
        return Err(AppError::Internal(
            "server name must not contain '__' (namespace separator)".into(),
        ));
    }
    if n.chars().any(|c| c.is_whitespace()) {
        return Err(AppError::Internal("server name must not contain whitespace".into()));
    }
    if cfg.servers.contains_key(n) {
        return Err(AppError::Internal(format!("server '{n}' already exists")));
    }
    Ok(())
}

fn add_server(
    cfg: &mut McpConfig,
    name: &str,
    command: &str,
    args: Vec<String>,
    env: HashMap<String, String>,
) -> AppResult<()> {
    validate_new_name(name, cfg)?;
    if command.trim().is_empty() {
        return Err(AppError::Internal("command must not be empty".into()));
    }
    cfg.servers.insert(
        name.trim().to_string(),
        ServerSpec { command: command.to_string(), args, env, disabled: false },
    );
    Ok(())
}

fn remove_server(cfg: &mut McpConfig, name: &str) -> AppResult<()> {
    if cfg.servers.remove(name).is_none() {
        return Err(AppError::NotFound(format!("mcp server '{name}' not found")));
    }
    Ok(())
}

fn set_disabled(cfg: &mut McpConfig, name: &str, disabled: bool) -> AppResult<()> {
    let spec = cfg
        .servers
        .get_mut(name)
        .ok_or_else(|| AppError::NotFound(format!("mcp server '{name}' not found")))?;
    spec.disabled = disabled;
    Ok(())
}

// ── Tauri commands ──────────────────────────────────────────────────────

/// Connect every configured server (best effort) and return aggregated tool
/// definitions as JSON: `[{ name, description, parameters }]`.
#[tauri::command]
pub async fn tool_mcp_list_tools() -> Result<String, String> {
    tokio::task::spawn_blocking(|| {
        let config = read_config();
        let mut reg = registry().lock().map_err(|_| "mcp registry poisoned")?;
        let mut all: Vec<Value> = Vec::new();
        for (name, spec) in &config.servers {
            if spec.disabled {
                continue;
            }
            if !reg.contains_key(name) {
                match spawn_and_init(name, spec) {
                    Ok(server) => {
                        reg.insert(name.clone(), server);
                    }
                    Err(e) => {
                        log::warn!("[mcp] server '{name}' init failed: {e}");
                        continue;
                    }
                }
            }
            if let Some(server) = reg.get(name) {
                for t in &server.tools {
                    all.push(json!({
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters,
                    }));
                }
            }
        }
        serde_json::to_string(&all).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Call a namespaced MCP tool (`mcp__<server>__<tool>`) with JSON arguments.
#[tauri::command]
pub async fn tool_mcp_call(name: String, arguments: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let rest = name.strip_prefix("mcp__").ok_or("not an mcp tool name")?;
        let (server_name, raw_tool) = rest.split_once("__").ok_or("malformed mcp tool name")?;
        let args: Value = serde_json::from_str(&arguments).unwrap_or_else(|_| json!({}));
        let config = read_config();
        let mut reg = registry().lock().map_err(|_| "mcp registry poisoned")?;
        with_server(&mut reg, server_name, &config, |server| server.call_tool(raw_tool, args))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Configured servers + connection state, for diagnostics / UI.
#[tauri::command]
pub async fn tool_mcp_status() -> Result<String, String> {
    tokio::task::spawn_blocking(|| {
        let config = read_config();
        let reg = registry().lock().map_err(|_| "mcp registry poisoned")?;
        let servers: Vec<Value> = config
            .servers
            .iter()
            .map(|(name, spec)| {
                json!({
                    "name": name,
                    "connected": reg.contains_key(name),
                    "toolCount": reg.get(name).map(|s| s.tools.len()).unwrap_or(0),
                    "disabled": spec.disabled,
                })
            })
            .collect();
        serde_json::to_string(&servers).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// List configured servers (independent of connection state): JSON
/// `[{ name, command, args, env, disabled }]`, sorted by name.
#[tauri::command]
pub async fn mcp_config_list() -> AppResult<String> {
    tokio::task::spawn_blocking(|| {
        let config = read_config();
        let mut servers: Vec<Value> = config
            .servers
            .iter()
            .map(|(name, spec)| {
                json!({
                    "name": name,
                    "command": spec.command,
                    "args": spec.args,
                    "env": spec.env,
                    "disabled": spec.disabled,
                })
            })
            .collect();
        servers.sort_by(|a, b| {
            a["name"].as_str().unwrap_or("").cmp(b["name"].as_str().unwrap_or(""))
        });
        serde_json::to_string(&servers).map_err(AppError::from)
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?
}

#[tauri::command]
pub async fn mcp_config_add(
    name: String,
    command: String,
    args: Vec<String>,
    env: HashMap<String, String>,
) -> AppResult<()> {
    tokio::task::spawn_blocking(move || {
        let path = config_path()
            .ok_or_else(|| AppError::Internal("cannot resolve mcp config path".into()))?;
        let mut cfg = read_config();
        add_server(&mut cfg, &name, &command, args, env)?;
        write_config_at(&path, &cfg)
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?
}

#[tauri::command]
pub async fn mcp_config_remove(name: String) -> AppResult<()> {
    tokio::task::spawn_blocking(move || {
        let path = config_path()
            .ok_or_else(|| AppError::Internal("cannot resolve mcp config path".into()))?;
        let mut cfg = read_config();
        remove_server(&mut cfg, &name)?;
        write_config_at(&path, &cfg)?;
        let mut reg = registry()
            .lock()
            .map_err(|_| AppError::Internal("mcp registry poisoned".into()))?;
        disconnect(&mut reg, &name);
        Ok(())
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?
}

#[tauri::command]
pub async fn mcp_config_set_disabled(name: String, disabled: bool) -> AppResult<()> {
    tokio::task::spawn_blocking(move || {
        let path = config_path()
            .ok_or_else(|| AppError::Internal("cannot resolve mcp config path".into()))?;
        let mut cfg = read_config();
        set_disabled(&mut cfg, &name, disabled)?;
        write_config_at(&path, &cfg)?;
        if disabled {
            let mut reg = registry()
                .lock()
                .map_err(|_| AppError::Internal("mcp registry poisoned".into()))?;
            disconnect(&mut reg, &name);
        }
        Ok(())
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_request_shape() {
        let line = build_request(7, "tools/list", json!({ "a": 1 }));
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["id"], 7);
        assert_eq!(v["method"], "tools/list");
        assert_eq!(v["params"]["a"], 1);
    }

    #[test]
    fn notification_has_no_id() {
        let line = build_notification("notifications/initialized", json!({}));
        let v: Value = serde_json::from_str(&line).unwrap();
        assert!(v.get("id").is_none());
        assert_eq!(v["method"], "notifications/initialized");
    }

    #[test]
    fn parse_result_response() {
        let line = r#"{"jsonrpc":"2.0","id":3,"result":{"tools":[]}}"#;
        let (id, res) = parse_response(line).unwrap();
        assert_eq!(id, 3);
        assert!(res.unwrap()["tools"].is_array());
    }

    #[test]
    fn parse_error_response() {
        let line = r#"{"jsonrpc":"2.0","id":4,"error":{"code":-32601,"message":"method not found"}}"#;
        let (id, res) = parse_response(line).unwrap();
        assert_eq!(id, 4);
        assert_eq!(res.unwrap_err(), "method not found");
    }

    #[test]
    fn parse_notification_returns_none() {
        let line = r#"{"jsonrpc":"2.0","method":"notifications/message","params":{}}"#;
        assert!(parse_response(line).is_none());
    }

    #[test]
    fn parse_garbage_returns_none() {
        assert!(parse_response("not json").is_none());
    }

    #[test]
    fn add_server_validates() {
        let mut cfg = McpConfig::default();
        add_server(&mut cfg, "fs", "npx", vec![], HashMap::new()).unwrap();
        assert!(cfg.servers.contains_key("fs"));
        // duplicate
        assert!(add_server(&mut cfg, "fs", "x", vec![], HashMap::new()).is_err());
        // empty name
        assert!(add_server(&mut cfg, "  ", "x", vec![], HashMap::new()).is_err());
        // contains namespace separator
        assert!(add_server(&mut cfg, "a__b", "x", vec![], HashMap::new()).is_err());
        // whitespace in name
        assert!(add_server(&mut cfg, "a b", "x", vec![], HashMap::new()).is_err());
        // empty command
        assert!(add_server(&mut cfg, "ok", "  ", vec![], HashMap::new()).is_err());
    }

    #[test]
    fn remove_and_set_disabled_mutate() {
        let mut cfg = McpConfig::default();
        add_server(&mut cfg, "fs", "npx", vec![], HashMap::new()).unwrap();
        set_disabled(&mut cfg, "fs", true).unwrap();
        assert!(cfg.servers["fs"].disabled);
        set_disabled(&mut cfg, "fs", false).unwrap();
        assert!(!cfg.servers["fs"].disabled);
        remove_server(&mut cfg, "fs").unwrap();
        assert!(cfg.servers.is_empty());
        // missing targets error
        assert!(remove_server(&mut cfg, "nope").is_err());
        assert!(set_disabled(&mut cfg, "nope", true).is_err());
    }

    #[test]
    fn config_write_read_roundtrip() {
        let dir = std::env::temp_dir().join("mcp-test-roundtrip");
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("mcp.json");
        let mut cfg = McpConfig::default();
        cfg.servers.insert(
            "fs".to_string(),
            ServerSpec {
                command: "npx".to_string(),
                args: vec!["-y".to_string(), "server-fs".to_string()],
                env: HashMap::new(),
                disabled: true,
            },
        );
        write_config_at(&path, &cfg).unwrap();
        let back = read_config_at(&path);
        let s = back.servers.get("fs").unwrap();
        assert_eq!(s.command, "npx");
        assert_eq!(s.args, vec!["-y".to_string(), "server-fs".to_string()]);
        assert!(s.disabled);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
