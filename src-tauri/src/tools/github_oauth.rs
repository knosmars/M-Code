use serde::Serialize;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc;
use std::time::Duration;
use url::Url;

const GITHUB_CLIENT_ID: &str = env!("GITHUB_OAUTH_CLIENT_ID");
const GITHUB_CLIENT_SECRET: &str = env!("GITHUB_OAUTH_CLIENT_SECRET");

const OAUTH_TIMEOUT_SECS: u64 = 300;
const OAUTH_SCOPES: &str = "repo read:org user";
const TOKEN_FILENAME: &str = "github_token";

/// Returned after a successful OAuth login / token storage / gh configuration.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthStatus {
    pub logged_in: bool,
    pub username: Option<String>,
}

// ═══════════════════════════════════════════════════════════════════════
//  Tauri commands
// ═══════════════════════════════════════════════════════════════════════

/// Run the full OAuth flow entirely in the backend:
///   1. Spin up a local TCP server on 127.0.0.1 and open the browser to
///      GitHub's authorize page with a CSRF state.
///   2. Capture the redirect, validate the state, and exchange the
///      authorization code for an access token via reqwest (rustls-tls).
///   3. Store the token, configure the `gh` CLI, and return login status.
///
/// The `client_secret` never leaves the backend process — it is read from a
/// compile-time constant and used only for the server-to-server token
/// exchange here.
#[tauri::command]
pub async fn tool_github_oauth_login() -> Result<OAuthStatus, String> {
    let client_id = GITHUB_CLIENT_ID;
    let client_secret = GITHUB_CLIENT_SECRET;
    if client_id.is_empty() {
        return Err("GitHub OAuth 凭据未配置，请联系开发者。".to_string());
    }

    // ── 1.  Start local callback server on a random port ──────────────
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("无法启动本地服务器: {}", e))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("无法获取端口: {}", e))?
        .port();

    let (tx, rx) = mpsc::channel::<(String, String)>();

    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0u8; 8192];
            if let Ok(n) = stream.read(&mut buf) {
                let request = String::from_utf8_lossy(&buf[..n]);
                if let Some((code, state)) = extract_code_and_state(&request) {
                    let _ = tx.send((code, state));
                } else {
                    let _ = tx.send((String::new(), String::new()));
                }
            }
            let body = concat!(
                "<html><body style='display:flex;align-items:center;",
                "justify-content:center;height:100vh;margin:0;",
                "font-family:-apple-system,BlinkMacSystemFont,",
                "'Segoe UI',Helvetica,Arial,sans-serif;",
                "background:#0d1117;color:#c9d1d9'>",
                "<div style='text-align:center'>",
                "<h2>✅ GitHub 登录成功！</h2>",
                "<p>你现在可以关闭此窗口，返回编辑器。</p>",
                "</div></body></html>"
            );
            let response = format!(
                "HTTP/1.1 200 OK\r\n\
                 Content-Type: text/html; charset=utf-8\r\n\
                 Content-Length: {}\r\n\
                 Connection: close\r\n\
                 \r\n\
                 {}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    });

    // ── 2.  Build the GitHub Authorize URL ────────────────────────────
    let redirect_uri = format!("http://127.0.0.1:{}/callback", port);
    let csrf_state = uuid::Uuid::new_v4().to_string();

    let authorize_url = Url::parse_with_params(
        "https://github.com/login/oauth/authorize",
        &[
            ("client_id", client_id),
            ("redirect_uri", redirect_uri.as_str()),
            ("scope", OAUTH_SCOPES),
            ("state", &csrf_state),
            ("response_type", "code"),
        ],
    )
    .map_err(|e| format!("无法构建授权 URL: {}", e))?
    .to_string();

    // ── 3.  Open the browser ──────────────────────────────────────────
    webbrowser::open(&authorize_url)
        .map_err(|e| format!("无法打开浏览器: {}", e))?;

    // ── 4.  Wait for the authorization code ───────────────────────────
    let (code, state) = rx
        .recv_timeout(Duration::from_secs(OAUTH_TIMEOUT_SECS))
        .map_err(|_| {
            "登录超时，请在 5 分钟内通过浏览器完成 GitHub 授权。".to_string()
        })?;

    if code.is_empty() {
        return Err("未能从回调中获取授权码，请重试。".to_string());
    }

    // CSRF check
    if state != csrf_state {
        return Err("CSRF 校验失败 — 可能存在中间人攻击，请重试。".to_string());
    }

    // ── 5.  Exchange the code for an access token (server-to-server) ──
    let access_token =
        exchange_code_for_token(client_id, client_secret, &code, &redirect_uri).await?;

    // ── 6.  Store token + configure gh CLI ────────────────────────────
    store_token(&access_token)?;
    if let Err(e) = configure_gh_cli(&access_token) {
        eprintln!("[github_oauth] 配置 gh CLI 失败（不影响登录）: {e}");
    }

    let username = fetch_username_via_gh().unwrap_or(None);
    Ok(OAuthStatus {
        logged_in: true,
        username,
    })
}

/// Exchange an OAuth authorization code for an access token via GitHub's
/// token endpoint. Runs server-side so the client secret stays in-process.
async fn exchange_code_for_token(
    client_id: &str,
    client_secret: &str,
    code: &str,
    redirect_uri: &str,
) -> Result<String, String> {
    let resp = reqwest::Client::new()
        .post("https://github.com/login/oauth/access_token")
        .header("Accept", "application/json")
        .form(&[
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("code", code),
            ("redirect_uri", redirect_uri),
        ])
        .send()
        .await
        .map_err(|e| format!("token 交换请求失败: {}", e))?;

    let data: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("解析 token 响应失败: {}", e))?;

    if let Some(err) = data.get("error").and_then(|v| v.as_str()) {
        let desc = data
            .get("error_description")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        return Err(format!("GitHub 拒绝授权: {} {}", err, desc));
    }

    data.get("access_token")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .ok_or_else(|| "token 响应中缺少 access_token".to_string())
}

/// Store an access token obtained by the frontend, configure the gh CLI,
/// and return login status with username.
#[tauri::command(rename_all = "camelCase")]
pub async fn tool_github_oauth_store_token(access_token: String) -> Result<OAuthStatus, String> {
    if access_token.is_empty() {
        return Err("access_token 为空".to_string());
    }

    // ── Store token ───────────────────────────────────────────────────
    store_token(&access_token)?;

    // ── Configure gh CLI (best-effort) ────────────────────────────────
    if let Err(e) = configure_gh_cli(&access_token) {
        eprintln!("[github_oauth] 配置 gh CLI 失败（不影响登录）: {e}");
    }

    // ── Fetch username via gh CLI (no reqwest dependency) ─────────────
    let username = fetch_username_via_gh().unwrap_or(None);

    Ok(OAuthStatus {
        logged_in: true,
        username,
    })
}

/// Log out: remove stored token, log out of gh CLI.
#[tauri::command]
pub async fn tool_github_oauth_logout() -> Result<(), String> {
    remove_token()?;
    let _ = Command::new("gh")
        .args(["auth", "logout"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    Ok(())
}

/// Check login status from the stored token.
#[tauri::command]
pub async fn tool_github_oauth_status() -> Result<OAuthStatus, String> {
    // We only need to know a token exists; the username comes from the gh CLI.
    if !matches!(read_stored_token(), Ok(Some(_))) {
        return Ok(OAuthStatus {
            logged_in: false,
            username: None,
        });
    }

    let username = fetch_username_via_gh().unwrap_or(None);
    Ok(OAuthStatus {
        logged_in: true,
        username,
    })
}

// ═══════════════════════════════════════════════════════════════════════
//  Internal helpers
// ═══════════════════════════════════════════════════════════════════════

/// Parse the authorization code AND state from an HTTP GET request line.
/// Uses the `url` crate for correct percent-decoding (incl. multi-byte UTF-8).
fn extract_code_and_state(request: &str) -> Option<(String, String)> {
    let first_line = request.lines().next()?;
    let query_start = first_line.find('?')?;
    let query_end = first_line[query_start..]
        .find(' ')
        .map(|i| query_start + i)
        .unwrap_or(first_line.len());
    let query_str = &first_line[query_start + 1..query_end];

    let parsed = Url::parse(&format!("http://localhost/?{}", query_str)).ok()?;
    let mut code = None;
    let mut state = None;
    for (k, v) in parsed.query_pairs() {
        match k.as_ref() {
            "code" => code = Some(v.into_owned()),
            "state" => state = Some(v.into_owned()),
            _ => {}
        }
    }
    Some((code?, state?))
}

/// ── Token file helpers ──────────────────────────────────────────────
fn token_dir() -> Result<PathBuf, String> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map_err(|_| "无法确定用户主目录".to_string())?;
    Ok(PathBuf::from(home).join(".config/meyatu-code"))
}

fn token_path() -> Result<PathBuf, String> {
    Ok(token_dir()?.join(TOKEN_FILENAME))
}

fn ensure_token_dir() -> Result<(), String> {
    let dir = token_dir()?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("无法创建配置目录: {}", e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).ok();
    }
    Ok(())
}

fn store_token(token: &str) -> Result<(), String> {
    ensure_token_dir()?;
    let path = token_path()?;
    std::fs::write(&path, token)
        .map_err(|e| format!("写入令牌文件失败: {}", e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).ok();
    }
    Ok(())
}

fn remove_token() -> Result<(), String> {
    let path = token_path()?;
    if path.exists() {
        std::fs::remove_file(&path)
            .map_err(|e| format!("删除令牌文件失败: {}", e))?;
    }
    Ok(())
}

fn read_stored_token() -> Result<Option<String>, String> {
    let path = token_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("读取令牌文件失败: {}", e))?;
    let trimmed = content.trim().to_string();
    if trimmed.is_empty() {
        let _ = std::fs::remove_file(&path);
        Ok(None)
    } else {
        Ok(Some(trimmed))
    }
}

/// Pipe the token into `gh auth login --with-token`.
fn configure_gh_cli(token: &str) -> Result<(), String> {
    let mut child = Command::new("gh")
        .args(["auth", "login", "--with-token"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                "未找到 gh CLI — 请先安装 GitHub CLI (gh)".to_string()
            } else {
                format!("无法启动 gh CLI: {}", e)
            }
        })?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(token.as_bytes())
            .map_err(|e| format!("写入 token 到 gh 失败: {}", e))?;
        drop(stdin);
    }

    let output = child
        .wait_with_output()
        .map_err(|e| format!("等待 gh 完成失败: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("already logged in") {
            return Ok(());
        }
        return Err(format!("gh CLI 配置失败: {}", stderr.trim()));
    }

    Ok(())
}

/// Fetch the GitHub username via `gh api user` (no reqwest dependency,
/// avoids potential TLS issues on Windows).
fn fetch_username_via_gh() -> Result<Option<String>, String> {
    let output = Command::new("gh")
        .args(["api", "user", "--jq", ".login"])
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                "gh CLI 未安装".to_string()
            } else {
                format!("gh api user 失败: {}", e)
            }
        })?;

    if output.status.success() {
        let login = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !login.is_empty() {
            return Ok(Some(login));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_code_and_state() {
        let req = "GET /callback?code=abc123&state=xyz HTTP/1.1\r\nHost: localhost\r\n";
        assert_eq!(
            extract_code_and_state(req),
            Some(("abc123".to_string(), "xyz".to_string()))
        );
    }

    #[test]
    fn decodes_percent_encoding() {
        // %2B is a literal '+', not a space.
        let req = "GET /callback?code=a%2Bb&state=s+t HTTP/1.1";
        assert_eq!(
            extract_code_and_state(req),
            Some(("a+b".to_string(), "s t".to_string()))
        );
    }

    #[test]
    fn decodes_multibyte_utf8() {
        // Regression guard: the old byte-wise decoder produced mojibake here.
        let req = "GET /callback?code=%E4%B8%AD&state=x HTTP/1.1";
        assert_eq!(
            extract_code_and_state(req),
            Some(("中".to_string(), "x".to_string()))
        );
    }

    #[test]
    fn none_when_code_missing() {
        let req = "GET /callback?state=xyz HTTP/1.1";
        assert_eq!(extract_code_and_state(req), None);
    }
}
