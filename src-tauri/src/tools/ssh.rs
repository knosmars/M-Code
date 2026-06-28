use serde::{Deserialize, Serialize};
use std::process::Command;
use std::time::{Duration, Instant};

pub fn shell_command(cmd: &str) -> (String, Vec<String>) {
    if cfg!(windows) {
        // Switch the console to UTF-8 (65001) first so non-UTF-8 Windows
        // locales (e.g. GBK) don't return mojibake; `&` still runs cmd after.
        (
            "cmd".to_string(),
            vec!["/C".to_string(), format!("chcp 65001>nul & {cmd}")],
        )
    } else {
        ("sh".to_string(), vec!["-c".to_string(), cmd.to_string()])
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SshExecResult {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub message: String,
}

/// Execute a command via SSH to a remote host.
/// Uses sshpass for password auth when available, falls back to prompt-based SSH.
/// Cross-platform: shell selection via cfg!(windows).
#[tauri::command]
pub async fn tool_ssh_exec(
    host: String,
    port: Option<u16>,
    username: Option<String>,
    password: Option<String>,
    key_path: Option<String>,
    command: Option<String>,
) -> Result<SshExecResult, String> {
    let port = port.unwrap_or(22);
    let username = username.unwrap_or_else(|| "root".to_string());
    let ssh_cmd = command.unwrap_or_else(|| "hostname".to_string());

    let has_key = key_path.as_ref().map(|p| !p.is_empty()).unwrap_or(false);
    if has_key {
        return ssh_with_key(&host, port, &username, key_path.as_deref().unwrap(), &ssh_cmd).await;
    }

    let has_sshpass = check_command_available(if cfg!(target_os = "windows") { "sshpass.exe" } else { "sshpass" });

    let has_password = password.as_ref().map(|p| !p.is_empty()).unwrap_or(false);

    // Password auth without sshpass → drive OpenSSH's SSH_ASKPASS (built into
    // Windows 10/11 OpenSSH and most Linux/macOS), so no external sshpass needed.
    if !has_sshpass && has_password {
        return ssh_with_askpass(&host, port, &username, password.as_deref().unwrap_or(""), &ssh_cmd).await;
    }

    let null_hosts = if cfg!(windows) { "NUL" } else { "/dev/null" };
    let full_cmd = if has_sshpass {
        let pass = password.unwrap_or_default();
        format!(
            "sshpass -p '{}' ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile={} -o ConnectTimeout=10 -p {} {}@{} {}",
            escape_single_quotes(&pass),
            null_hosts,
            port,
            username,
            host,
            ssh_cmd
        )
    } else {
        format!(
            "ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile={} -o ConnectTimeout=10 -p {} {}@{} {}",
            null_hosts, port, username, host, ssh_cmd
        )
    };

    let (shell, shell_args) = shell_command(&full_cmd);
    let child = Command::new(&shell)
        .args(&shell_args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn SSH process: {}", e))?;

    poll_ssh_child(child, &host, port).await
}

/// Run a password-authenticated SSH command without sshpass by pointing
/// OpenSSH at a temporary `SSH_ASKPASS` helper that echoes the password.
/// `SSH_ASKPASS_REQUIRE=force` makes OpenSSH 8.4+ (incl. Windows 10/11's bundled
/// client) use the helper even without a TTY. The helper file is deleted before
/// returning. No external dependency (sshpass/plink) required.
async fn ssh_with_askpass(
    host: &str,
    port: u16,
    username: &str,
    password: &str,
    ssh_cmd: &str,
) -> Result<SshExecResult, String> {
    let dir = std::env::temp_dir();
    let (script_path, body) = if cfg!(windows) {
        (
            dir.join(format!("meyatu_askpass_{}.bat", std::process::id())),
            // @echo OFF then echo the password; CRLF for cmd.
            format!("@echo off\r\necho {}\r\n", password),
        )
    } else {
        (
            dir.join(format!("meyatu_askpass_{}.sh", std::process::id())),
            format!("#!/bin/sh\nprintf '%s\\n' '{}'\n", escape_single_quotes(password)),
        )
    };

    if let Err(e) = std::fs::write(&script_path, body.as_bytes()) {
        return Ok(ssh_error(format!("Failed to create askpass helper: {e}")));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o700));
    }

    let null_hosts = if cfg!(windows) { "NUL" } else { "/dev/null" };
    let mut cmd = Command::new("ssh");
    cmd.arg("-o").arg("StrictHostKeyChecking=no")
        .arg("-o").arg(format!("UserKnownHostsFile={null_hosts}"))
        .arg("-o").arg("ConnectTimeout=10")
        .arg("-o").arg("NumberOfPasswordPrompts=1")
        .arg("-p").arg(port.to_string())
        .arg(format!("{username}@{host}"))
        .arg(ssh_cmd)
        .env("SSH_ASKPASS", &script_path)
        .env("SSH_ASKPASS_REQUIRE", "force")
        .env("DISPLAY", ":0") // some OpenSSH builds still gate askpass on DISPLAY
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let spawned = cmd.spawn();
    let result = match spawned {
        Ok(child) => poll_ssh_child(child, host, port).await,
        Err(e) => Ok(ssh_error(format!("Failed to spawn ssh: {e}. Is the OpenSSH client installed?"))),
    };
    let _ = std::fs::remove_file(&script_path);
    result
}

async fn ssh_with_key(
    host: &str,
    port: u16,
    username: &str,
    key_path: &str,
    ssh_cmd: &str,
) -> Result<SshExecResult, String> {
    let key = std::path::Path::new(key_path);
    if !key.exists() {
        return Ok(ssh_error(format!("SSH key file not found: {key_path}")));
    }

    let null_hosts = if cfg!(windows) { "NUL" } else { "/dev/null" };
    let mut cmd = Command::new("ssh");
    cmd.arg("-o").arg("StrictHostKeyChecking=no")
        .arg("-o").arg(format!("UserKnownHostsFile={null_hosts}"))
        .arg("-o").arg("ConnectTimeout=10")
        .arg("-o").arg("NumberOfPasswordPrompts=0")
        .arg("-i").arg(key_path)
        .arg("-p").arg(port.to_string())
        .arg(format!("{username}@{host}"))
        .arg(ssh_cmd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    match cmd.spawn() {
        Ok(child) => poll_ssh_child(child, host, port).await,
        Err(e) => Ok(ssh_error(format!("Failed to spawn ssh: {e}. Is the OpenSSH client installed?"))),
    }
}

fn ssh_error(message: String) -> SshExecResult {
    SshExecResult { success: false, stdout: String::new(), stderr: String::new(), exit_code: None, message }
}

/// Poll a spawned SSH child to completion with a 30s timeout.
async fn poll_ssh_child(
    mut child: std::process::Child,
    host: &str,
    port: u16,
) -> Result<SshExecResult, String> {
    let start = Instant::now();
    let timeout = Duration::from_secs(30);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let output = child.wait_with_output().map_err(|e| format!("Failed to read SSH output: {}", e))?;
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let exit_code = status.code();
                let clean_stderr = clean_ssh_stderr(&stderr);
                let success = status.success();
                return Ok(SshExecResult {
                    success,
                    stdout,
                    stderr: clean_stderr,
                    exit_code,
                    message: if success {
                        String::new()
                    } else {
                        format!("SSH command failed with exit code {:?}", exit_code)
                    },
                });
            }
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Ok(SshExecResult {
                        success: false,
                        stdout: String::new(),
                        stderr: String::new(),
                        exit_code: None,
                        message: format!(
                            "SSH connection to {} timed out after {} seconds. Check:\n\
                             - Host is reachable (ping {})\n\
                             - Port {} is open and SSH is running\n\
                             - Firewall rules allow the connection",
                            host, timeout.as_secs(), host, port
                        ),
                    });
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            Err(e) => {
                return Ok(ssh_error(format!("SSH process error: {}", e)));
            }
        }
    }
}

fn check_command_available(name: &str) -> bool {
    let (shell, shell_args) = if cfg!(windows) {
        ("cmd".to_string(), vec!["/C".to_string(), format!("where {}", name)])
    } else {
        ("sh".to_string(), vec!["-c".to_string(), format!("command -v {}", name)])
    };
    Command::new(&shell)
        .args(&shell_args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn escape_single_quotes(s: &str) -> String {
    s.replace('\'', "'\\''")
}

/// Clean up garbled SSH error messages and provide actionable English feedback.
fn clean_ssh_stderr(stderr: &str) -> String {
    let lower = stderr.to_lowercase();

    if lower.contains("connection refused") {
        format!("Connection refused: SSH is not running on the remote host or the port is wrong.\n{}", stderr)
    } else if lower.contains("connection timed out") || lower.contains("operation timed out") {
        format!("Connection timed out: The remote host is unreachable. Check network and firewall.\n{}", stderr)
    } else if lower.contains("host key verification failed") {
        format!("Host key verification failed. The remote host's fingerprint has changed or is unknown.\n{}", stderr)
    } else if lower.contains("permission denied") {
        format!("Permission denied: Invalid username, password, or SSH key.\n{}", stderr)
    } else if lower.contains("no route to host") {
        format!("No route to host: The remote host cannot be reached on the network.\n{}", stderr)
    } else if lower.contains("name or service not known") || lower.contains("could not resolve hostname") {
        format!("Hostname resolution failed: The remote host name could not be resolved.\n{}", stderr)
    } else {
        stderr.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_single_quotes() {
        assert_eq!(escape_single_quotes("hello"), "hello");
        assert_eq!(escape_single_quotes("it's"), "it'\\''s");
        assert_eq!(escape_single_quotes(""), "");
    }

    #[test]
    fn test_clean_ssh_stderr_connection_refused() {
        let result = clean_ssh_stderr("ssh: connect to host 1.2.3.4 port 22: Connection refused");
        assert!(result.contains("Connection refused"), "Result: {}", result);
    }

    #[test]
    fn test_clean_ssh_stderr_permission_denied() {
        let result = clean_ssh_stderr("Permission denied, please try again.");
        assert!(result.contains("Permission denied"), "Result: {}", result);
    }

    #[test]
    fn test_clean_ssh_stderr_timeout() {
        let result = clean_ssh_stderr("ssh: connect to host example.com port 22: Operation timed out");
        assert!(result.contains("Connection timed out"), "Result: {}", result);
    }

    #[test]
    fn test_ssh_error_helper() {
        let r = ssh_error("boom".to_string());
        assert!(!r.success);
        assert_eq!(r.message, "boom");
        assert!(r.stdout.is_empty() && r.stderr.is_empty());
    }

    #[test]
    #[ignore] // Requires actual SSH server
    fn test_ssh_localhost_key_auth() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(tool_ssh_exec(
            "localhost".to_string(),
            Some(22),
            Some(std::env::var("USER").unwrap_or_else(|_| "root".to_string())),
            None,
            None,
            Some("echo hello".to_string()),
        ));
        if let Ok(r) = result {
            // Either works (key auth) or fails with clear message
            assert!(!r.message.contains("sshpass"), "Should not mention sshpass when no password provided");
        }
    }
}
