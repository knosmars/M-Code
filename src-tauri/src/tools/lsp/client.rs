//! One LSP server connection over stdio. Mirrors the shape of `tools/mcp.rs`
//! but uses Content-Length framing and adds document sync + readiness retry.

use super::framing;
use super::registry::Lang;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

/// Cap on messages read while waiting for a matching response, so a chatty
/// server can't loop forever.
const MAX_MESSAGES: usize = 5000;

pub struct LspClient {
    #[allow(dead_code)]
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: i64,
    /// uri -> last version sent.
    open_docs: HashMap<String, i64>,
}

impl LspClient {
    fn write_msg(&mut self, value: &Value) -> Result<(), String> {
        let frame = framing::encode(value);
        self.stdin
            .write_all(&frame)
            .and_then(|_| self.stdin.flush())
            .map_err(|e| format!("write to lsp server failed: {e}"))
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<(), String> {
        self.write_msg(&json!({ "jsonrpc": "2.0", "method": method, "params": params }))
    }

    /// Send a request, read frames until the matching `id`. Skips notifications;
    /// acks server→client requests with an empty result so the server proceeds.
    /// JSON-RPC errors return `Err("<code>: <message>")`.
    fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.next_id += 1;
        let id = self.next_id;
        self.write_msg(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))?;
        for _ in 0..MAX_MESSAGES {
            let msg = framing::decode(&mut self.stdout)?;
            if msg.get("method").is_some() {
                // Notification (no id) or server→client request (has id).
                if let Some(srv_id) = msg.get("id") {
                    let _ = self.write_msg(
                        &json!({ "jsonrpc": "2.0", "id": srv_id, "result": null }),
                    );
                }
                continue;
            }
            if msg.get("id").and_then(|v| v.as_i64()) == Some(id) {
                if let Some(err) = msg.get("error") {
                    let code = err.get("code").and_then(|c| c.as_i64()).unwrap_or(0);
                    let m = err.get("message").and_then(|m| m.as_str()).unwrap_or("error");
                    return Err(format!("{code}: {m}"));
                }
                return Ok(msg.get("result").cloned().unwrap_or(Value::Null));
            }
            // Response to an unrelated id: ignore.
        }
        Err("no matching lsp response".to_string())
    }

    /// Retry a query until it yields a non-empty result or the deadline passes.
    /// Retries on empty results and on ContentModified/Cancelled/NotInitialized
    /// errors (server still indexing).
    fn query_ready(
        &mut self,
        method: &str,
        params: Value,
        deadline: Instant,
    ) -> Result<Value, String> {
        loop {
            match self.request(method, params.clone()) {
                Ok(v) => {
                    let empty =
                        v.is_null() || v.as_array().map(|a| a.is_empty()).unwrap_or(false);
                    if !empty || Instant::now() >= deadline {
                        return Ok(v);
                    }
                }
                Err(e) => {
                    let retryable = e.starts_with("-32801") // ContentModified
                        || e.starts_with("-32802") // ServerCancelled
                        || e.starts_with("-32002"); // ServerNotInitialized
                    if !retryable {
                        return Err(e);
                    }
                    if Instant::now() >= deadline {
                        return Err("language server not ready (indexing), retry shortly".to_string());
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    /// Ensure the server has current content for `uri` (full-text sync).
    fn sync_doc(&mut self, uri: &str, language_id: &str, text: &str) -> Result<(), String> {
        if let Some(v) = self.open_docs.get_mut(uri) {
            *v += 1;
            let version = *v;
            self.notify(
                "textDocument/didChange",
                json!({
                    "textDocument": { "uri": uri, "version": version },
                    "contentChanges": [ { "text": text } ]
                }),
            )
        } else {
            self.open_docs.insert(uri.to_string(), 1);
            self.notify(
                "textDocument/didOpen",
                json!({
                    "textDocument": {
                        "uri": uri,
                        "languageId": language_id,
                        "version": 1,
                        "text": text
                    }
                }),
            )
        }
    }

    pub fn hover(
        &mut self,
        uri: &str,
        language_id: &str,
        text: &str,
        line: u32,
        character: u32,
        deadline: Instant,
    ) -> Result<Value, String> {
        self.sync_doc(uri, language_id, text)?;
        self.query_ready(
            "textDocument/hover",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }),
            deadline,
        )
    }

    pub fn definition(
        &mut self,
        uri: &str,
        language_id: &str,
        text: &str,
        line: u32,
        character: u32,
        deadline: Instant,
    ) -> Result<Value, String> {
        self.sync_doc(uri, language_id, text)?;
        self.query_ready(
            "textDocument/definition",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }),
            deadline,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn references(
        &mut self,
        uri: &str,
        language_id: &str,
        text: &str,
        line: u32,
        character: u32,
        include_declaration: bool,
        deadline: Instant,
    ) -> Result<Value, String> {
        self.sync_doc(uri, language_id, text)?;
        self.query_ready(
            "textDocument/references",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character },
                "context": { "includeDeclaration": include_declaration }
            }),
            deadline,
        )
    }

    /// Spawn the server for `lang`, run the initialize handshake.
    pub fn spawn(lang: Lang, root_uri: &str) -> Result<LspClient, String> {
        let (program, args) = lang.command();
        let mut cmd = Command::new(program);
        cmd.args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("failed to spawn {program}: {e}"))?;
        let stdin = child.stdin.take().ok_or("lsp server has no stdin")?;
        let stdout = BufReader::new(child.stdout.take().ok_or("lsp server has no stdout")?);
        let mut client = LspClient {
            child,
            stdin,
            stdout,
            next_id: 0,
            open_docs: HashMap::new(),
        };
        client.request(
            "initialize",
            json!({
                "processId": std::process::id(),
                "rootUri": root_uri,
                "capabilities": {
                    "textDocument": {
                        "hover": { "contentFormat": ["markdown", "plaintext"] },
                        "definition": {},
                        "references": {}
                    }
                },
                "clientInfo": { "name": "meyatu-code", "version": env!("CARGO_PKG_VERSION") }
            }),
        )?;
        client.notify("initialized", json!({}))?;
        Ok(client)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Manual integration smoke: needs `rust-analyzer` on PATH. Run with
    /// `cargo test lsp::client -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn smoke_rust_analyzer_hover() {
        let root = std::env::current_dir().unwrap();
        let root_uri = format!("file://{}", root.display());
        let mut client = LspClient::spawn(Lang::Rust, &root_uri).expect("spawn rust-analyzer");
        let file = root.join("src/tools/lsp/framing.rs");
        let text = std::fs::read_to_string(&file).unwrap();
        let uri = format!("file://{}", file.display());
        let (lnum, line) = text
            .lines()
            .enumerate()
            .find(|(_, l)| l.contains("pub fn encode"))
            .expect("encode fn present");
        let col = line.find("encode").unwrap();
        let deadline = Instant::now() + Duration::from_secs(60);
        let res = client
            .hover(&uri, "rust", &text, lnum as u32, col as u32, deadline)
            .expect("hover ok");
        eprintln!("HOVER RESULT: {res}");
        assert!(!res.is_null(), "expected non-null hover from rust-analyzer");
    }

    /// Manual integration smoke: needs `typescript-language-server` on PATH.
    /// Run with `cargo test lsp::client -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn smoke_typescript_hover() {
        let dir = std::env::temp_dir().join(format!("meyatu_lsp_ts_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.ts");
        let text = "export function greet(name: string): string {\n  return \"hi \" + name;\n}\nconst x = greet(\"a\");\n";
        std::fs::write(&file, text).unwrap();
        let root_uri = format!("file://{}", dir.display());
        let uri = format!("file://{}", file.display());

        let mut client =
            LspClient::spawn(Lang::TypeScript, &root_uri).expect("spawn typescript-language-server");
        // hover the `greet` call on line 3 (0-based), char 10
        let deadline = Instant::now() + Duration::from_secs(20);
        let res = client
            .hover(&uri, "typescript", text, 3, 10, deadline)
            .expect("hover ok");
        eprintln!("TS HOVER RESULT: {res}");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(!res.is_null(), "expected non-null hover from tsserver");
    }
}
