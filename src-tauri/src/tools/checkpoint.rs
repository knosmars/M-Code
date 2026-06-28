//! Per-turn content checkpoints so the agent's file edits can be rolled back.
//!
//! Before a turn's first write to a file, its prior content is captured. A
//! checkpoint groups every file an agent turn touched; restoring it writes the
//! prior content back (or deletes files the turn created). In-memory only.

use crate::error::{AppError, AppResult};
use serde_json::json;
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::sync::{Mutex, OnceLock};

const MAX_CHECKPOINTS: usize = 20;

enum CheckpointMode {
    /// git workspace: a shadow commit SHA capturing the whole working tree.
    Git(String),
    /// non-git workspace: path -> prior content (None = file did not exist).
    Content(HashMap<String, Option<String>>),
}

struct Checkpoint {
    id: String,
    label: String,
    workspace: String,
    mode: CheckpointMode,
}

impl Checkpoint {
    fn new_content(id: String, label: String, workspace: String) -> Self {
        Self { id, label, workspace, mode: CheckpointMode::Content(HashMap::new()) }
    }

    fn new_git(id: String, label: String, workspace: String, snap: String) -> Self {
        Self { id, label, workspace, mode: CheckpointMode::Git(snap) }
    }

    /// Content-mode capture (no-op in git mode — the shadow commit covers all).
    fn capture(&mut self, path: &str) {
        if let CheckpointMode::Content(files) = &mut self.mode {
            if !files.contains_key(path) {
                let prior = std::fs::read_to_string(path).ok();
                files.insert(path.to_string(), prior);
            }
        }
    }
}

/// Restore captured files. Returns `(restored, deleted, errors)`.
fn restore_files(files: &HashMap<String, Option<String>>) -> (usize, usize, Vec<String>) {
    let mut restored = 0;
    let mut deleted = 0;
    let mut errors = Vec::new();
    for (path, prior) in files {
        match prior {
            Some(content) => match std::fs::write(path, content) {
                Ok(()) => restored += 1,
                Err(e) => errors.push(format!("{path}: {e}")),
            },
            None => {
                if Path::new(path).exists() {
                    match std::fs::remove_file(path) {
                        Ok(()) => deleted += 1,
                        Err(e) => errors.push(format!("{path}: {e}")),
                    }
                }
            }
        }
    }
    (restored, deleted, errors)
}

fn active() -> &'static Mutex<Option<Checkpoint>> {
    static ACTIVE: OnceLock<Mutex<Option<Checkpoint>>> = OnceLock::new();
    ACTIVE.get_or_init(|| Mutex::new(None))
}

fn store() -> &'static Mutex<Vec<Checkpoint>> {
    static STORE: OnceLock<Mutex<Vec<Checkpoint>>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(Vec::new()))
}

/// Called by the write tools before they modify `path`. No-op unless a
/// checkpoint is active. Best-effort: a poisoned lock is ignored.
pub fn record_if_active(path: &str) {
    if let Ok(mut guard) = active().lock() {
        if let Some(cp) = guard.as_mut() {
            cp.capture(path);
        }
    }
}

/// Run `git -C ws <args>`. Sets a fixed identity (so commit-tree works without
/// user git config) and an optional alternate index. Trims stdout; on non-zero
/// exit returns an Err carrying stderr.
fn run_git(ws: &str, args: &[&str], index_file: Option<&str>) -> Result<String, String> {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(ws).args(args);
    cmd.env("GIT_AUTHOR_NAME", "meyatu")
        .env("GIT_AUTHOR_EMAIL", "checkpoint@meyatu.local")
        .env("GIT_COMMITTER_NAME", "meyatu")
        .env("GIT_COMMITTER_EMAIL", "checkpoint@meyatu.local");
    if let Some(idx) = index_file {
        cmd.env("GIT_INDEX_FILE", idx);
    }
    let out = cmd.output().map_err(|e| format!("git 执行失败: {e}"))?;
    if !out.status.success() {
        return Err(format!("git {:?} 失败: {}", args, String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn is_git_repo(ws: &str) -> bool {
    run_git(ws, &["rev-parse", "--is-inside-work-tree"], None)
        .map(|s| s == "true")
        .unwrap_or(false)
}

/// Capture the full working tree (tracked changes + untracked, .gitignore-respected)
/// as a dangling commit, without touching HEAD / the real index / stash / worktree.
fn git_snapshot(ws: &str) -> Result<String, String> {
    let tmp = std::env::temp_dir().join(format!(
        "meyatu_idx_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let tmp = tmp.to_string_lossy().into_owned();

    let has_head = run_git(ws, &["rev-parse", "--verify", "HEAD"], None).is_ok();
    let result = (|| {
        if has_head {
            run_git(ws, &["read-tree", "HEAD"], Some(&tmp))?;
        }
        run_git(ws, &["add", "-A"], Some(&tmp))?;
        let tree = run_git(ws, &["write-tree"], Some(&tmp))?;
        if has_head {
            run_git(ws, &["commit-tree", &tree, "-p", "HEAD", "-m", "meyatu-checkpoint"], None)
        } else {
            run_git(ws, &["commit-tree", &tree, "-m", "meyatu-checkpoint"], None)
        }
    })();
    let _ = std::fs::remove_file(&tmp);
    result
}

/// Reset the working tree to `snap` (HEAD untouched) and remove files created
/// after the snapshot. Returns `(changed_tracked, deleted_created)`.
fn git_restore(ws: &str, snap: &str) -> Result<(usize, usize), String> {
    let changed = run_git(ws, &["diff", "--name-only", snap, "--"], None)?
        .lines()
        .filter(|l| !l.is_empty())
        .count();
    run_git(ws, &["read-tree", "--reset", "-u", snap], None)?;
    // Snapshot + reset operate repo-wide (git resolves ws to the repo root),
    // so clean turn-created untracked files repo-wide too — otherwise stray
    // untracked files outside a subdir workspace would linger. Fall back to ws
    // if the repo root can't be resolved.
    let root = run_git(ws, &["rev-parse", "--show-toplevel"], None)
        .unwrap_or_else(|_| ws.to_string());
    let untracked = run_git(&root, &["ls-files", "--others", "--exclude-standard"], None)?;
    let mut deleted = 0;
    for f in untracked.lines().filter(|l| !l.is_empty()) {
        if std::fs::remove_file(Path::new(&root).join(f)).is_ok() {
            deleted += 1;
        }
    }
    Ok((changed, deleted))
}

// ── Tauri commands ──────────────────────────────────────────────────────

#[tauri::command]
pub fn tool_checkpoint_begin(label: String, workspace: String) -> Result<String, String> {
    let id = uuid::Uuid::new_v4().to_string();
    // git workspace → shadow snapshot; on any failure fall back to content mode.
    let cp = if !workspace.is_empty() && is_git_repo(&workspace) {
        match git_snapshot(&workspace) {
            Ok(snap) => Checkpoint::new_git(id.clone(), label, workspace.clone(), snap),
            Err(_) => Checkpoint::new_content(id.clone(), label, workspace.clone()),
        }
    } else {
        Checkpoint::new_content(id.clone(), label, workspace.clone())
    };
    let mut guard = active().lock().map_err(|_| "checkpoint state poisoned")?;
    *guard = Some(cp);
    Ok(id)
}

#[tauri::command]
pub fn tool_checkpoint_end() -> Result<(), String> {
    let taken = active().lock().map_err(|_| "checkpoint state poisoned")?.take();
    if let Some(cp) = taken {
        let mut store = store().lock().map_err(|_| "checkpoint store poisoned")?;
        store.push(cp);
        while store.len() > MAX_CHECKPOINTS {
            store.remove(0);
        }
    }
    Ok(())
}

#[tauri::command]
pub fn tool_checkpoint_restore(id: String) -> AppResult<String> {
    let store = store().lock().map_err(|_| AppError::Internal("checkpoint store poisoned".into()))?;
    let cp = store
        .iter()
        .find(|c| c.id == id)
        .ok_or_else(|| AppError::NotFound("检查点已过期或不存在（重启后旧检查点会失效）".into()))?;
    match &cp.mode {
        CheckpointMode::Git(snap) => {
            let (changed, deleted) = git_restore(&cp.workspace, snap)
                .map_err(|e| AppError::Internal(format!("git 回滚失败: {e}")))?;
            Ok(format!("已回滚：恢复 {changed} 个文件，删除 {deleted} 个新建文件"))
        }
        CheckpointMode::Content(files) => {
            let (restored, deleted, errors) = restore_files(files);
            if errors.is_empty() {
                Ok(format!("已回滚：恢复 {restored} 个文件，删除 {deleted} 个新建文件"))
            } else {
                Ok(format!(
                    "回滚完成（部分失败）：恢复 {restored}，删除 {deleted}，错误 {}: {}",
                    errors.len(),
                    errors.join("; ")
                ))
            }
        }
    }
}

#[tauri::command]
pub fn tool_checkpoint_list() -> AppResult<String> {
    let store = store().lock().map_err(|_| AppError::Internal("checkpoint store poisoned".into()))?;
    let list: Vec<_> = store
        .iter()
        .map(|c| {
            let file_count = match &c.mode {
                CheckpointMode::Git(snap) => run_git(
                    &c.workspace,
                    &["diff", "--name-only", snap, "--"],
                    None,
                )
                .map(|s| s.lines().filter(|l| !l.is_empty()).count())
                .unwrap_or(0),
                CheckpointMode::Content(files) => files.len(),
            };
            json!({ "id": c.id, "label": c.label, "fileCount": file_count })
        })
        .collect();
    serde_json::to_string(&list).map_err(AppError::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Serializes tests that touch the process-global checkpoint singletons
    // (active()/store()) so default-parallel `cargo test` doesn't race.
    static CKPT_TEST_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn temp_file(name: &str, content: &str) -> String {
        let dir = std::env::temp_dir().join(format!("meyatu_ckpt_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, content).unwrap();
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn capture_is_idempotent() {
        let p = temp_file("idem.txt", "original");
        let mut cp = Checkpoint::new_content("c1".into(), "t".into(), String::new());
        cp.capture(&p);
        std::fs::write(&p, "modified").unwrap();
        cp.capture(&p); // second touch must NOT overwrite the captured prior
        if let CheckpointMode::Content(files) = &cp.mode {
            assert_eq!(files.get(&p).unwrap().as_deref(), Some("original"));
        } else {
            panic!("expected Content mode");
        }
    }

    #[test]
    fn capture_records_none_for_missing_file() {
        let dir = std::env::temp_dir().join(format!("meyatu_ckpt_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("does_not_exist_yet.txt").to_string_lossy().into_owned();
        let mut cp = Checkpoint::new_content("c1".into(), "t".into(), String::new());
        cp.capture(&p);
        if let CheckpointMode::Content(files) = &cp.mode {
            assert!(files.get(&p).unwrap().is_none());
        } else {
            panic!("expected Content mode");
        }
    }

    #[test]
    fn restore_writes_back_and_deletes_created() {
        let kept = temp_file("kept.txt", "v-orig");
        let created = temp_file("created.txt", "exists-now");
        let mut files = HashMap::new();
        files.insert(kept.clone(), Some("v-orig".to_string()));
        files.insert(created.clone(), None); // was created during the turn

        // Simulate the turn's edits.
        std::fs::write(&kept, "v-changed").unwrap();

        let (restored, deleted, errors) = restore_files(&files);
        assert_eq!(restored, 1);
        assert_eq!(deleted, 1);
        assert!(errors.is_empty());
        assert_eq!(std::fs::read_to_string(&kept).unwrap(), "v-orig");
        assert!(!Path::new(&created).exists());
    }

    #[test]
    fn begin_record_end_restore_roundtrip() {
        let _guard = CKPT_TEST_GUARD.lock().unwrap_or_else(|p| p.into_inner());
        let p = temp_file("round.txt", "before");
        let dir = std::env::temp_dir().join(format!("meyatu_ckpt_{}", std::process::id()));
        let id = tool_checkpoint_begin("turn".into(), dir.to_string_lossy().into_owned()).unwrap();
        record_if_active(&p);
        std::fs::write(&p, "after").unwrap();
        tool_checkpoint_end().unwrap();

        let summary = tool_checkpoint_restore(id).unwrap();
        assert!(summary.contains("恢复 1"));
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "before");
    }

    #[test]
    fn restore_unknown_id_is_graceful() {
        let _guard = CKPT_TEST_GUARD.lock().unwrap_or_else(|p| p.into_inner());
        let err = tool_checkpoint_restore("no-such-id".into());
        assert!(err.is_err());
    }

    #[test]
    fn restore_unknown_id_is_not_found() {
        let _guard = CKPT_TEST_GUARD.lock().unwrap_or_else(|p| p.into_inner());
        let err = tool_checkpoint_restore("does-not-exist".into()).unwrap_err();
        let value: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&err).unwrap()).unwrap();
        assert_eq!(value["code"], "not_found");
    }

    fn git_ws(tag: &str) -> String {
        let dir = std::env::temp_dir().join(format!(
            "meyatu_gitckpt_{}_{}_{}",
            tag,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let ws = dir.to_string_lossy().into_owned();
        run_git(&ws, &["init", "-q"], None).unwrap();
        run_git(&ws, &["config", "user.email", "t@t"], None).unwrap();
        run_git(&ws, &["config", "user.name", "t"], None).unwrap();
        ws
    }

    fn write(ws: &str, name: &str, content: &str) {
        std::fs::write(std::path::Path::new(ws).join(name), content).unwrap();
    }
    fn read(ws: &str, name: &str) -> String {
        std::fs::read_to_string(std::path::Path::new(ws).join(name)).unwrap()
    }

    #[test]
    fn is_git_repo_detects() {
        let ws = git_ws("detect");
        assert!(is_git_repo(&ws));
        let plain = std::env::temp_dir()
            .join(format!("meyatu_plain_{}", std::process::id()))
            .to_string_lossy()
            .into_owned();
        std::fs::create_dir_all(&plain).unwrap();
        assert!(!is_git_repo(&plain));
    }

    #[test]
    fn snapshot_leaves_head_and_worktree_untouched() {
        let ws = git_ws("clean_snap");
        write(&ws, "a.txt", "orig");
        run_git(&ws, &["add", "-A"], None).unwrap();
        run_git(&ws, &["commit", "-qm", "init"], None).unwrap();
        let head_before = run_git(&ws, &["rev-parse", "HEAD"], None).unwrap();
        let status_before = run_git(&ws, &["status", "--porcelain"], None).unwrap();

        let snap = git_snapshot(&ws).unwrap();
        assert!(!snap.is_empty());

        assert_eq!(run_git(&ws, &["rev-parse", "HEAD"], None).unwrap(), head_before);
        assert_eq!(run_git(&ws, &["status", "--porcelain"], None).unwrap(), status_before);
        // stash list stays empty
        assert!(run_git(&ws, &["stash", "list"], None).unwrap().is_empty());
    }

    #[test]
    fn restore_reverts_modify_create_delete() {
        let ws = git_ws("restore");
        write(&ws, "keep.txt", "v-orig");
        write(&ws, "gone.txt", "to-delete");
        run_git(&ws, &["add", "-A"], None).unwrap();
        run_git(&ws, &["commit", "-qm", "init"], None).unwrap();

        let snap = git_snapshot(&ws).unwrap();

        // simulate run_command shell mutations:
        write(&ws, "keep.txt", "v-changed");                       // modify tracked
        write(&ws, "created.txt", "new");                          // create untracked
        std::fs::remove_file(std::path::Path::new(&ws).join("gone.txt")).unwrap(); // delete tracked

        let (changed, deleted) = git_restore(&ws, &snap).unwrap();

        assert_eq!(read(&ws, "keep.txt"), "v-orig");               // modification rolled back
        assert!(!std::path::Path::new(&ws).join("created.txt").exists()); // created removed
        assert_eq!(read(&ws, "gone.txt"), "to-delete");            // deletion restored
        assert!(changed >= 1);
        assert_eq!(deleted, 1);
        // HEAD unchanged by restore
    }

    #[test]
    fn snapshot_restore_on_empty_repo() {
        let ws = git_ws("empty");
        write(&ws, "first.txt", "hello");                          // untracked, no commit yet
        let snap = git_snapshot(&ws).unwrap();
        write(&ws, "first.txt", "changed");
        write(&ws, "second.txt", "extra");
        git_restore(&ws, &snap).unwrap();
        assert_eq!(read(&ws, "first.txt"), "hello");
        assert!(!std::path::Path::new(&ws).join("second.txt").exists());
    }

    #[test]
    fn begin_git_mode_roundtrip_via_commands() {
        let _guard = CKPT_TEST_GUARD.lock().unwrap_or_else(|p| p.into_inner());
        let ws = git_ws("cmd_round");
        write(&ws, "f.txt", "before");
        run_git(&ws, &["add", "-A"], None).unwrap();
        run_git(&ws, &["commit", "-qm", "init"], None).unwrap();

        let id = tool_checkpoint_begin("turn".into(), ws.clone()).unwrap();
        // simulate shell mutation that record_if_active would NOT catch:
        write(&ws, "f.txt", "after");
        write(&ws, "shell_created.txt", "x");
        tool_checkpoint_end().unwrap();

        let summary = tool_checkpoint_restore(id).unwrap();
        assert!(summary.contains("恢复") || summary.contains("回滚"));
        assert_eq!(read(&ws, "f.txt"), "before");
        assert!(!std::path::Path::new(&ws).join("shell_created.txt").exists());
    }

    #[test]
    fn restore_cleans_untracked_repo_wide_from_subdir() {
        let ws = git_ws("subdir");
        write(&ws, "committed.txt", "v1");
        run_git(&ws, &["add", "-A"], None).unwrap();
        run_git(&ws, &["commit", "-qm", "init"], None).unwrap();

        // workspace = a subdirectory of the repo
        let sub = std::path::Path::new(&ws).join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        let sub = sub.to_string_lossy().into_owned();

        // snapshot taken from the subdir (git resolves to repo root → repo-wide)
        let snap = git_snapshot(&sub).unwrap();

        // a turn creates untracked files at the repo ROOT and inside the subdir
        write(&ws, "root_untracked.txt", "stray");
        write(&sub, "sub_untracked.txt", "stray2");

        // restore from the subdir
        git_restore(&sub, &snap).unwrap();

        // both untracked files removed — the repo-root one was the previous gap
        assert!(!std::path::Path::new(&ws).join("root_untracked.txt").exists());
        assert!(!std::path::Path::new(&sub).join("sub_untracked.txt").exists());
        // committed file untouched
        assert_eq!(read(&ws, "committed.txt"), "v1");
    }

    #[test]
    fn restore_keeps_gitignored_untracked() {
        let ws = git_ws("ignore");
        write(&ws, ".gitignore", "ignored.txt\n");
        run_git(&ws, &["add", "-A"], None).unwrap();
        run_git(&ws, &["commit", "-qm", "init"], None).unwrap();

        let snap = git_snapshot(&ws).unwrap();
        write(&ws, "ignored.txt", "keepme");      // gitignored untracked
        write(&ws, "stray.txt", "remove-me");     // plain untracked

        git_restore(&ws, &snap).unwrap();

        assert!(std::path::Path::new(&ws).join("ignored.txt").exists()); // kept (--exclude-standard)
        assert!(!std::path::Path::new(&ws).join("stray.txt").exists());  // removed
        assert_eq!(read(&ws, "ignored.txt"), "keepme");
    }

    #[test]
    fn begin_non_git_uses_content_mode() {
        let _guard = CKPT_TEST_GUARD.lock().unwrap_or_else(|p| p.into_inner());
        let plain = std::env::temp_dir()
            .join(format!("meyatu_content_{}_{}", std::process::id(),
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()))
            .to_string_lossy()
            .into_owned();
        std::fs::create_dir_all(&plain).unwrap();
        let p = std::path::Path::new(&plain).join("c.txt").to_string_lossy().into_owned();
        std::fs::write(&p, "orig").unwrap();

        let id = tool_checkpoint_begin("t".into(), plain.clone()).unwrap();
        record_if_active(&p);                 // content mode captures via record
        std::fs::write(&p, "changed").unwrap();
        tool_checkpoint_end().unwrap();

        tool_checkpoint_restore(id).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "orig");
    }
}
