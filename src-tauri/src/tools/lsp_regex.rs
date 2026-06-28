use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::tools::resolve_workspace_path;

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub(crate) struct Location {
    pub path: String,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub(crate) struct HoverResult {
    pub symbol: String,
    pub kind: String,
    pub signature: Option<String>,
    pub documentation: Option<String>,
    pub location: Location,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub(crate) struct DefinitionResult {
    pub found: bool,
    pub location: Option<Location>,
    pub context: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub(crate) struct Reference {
    pub location: Location,
    pub context: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub(crate) struct ReferencesResult {
    pub count: usize,
    pub references: Vec<Reference>,
}

// ---------------------------------------------------------------------------
// Symbol extraction
// ---------------------------------------------------------------------------

/// Extract the identifier at the given 0-based line and column.
pub(crate) fn extract_symbol_at_position(content: &str, line: usize, column: usize) -> Option<String> {
    let lines: Vec<&str> = content.lines().collect();
    if line >= lines.len() {
        return None;
    }

    let line_content = lines[line];
    let byte_idx = line_content
        .char_indices()
        .nth(column)
        .map(|(i, _)| i)
        .unwrap_or(line_content.len());

    if byte_idx >= line_content.len() {
        return None;
    }

    let bytes = line_content.as_bytes();
    if !is_identifier_byte(bytes[byte_idx]) {
        return None;
    }

    // Expand left
    let mut start = byte_idx;
    while start > 0 && is_identifier_byte(bytes[start - 1]) {
        start -= 1;
    }

    // Expand right
    let mut end = byte_idx;
    while end < bytes.len() && is_identifier_byte(bytes[end]) {
        end += 1;
    }

    if start == end {
        return None;
    }

    Some(String::from_utf8_lossy(&bytes[start..end]).to_string())
}

fn is_identifier_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

// ---------------------------------------------------------------------------
// Language patterns
// ---------------------------------------------------------------------------

fn get_definition_patterns(ext: &str) -> &'static [Regex] {
    fn compile(pats: &[&str]) -> Vec<Regex> {
        pats.iter().filter_map(|p| Regex::new(p).ok()).collect()
    }
    match ext {
        "ts" | "tsx" | "js" | "jsx" => {
            // function foo, const foo =, let foo =, var foo =
            // class Foo, interface Foo, type Foo, enum Foo
            static RE: OnceLock<Vec<Regex>> = OnceLock::new();
            RE.get_or_init(|| {
                compile(&[
                    r"(?m)^\s*(?:export\s+)?(?:async\s+)?function\s+([A-Za-z_][A-Za-z0-9_]*)",
                    r"(?m)^\s*(?:export\s+)?(?:const|let|var)\s+([A-Za-z_][A-Za-z0-9_]*)\s*[=:]",
                    r"(?m)^\s*(?:export\s+)?class\s+([A-Za-z_][A-Za-z0-9_]*)",
                    r"(?m)^\s*(?:export\s+)?interface\s+([A-Za-z_][A-Za-z0-9_]*)",
                    r"(?m)^\s*(?:export\s+)?type\s+([A-Za-z_][A-Za-z0-9_]*)\s*=",
                    r"(?m)^\s*(?:export\s+)?enum\s+([A-Za-z_][A-Za-z0-9_]*)",
                ])
            })
            .as_slice()
        }
        "rs" => {
            static RE: OnceLock<Vec<Regex>> = OnceLock::new();
            RE.get_or_init(|| {
                compile(&[
                    r"(?m)^\s*(?:pub\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)",
                    r"(?m)^\s*(?:pub\s+)?struct\s+([A-Za-z_][A-Za-z0-9_]*)",
                    r"(?m)^\s*(?:pub\s+)?enum\s+([A-Za-z_][A-Za-z0-9_]*)",
                    r"(?m)^\s*(?:pub\s+)?trait\s+([A-Za-z_][A-Za-z0-9_]*)",
                    r"(?m)^\s*(?:pub\s+)?impl\s+(?:<[^>]+>\s+)?([A-Za-z_][A-Za-z0-9_]*)",
                    r"(?m)^\s*(?:pub\s+)?type\s+([A-Za-z_][A-Za-z0-9_]*)\s*=",
                    r"(?m)^\s*(?:pub\s+)?mod\s+([A-Za-z_][A-Za-z0-9_]*)",
                    r"(?m)^\s*(?:pub\s+)?const\s+([A-Za-z_][A-Za-z0-9_]*)\s*:",
                    r"(?m)^\s*(?:pub\s+)?static\s+([A-Za-z_][A-Za-z0-9_]*)\s*:",
                    r"(?m)^\s*let\s+(?:mut\s+)?([A-Za-z_][A-Za-z0-9_]*)",
                ])
            })
            .as_slice()
        }
        "py" => {
            static RE: OnceLock<Vec<Regex>> = OnceLock::new();
            RE.get_or_init(|| {
                compile(&[
                    r"(?m)^\s*def\s+([A-Za-z_][A-Za-z0-9_]*)",
                    r"(?m)^\s*class\s+([A-Za-z_][A-Za-z0-9_]*)",
                    r"(?m)^\s*([A-Za-z_][A-Za-z0-9_]*)\s*=",
                ])
            })
            .as_slice()
        }
        "go" => {
            static RE: OnceLock<Vec<Regex>> = OnceLock::new();
            RE.get_or_init(|| {
                compile(&[
                    r"(?m)^\s*func\s+([A-Za-z_][A-Za-z0-9_]*)",
                    r"(?m)^\s*type\s+([A-Za-z_][A-Za-z0-9_]*)\s+",
                    r"(?m)^\s*struct\s+([A-Za-z_][A-Za-z0-9_]*)",
                    r"(?m)^\s*interface\s+([A-Za-z_][A-Za-z0-9_]*)",
                    r"(?m)^\s*(?:var|const)\s+([A-Za-z_][A-Za-z0-9_]*)\s+",
                ])
            })
            .as_slice()
        }
        "java" | "kt" => {
            static RE: OnceLock<Vec<Regex>> = OnceLock::new();
            RE.get_or_init(|| {
                compile(&[
                    r"(?m)^\s*(?:public|private|protected)?\s*(?:static\s+)?(?:final\s+)?(?:class|interface|enum|@interface|record)\s+([A-Za-z_][A-Za-z0-9_]*)",
                    r"(?m)^\s*(?:public|private|protected)?\s*(?:static\s+)?(?:final\s+)?\w+\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(",
                ])
            })
            .as_slice()
        }
        _ => &[],
    }
}

fn kind_from_pattern_line(line: &str, ext: &str) -> String {
    let trimmed = line.trim_start();
    if ext == "rs" {
        if trimmed.starts_with("fn ") || trimmed.starts_with("pub fn ") {
            return "function".to_string();
        }
        if trimmed.starts_with("struct ") || trimmed.starts_with("pub struct ") {
            return "struct".to_string();
        }
        if trimmed.starts_with("enum ") || trimmed.starts_with("pub enum ") {
            return "enum".to_string();
        }
        if trimmed.starts_with("trait ") || trimmed.starts_with("pub trait ") {
            return "trait".to_string();
        }
        if trimmed.starts_with("impl ") || trimmed.starts_with("pub impl ") {
            return "impl".to_string();
        }
        if trimmed.starts_with("mod ") || trimmed.starts_with("pub mod ") {
            return "module".to_string();
        }
        if trimmed.starts_with("const ") || trimmed.starts_with("pub const ") {
            return "constant".to_string();
        }
        if trimmed.starts_with("static ") || trimmed.starts_with("pub static ") {
            return "static".to_string();
        }
        if trimmed.starts_with("let ") {
            return "variable".to_string();
        }
        if trimmed.starts_with("type ") || trimmed.starts_with("pub type ") {
            return "type alias".to_string();
        }
    } else if ext == "py" {
        if trimmed.starts_with("def ") {
            return "function".to_string();
        }
        if trimmed.starts_with("class ") {
            return "class".to_string();
        }
        return "variable".to_string();
    } else if ext == "go" {
        if trimmed.starts_with("func ") {
            return "function".to_string();
        }
        if trimmed.starts_with("struct ") {
            return "struct".to_string();
        }
        if trimmed.starts_with("interface ") {
            return "interface".to_string();
        }
        if trimmed.starts_with("type ") {
            return "type alias".to_string();
        }
        if trimmed.starts_with("var ") || trimmed.starts_with("const ") {
            return "variable".to_string();
        }
    } else if ext == "java" || ext == "kt" {
        if trimmed.contains("class ") || trimmed.contains("interface ") || trimmed.contains("enum ") || trimmed.contains("record ") {
            return "class".to_string();
        }
        return "method".to_string();
    } else {
        // ts/js
        if trimmed.contains("function ") {
            return "function".to_string();
        }
        if trimmed.contains("class ") {
            return "class".to_string();
        }
        if trimmed.contains("interface ") {
            return "interface".to_string();
        }
        if trimmed.contains("type ") && trimmed.contains("=") {
            return "type alias".to_string();
        }
        if trimmed.contains("enum ") {
            return "enum".to_string();
        }
        if trimmed.contains("const ") || trimmed.contains("let ") || trimmed.contains("var ") {
            return "variable".to_string();
        }
    }
    "symbol".to_string()
}

// ---------------------------------------------------------------------------
// Definition search
// ---------------------------------------------------------------------------

fn find_symbol_definition(
    symbol: &str,
    start_path: &Path,
    ext: &str,
) -> Option<(PathBuf, usize, usize, String)> {
    let patterns = get_definition_patterns(ext);
    if patterns.is_empty() {
        return None;
    }

    let workspace = std::env::current_dir().ok()?.canonicalize().ok()?;

    let candidates = if start_path.is_file() {
        vec![start_path.to_path_buf()]
    } else {
        walk_source_files(&workspace, ext)
    };

    for file_path in candidates {
        let content = fs::read_to_string(&file_path).ok()?;
        for (line_num, line) in content.lines().enumerate() {
            for re in patterns {
                if let Some(cap) = re.captures(line) {
                    if cap.get(1).map(|m| m.as_str()) == Some(symbol) {
                        let col = line.find(symbol).unwrap_or(0);
                        return Some((file_path.clone(), line_num, col, line.trim().to_string()));
                    }
                }
            }
        }
    }

    None
}

fn walk_source_files(workspace: &Path, ext: &str) -> Vec<PathBuf> {
    let mut results = Vec::new();
    let exts: Vec<&str> = if ext == "ts" {
        vec!["ts", "tsx"]
    } else if ext == "js" {
        vec!["js", "jsx"]
    } else {
        vec![ext]
    };

    if let Ok(entries) = fs::read_dir(workspace) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                if name.starts_with('.') || name == "node_modules" || name == "target" {
                    continue;
                }
                walk_source_files_recursive(&path, &exts, &mut results);
            } else if path.is_file() {
                if let Some(e) = path.extension() {
                    if exts.contains(&e.to_string_lossy().as_ref()) {
                        results.push(path);
                    }
                }
            }
        }
    }
    results
}

fn walk_source_files_recursive(dir: &Path, exts: &[&str], results: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                if name.starts_with('.') || name == "node_modules" || name == "target" {
                    continue;
                }
                walk_source_files_recursive(&path, exts, results);
            } else if path.is_file() {
                if let Some(e) = path.extension() {
                    if exts.contains(&e.to_string_lossy().as_ref()) {
                        results.push(path);
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Documentation extraction
// ---------------------------------------------------------------------------

fn extract_documentation(content: &str, def_line: usize) -> Option<String> {
    let lines: Vec<&str> = content.lines().collect();
    if def_line >= lines.len() {
        return None;
    }

    let mut docs: Vec<String> = Vec::new();
    for i in (0..def_line).rev() {
        let line = lines[i];
        let trimmed = line.trim_start();
        if trimmed.starts_with("///") {
            docs.push(trimmed.trim_start_matches("///").trim().to_string());
        } else if trimmed.starts_with("/**") || trimmed.starts_with("* ") {
            let clean = trimmed
                .trim_start_matches("/**")
                .trim_start_matches("* ")
                .trim_start_matches("*/")
                .trim();
            if !clean.is_empty() {
                docs.push(clean.to_string());
            }
        } else if trimmed.starts_with("//!") {
            // module doc, skip
            continue;
        } else if trimmed.starts_with("//") {
            // regular comment, stop
            break;
        } else if trimmed.is_empty() {
            continue;
        } else {
            break;
        }
    }

    if docs.is_empty() {
        return None;
    }

    docs.reverse();
    Some(docs.join(" "))
}

pub(crate) fn extract_context(content: &str, def_line: usize, radius: usize) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let start = def_line.saturating_sub(radius);
    let end = (def_line + radius + 1).min(lines.len());
    lines[start..end].join("\n")
}

// ---------------------------------------------------------------------------
// Reference search
// ---------------------------------------------------------------------------

fn find_all_references(
    symbol: &str,
    workspace: &Path,
    source_ext: &str,
) -> Vec<(PathBuf, usize, usize, String)> {
    let mut results = Vec::new();
    let symbol_re = match Regex::new(&format!(r"\b{}\b", regex::escape(symbol))) {
        Ok(re) => re,
        Err(_) => return results,
    };

    // Determine which extensions to search
    let exts: Vec<&str> = match source_ext {
        "ts" => vec!["ts", "tsx"],
        "tsx" => vec!["ts", "tsx"],
        "js" => vec!["js", "jsx"],
        "jsx" => vec!["js", "jsx"],
        other => vec![other],
    };

    walk_for_references(workspace, &exts, &symbol_re, symbol, &mut results);
    results
}

#[allow(clippy::only_used_in_recursion)]
fn walk_for_references(
    dir: &Path,
    exts: &[&str],
    symbol_re: &Regex,
    symbol: &str,
    results: &mut Vec<(PathBuf, usize, usize, String)>,
) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                if name.starts_with('.') || name == "node_modules" || name == "target" || name == ".git" {
                    continue;
                }
                walk_for_references(&path, exts, symbol_re, symbol, results);
            } else if path.is_file() {
                if let Some(e) = path.extension() {
                    if exts.contains(&e.to_string_lossy().as_ref()) {
                        if let Ok(content) = fs::read_to_string(&path) {
                            for (line_num, line) in content.lines().enumerate() {
                                for m in symbol_re.find_iter(line) {
                                    let context = extract_context(&content, line_num, 2);
                                    results.push((
                                        path.clone(),
                                        line_num,
                                        m.start(),
                                        context,
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn is_definition_line(line: &str, symbol: &str, ext: &str) -> bool {
    let patterns = get_definition_patterns(ext);
    for re in patterns {
        if let Some(cap) = re.captures(line) {
            if cap.get(1).map(|m| m.as_str()) == Some(symbol) {
                return true;
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Tool commands
// ---------------------------------------------------------------------------

pub fn tool_lsp_hover(path: String, line: usize, column: usize) -> Result<String, String> {
    let file_path = Path::new(&path);
    if !file_path.exists() {
        return Err(format!("File not found: {path}"));
    }
    let _safe = resolve_workspace_path(&path)?;

    let content = fs::read_to_string(&path).map_err(|e| format!("Failed to read {path}: {e}"))?;
    let symbol = extract_symbol_at_position(&content, line, column)
        .ok_or_else(|| format!("No symbol found at {path}:{line}:{column}"))?;

    let ext = file_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    let (def_path, def_line, def_col, def_line_text) =
        find_symbol_definition(&symbol, file_path, ext)
            .ok_or_else(|| format!("Definition not found for '{symbol}'"))?;

    let def_content = fs::read_to_string(&def_path)
        .map_err(|e| format!("Failed to read definition file: {e}"))?;

    let kind = kind_from_pattern_line(&def_line_text, ext);
    let documentation = extract_documentation(&def_content, def_line);
    let signature = Some(def_line_text);

    let result = HoverResult {
        symbol,
        kind,
        signature,
        documentation,
        location: Location {
            path: def_path.to_string_lossy().to_string(),
            line: def_line,
            column: def_col,
        },
    };

    serde_json::to_string(&result).map_err(|e| format!("JSON error: {e}"))
}

pub fn tool_lsp_go_to_definition(
    path: String,
    line: usize,
    column: usize,
) -> Result<String, String> {
    let file_path = Path::new(&path);
    if !file_path.exists() {
        return Err(format!("File not found: {path}"));
    }
    let _safe = resolve_workspace_path(&path)?;

    let content = fs::read_to_string(&path).map_err(|e| format!("Failed to read {path}: {e}"))?;
    let symbol = extract_symbol_at_position(&content, line, column)
        .ok_or_else(|| format!("No symbol found at {path}:{line}:{column}"))?;

    let ext = file_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    if let Some((def_path, def_line, def_col, _def_line_text)) =
        find_symbol_definition(&symbol, file_path, ext)
    {
        let def_content = fs::read_to_string(&def_path).ok();
        let context = def_content.map(|c| extract_context(&c, def_line, 3));

        let result = DefinitionResult {
            found: true,
            location: Some(Location {
                path: def_path.to_string_lossy().to_string(),
                line: def_line,
                column: def_col,
            }),
            context,
        };
        serde_json::to_string(&result).map_err(|e| format!("JSON error: {e}"))
    } else {
        let result = DefinitionResult {
            found: false,
            location: None,
            context: None,
        };
        serde_json::to_string(&result).map_err(|e| format!("JSON error: {e}"))
    }
}

pub fn tool_lsp_find_references(
    path: String,
    line: usize,
    column: usize,
    include_definition: Option<bool>,
) -> Result<String, String> {
    let file_path = Path::new(&path);
    if !file_path.exists() {
        return Err(format!("File not found: {path}"));
    }
    let _safe = resolve_workspace_path(&path)?;

    let content = fs::read_to_string(&path).map_err(|e| format!("Failed to read {path}: {e}"))?;
    let symbol = extract_symbol_at_position(&content, line, column)
        .ok_or_else(|| format!("No symbol found at {path}:{line}:{column}"))?;

    let ext = file_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    let workspace = std::env::current_dir()
        .map_err(|e| format!("Failed to get workspace: {e}"))?
        .canonicalize()
        .map_err(|e| format!("Failed to resolve workspace: {e}"))?;

    let raw_refs = find_all_references(&symbol, &workspace, ext);
    let include_def = include_definition.unwrap_or(true);

    let mut seen = HashSet::new();
    let mut references: Vec<Reference> = Vec::new();

    for (ref_path, ref_line, ref_col, context) in raw_refs {
        if !include_def {
            if let Ok(ref_content) = fs::read_to_string(&ref_path) {
                let lines: Vec<&str> = ref_content.lines().collect();
                if ref_line < lines.len() && is_definition_line(lines[ref_line], &symbol, ext) {
                    continue;
                }
            }
        }

        let key = format!(
            "{}:{}:{}",
            ref_path.display(),
            ref_line,
            ref_col
        );
        if seen.insert(key) {
            references.push(Reference {
                location: Location {
                    path: ref_path.to_string_lossy().to_string(),
                    line: ref_line,
                    column: ref_col,
                },
                context,
            });
        }

        if references.len() >= 100 {
            break;
        }
    }

    let result = ReferencesResult {
        count: references.len(),
        references,
    };

    serde_json::to_string(&result).map_err(|e| format!("JSON error: {e}"))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup_temp_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("meyatu_lsp_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn definition_patterns_route_by_ext() {
        assert!(!get_definition_patterns("rs").is_empty());
        assert!(!get_definition_patterns("ts").is_empty());
        assert!(get_definition_patterns("unknown_ext").is_empty());
        // compiled once: repeat calls return the same cached slice
        assert!(std::ptr::eq(
            get_definition_patterns("rs").as_ptr(),
            get_definition_patterns("rs").as_ptr(),
        ));
    }

    #[test]
    fn test_lsp_hover_extracts_symbol() {
        let content = r#"fn hello_world() {
    let x = 42;
}"#;
        assert_eq!(extract_symbol_at_position(content, 0, 3), Some("hello_world".to_string()));
        assert_eq!(extract_symbol_at_position(content, 1, 8), Some("x".to_string()));
        assert_eq!(extract_symbol_at_position(content, 0, 0), Some("fn".to_string()));
    }

    #[test]
    fn test_lsp_go_to_definition_finds_function() {
        let dir = setup_temp_dir();
        let file = dir.join("test.rs");
        fs::write(
            &file,
            r#"fn helper() -> i32 {
    42
}

fn main() {
    let v = helper();
}
"#,
        )
        .unwrap();

        // helper() on line 5, column 12 (inside "helper")
        let content = fs::read_to_string(&file).unwrap();
        let symbol = extract_symbol_at_position(&content, 5, 12).unwrap();
        assert_eq!(symbol, "helper");

        let result = find_symbol_definition(&symbol, &file, "rs");
        assert!(result.is_some());
        let (path, line, _col, line_text) = result.unwrap();
        assert_eq!(path, file);
        assert_eq!(line, 0);
        assert!(line_text.contains("fn helper"));
    }

    #[test]
    fn test_lsp_find_references_counts_occurrences() {
        let dir = setup_temp_dir();
        let file_a = dir.join("a.rs");
        fs::write(
            &file_a,
            r#"fn compute() -> i32 {
    1
}

fn use_it() -> i32 {
    compute() + compute()
}
"#,
        )
        .unwrap();

        let workspace = dir.clone();
        let refs = find_all_references("compute", &workspace, "rs");
        // Should find: definition on line 0 + 2 calls on line 5 = 3 total
        assert_eq!(refs.len(), 3);

        // Verify at least one reference is from the call site
        let call_refs: Vec<_> = refs
            .iter()
            .filter(|(_, line, _, _)| *line == 5)
            .collect();
        assert_eq!(call_refs.len(), 2);
    }

    #[test]
    fn test_extract_documentation_rust() {
        let content = r#"/// Adds two numbers.
/// Returns the sum.
fn add(a: i32, b: i32) -> i32 {
    a + b
}"#;
        let docs = extract_documentation(content, 2);
        assert!(docs.is_some());
        assert!(docs.unwrap().contains("Adds two numbers"));
    }

    #[test]
    fn test_lsp_go_definition_patterns() {
        let dir = setup_temp_dir();
        let file = dir.join("main.go");
        fs::write(
            &file,
            r#"package main

func hello() string {
    return "hello"
}

func main() {
    println(hello())
}
"#,
        )
        .unwrap();

        let content = fs::read_to_string(&file).unwrap();
        let symbol = extract_symbol_at_position(&content, 7, 14).unwrap();
        assert_eq!(symbol, "hello");

        let result = find_symbol_definition(&symbol, &file, "go");
        assert!(result.is_some());
        let (path, line, _col, _) = result.unwrap();
        assert_eq!(path, file);
        assert_eq!(line, 2);
    }

    #[test]
    fn test_lsp_java_definition_patterns() {
        let dir = setup_temp_dir();
        let file = dir.join("App.java");
        fs::write(
            &file,
            r#"public class App {
    public static void main(String[] args) {
        greet();
    }

    static void greet() {
        System.out.println("hello");
    }
}
"#,
        )
        .unwrap();

        let content = fs::read_to_string(&file).unwrap();
        let symbol = extract_symbol_at_position(&content, 2, 8).unwrap();
        assert_eq!(symbol, "greet");

        let result = find_symbol_definition(&symbol, &file, "java");
        assert!(result.is_some());
        let (path, line, _col, _) = result.unwrap();
        assert_eq!(path, file);
        assert_eq!(line, 5);
    }

    #[test]
    fn test_kind_from_pattern_line() {
        assert_eq!(kind_from_pattern_line("fn foo() {}", "rs"), "function");
        assert_eq!(kind_from_pattern_line("struct Bar {}", "rs"), "struct");
        assert_eq!(kind_from_pattern_line("class Foo {}", "ts"), "class");
        assert_eq!(kind_from_pattern_line("function baz() {}", "ts"), "function");
    }

    #[test]
    fn test_extract_symbol_edge_cases() {
        let content = "let abc_def = 1;";
        assert_eq!(extract_symbol_at_position(content, 0, 4), Some("abc_def".to_string()));
        assert_eq!(extract_symbol_at_position(content, 0, 10), Some("abc_def".to_string()));
        // Space should return None
        assert_eq!(extract_symbol_at_position(content, 0, 3), None);
    }
}
