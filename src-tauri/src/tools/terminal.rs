use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;

use crate::tools::resolve_workspace_path;

const MAX_SESSIONS: usize = 5;
const SESSION_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const MAX_OUTPUT_BYTES: usize = 100_000;
const READ_CHUNK_TIMEOUT_MS: u64 = 50;
const MAX_QUIET_ITERATIONS: u32 = 3;

pub(crate) struct ShellSession {
    child: Child,
    stdin: ChildStdin,
    stdout: ChildStdout,
    stderr: ChildStderr,
    created_at: Instant,
    last_activity: Instant,
}

pub type ShellStore = Arc<Mutex<HashMap<String, Arc<Mutex<ShellSession>>>>>;

pub fn new_shell_store() -> ShellStore {
    Arc::new(Mutex::new(HashMap::new()))
}

fn spawn_shell(cwd: &str) -> Result<ShellSession, String> {
    let mut cmd = if cfg!(target_os = "windows") {
        Command::new("cmd")
    } else {
        Command::new("sh")
    };

    // Disable ANSI colors and set UTF-8 locale to prevent garbling on non-ASCII output.
    let mut cmd_builder = cmd.current_dir(cwd);
    if !cfg!(target_os = "windows") {
        cmd_builder = cmd_builder
            .env("LANG", "en_US.UTF-8")
            .env("LC_ALL", "en_US.UTF-8")
            .env("TERM", "dumb");
    }
    let mut child = cmd_builder
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to start shell: {e}"))?;

    let stdin = child.stdin.take().ok_or("Failed to open stdin")?;
    let stdout = child.stdout.take().ok_or("Failed to open stdout")?;
    let stderr = child.stderr.take().ok_or("Failed to open stderr")?;

    Ok(ShellSession {
        child,
        stdin,
        stdout,
        stderr,
        created_at: Instant::now(),
        last_activity: Instant::now(),
    })
}

async fn cleanup_expired(shells: &ShellStore) {
    let mut map = shells.lock().await;
    let now = Instant::now();
    let expired: Vec<String> = map
        .iter()
        .filter_map(|(id, session)| {
            let guard = session.try_lock();
            match guard {
                Ok(s) => {
                    if now.duration_since(s.last_activity) > SESSION_TIMEOUT {
                        Some(id.clone())
                    } else {
                        None
                    }
                }
                Err(_) => None,
            }
        })
        .collect();

    for id in expired {
        if let Some(session) = map.remove(&id) {
            if let Ok(mut s) = session.try_lock() {
                let _ = s.child.kill().await;
                let _ = s.child.wait().await;
            }
        }
    }
}

async fn read_output(
    session: &mut ShellSession,
    max_bytes: usize,
) -> Result<(String, String), String> {
    let mut out = Vec::new();
    let mut err = Vec::new();
    let mut buf = [0u8; 4096];
    let mut quiet_iterations = 0;
    let chunk_timeout = Duration::from_millis(READ_CHUNK_TIMEOUT_MS);

    loop {
        let mut got_data = false;

        match tokio::time::timeout(chunk_timeout, session.stdout.read(&mut buf)).await {
            Ok(Ok(0)) => {}
            Ok(Ok(n)) => {
                let add = n.min(max_bytes.saturating_sub(out.len() + err.len()));
                out.extend_from_slice(&buf[..add]);
                got_data = true;
            }
            Ok(Err(e)) => return Err(format!("stdout read error: {e}")),
            Err(_) => {}
        }

        if out.len() + err.len() >= max_bytes {
            break;
        }

        match tokio::time::timeout(chunk_timeout, session.stderr.read(&mut buf)).await {
            Ok(Ok(0)) => {}
            Ok(Ok(n)) => {
                let add = n.min(max_bytes.saturating_sub(out.len() + err.len()));
                err.extend_from_slice(&buf[..add]);
                got_data = true;
            }
            Ok(Err(e)) => return Err(format!("stderr read error: {e}")),
            Err(_) => {}
        }

        if out.len() + err.len() >= max_bytes {
            break;
        }

        if got_data {
            quiet_iterations = 0;
        } else {
            quiet_iterations += 1;
            if quiet_iterations >= MAX_QUIET_ITERATIONS {
                break;
            }
        }
    }

    Ok((
        String::from_utf8_lossy(&out).to_string(),
        String::from_utf8_lossy(&err).to_string(),
    ))
}

// ---------------------------------------------------------------------------
// Internal implementations (testable without Tauri State wrapper)
// ---------------------------------------------------------------------------

async fn terminal_start_impl(
    session_id: String,
    cwd: Option<String>,
    shells: &ShellStore,
) -> Result<String, String> {
    cleanup_expired(shells).await;

    let mut map = shells.lock().await;

    // Idempotent restart: if a session with this id already exists, replace it.
    // The frontend reuses a stable session id across React re-mounts / workspace
    // changes; its cleanup `terminal_stop` is fire-and-forget, so a fresh
    // `terminal_start` can race ahead of it and otherwise collide with the old
    // entry ("Session already exists"). Kill the old child and drop it first.
    if let Some(old) = map.remove(&session_id) {
        if let Ok(mut s) = old.try_lock() {
            let _ = s.child.kill().await;
            let _ = s.child.wait().await;
        }
    }

    if map.len() >= MAX_SESSIONS {
        return Err(format!(
            "Maximum number of concurrent terminal sessions ({MAX_SESSIONS}) reached. Stop an existing session first."
        ));
    }

    let workdir = cwd.unwrap_or_else(|| ".".to_string());
    let safe_cwd = resolve_workspace_path(&workdir).map_err(|e| format!("Path error: {e}"))?;

    let session = spawn_shell(&safe_cwd.to_string_lossy())?;
    let session = Arc::new(Mutex::new(session));
    map.insert(session_id.clone(), session);

    Ok(format!("Session started: {session_id}"))
}

async fn terminal_send_impl(
    session_id: String,
    input: String,
    shells: &ShellStore,
) -> Result<String, String> {
    let session_arc = {
        let map = shells.lock().await;
        map.get(&session_id)
            .cloned()
            .ok_or_else(|| format!("Session '{session_id}' not found"))?
    };

    let mut session = session_arc.lock().await;

    session
        .stdin
        .write_all(input.as_bytes())
        .await
        .map_err(|e| format!("Failed to write to stdin: {e}"))?;
    if !input.ends_with('\n') {
        session
            .stdin
            .write_all(b"\n")
            .await
            .map_err(|e| format!("Failed to write newline: {e}"))?;
    }
    session
        .stdin
        .flush()
        .await
        .map_err(|e| format!("Failed to flush stdin: {e}"))?;

    let (stdout_data, stderr_data) = read_output(&mut session, MAX_OUTPUT_BYTES).await?;
    session.last_activity = Instant::now();

    let mut result = stdout_data;
    if !stderr_data.is_empty() {
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str(&format!("[stderr]\n{stderr_data}"));
    }

    Ok(result)
}

async fn terminal_stop_impl(
    session_id: String,
    shells: &ShellStore,
) -> Result<(), String> {
    let session_arc = {
        let mut map = shells.lock().await;
        map.remove(&session_id)
            .ok_or_else(|| format!("Session '{session_id}' not found"))?
    };

    let mut session = session_arc.lock().await;
    let _ = session.child.kill().await;
    let _ = session.child.wait().await;
    Ok(())
}

async fn terminal_list_impl(shells: &ShellStore) -> Result<String, String> {
    cleanup_expired(shells).await;
    let map = shells.lock().await;
    if map.is_empty() {
        return Ok("No active terminal sessions".to_string());
    }
    let lines: Vec<String> = map
        .iter()
        .map(|(id, session)| {
            let age = session
                .try_lock()
                .map(|g| format!("{:.0}s", g.created_at.elapsed().as_secs()))
                .unwrap_or_default();
            format!("{id} ({age})")
        })
        .collect();
    Ok(lines.join("\n"))
}

// ---------------------------------------------------------------------------
// Tauri command wrappers
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn tool_terminal_start(
    session_id: String,
    cwd: Option<String>,
    shells: tauri::State<'_, ShellStore>,
) -> Result<String, String> {
    terminal_start_impl(session_id, cwd, &shells).await
}

#[tauri::command]
pub async fn tool_terminal_send(
    session_id: String,
    input: String,
    shells: tauri::State<'_, ShellStore>,
) -> Result<String, String> {
    terminal_send_impl(session_id, input, &shells).await
}

#[tauri::command]
pub async fn tool_terminal_stop(
    session_id: String,
    shells: tauri::State<'_, ShellStore>,
) -> Result<(), String> {
    terminal_stop_impl(session_id, &shells).await
}

#[tauri::command]
pub async fn tool_terminal_list(
    shells: tauri::State<'_, ShellStore>,
) -> Result<String, String> {
    terminal_list_impl(&shells).await
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_store() -> ShellStore {
        Arc::new(Mutex::new(HashMap::new()))
    }

    #[tokio::test]
    async fn terminal_start_creates_session() {
        let store = setup_store();
        let result = terminal_start_impl("test1".to_string(), Some("/tmp".to_string()), &store).await;
        assert!(result.is_ok(), "{:?}", result);
        assert!(result.unwrap().contains("Session started"));

        let map = store.lock().await;
        assert!(map.contains_key("test1"));
    }

    #[tokio::test]
    async fn terminal_send_receives_output() {
        let store = setup_store();
        terminal_start_impl("test2".to_string(), Some("/tmp".to_string()), &store)
            .await
            .unwrap();

        let result =
            terminal_send_impl("test2".to_string(), "echo hello_terminal".to_string(), &store).await;
        assert!(result.is_ok(), "{:?}", result);
        let output = result.unwrap();
        assert!(
            output.contains("hello_terminal"),
            "Expected output to contain 'hello_terminal', got: {}",
            output
        );
    }

    #[tokio::test]
    async fn terminal_stop_cleans_up() {
        let store = setup_store();
        terminal_start_impl("test3".to_string(), Some("/tmp".to_string()), &store)
            .await
            .unwrap();

        let result = terminal_stop_impl("test3".to_string(), &store).await;
        assert!(result.is_ok());

        let map = store.lock().await;
        assert!(!map.contains_key("test3"));
    }
}
