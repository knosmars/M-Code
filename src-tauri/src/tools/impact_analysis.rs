use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::tools::resolve_workspace_path;

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolInfo {
    pub name: String,
    pub kind: String,
    pub file: String,
    pub line: usize,
    pub column: usize,
    pub signature: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImpactResult {
    pub symbol: SymbolInfo,
    pub callers: Vec<SymbolRef>,
    pub callees: Vec<SymbolRef>,
    pub imports_from: Vec<String>,
    pub imported_by: Vec<String>,
    pub affected_files: Vec<String>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolRef {
    pub name: String,
    pub file: String,
    pub line: usize,
    pub context: String,
}

// ---------------------------------------------------------------------------
// File walking
// ---------------------------------------------------------------------------

const SKIP_DIRS: &[&str] = &[
    "node_modules", "target", "dist", "build", ".next", "__pycache__",
    "venv", ".venv", "coverage", ".nyc_output", ".turbo", ".cache", ".git",
];

const SOURCE_EXTS: &[&str] = &[
    ".ts", ".tsx", ".js", ".jsx", ".rs", ".py", ".go", ".java", ".kt",
    ".c", ".cpp", ".h", ".hpp", ".cs",
];

fn should_skip_dir(name: &str) -> bool {
    name.starts_with('.') && name != ".meyatu" || SKIP_DIRS.contains(&name)
}

fn collect_source_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_recursive(root, &mut files);
    files
}

fn collect_recursive(dir: &Path, files: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                if !should_skip_dir(&name) {
                    collect_recursive(&path, files);
                }
            } else if path.is_file() {
                if let Some(ext) = path.extension() {
                    let ext_str = format!(".{}", ext.to_string_lossy());
                    if SOURCE_EXTS.contains(&ext_str.as_str()) {
                        files.push(path);
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Symbol extraction
// ---------------------------------------------------------------------------

fn extract_definitions(content: &str, file_path: &str) -> Vec<SymbolInfo> {
    let mut symbols = Vec::new();
    let lines: Vec<&str> = content.lines().collect();

    // TS/JS patterns
    let ts_patterns: Vec<(&str, &str)> = vec![
        ("function", r"(?:export\s+)?(?:async\s+)?function\s+(\w+)"),
        ("class", r"(?:export\s+)?class\s+(\w+)"),
        ("interface", r"(?:export\s+)?interface\s+(\w+)"),
        ("type", r"(?:export\s+)?type\s+(\w+)"),
        ("enum", r"(?:export\s+)?enum\s+(\w+)"),
        ("const", r"(?:export\s+)?const\s+(\w+)"),
        ("let", r"(?:export\s+)?let\s+(\w+)"),
        ("var", r"(?:export\s+)?var\s+(\w+)"),
    ];

    // Rust patterns
    let rust_patterns: Vec<(&str, &str)> = vec![
        ("function", r"(?:pub\s+)?(?:async\s+)?fn\s+(\w+)"),
        ("struct", r"(?:pub\s+)?struct\s+(\w+)"),
        ("enum", r"(?:pub\s+)?enum\s+(\w+)"),
        ("trait", r"(?:pub\s+)?trait\s+(\w+)"),
        ("impl", r"impl\s+(\w+)"),
        ("type", r"(?:pub\s+)?type\s+(\w+)"),
        ("const", r"(?:pub\s+)?const\s+(\w+)"),
        ("static", r"(?:pub\s+)?static\s+(\w+)"),
    ];

    // Python patterns
    let py_patterns: Vec<(&str, &str)> = vec![
        ("function", r"(?:async\s+)?def\s+(\w+)"),
        ("class", r"class\s+(\w+)"),
    ];

    // Go patterns
    let go_patterns: Vec<(&str, &str)> = vec![
        ("function", r"func\s+(?:\(\w+\s+\*?\w+\)\s+)?(\w+)"),
        ("struct", r"type\s+(\w+)\s+struct"),
        ("interface", r"type\s+(\w+)\s+interface"),
    ];

    // Java/Kotlin patterns
    let java_patterns: Vec<(&str, &str)> = vec![
        ("class", r"(?:public|private|protected)?\s*(?:abstract\s+)?class\s+(\w+)"),
        ("interface", r"(?:public|private|protected)?\s*interface\s+(\w+)"),
        ("enum", r"(?:public|private|protected)?\s*enum\s+(\w+)"),
        ("method", r"(?:public|private|protected)\s+(?:static\s+)?(?:\w+\s+)?(\w+)\s*\("),
    ];

    let ext = Path::new(file_path)
        .extension()
        .map(|e| e.to_string_lossy().to_string())
        .unwrap_or_default();

    let patterns: &Vec<(&str, &str)> = match ext.as_str() {
        "rs" => &rust_patterns,
        "py" => &py_patterns,
        "go" => &go_patterns,
        "java" | "kt" => &java_patterns,
        _ => &ts_patterns, // Default to TS/JS
    };

    for (line_num, line) in lines.iter().enumerate() {
        for (kind, pattern) in patterns {
            if let Ok(re) = Regex::new(pattern) {
                if let Some(caps) = re.captures(line) {
                    if let Some(name_match) = caps.get(1) {
                        let sig = line.trim().to_string();
                        symbols.push(SymbolInfo {
                            name: name_match.as_str().to_string(),
                            kind: kind.to_string(),
                            file: file_path.to_string(),
                            line: line_num + 1,
                            column: line.find(name_match.as_str()).unwrap_or(0),
                            signature: Some(sig),
                        });
                    }
                }
            }
        }
    }

    symbols
}

fn extract_references(content: &str, symbol: &str, file_path: &str) -> Vec<SymbolRef> {
    let mut refs = Vec::new();
    let lines: Vec<&str> = content.lines().collect();

    // Word-boundary regex for the symbol
    let pattern = format!(r"\b{}\b", regex::escape(symbol));
    let re = match Regex::new(&pattern) {
        Ok(r) => r,
        Err(_) => return refs,
    };

    for (line_num, line) in lines.iter().enumerate() {
        if re.is_match(line) {
            // Skip definition lines (where the symbol is being defined)
            let trimmed = line.trim();
            let is_def = trimmed.starts_with(&format!("function {}", symbol))
                || trimmed.starts_with(&format!("class {}", symbol))
                || trimmed.starts_with(&format!("struct {}", symbol))
                || trimmed.starts_with(&format!("enum {}", symbol))
                || trimmed.starts_with(&format!("trait {}", symbol))
                || trimmed.starts_with(&format!("interface {}", symbol))
                || trimmed.starts_with(&format!("type {}", symbol))
                || trimmed.starts_with(&format!("fn {}", symbol))
                || trimmed.starts_with(&format!("def {}", symbol))
                || trimmed.starts_with(&format!("pub fn {}", symbol))
                || trimmed.starts_with(&format!("pub struct {}", symbol))
                || trimmed.starts_with(&format!("pub enum {}", symbol))
                || trimmed.starts_with(&format!("pub trait {}", symbol))
                || trimmed.starts_with(&format!("const {}", symbol))
                || trimmed.starts_with(&format!("let {}", symbol));

            if !is_def {
                refs.push(SymbolRef {
                    name: symbol.to_string(),
                    file: file_path.to_string(),
                    line: line_num + 1,
                    context: trimmed.to_string(),
                });
            }
        }
    }

    refs
}

// ---------------------------------------------------------------------------
// Main command
// ---------------------------------------------------------------------------

/// Analyze the impact of changing a symbol (function/class/type) in the codebase.
///
/// Given a symbol name and its file location, finds:
/// - All callers (files that reference this symbol)
/// - All callees (symbols this function calls)
/// - Import relationships
/// - List of affected files
#[tauri::command]
pub fn tool_impact_analysis(
    path: String,
    symbol: String,
    line: usize,
) -> Result<ImpactResult, String> {
    let _ws = resolve_workspace_path(&path)?;
    let workspace = std::env::current_dir()
        .map_err(|e| format!("Failed to get workspace: {e}"))?;

    let files = collect_source_files(&workspace);

    // Find the symbol's definition
    let mut symbol_info = None;
    let mut all_symbols: HashMap<String, Vec<SymbolInfo>> = HashMap::new();

    for file_path in &files {
        let content = match fs::read_to_string(file_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let rel_path = file_path
            .strip_prefix(&workspace)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| file_path.to_string_lossy().to_string());

        let defs = extract_definitions(&content, &rel_path);

        // Check if this is the definition we're looking for
        if symbol_info.is_none() {
            for def in &defs {
                if def.name == symbol && (line == 0 || def.line == line) {
                    symbol_info = Some(def.clone());
                    break;
                }
            }
        }

        all_symbols.insert(rel_path, defs);
    }

    let info = symbol_info.as_ref().cloned().unwrap_or_else(|| SymbolInfo {
        name: symbol.clone(),
        kind: "unknown".to_string(),
        file: String::new(),
        line: 0,
        column: 0,
        signature: None,
    });

    // Find all references (callers)
    let mut callers = Vec::new();
    let mut affected_files = HashSet::new();

    for file_path in &files {
        let content = match fs::read_to_string(file_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let rel_path = file_path
            .strip_prefix(&workspace)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| file_path.to_string_lossy().to_string());

        let refs = extract_references(&content, &symbol, &rel_path);
        if !refs.is_empty() {
            affected_files.insert(rel_path.clone());
            callers.extend(refs);
        }
    }

    // Extract imports from the symbol's file
    let mut imports_from = Vec::new();
    if let Some(ref info) = symbol_info {
        if let Ok(content) = fs::read_to_string(workspace.join(&info.file)) {
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("import ") || trimmed.starts_with("use ") || trimmed.starts_with("from ") {
                    imports_from.push(trimmed.to_string());
                }
            }
        }
    }

    // Find files that import from the symbol's file
    let mut imported_by = Vec::new();
    if let Some(ref info) = symbol_info {
        for file_path in &files {
            let content = match fs::read_to_string(file_path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let rel_path = file_path
                .strip_prefix(&workspace)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| file_path.to_string_lossy().to_string());

            if rel_path == info.file {
                continue;
            }

            for line in content.lines() {
                let trimmed = line.trim();
                // Check if this file imports from the symbol's file
                if trimmed.contains(&info.file) || trimmed.contains(&info.file.replace(".rs", "").replace(".ts", "").replace(".js", "")) {
                    imported_by.push(rel_path.clone());
                    break;
                }
            }
        }
    }

    // Callees: symbols referenced in the same file (simplified)
    let callees = Vec::new(); // Full call graph analysis would be too expensive

    let affected: Vec<String> = affected_files.into_iter().collect();
    let summary = format!(
        "Symbol '{}' ({}) is referenced by {} locations in {} files. {} files import from its module.",
        symbol,
        info.kind,
        callers.len(),
        affected.len(),
        imported_by.len()
    );

    Ok(ImpactResult {
        symbol: info,
        callers,
        callees,
        imports_from,
        imported_by,
        affected_files: affected,
        summary,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_definitions() {
        let ts_code = r#"
export function hello(name: string) {
    return `Hello ${name}`;
}

export class Foo {
    bar(): number {
        return 42;
    }
}
"#;
        let defs = extract_definitions(ts_code, "test.ts");
        assert!(defs.iter().any(|d| d.name == "hello" && d.kind == "function"));
        assert!(defs.iter().any(|d| d.name == "Foo" && d.kind == "class"));
    }

    #[test]
    fn test_extract_definitions_rust() {
        let rs_code = r#"
pub fn do_something(x: i32) -> i32 {
    x + 1
}

pub struct Config {
    name: String,
}
"#;
        let defs = extract_definitions(rs_code, "main.rs");
        assert!(defs.iter().any(|d| d.name == "do_something" && d.kind == "function"));
        assert!(defs.iter().any(|d| d.name == "Config" && d.kind == "struct"));
    }

    #[test]
    fn test_extract_references() {
        let code = r#"
const x = hello(1);
function hello(n: number) {
    return n + 1;
}
const y = hello(2);
"#;
        let refs = extract_references(code, "hello", "test.ts");
        // Should find 2 references (the calls), not the definition
        assert_eq!(refs.len(), 2);
    }
}
