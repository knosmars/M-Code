use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookConfig {
    /// The lifecycle event this hook responds to.
    pub event: String,
    /// Optional tool name filter (only for before_tool / after_tool events).
    #[serde(default)]
    pub tool: Option<String>,
    /// Shell command to execute.
    pub command: String,
    /// Timeout in milliseconds (default: 5000).
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
}

fn default_timeout() -> u64 {
    5000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HooksFile {
    pub hooks: Vec<HookConfig>,
}

/// Context passed to hook commands via environment variables.
#[derive(Debug, Clone, Serialize)]
pub struct HookContext {
    pub event: String,
    pub tool_name: Option<String>,
    pub tool_args: Option<String>,
    pub tool_result: Option<String>,
    pub tool_error: Option<String>,
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HookResult {
    pub event: String,
    pub tool: Option<String>,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub blocked: bool,
}

// ---------------------------------------------------------------------------
// Load hooks from .meyatu/hooks.json
// ---------------------------------------------------------------------------

fn load_hooks_config(workspace: &Path) -> Result<Option<HooksFile>, String> {
    let hooks_path = workspace.join(".meyatu").join("hooks.json");
    if !hooks_path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(&hooks_path)
        .map_err(|e| format!("Failed to read hooks config: {e}"))?;
    let config: HooksFile = serde_json::from_str(&content)
        .map_err(|e| format!("Invalid hooks.json: {e}"))?;
    Ok(Some(config))
}

// ---------------------------------------------------------------------------
// Run hooks for a given event
// ---------------------------------------------------------------------------

/// Run all hooks matching `event` (and optional `tool_name` filter).
/// Returns a list of results. If any hook exits non-zero with a blocked tool,
/// the caller should stop execution.
#[tauri::command]
pub fn tool_hooks_run(
    path: String,
    event: String,
    tool_name: Option<String>,
    tool_args: Option<String>,
    tool_result: Option<String>,
    tool_error: Option<String>,
    session_id: Option<String>,
) -> Result<Vec<HookResult>, String> {
    let workspace = super::resolve_workspace_path(&path)?;

    let config = match load_hooks_config(&workspace) {
        Ok(Some(c)) => c,
        Ok(None) => return Ok(vec![]),
        Err(e) => return Err(e),
    };

    let ctx = HookContext {
        event: event.clone(),
        tool_name,
        tool_args,
        tool_result,
        tool_error,
        session_id,
    };

    let mut results: Vec<HookResult> = Vec::new();

    for hook in &config.hooks {
        if hook.event != event {
            continue;
        }
        // If hook has a tool filter and this isn't a match, skip
        if let Some(ref filter) = hook.tool {
            if let Some(ref name) = ctx.tool_name {
                if name != filter.as_str() {
                    continue;
                }
            } else {
                // No tool context but hook wants one → skip
                continue;
            }
        }

        let timeout = Duration::from_millis(hook.timeout_ms);

        let output = execute_hook_command(&hook.command, &ctx, timeout);

        let exit_code = output.status.code().unwrap_or(-1);
        // Non-zero exit code on before_tool = block
        let blocked = event == "before_tool" && exit_code != 0;

        results.push(HookResult {
            event: event.clone(),
            tool: hook.tool.clone(),
            exit_code,
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            blocked,
        });
    }

    Ok(results)
}

fn execute_hook_command(command: &str, ctx: &HookContext, timeout: Duration) -> std::process::Output {
    // Build a child process with env vars for hook context
    let mut cmd = if cfg!(target_os = "windows") {
        let mut c = Command::new("cmd");
        c.args(["/C", command]);
        c
    } else {
        let mut c = Command::new("sh");
        c.args(["-c", command]);
        c
    };

    cmd.env("MEYATU_HOOK_EVENT", &ctx.event)
        .env(
            "MEYATU_HOOK_TOOL_NAME",
            ctx.tool_name.as_deref().unwrap_or(""),
        )
        .env(
            "MEYATU_HOOK_TOOL_ARGS",
            ctx.tool_args.as_deref().unwrap_or(""),
        )
        .env(
            "MEYATU_HOOK_TOOL_RESULT",
            ctx.tool_result.as_deref().unwrap_or(""),
        )
        .env(
            "MEYATU_HOOK_TOOL_ERROR",
            ctx.tool_error.as_deref().unwrap_or(""),
        )
        .env(
            "MEYATU_HOOK_SESSION_ID",
            ctx.session_id.as_deref().unwrap_or(""),
        )
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return std::process::Output {
                status: std::process::ExitStatus::default(),
                stdout: vec![],
                stderr: format!("Hook spawn error: {e}").into_bytes(),
            };
        }
    };

    if timeout.is_zero() {
        return match child.wait_with_output() {
            Ok(output) => output,
            Err(e) => std::process::Output {
                status: std::process::ExitStatus::default(),
                stdout: vec![],
                stderr: format!("Hook wait error: {e}").into_bytes(),
            },
        };
    }

    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut stdout = vec![];
                let mut stderr_buf = vec![];
                if let Some(mut s) = child.stdout.take() {
                    let _ = std::io::Read::read_to_end(&mut s, &mut stdout);
                }
                if let Some(mut s) = child.stderr.take() {
                    let _ = std::io::Read::read_to_end(&mut s, &mut stderr_buf);
                }
                return std::process::Output {
                    status,
                    stdout,
                    stderr: stderr_buf,
                };
            }
            Ok(None) if start.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return std::process::Output {
                    status: std::process::ExitStatus::default(),
                    stdout: vec![],
                    stderr: format!("Hook timed out after {timeout:?}").into_bytes(),
                };
            }
            Err(e) => {
                let _ = child.kill();
                return std::process::Output {
                    status: std::process::ExitStatus::default(),
                    stdout: vec![],
                    stderr: format!("Hook wait error: {e}").into_bytes(),
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup_hooks_workspace() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("meyatu_hooks_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(dir.join(".meyatu")).unwrap();
        dir
    }

    fn write_hooks_config(workspace: &Path, content: &str) {
        fs::write(workspace.join(".meyatu").join("hooks.json"), content).unwrap();
    }

    #[test]
    fn no_hooks_file_returns_empty() {
        let dir = setup_hooks_workspace();
        let result = tool_hooks_run(
            dir.to_string_lossy().to_string(),
            "before_tool".into(),
            Some("write_file".into()),
            None,
            None,
            None,
            None,
        )
        .unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn hook_matching_tool_runs() {
        let dir = setup_hooks_workspace();
        write_hooks_config(
            &dir,
            r#"{"hooks":[{"event":"before_tool","tool":"write_file","command":"echo blocked; exit 1","timeout_ms":5000}]}"#,
        );
        let result = tool_hooks_run(
            dir.to_string_lossy().to_string(),
            "before_tool".into(),
            Some("write_file".into()),
            Some("{\"path\":\"/test.txt\"}".into()),
            None,
            None,
            None,
        )
        .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].exit_code, 1);
        assert!(result[0].blocked);
    }

    #[test]
    fn hook_does_not_match_different_tool() {
        let dir = setup_hooks_workspace();
        write_hooks_config(
            &dir,
            r#"{"hooks":[{"event":"before_tool","tool":"write_file","command":"echo nope; exit 0","timeout_ms":5000}]}"#,
        );
        let result = tool_hooks_run(
            dir.to_string_lossy().to_string(),
            "before_tool".into(),
            Some("read_file".into()),
            None,
            None,
            None,
            None,
        )
        .unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn after_chat_event_runs_without_tool_filter() {
        let dir = setup_hooks_workspace();
        write_hooks_config(
            &dir,
            r#"{"hooks":[{"event":"after_chat","command":"echo done","timeout_ms":5000}]}"#,
        );
        let result = tool_hooks_run(
            dir.to_string_lossy().to_string(),
            "after_chat".into(),
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].exit_code, 0);
    }

    #[test]
    fn invalid_json_returns_error() {
        let dir = setup_hooks_workspace();
        fs::write(dir.join(".meyatu").join("hooks.json"), "not json").unwrap();
        let result = tool_hooks_run(
            dir.to_string_lossy().to_string(),
            "before_chat".into(),
            None, None, None, None, None,
        );
        assert!(result.is_err());
    }
}
