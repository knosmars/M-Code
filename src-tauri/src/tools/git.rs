use crate::error::{AppError, AppResult};
use serde::Serialize;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

fn run_git(workspace: &PathBuf, args: &[&str]) -> AppResult<String> {
    let output = Command::new("git")
        .current_dir(workspace)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(AppError::from)?;

    let result = output.wait_with_output().map_err(AppError::from)?;

    let stdout = String::from_utf8_lossy(&result.stdout);
    let stderr = String::from_utf8_lossy(&result.stderr);

    if !result.status.success() {
        let msg = if stderr.is_empty() { &*stdout } else { &*stderr };
        return Err(AppError::Internal(format!("Git command failed: {}", msg.trim())));
    }

    Ok(stdout.into_owned())
}

/// Show the working tree status (short format).
#[tauri::command]
pub fn tool_git_status(path: String) -> AppResult<String> {
    let workspace = super::resolve_workspace_path(&path).map_err(AppError::PermissionDenied)?;
    run_git(&workspace, &["status", "--short", "--branch"])
}

/// Show unstaged diff.
#[tauri::command]
pub fn tool_git_diff(path: String) -> AppResult<String> {
    let workspace = super::resolve_workspace_path(&path).map_err(AppError::PermissionDenied)?;
    run_git(&workspace, &["diff"])
}

/// Show staged diff (changes ready to commit).
#[tauri::command]
pub fn tool_git_diff_staged(path: String) -> AppResult<String> {
    let workspace = super::resolve_workspace_path(&path).map_err(AppError::PermissionDenied)?;
    run_git(&workspace, &["diff", "--staged"])
}

/// Show recent commit history.
#[tauri::command]
pub fn tool_git_log(path: String, n: Option<usize>) -> AppResult<String> {
    let workspace = super::resolve_workspace_path(&path).map_err(AppError::PermissionDenied)?;
    let count = n.unwrap_or(10).to_string();
    run_git(
        &workspace,
        &["log", "-n", &count, "--oneline", "--decorate"],
    )
}

/// Stage and commit changes. Requires a non-empty message. Staged files
/// must be explicitly added via run_command git add first.
#[tauri::command]
pub fn tool_git_commit(path: String, message: String) -> AppResult<String> {
    if message.trim().is_empty() {
        return Err(AppError::Internal("Commit message must not be empty".into()));
    }
    let workspace = super::resolve_workspace_path(&path).map_err(AppError::PermissionDenied)?;
    run_git(&workspace, &["commit", "-m", &message])
}

/// Create or switch branches.
#[tauri::command]
pub fn tool_git_branch(path: String, name: String, create: Option<bool>) -> AppResult<String> {
    if name.trim().is_empty() {
        return Err(AppError::Internal("Branch name must not be empty".into()));
    }
    let workspace = super::resolve_workspace_path(&path).map_err(AppError::PermissionDenied)?;
    if create.unwrap_or(false) {
        run_git(&workspace, &["checkout", "-b", &name])
    } else {
        run_git(&workspace, &["checkout", &name])
    }
}

/// Push current branch to origin.
#[tauri::command]
pub fn tool_git_push(path: String) -> AppResult<String> {
    let workspace = super::resolve_workspace_path(&path).map_err(AppError::PermissionDenied)?;
    run_git(&workspace, &["push", "origin", "HEAD"])
}

/// Create a GitHub PR from the current branch using gh CLI.
/// Returns the PR URL on success, or an error if gh is not installed / auth failed.
#[tauri::command]
pub fn tool_gh_pr_create(
    path: String,
    title: String,
    body: Option<String>,
    base: Option<String>,
) -> AppResult<String> {
    let workspace = super::resolve_workspace_path(&path).map_err(AppError::PermissionDenied)?;
    let mut args = vec!["pr", "create", "--title", &title];
    if let Some(ref b) = body {
        args.push("--body");
        args.push(b);
    }
    if let Some(ref b) = base {
        args.push("--base");
        args.push(b);
    }
    run_gh(&workspace, &args)
}

/// Structured remote info for the Git popup status bar.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitRemoteInfo {
    /// Full remote URL (e.g. `https://github.com/user/repo.git` or `git@github.com:user/repo.git`).
    pub remote_url: String,
    /// Owner / organization extracted from the URL.
    pub owner: String,
    /// Repository name (without `.git` suffix).
    pub repo: String,
    /// Current branch name.
    pub branch: String,
}

/// Return structured info about the current git remote and branch.
/// Used by the Git popup status bar to show repo name / owner.
#[tauri::command]
pub fn tool_git_remote_info(path: String) -> AppResult<GitRemoteInfo> {
    let workspace = super::resolve_workspace_path(&path).map_err(AppError::PermissionDenied)?;

    let url = run_git(&workspace, &["remote", "get-url", "origin"])?;
    let url = url.trim().to_string();
    if url.is_empty() {
        return Err(AppError::Internal("No remote origin configured".into()));
    }

    let path_part = if url.starts_with("git@") {
        url.split(':').nth(1).unwrap_or_default()
    } else if let Some(rest) = url.strip_prefix("https://") {
        rest.split_once('/').map(|x| x.1).unwrap_or_default()
    } else {
        &url
    };
    let path_part = path_part.trim_end_matches(".git").trim_end_matches('/');
    let parts: Vec<&str> = path_part.split('/').collect();
    let (owner, repo) = if parts.len() >= 2 {
        (parts[parts.len() - 2].to_string(), parts[parts.len() - 1].to_string())
    } else {
        (String::new(), path_part.to_string())
    };

    let branch = run_git(&workspace, &["branch", "--show-current"])
        .unwrap_or_default()
        .trim()
        .to_string();

    Ok(GitRemoteInfo { remote_url: url, owner, repo, branch })
}

/// GitHub auth status for the Git popup status bar.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GhAuthInfo {
    /// Whether the user is logged in to GitHub CLI.
    pub logged_in: bool,
    /// GitHub username (empty if not logged in).
    pub username: String,
}

#[tauri::command]
pub fn tool_gh_auth_status(path: String) -> AppResult<GhAuthInfo> {
    let workspace = super::resolve_workspace_path(&path).map_err(AppError::PermissionDenied)?;

    let mut candidates: Vec<String> = vec!["gh".to_string()];
    if cfg!(target_os = "windows") {
        candidates.push(r"C:\Program Files\GitHub CLI\gh.exe".to_string());
        candidates.push(r"C:\Program Files (x86)\GitHub CLI\gh.exe".to_string());
        if let Ok(home) = std::env::var("USERPROFILE") {
            candidates.push(format!(r"{home}\AppData\Local\GitHub CLI\gh.exe", home = home));
        }
    } else {
        candidates.insert(0, "/mnt/c/Program Files/GitHub CLI/gh.exe".to_string());
        candidates.insert(1, r"/mnt/c/Program Files (x86)\GitHub CLI/gh.exe".to_string());
        if let Ok(home) = std::env::var("HOME") {
            candidates.push(format!("{home}/go/bin/gh", home = home));
        }
    }

    for gh_bin in &candidates {
        let output = Command::new(gh_bin)
            .current_dir(&workspace)
            .args(["auth", "status"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();

        let child = match output {
            Ok(c) => c,
            Err(_) => continue,
        };

        let result = match child.wait_with_output() {
            Ok(r) => r,
            Err(_) => continue,
        };

        let stderr = String::from_utf8_lossy(&result.stderr);
        let stdout = String::from_utf8_lossy(&result.stdout);
        let combined = format!("{}{}", stdout, stderr);

        let logged_in = result.status.success()
            || combined.contains("Logged in")
            || combined.contains("account ");

        if logged_in {
            let username = combined.lines()
                .find_map(|line| {
                    let line = line.trim();
                    if line.contains("account ") {
                        line.split("account ").nth(1)?.split_whitespace().next().map(|s| s.to_string())
                    } else if line.contains("Logged in to") {
                        line.split("Logged in to").nth(1)?
                            .split(" as ").nth(1)?
                            .split_whitespace().next()
                            .map(|s| s.trim_end_matches(|c: char| !c.is_alphanumeric()).to_string())
                    } else {
                        None
                    }
                })
                .unwrap_or_default();
            return Ok(GhAuthInfo { logged_in: true, username });
        }
    }

    Ok(GhAuthInfo { logged_in: false, username: String::new() })
}

fn run_gh(workspace: &PathBuf, args: &[&str]) -> AppResult<String> {
    let output = Command::new("gh")
        .current_dir(workspace)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| AppError::Internal(format!("Failed to spawn gh: {e}. Is GitHub CLI installed?")))?;

    let result = output.wait_with_output().map_err(AppError::from)?;

    let stdout = String::from_utf8_lossy(&result.stdout);
    let stderr = String::from_utf8_lossy(&result.stderr);

    if !result.status.success() {
        let msg = if stderr.is_empty() { &*stdout } else { &*stderr };
        return Err(AppError::Internal(format!("gh command failed: {}", msg.trim())));
    }

    Ok(stdout.trim().to_string())
}

#[tauri::command]
pub async fn tool_gh_auth_login() -> AppResult<String> {
    let child = if cfg!(windows) {
        Command::new("cmd")
            .args(["/C", "chcp 65001>nul & gh auth login --hostname github.com --git-protocol https --web"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
    } else {
        Command::new("sh")
            .args(["-c", "gh auth login --hostname github.com --git-protocol https --web"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
    }
    .map_err(|e| AppError::Internal(format!("Failed to run gh auth login: {e}. Is GitHub CLI installed?")))?;

    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });

    rx.recv_timeout(Duration::from_secs(120))
        .map_err(|_| AppError::Internal("GitHub login timed out after 120 seconds. Please try again.".to_string()))
        .and_then(|r| r.map_err(AppError::from))
        .map(|output| {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            if output.status.success() {
                if stdout.trim().is_empty() {
                    "GitHub 登录成功！".to_string()
                } else {
                    stdout
                }
            } else if !stderr.is_empty() {
                stderr
            } else {
                format!("gh auth login exited with code {:?}", output.status.code())
            }
        })
}

#[cfg(test)]
mod tests {
    #[test]
    fn git_status_returns_ok_on_repo() {
        let result = super::tool_git_status(".".into());
        // Workspace is a git repo, should succeed.
        assert!(result.is_ok());
    }

    #[test]
    fn git_log_returns_ok() {
        let result = super::tool_git_log(".".into(), Some(3));
        assert!(result.is_ok(), "git log failed: {:?}", result.err());
    }

    #[test]
    fn git_commit_empty_message_rejected() {
        let result = super::tool_git_commit(".".into(), "".into());
        assert!(result.is_err());
    }

    #[test]
    fn git_branch_empty_name_rejected() {
        let result = super::tool_git_branch(".".into(), "".into(), Some(false));
        assert!(result.is_err());
    }

    #[test]
    fn git_commit_empty_message_is_internal() {
        let err = super::tool_git_commit(".".into(), "   ".into()).unwrap_err();
        let value: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&err).unwrap()).unwrap();
        assert_eq!(value["code"], "internal");
    }
}
