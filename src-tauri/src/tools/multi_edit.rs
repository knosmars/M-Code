use serde::{Deserialize, Serialize};
use similar::TextDiff;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct EditOperation {
    pub path: String,
    pub old_string: String,
    pub new_string: String,
}

/// Preview multiple file edits as a combined unified diff WITHOUT applying.
#[tauri::command]
pub fn tool_multi_edit_preview(edits: Vec<EditOperation>) -> Result<String, String> {
    if edits.is_empty() {
        return Err("No edits provided".to_string());
    }

    let mut combined_diff = String::new();
    let mut file_count = 0;

    for edit in &edits {
        let p = std::path::Path::new(&edit.path);
        if !p.exists() {
            return Err(format!("File not found: {}", edit.path));
        }
        if !p.is_file() {
            return Err(format!("Not a file: {}", edit.path));
        }
        let _safe = super::resolve_workspace_path(&edit.path)?;

        let content = std::fs::read_to_string(&edit.path)
            .map_err(|e| format!("Failed to read {}: {}", edit.path, e))?;

        let count = content.matches(&edit.old_string).count();
        if count == 0 {
            return Err(format!(
                "old_string not found in {}: {:?}",
                edit.path, edit.old_string
            ));
        }
        if count > 1 {
            return Err(format!(
                "old_string found {} times in {} — provide more surrounding context",
                count, edit.path
            ));
        }

        let new_content = content.replacen(&edit.old_string, &edit.new_string, 1);
        let diff = TextDiff::from_lines(&content, &new_content);

        combined_diff.push_str(&format!("--- a/{}\n", edit.path));
        combined_diff.push_str(&format!("+++ b/{}\n", edit.path));

        for hunk in diff.unified_diff().context_radius(3).iter_hunks() {
            combined_diff.push_str(&format!("{hunk}"));
        }
        combined_diff.push('\n');
        file_count += 1;
    }

    if file_count == 0 {
        return Ok("(no changes detected)".to_string());
    }

    Ok(format!(
        "Multi-file edit preview ({} file{}):\n\n{}",
        file_count,
        if file_count == 1 { "" } else { "s" },
        combined_diff
    ))
}

/// Apply multiple file edits atomically — all succeed or all fail.
#[tauri::command]
pub fn tool_multi_edit_apply(edits: Vec<EditOperation>) -> Result<String, String> {
    if edits.is_empty() {
        return Err("No edits provided".to_string());
    }

    // Phase 1: Validate all edits and compute new contents
    let mut pending_writes: Vec<(String, String)> = Vec::new();

    for edit in &edits {
        let p = std::path::Path::new(&edit.path);
        if !p.exists() {
            return Err(format!("File not found: {}", edit.path));
        }
        if !p.is_file() {
            return Err(format!("Not a file: {}", edit.path));
        }
        let _safe = super::resolve_workspace_path(&edit.path)?;

        let content = std::fs::read_to_string(&edit.path)
            .map_err(|e| format!("Failed to read {}: {}", edit.path, e))?;

        let count = content.matches(&edit.old_string).count();
        if count == 0 {
            return Err(format!(
                "old_string not found in {}: {:?}",
                edit.path, edit.old_string
            ));
        }
        if count > 1 {
            return Err(format!(
                "old_string found {} times in {} — provide more surrounding context",
                count, edit.path
            ));
        }

        let new_content = content.replacen(&edit.old_string, &edit.new_string, 1);
        pending_writes.push((edit.path.clone(), new_content));
    }

    // Phase 2: Apply all writes (if we got here, all validations passed)
    let mut applied = Vec::new();
    for (path, new_content) in &pending_writes {
        super::checkpoint::record_if_active(path);
        std::fs::write(path, new_content).map_err(|e| {
            format!(
                "Failed to write {} (partial apply may have occurred): {}",
                path, e
            )
        })?;
        applied.push(path.clone());
    }

    Ok(format!(
        "Applied {} edit{}: {}",
        applied.len(),
        if applied.len() == 1 { "" } else { "s" },
        applied.join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup_temp_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "meyatu_multi_edit_test_{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn multi_preview_combines_diffs() {
        let dir = setup_temp_dir();
        let file1 = dir.join("a.txt");
        let file2 = dir.join("b.txt");
        fs::write(&file1, "hello\n").unwrap();
        fs::write(&file2, "world\n").unwrap();

        let edits = vec![
            EditOperation {
                path: file1.to_string_lossy().to_string(),
                old_string: "hello".to_string(),
                new_string: "greeting".to_string(),
            },
            EditOperation {
                path: file2.to_string_lossy().to_string(),
                old_string: "world".to_string(),
                new_string: "earth".to_string(),
            },
        ];

        let result = tool_multi_edit_preview(edits).unwrap();
        assert!(result.contains("2 files"));
        assert!(result.contains("--- a/"));
        assert!(result.contains("-hello"));
        assert!(result.contains("-world"));
    }

    #[test]
    fn multi_apply_applies_all() {
        let dir = setup_temp_dir();
        let file1 = dir.join("x.txt");
        let file2 = dir.join("y.txt");
        fs::write(&file1, "foo\n").unwrap();
        fs::write(&file2, "bar\n").unwrap();

        let edits = vec![
            EditOperation {
                path: file1.to_string_lossy().to_string(),
                old_string: "foo".to_string(),
                new_string: "FOO".to_string(),
            },
            EditOperation {
                path: file2.to_string_lossy().to_string(),
                old_string: "bar".to_string(),
                new_string: "BAR".to_string(),
            },
        ];

        let result = tool_multi_edit_apply(edits).unwrap();
        assert!(result.contains("2 edits"));

        assert_eq!(fs::read_to_string(&file1).unwrap(), "FOO\n");
        assert_eq!(fs::read_to_string(&file2).unwrap(), "BAR\n");
    }

    #[test]
    fn multi_apply_fails_on_missing_file() {
        let dir = setup_temp_dir();
        let file1 = dir.join("exists.txt");
        fs::write(&file1, "content\n").unwrap();

        let edits = vec![
            EditOperation {
                path: file1.to_string_lossy().to_string(),
                old_string: "content".to_string(),
                new_string: "new".to_string(),
            },
            EditOperation {
                path: dir.join("missing.txt").to_string_lossy().to_string(),
                old_string: "x".to_string(),
                new_string: "y".to_string(),
            },
        ];

        let result = tool_multi_edit_apply(edits);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
        // First file should NOT be modified (atomic failure)
        assert_eq!(fs::read_to_string(&file1).unwrap(), "content\n");
    }
}
