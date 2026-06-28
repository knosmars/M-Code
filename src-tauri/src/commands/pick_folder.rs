use tauri_plugin_dialog::DialogExt;

/// Open a native folder picker dialog and return the selected path.
///
/// Uses tauri_plugin_dialog on the Rust side — no JS npm dependency needed.
/// Called from the frontend via `invoke('tool_pick_folder')`.
#[tauri::command]
pub async fn tool_pick_folder(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let result = app
        .dialog()
        .file()
        .blocking_pick_folder();

    Ok(result.map(|p| p.to_string()))
}

/// Open a native file picker dialog and return the selected file path.
///
/// Used by the composer's "attach file" (+) button. Returns `None` if the
/// user cancels. Reading the file's content is done via `tool_read_attachment`.
#[tauri::command]
pub async fn tool_pick_file(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let result = app.dialog().file().blocking_pick_file();

    Ok(result.map(|p| p.to_string()))
}

/// Set the session working directory to `path`. All file tools
/// (read/write/edit/grep/glob/run_command/git) resolve against
/// `std::env::current_dir()`, so changing the process cwd is what actually
/// repoints the AI's workspace — updating only the UI path would leave tools
/// operating in the launch directory. Returns the canonical path on success.
/// Accepts `~` or `~user` which is expanded to the home directory.
#[tauri::command]
pub fn tool_set_workspace(path: String) -> Result<String, String> {
    use std::path::Path;
    let expanded = if let Some(stripped) = path.strip_prefix('~') {
        let home = dirs::home_dir().ok_or("Cannot determine home directory")?;
        if stripped.is_empty() {
            home
        } else {
            home.join(stripped.strip_prefix('/').unwrap_or(stripped))
        }
    } else {
        Path::new(&path).to_path_buf()
    };
    if !expanded.is_dir() {
        return Err(format!("Not a directory: {path}"));
    }
    std::env::set_current_dir(&expanded).map_err(|e| format!("Failed to set workspace to {path}: {e}"))?;
    std::env::current_dir()
        .map(|c| c.to_string_lossy().to_string())
        .map_err(|e| format!("Failed to read workspace after change: {e}"))
}

/// Read a user-picked attachment as UTF-8 text, WITHOUT the workspace sandbox
/// check that `tool_read_file` enforces — an attachment is chosen from anywhere
/// via the native file dialog, so restricting it to the workspace is wrong.
///
/// Safety: this is a UI-only command (not exposed to the LLM as a tool), and
/// the path comes from an explicit native-dialog selection, i.e. user consent.
/// A size cap prevents accidentally loading a huge/binary file into the prompt.
#[tauri::command]
pub fn tool_read_attachment(path: String) -> Result<String, String> {
    use std::path::Path;
    let p = Path::new(&path);
    if !p.is_file() {
        return Err(format!("Not a file: {path}"));
    }
    const MAX_BYTES: u64 = 512 * 1024; // 512 KB cap for text attachments
    let size = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    if size > MAX_BYTES {
        return Err(format!(
            "File too large to attach as text ({size} bytes, limit {MAX_BYTES}). Paste the relevant part instead."
        ));
    }
    std::fs::read_to_string(p)
        .map_err(|e| format!("Failed to read {path} (binary file or not UTF-8 text?): {e}"))
}
