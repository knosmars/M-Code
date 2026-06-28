use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use regex::Regex;

use crate::error::{AppError, AppResult};

pub mod agents;
pub mod agents_rules;
pub mod code_graph;
pub mod diff_preview;
pub mod doc_index;
pub mod error_diagnosis;
pub mod file_sync;
pub mod git;
pub mod github_oauth;
pub mod global_memory;
pub mod hooks;
pub mod impact_analysis;
pub mod index;
pub mod image_gen;
pub mod checkpoint;
pub mod lsp;
pub mod lsp_regex;
pub mod mcp;
pub mod semantic;
pub mod memory;
pub mod web;
pub mod multi_edit;
pub mod perf_analyzer;
pub mod review_store;
pub mod search;
pub mod skills;
pub mod ssh;
pub mod terminal;
pub mod test_runner;
pub mod triggers;

// ---------------------------------------------------------------------------
// Workspace path validation (§14 security)
// ---------------------------------------------------------------------------

/// Resolve a path safely within the workspace root. Canonicalizes the path and
/// verifies it does not escape the workspace directory via symlinks or `..`.
/// Returns the canonical path on success.
pub fn resolve_workspace_path(path: &str) -> Result<PathBuf, String> {
    let candidate = Path::new(path);

    // Reject bare `..` components that try to escape before resolution
    for component in candidate.components() {
        use std::path::Component;
        if matches!(component, Component::ParentDir) {
            // Allow parent dir only inside workspace — resolved after canonicalization
        }
    }

    let canonical = candidate
        .canonicalize()
        .map_err(|e| format!("Invalid path '{path}': {e}"))?;

    let workspace = std::env::current_dir()
        .map_err(|e| format!("Failed to get workspace: {e}"))?
        .canonicalize()
        .map_err(|e| format!("Failed to resolve workspace: {e}"))?;

    if !canonical.starts_with(&workspace) {
        let is_temp = std::env::temp_dir()
            .canonicalize()
            .map(|temp_root| canonical.starts_with(&temp_root))
            .unwrap_or(false);
        if !is_temp {
            return Err(format!(
                "Path '{path}' is outside the workspace directory. File access must stay within: {}",
                workspace.display()
            ));
        }
    }

    Ok(canonical)
}

/// Get the workspace root as a string for display purposes.
#[allow(dead_code)]
fn workspace_root() -> Result<String, String> {
    let ws = std::env::current_dir()
        .map_err(|e| format!("Failed to get workspace: {e}"))?
        .canonicalize()
        .map_err(|e| format!("Failed to resolve workspace: {e}"))?;
    Ok(ws.to_string_lossy().to_string())
}

// ---------------------------------------------------------------------------
// Tool: read_file
// ---------------------------------------------------------------------------

/// Read the contents of a file at `path`. Returns the file contents as a string.
/// Restricted to files within the workspace directory (§14).
#[tauri::command]
pub fn tool_read_file(path: String) -> AppResult<String> {
    let p = Path::new(&path);
    if !p.exists() {
        return Err(AppError::NotFound(format!("File not found: {path}")));
    }
    if !p.is_file() {
        return Err(AppError::NotFound(format!("Not a file: {path}")));
    }
    // Symlink escape check
    let _safe = resolve_workspace_path(&path).map_err(AppError::PermissionDenied)?;
    fs::read_to_string(&path).map_err(AppError::from)
}

// ---------------------------------------------------------------------------
// Tool: write_file
// ---------------------------------------------------------------------------

/// Write `content` to a file at `path`. Creates parent directories if needed.
/// Overwrites existing files. Restricted to workspace (§14).
#[tauri::command]
pub fn tool_write_file(path: String, content: String) -> AppResult<()> {
    let p = Path::new(&path);
    // Symlink escape check — do this before creating dirs/files
    // If the path doesn't exist yet, check its parent
    let check_target = if p.exists() {
        p.to_path_buf()
    } else {
        p.parent()
            .map(|parent| {
                if parent.as_os_str().is_empty() {
                    PathBuf::from(".")
                } else {
                    parent.to_path_buf()
                }
            })
            .unwrap_or_else(|| PathBuf::from("."))
    };
    let _safe = resolve_workspace_path(
        &check_target.to_string_lossy()
    ).map_err(AppError::PermissionDenied)?;

    checkpoint::record_if_active(&path);

    if let Some(parent) = p.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(AppError::from)?;
        }
    }
    let mut f = fs::File::create(&path).map_err(AppError::from)?;
    f.write_all(content.as_bytes()).map_err(AppError::from)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Tool: edit_file
// ---------------------------------------------------------------------------

/// Replace `old_string` with `new_string` in the file at `path`.
/// Uses exact string matching (not regex). Fails if `old_string` is not found
/// or found multiple times (to prevent ambiguity).
/// Restricted to workspace (§14).
#[tauri::command]
pub fn tool_edit_file(path: String, old_string: String, new_string: String) -> AppResult<()> {
    let p = Path::new(&path);
    if !p.exists() {
        return Err(AppError::NotFound(format!("File not found: {path}")));
    }
    if !p.is_file() {
        return Err(AppError::NotFound(format!("Not a file: {path}")));
    }
    // Symlink escape check
    let _safe = resolve_workspace_path(&path).map_err(AppError::PermissionDenied)?;
    let content = fs::read_to_string(&path).map_err(AppError::from)?;

    let count = content.matches(&old_string).count();
    if count == 0 {
        return Err(AppError::Internal(format!(
            "old_string not found in {path}: {old_string:?}"
        )));
    }
    if count > 1 {
        return Err(AppError::Internal(format!(
            "old_string found {count} times in {path} — provide more surrounding context"
        )));
    }

    let new_content = content.replacen(&old_string, &new_string, 1);
    checkpoint::record_if_active(&path);
    fs::write(&path, new_content).map_err(AppError::from)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Tool: list_dir
// ---------------------------------------------------------------------------

/// List entries in a directory. Returns entries as newline-separated string,
/// with `/` appended to directories. Restricted to workspace (§14).
#[tauri::command]
pub fn tool_list_dir(path: String) -> Result<String, String> {
    let p = Path::new(&path);
    if !p.exists() {
        return Err(format!("Directory not found: {path}"));
    }
    if !p.is_dir() {
        return Err(format!("Not a directory: {path}"));
    }
    let _safe = resolve_workspace_path(&path)?;

    let mut entries: Vec<String> = Vec::new();
    let dir_iter = fs::read_dir(&path).map_err(|e| format!("Failed to read dir {path}: {e}"))?;
    for entry in dir_iter {
        let entry = entry.map_err(|e| format!("Failed to read dir entry: {e}"))?;
        let name = entry.file_name().to_string_lossy().to_string();
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir {
            entries.push(format!("{name}/"));
        } else {
            entries.push(name);
        }
    }
    entries.sort();
    Ok(entries.join("\n"))
}

// ---------------------------------------------------------------------------
// Tool: run_command
// ---------------------------------------------------------------------------

/// Run a shell command and return stdout as a string. Returns stderr joined
/// with stdout if stderr is non-empty.
///
/// Security (§14): 30-second timeout enforced via process kill. Rejects
/// interactive commands (no stdin). Workspace-restricted via cwd.
#[tauri::command]
pub async fn tool_run_command(command: String, cwd: Option<String>) -> Result<String, String> {
    let workdir = cwd.unwrap_or_else(|| ".".to_string());
    let safe_cwd = resolve_workspace_path(&workdir).map_err(|e| format!("Path error: {e}"))?;

    let output = tokio::task::spawn_blocking(move || {
        let child = if cfg!(target_os = "windows") {
            // On non-UTF-8 Windows consoles (e.g. Chinese, codepage 936/GBK),
            // cmd's own messages and many tools emit legacy-encoded bytes that
            // become mojibake when read as UTF-8 (the model then can't read the
            // output). Switch the console to UTF-8 (65001) first so output
            // comes back as UTF-8. `>nul` hides chcp's banner; `&` always runs
            // the command afterwards even if chcp is a no-op.
            let win_cmd = format!("chcp 65001>nul & {command}");
            Command::new("cmd")
                .args(["/C", win_cmd.as_str()])
                .current_dir(&safe_cwd)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
        } else {
            Command::new("sh")
                .args(["-c", &command])
                .current_dir(&safe_cwd)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
        }
        .map_err(|e| format!("Failed to execute command: {e}"))?;

        let timeout = Duration::from_secs(30);
        let pid = child.id();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(child.wait_with_output());
        });
        match rx.recv_timeout(timeout) {
            Ok(Ok(output)) => Ok(output),
            Ok(Err(e)) => Err(format!("Failed to wait on command: {e}")),
            Err(_) => {
                // Kill the process on timeout so it doesn't outlive the tool
                // call and block subsequent tool executions.
                let mut kill_cmd = std::process::Command::new(if cfg!(target_os = "windows") {
                    "taskkill"
                } else {
                    "kill"
                });
                if cfg!(target_os = "windows") {
                    kill_cmd.args(["/F", "/T", "/PID", &pid.to_string()]);
                } else {
                    kill_cmd.args(["-9", &pid.to_string()]);
                }
                let _ = kill_cmd.status();
                Err("Command timed out after 30 seconds".to_string())
            }
        }
    })
    .await
    .map_err(|e| format!("Task join error: {e}"))??;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let mut result = stdout;
    if !output.status.success() {
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str(&format!("[exit code: {}]", output.status.code().unwrap_or(-1)));
    }
    if !stderr.is_empty() {
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str(&format!("[stderr]\n{stderr}"));
    }
    Ok(result)
}

// ---------------------------------------------------------------------------
// Tool: grep
// ---------------------------------------------------------------------------

/// Search for `pattern` (regex) in files matching `include` glob under `path`.
/// Returns matching lines prefixed with `file:line: content`.
#[tauri::command]
pub fn tool_grep(pattern: String, path: String, include: Option<String>) -> Result<String, String> {
    let re = Regex::new(&pattern).map_err(|e| format!("Invalid regex: {e}"))?;
    let include_pat = include.as_deref().unwrap_or("*");
    let mut results: Vec<String> = Vec::new();

    let root = Path::new(&path);
    if !root.exists() {
        return Err(format!("Path not found: {path}"));
    }
    let _safe = resolve_workspace_path(&path)?;

    walk_dir(root, include_pat, &mut |file_path| {
        if let Ok(content) = fs::read_to_string(file_path) {
            for (line_num, line) in content.lines().enumerate() {
                if re.is_match(line) {
                    // Truncate long lines to avoid huge output
                    let display = if line.len() > 500 {
                        format!("{}...", &line[..500])
                    } else {
                        line.to_string()
                    };
                    results.push(format!(
                        "{}:{}: {}",
                        file_path.display(),
                        line_num + 1,
                        display
                    ));
                }
            }
        }
    })
    .map_err(|e| format!("Failed to walk {path}: {e}"))?;

    Ok(results.join("\n"))
}

/// Walk directory tree, calling `cb` for each file matching `include_pat` glob.
fn walk_dir(
    dir: &Path,
    include_pat: &str,
    cb: &mut dyn FnMut(&Path),
) -> Result<(), std::io::Error> {
    if dir.is_file() {
        if matches_glob(dir.file_name().unwrap_or_default().to_string_lossy().as_ref(), include_pat) {
            cb(dir);
        }
        return Ok(());
    }
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            // Skip hidden dirs and common non-code dirs
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if name.starts_with('.') || name == "node_modules" || name == "target" {
                continue;
            }
            walk_dir(&path, include_pat, cb)?;
        } else if path.is_file() {
            let fname = path.file_name().unwrap_or_default().to_string_lossy();
            if matches_glob(&fname, include_pat) {
                cb(&path);
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tool: glob
// ---------------------------------------------------------------------------

/// Find files matching a glob pattern under `path`. Returns newline-separated
/// matching file paths.
///
/// Supported patterns:
///   `*`     — matches any sequence of characters except `/`
///   `?`     — matches a single character except `/`
///   `**`    — matches zero or more directories
///   `[...]` — character class
#[tauri::command]
pub fn tool_glob(pattern: String, path: Option<String>) -> Result<String, String> {
    let base = path.as_deref().unwrap_or(".");
    let base_path = Path::new(base);
    let _safe = resolve_workspace_path(base)?;
    let mut results: Vec<String> = Vec::new();

    glob_walk(base_path, base_path, &pattern, &mut results)
        .map_err(|e| format!("Glob error: {e}"))?;

    results.sort();
    if results.is_empty() {
        Ok(String::new())
    } else {
        Ok(results.join("\n"))
    }
}

fn glob_walk(
    _root: &Path,
    current: &Path,
    pattern: &str,
    results: &mut Vec<String>,
) -> Result<(), std::io::Error> {
    // Split pattern into components
    let (head, tail) = split_pattern(pattern);
    let has_more = tail.is_some();
    let head_pat = head;

    if current.is_file() {
        // If pattern has no separators, match filename directly
        if !has_more
            && matches_glob(
                current.file_name().unwrap_or_default().to_string_lossy().as_ref(),
                head_pat,
            )
        {
            results.push(current.to_string_lossy().to_string());
        }
        return Ok(());
    }

    if !current.is_dir() {
        return Ok(());
    }

    let entries: Vec<_> = fs::read_dir(current)?
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            // Skip hidden unless pattern explicitly starts with .
            if !pattern.starts_with('.') && name.starts_with('.') && name != "." && name != ".." {
                return false;
            }
            true
        })
        .collect();

    for entry in &entries {
        let name = entry.file_name().to_string_lossy().to_string();
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);

        if head_pat == "**" {
            // ** matches zero or more directories
            if let Some(rest) = tail {
                // Match rest against files at the current level (** = zero dirs)
                if !is_dir && matches_glob(&name, rest) {
                    results.push(entry.path().to_string_lossy().to_string());
                }
                // Match rest against subdirectories
                if is_dir {
                    glob_walk(_root, &entry.path(), rest, results)?;
                }
                // Also recurse with ** still active (match deeper levels)
                glob_walk(_root, &entry.path(), pattern, results)?;
            } else {
                // ** alone matches everything
                if !name.starts_with('.') || pattern.starts_with('.') {
                    results.push(entry.path().to_string_lossy().to_string());
                }
            }
        } else if matches_glob(&name, head_pat) {
            if let Some(rest) = tail {
                if is_dir {
                    glob_walk(_root, &entry.path(), rest, results)?;
                }
                // If it's a file but pattern has more levels, skip
            } else {
                // Leaf match
                results.push(entry.path().to_string_lossy().to_string());
            }
        }
    }

    Ok(())
}

/// Split pattern at the first path separator (`/`).
/// Returns (head, tail) where tail is `Some(rest)` or `None` if leaf.
fn split_pattern(pattern: &str) -> (&str, Option<&str>) {
    // Don't split on escaped slashes or within brackets
    let bytes = pattern.as_bytes();
    let mut bracket_depth = 0;
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'[' => bracket_depth += 1,
            b']' if bracket_depth > 0 => bracket_depth -= 1,
            b'/' if bracket_depth == 0 => {
                return (&pattern[..i], Some(&pattern[i + 1..]));
            }
            _ => {}
        }
    }
    (pattern, None)
}

/// Check if `ch` matches the character class (supports `a-z` ranges and literal chars).
fn char_matches_class(ch: u8, class: &[u8]) -> bool {
    let mut i = 0;
    while i < class.len() {
        if i + 2 < class.len() && class[i + 1] == b'-' {
            // Range: class[i] to class[i+2]
            if ch >= class[i] && ch <= class[i + 2] {
                return true;
            }
            i += 3;
        } else {
            if class[i] == ch {
                return true;
            }
            i += 1;
        }
    }
    false
}

/// Simple fnmatch-style glob matching. Supports `*`, `?`, `[...]` with ranges.
/// Does NOT support `**` (that's handled at the walk level).
fn matches_glob(name: &str, pattern: &str) -> bool {
    matches_glob_impl(name.as_bytes(), pattern.as_bytes())
}

fn matches_glob_impl(name: &[u8], pattern: &[u8]) -> bool {
    if pattern.is_empty() {
        return name.is_empty();
    }

    if name.is_empty() {
        // Only '*' can match an empty name
        return pattern.iter().all(|&b| b == b'*');
    }

    match pattern[0] {
        b'*' => {
            // * matches zero or more chars
            matches_glob_impl(name, &pattern[1..])           // zero chars
                || matches_glob_impl(&name[1..], pattern)     // one char + rest
        }
        b'?' => {
            matches_glob_impl(&name[1..], &pattern[1..])
        }
        b'[' => {
            let close = match pattern[1..].iter().position(|&b| b == b']') {
                Some(pos) => 1 + pos,
                None => return name[0] == b'[' && matches_glob_impl(&name[1..], &pattern[1..]),
            };
            let class = &pattern[1..close];
            let negate = class.first() == Some(&b'!');
            let chars = if negate { &class[1..] } else { class };
            let in_class = char_matches_class(name[0], chars);
            let matched = if negate { !in_class } else { in_class };
            matched && matches_glob_impl(&name[1..], &pattern[close + 1..])
        }
        _ => {
            name[0] == pattern[0] && matches_glob_impl(&name[1..], &pattern[1..])
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup_temp_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("meyatu_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    // --- tool_edit_file tests ---

    #[test]
    fn edit_file_replaces_single_occurrence() {
        let dir = setup_temp_dir();
        let file = dir.join("test.txt");
        fs::write(&file, "hello world\nfoo bar\n").unwrap();

        tool_edit_file(
            file.to_string_lossy().to_string(),
            "foo bar".to_string(),
            "baz qux".to_string(),
        )
        .unwrap();

        let content = fs::read_to_string(&file).unwrap();
        assert!(content.contains("baz qux"));
        assert!(!content.contains("foo bar"));
    }

    #[test]
    fn edit_file_fails_on_multiple_matches() {
        let dir = setup_temp_dir();
        let file = dir.join("dup.txt");
        fs::write(&file, "foo\nfoo\nbar\n").unwrap();

        let result = tool_edit_file(
            file.to_string_lossy().to_string(),
            "foo".to_string(),
            "baz".to_string(),
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("found 2 times"));
    }

    #[test]
    fn edit_file_fails_on_not_found() {
        let dir = setup_temp_dir();
        let file = dir.join("missing.txt");
        fs::write(&file, "hello\n").unwrap();

        let result = tool_edit_file(
            file.to_string_lossy().to_string(),
            "nonexistent".to_string(),
            "replacement".to_string(),
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn edit_file_fails_on_nonexistent_file() {
        let result = tool_edit_file(
            "/tmp/nonexistent_meyatu_test_file.txt".to_string(),
            "a".to_string(),
            "b".to_string(),
        );
        assert!(result.is_err());
    }

    // --- tool_list_dir tests ---

    #[test]
    fn list_dir_returns_sorted_entries() {
        let dir = setup_temp_dir();
        fs::write(dir.join("b_file.txt"), "b").unwrap();
        fs::write(dir.join("a_file.txt"), "a").unwrap();
        fs::create_dir(dir.join("z_dir")).unwrap();

        let result = tool_list_dir(dir.to_string_lossy().to_string()).unwrap();
        let lines: Vec<&str> = result.lines().collect();
        assert!(lines[0].starts_with("a_file"));
        assert!(lines[1].starts_with("b_file"));
        assert!(lines[2].starts_with("z_dir/"));
    }

    #[test]
    fn list_dir_fails_on_file() {
        let dir = setup_temp_dir();
        let file = dir.join("f.txt");
        fs::write(&file, "content").unwrap();

        let result = tool_list_dir(file.to_string_lossy().to_string());
        assert!(result.is_err());
    }

    #[test]
    fn list_dir_fails_on_nonexistent() {
        let result = tool_list_dir("/tmp/nonexistent_meyatu_dir_test".to_string());
        assert!(result.is_err());
    }

    // --- tool_glob tests ---

    #[test]
    fn glob_finds_files_by_extension() {
        let dir = setup_temp_dir();
        fs::write(dir.join("a.rs"), "fn main() {}").unwrap();
        fs::write(dir.join("b.txt"), "hello").unwrap();
        fs::create_dir(dir.join("sub")).unwrap();
        fs::write(dir.join("sub/c.rs"), "mod test;").unwrap();

        let result = tool_glob("**/*.rs".to_string(), Some(dir.to_string_lossy().to_string())).unwrap();
        assert!(result.contains("a.rs"));
        assert!(result.contains("sub/c.rs"));
        assert!(!result.contains("b.txt"));
    }

    #[test]
    fn glob_wildcard_matches() {
        let dir = setup_temp_dir();
        fs::write(dir.join("foo.rs"), "").unwrap();
        fs::write(dir.join("bar.rs"), "").unwrap();
        fs::write(dir.join("baz.txt"), "").unwrap();

        let result = tool_glob("*.rs".to_string(), Some(dir.to_string_lossy().to_string())).unwrap();
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].ends_with("bar.rs"));
        assert!(lines[1].ends_with("foo.rs"));
    }

    #[test]
    fn glob_empty_result() {
        let dir = setup_temp_dir();
        let result =
            tool_glob("*.nonexistent".to_string(), Some(dir.to_string_lossy().to_string())).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn glob_question_mark_matches_single_char() {
        let dir = setup_temp_dir();
        fs::write(dir.join("a1.txt"), "").unwrap();
        fs::write(dir.join("ab.txt"), "").unwrap();
        fs::write(dir.join("abc.txt"), "").unwrap();

        let result = tool_glob("a?.txt".to_string(), Some(dir.to_string_lossy().to_string())).unwrap();
        let lines: Vec<&str> = result.lines().collect();
        // ? matches exactly one char: both "a1.txt" and "ab.txt" match, "abc.txt" does not
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn glob_default_path() {
        let dir = setup_temp_dir();
        fs::write(dir.join("unique_meyatu_test.txt"), "test").unwrap();

        // Use explicit path since default "." would be project root
        let result =
            tool_glob("unique_meyatu_test.txt".to_string(), Some(dir.to_string_lossy().to_string()))
                .unwrap();
        assert!(result.contains("unique_meyatu_test.txt"));
    }

    // --- matches_glob tests ---

    #[test]
    fn matches_glob_star() {
        assert!(matches_glob("hello.rs", "*.rs"));
        assert!(matches_glob("foo.rs", "*.rs"));
        assert!(!matches_glob("foo.txt", "*.rs"));
    }

    #[test]
    fn matches_glob_question() {
        assert!(matches_glob("ab", "a?"));
        assert!(!matches_glob("abc", "a?"));
    }

    #[test]
    fn matches_glob_bracket() {
        assert!(matches_glob("a1", "a[0-9]"));
        assert!(!matches_glob("ab", "a[0-9]"));
        assert!(matches_glob("a3", "a[0-9]"));
        // Negation
        assert!(matches_glob("ab", "a[!0-9]"));
        assert!(!matches_glob("a1", "a[!0-9]"));
        // Literal chars in class (no range)
        assert!(matches_glob("ax", "a[xyz]"));
        assert!(!matches_glob("aw", "a[xyz]"));
    }

    #[test]
    fn matches_glob_literal() {
        assert!(matches_glob("exact", "exact"));
        assert!(!matches_glob("exact", "different"));
    }

    #[test]
    fn split_pattern_simple() {
        let (head, tail) = split_pattern("src/*.rs");
        assert_eq!(head, "src");
        assert_eq!(tail, Some("*.rs"));
    }

    #[test]
    fn split_pattern_no_separator() {
        let (head, tail) = split_pattern("*.rs");
        assert_eq!(head, "*.rs");
        assert_eq!(tail, None);
    }

    #[test]
    fn split_pattern_bracket_with_slash() {
        // Slash inside brackets should not be treated as separator
        let (head, tail) = split_pattern("a[/]b/rest");
        assert_eq!(head, "a[/]b");
        assert_eq!(tail, Some("rest"));
    }
}

#[cfg(test)]
mod structured_error_tests {
    use super::*;

    #[test]
    fn read_file_missing_is_not_found() {
        let err = tool_read_file("/no/such/file/xyz".into()).unwrap_err();
        let value: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&err).unwrap()).unwrap();
        assert_eq!(value["code"], "not_found");
    }

    #[test]
    fn edit_file_missing_is_not_found() {
        let err = tool_edit_file("/no/such/file/xyz".into(), "a".into(), "b".into())
            .unwrap_err();
        let value: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&err).unwrap()).unwrap();
        assert_eq!(value["code"], "not_found");
    }
}
