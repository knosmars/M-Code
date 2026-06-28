//! Real LSP client commands. Each command detects the language from the file
//! extension; supported languages with an available server use the real LSP
//! path; everything else falls back to the regex impl (`lsp_regex`).

pub mod client;
pub mod framing;
pub mod position;
pub mod registry;

use crate::tools::lsp_regex::{
    self, DefinitionResult, HoverResult, Location, Reference, ReferencesResult,
};
use crate::tools::resolve_workspace_path;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::Instant;

// ── helpers ──────────────────────────────────────────────────────────────

fn path_to_uri(path: &str) -> String {
    // MVP: no percent-encoding. Fine for typical paths; spaces/`#` would need it.
    format!("file://{path}")
}

fn uri_to_path(uri: &str) -> String {
    uri.strip_prefix("file://").unwrap_or(uri).to_string()
}

fn root_uri() -> Result<String, String> {
    let cwd = std::env::current_dir().map_err(|e| format!("cwd error: {e}"))?;
    Ok(format!("file://{}", cwd.to_string_lossy()))
}

fn nth_line(content: &str, line: usize) -> &str {
    content.lines().nth(line).unwrap_or("")
}

/// Pull plain text out of an LSP hover `contents` field, which may be a
/// `MarkupContent {value}`, a `MarkedString` (string or `{value}`), or an
/// array of those.
fn extract_hover_text(result: &Value) -> String {
    fn one(v: &Value) -> Option<String> {
        if let Some(s) = v.as_str() {
            return Some(s.to_string());
        }
        v.get("value").and_then(|x| x.as_str()).map(|s| s.to_string())
    }
    let contents = match result.get("contents") {
        Some(c) => c,
        None => return String::new(),
    };
    if let Some(arr) = contents.as_array() {
        arr.iter().filter_map(one).collect::<Vec<_>>().join("\n")
    } else {
        one(contents).unwrap_or_default()
    }
}

/// First location from a definition result (`Location`, `Location[]`,
/// `LocationLink`, or `LocationLink[]`): `(path, line, utf16_character)`.
fn first_location(v: &Value) -> Option<(String, u32, u32)> {
    let item = if v.is_array() { v.as_array()?.first()? } else { v };
    let (uri, range) = if let Some(u) = item.get("uri").and_then(|u| u.as_str()) {
        (u, item.get("range")?)
    } else {
        let u = item.get("targetUri")?.as_str()?;
        let r = item
            .get("targetSelectionRange")
            .or_else(|| item.get("targetRange"))?;
        (u, r)
    };
    let start = range.get("start")?;
    Some((
        uri_to_path(uri),
        start.get("line")?.as_u64()? as u32,
        start.get("character")?.as_u64()? as u32,
    ))
}

/// Convert an LSP (path, line, utf16-char) to our `Location` (char column),
/// caching file reads.
fn to_location(
    path: String,
    line: u32,
    utf16_char: u32,
    file_cache: &mut HashMap<String, String>,
) -> Location {
    let content = file_cache
        .entry(path.clone())
        .or_insert_with(|| fs::read_to_string(&path).unwrap_or_default());
    let line_text = content.lines().nth(line as usize).unwrap_or("");
    let column = position::utf16_to_char(line_text, utf16_char);
    Location {
        path,
        line: line as usize,
        column,
    }
}

// ── commands ─────────────────────────────────────────────────────────────

#[tauri::command]
pub fn tool_lsp_hover(path: String, line: usize, column: usize) -> Result<String, String> {
    let fp = Path::new(&path);
    if !fp.exists() {
        return Err(format!("File not found: {path}"));
    }
    let _safe = resolve_workspace_path(&path)?;
    let ext = fp.extension().and_then(|e| e.to_str()).unwrap_or("");

    let lang = match registry::lang_for_ext(ext) {
        Some(l) => l,
        None => return lsp_regex::tool_lsp_hover(path, line, column),
    };
    let client = match registry::get_or_spawn(lang, &root_uri()?) {
        Some(c) => c,
        None => return lsp_regex::tool_lsp_hover(path, line, column),
    };

    let content = fs::read_to_string(&path).map_err(|e| format!("Failed to read {path}: {e}"))?;
    let character = position::char_to_utf16(nth_line(&content, line), column);
    let uri = path_to_uri(&path);
    let deadline = Instant::now() + lang.timeout();

    let result = {
        let mut c = client.lock().map_err(|_| "lsp client poisoned")?;
        c.hover(&uri, lang.language_id(), &content, line as u32, character, deadline)?
    };

    let hover_text = extract_hover_text(&result);
    if hover_text.trim().is_empty() {
        return Err(format!("No hover info at {path}:{line}:{column}"));
    }
    let symbol = lsp_regex::extract_symbol_at_position(&content, line, column).unwrap_or_default();
    let mut lines = hover_text.lines();
    let signature = lines.next().map(|s| s.trim().to_string());
    let rest = lines.collect::<Vec<_>>().join("\n").trim().to_string();
    let documentation = if rest.is_empty() { None } else { Some(rest) };

    let out = HoverResult {
        symbol,
        kind: String::new(), // LSP hover carries no SymbolKind; left blank
        signature,
        documentation,
        location: Location { path: path.clone(), line, column },
    };
    serde_json::to_string(&out).map_err(|e| format!("JSON error: {e}"))
}

#[tauri::command]
pub fn tool_lsp_go_to_definition(
    path: String,
    line: usize,
    column: usize,
) -> Result<String, String> {
    let fp = Path::new(&path);
    if !fp.exists() {
        return Err(format!("File not found: {path}"));
    }
    let _safe = resolve_workspace_path(&path)?;
    let ext = fp.extension().and_then(|e| e.to_str()).unwrap_or("");

    let lang = match registry::lang_for_ext(ext) {
        Some(l) => l,
        None => return lsp_regex::tool_lsp_go_to_definition(path, line, column),
    };
    let client = match registry::get_or_spawn(lang, &root_uri()?) {
        Some(c) => c,
        None => return lsp_regex::tool_lsp_go_to_definition(path, line, column),
    };

    let content = fs::read_to_string(&path).map_err(|e| format!("Failed to read {path}: {e}"))?;
    let character = position::char_to_utf16(nth_line(&content, line), column);
    let uri = path_to_uri(&path);
    let deadline = Instant::now() + lang.timeout();

    let result = {
        let mut c = client.lock().map_err(|_| "lsp client poisoned")?;
        c.definition(&uri, lang.language_id(), &content, line as u32, character, deadline)?
    };

    let out = match first_location(&result) {
        Some((def_path, def_line, def_char)) => {
            let mut cache = HashMap::new();
            let location = to_location(def_path, def_line, def_char, &mut cache);
            let def_content = cache.get(&location.path).cloned().unwrap_or_default();
            let context = Some(lsp_regex::extract_context(&def_content, location.line, 3));
            DefinitionResult { found: true, location: Some(location), context }
        }
        None => DefinitionResult { found: false, location: None, context: None },
    };
    serde_json::to_string(&out).map_err(|e| format!("JSON error: {e}"))
}

#[tauri::command]
pub fn tool_lsp_find_references(
    path: String,
    line: usize,
    column: usize,
    include_definition: Option<bool>,
) -> Result<String, String> {
    let fp = Path::new(&path);
    if !fp.exists() {
        return Err(format!("File not found: {path}"));
    }
    let _safe = resolve_workspace_path(&path)?;
    let ext = fp.extension().and_then(|e| e.to_str()).unwrap_or("");

    let lang = match registry::lang_for_ext(ext) {
        Some(l) => l,
        None => return lsp_regex::tool_lsp_find_references(path, line, column, include_definition),
    };
    let client = match registry::get_or_spawn(lang, &root_uri()?) {
        Some(c) => c,
        None => return lsp_regex::tool_lsp_find_references(path, line, column, include_definition),
    };

    let content = fs::read_to_string(&path).map_err(|e| format!("Failed to read {path}: {e}"))?;
    let character = position::char_to_utf16(nth_line(&content, line), column);
    let uri = path_to_uri(&path);
    let include_decl = include_definition.unwrap_or(true);
    let deadline = Instant::now() + lang.timeout();

    let result = {
        let mut c = client.lock().map_err(|_| "lsp client poisoned")?;
        c.references(&uri, lang.language_id(), &content, line as u32, character, include_decl, deadline)?
    };

    let mut cache: HashMap<String, String> = HashMap::new();
    let mut references: Vec<Reference> = Vec::new();
    if let Some(arr) = result.as_array() {
        for loc in arr.iter().take(100) {
            if let Some((rpath, rline, rchar)) = first_location(loc) {
                let location = to_location(rpath, rline, rchar, &mut cache);
                let ctx_src = cache.get(&location.path).cloned().unwrap_or_default();
                let context = ctx_src
                    .lines()
                    .nth(location.line)
                    .unwrap_or("")
                    .trim()
                    .to_string();
                references.push(Reference { location, context });
            }
        }
    }
    let out = ReferencesResult { count: references.len(), references };
    serde_json::to_string(&out).map_err(|e| format!("JSON error: {e}"))
}
