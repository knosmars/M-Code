use similar::TextDiff;

/// Generate a unified diff preview for an edit_file operation WITHOUT applying it.
/// Takes the same parameters as tool_edit_file but returns the diff string.
#[tauri::command]
pub fn tool_edit_file_preview(
    path: String,
    old_string: String,
    new_string: String,
) -> Result<String, String> {
    let p = std::path::Path::new(&path);
    if !p.exists() {
        return Err(format!("File not found: {path}"));
    }
    if !p.is_file() {
        return Err(format!("Not a file: {path}"));
    }
    // Security: workspace restriction (same as other tools)
    let _safe = super::resolve_workspace_path(&path)?;

    let content =
        std::fs::read_to_string(&path).map_err(|e| format!("Failed to read {path}: {e}"))?;

    // Check old_string exists (same validation as edit_file)
    let count = content.matches(&old_string).count();
    if count == 0 {
        return Err(format!("old_string not found in {path}: {old_string:?}"));
    }
    if count > 1 {
        return Err(format!(
            "old_string found {count} times in {path} — provide more surrounding context"
        ));
    }

    // Compute the new content (without writing)
    let new_content = content.replacen(&old_string, &new_string, 1);

    // Generate unified diff using `similar`
    let diff = TextDiff::from_lines(&content, &new_content);
    let mut output = String::new();

    // Add file header in unified diff format
    output.push_str(&format!("--- a/{path}\n"));
    output.push_str(&format!("+++ b/{path}\n"));

    for hunk in diff.unified_diff().context_radius(3).iter_hunks() {
        output.push_str(&format!("{hunk}"));
    }

    if output.lines().count() <= 2 {
        // Only header lines, no actual changes
        return Ok("(no changes detected)".to_string());
    }

    Ok(output)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup_temp_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("meyatu_diff_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn preview_produces_unified_diff() {
        let dir = setup_temp_dir();
        let file = dir.join("test.txt");
        fs::write(&file, "hello world\nfoo bar\n").unwrap();

        let result = tool_edit_file_preview(
            file.to_string_lossy().to_string(),
            "foo bar".to_string(),
            "baz qux".to_string(),
        )
        .unwrap();

        assert!(result.contains("--- a/"));
        assert!(result.contains("+++ b/"));
        assert!(result.contains("-foo bar"));
        assert!(result.contains("+baz qux"));
    }

    #[test]
    fn preview_fails_on_not_found() {
        let dir = setup_temp_dir();
        let file = dir.join("missing.txt");
        fs::write(&file, "hello\n").unwrap();

        let result = tool_edit_file_preview(
            file.to_string_lossy().to_string(),
            "nonexistent".to_string(),
            "replacement".to_string(),
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }

    #[test]
    fn preview_fails_on_multiple_matches() {
        let dir = setup_temp_dir();
        let file = dir.join("dup.txt");
        fs::write(&file, "foo\nfoo\nbar\n").unwrap();

        let result = tool_edit_file_preview(
            file.to_string_lossy().to_string(),
            "foo".to_string(),
            "baz".to_string(),
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("found 2 times"));
    }

    #[test]
    fn preview_no_changes_when_strings_identical() {
        let dir = setup_temp_dir();
        let file = dir.join("same.txt");
        fs::write(&file, "unchanged content\n").unwrap();

        let result = tool_edit_file_preview(
            file.to_string_lossy().to_string(),
            "unchanged content".to_string(),
            "unchanged content".to_string(),
        )
        .unwrap();

        assert_eq!(result, "(no changes detected)");
    }
}
