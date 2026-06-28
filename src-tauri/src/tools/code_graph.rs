use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::tools::resolve_workspace_path;

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub modules: Vec<ModuleInfo>,
    pub stats: GraphStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: String,
    pub name: String,
    pub kind: String, // function, class, struct, enum, type, variable, module
    pub file: String,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    pub from: String, // node id
    pub to: String,   // node id
    pub kind: String, // calls, imports, extends, implements, uses
    pub file: String,
    pub line: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleInfo {
    pub path: String,
    pub name: String,
    pub exports: Vec<String>,
    pub imports: Vec<String>,
    pub size_bytes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphStats {
    pub total_nodes: usize,
    pub total_edges: usize,
    pub total_modules: usize,
    pub languages: HashMap<String, usize>,
}

// ---------------------------------------------------------------------------
// File walking
// ---------------------------------------------------------------------------

const SKIP_DIRS: &[&str] = &[
    "node_modules", "target", "dist", "build", ".next", "__pycache__",
    "venv", ".venv", ".git", ".cache", "coverage",
];

const SOURCE_EXTS: &[&str] = &[
    ".ts", ".tsx", ".js", ".jsx", ".rs", ".py", ".go", ".java", ".kt",
    ".c", ".cpp", ".h", ".hpp",
];

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
                if (!name.starts_with('.') || name == ".meyatu")
                    && !SKIP_DIRS.contains(&name.as_ref()) {
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

fn extract_nodes(content: &str, file_path: &str) -> Vec<GraphNode> {
    let mut nodes = Vec::new();
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
    ];

    // Rust patterns
    let rust_patterns: Vec<(&str, &str)> = vec![
        ("function", r"(?:pub\s+)?(?:async\s+)?fn\s+(\w+)"),
        ("struct", r"(?:pub\s+)?struct\s+(\w+)"),
        ("enum", r"(?:pub\s+)?enum\s+(\w+)"),
        ("trait", r"(?:pub\s+)?trait\s+(\w+)"),
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

    let ext = Path::new(file_path)
        .extension()
        .map(|e| e.to_string_lossy().to_string())
        .unwrap_or_default();

    let patterns: &Vec<(&str, &str)> = match ext.as_str() {
        "rs" => &rust_patterns,
        "py" => &py_patterns,
        "go" => &go_patterns,
        _ => &ts_patterns,
    };

    for (line_num, line) in lines.iter().enumerate() {
        for (kind, pattern) in patterns {
            if let Ok(re) = Regex::new(pattern) {
                if let Some(caps) = re.captures(line) {
                    if let Some(name_match) = caps.get(1) {
                        let id = format!("{}:{}:{}", file_path, kind, name_match.as_str());
                        nodes.push(GraphNode {
                            id,
                            name: name_match.as_str().to_string(),
                            kind: kind.to_string(),
                            file: file_path.to_string(),
                            line: line_num + 1,
                            column: line.find(name_match.as_str()).unwrap_or(0),
                        });
                    }
                }
            }
        }
    }

    nodes
}

fn extract_edges(content: &str, file_path: &str, node_ids: &HashSet<String>) -> Vec<GraphEdge> {
    let mut edges = Vec::new();
    let lines: Vec<&str> = content.lines().collect();

    // Function call pattern
    static CALL_RE: OnceLock<Regex> = OnceLock::new();
    let call_re = CALL_RE.get_or_init(|| Regex::new(r"(\w+)\s*\(").unwrap());
    // Import pattern
    static IMPORT_RE: OnceLock<Regex> = OnceLock::new();
    let import_re = IMPORT_RE.get_or_init(|| Regex::new(r"(?:import|use|from|require)\s+\{?\s*(\w+)").unwrap());
    // Extend/implement pattern
    static EXTENDS_RE: OnceLock<Regex> = OnceLock::new();
    let extends_re = EXTENDS_RE.get_or_init(|| Regex::new(r"(?:extends|implements|:)\s+(\w+)").unwrap());

    for (line_num, line) in lines.iter().enumerate() {
        // Find function calls
        for cap in call_re.captures_iter(line) {
            if let Some(name) = cap.get(1) {
                let target_id = format!("{}:function:{}", file_path, name.as_str());
                if node_ids.contains(&target_id) {
                    // Find the caller (any node in this file)
                    if let Some(caller) = find_caller_node(file_path, line_num + 1, node_ids) {
                        edges.push(GraphEdge {
                            from: caller,
                            to: target_id,
                            kind: "calls".to_string(),
                            file: file_path.to_string(),
                            line: line_num + 1,
                        });
                    }
                }
            }
        }

        // Find imports
        for cap in import_re.captures_iter(line) {
            if let Some(name) = cap.get(1) {
                // Try to find the imported symbol
                for node_id in node_ids {
                    if node_id.ends_with(&format!(":{}", name.as_str())) {
                        edges.push(GraphEdge {
                            from: format!("{}:module", file_path),
                            to: node_id.clone(),
                            kind: "imports".to_string(),
                            file: file_path.to_string(),
                            line: line_num + 1,
                        });
                    }
                }
            }
        }

        // Find extends/implements
        for cap in extends_re.captures_iter(line) {
            if let Some(name) = cap.get(1) {
                for node_id in node_ids {
                    if node_id.ends_with(&format!(":{}", name.as_str())) {
                        edges.push(GraphEdge {
                            from: format!("{}:class:{}", file_path, find_current_class(&lines, line_num)),
                            to: node_id.clone(),
                            kind: "extends".to_string(),
                            file: file_path.to_string(),
                            line: line_num + 1,
                        });
                    }
                }
            }
        }
    }

    edges
}

fn find_caller_node(file_path: &str, line: usize, node_ids: &HashSet<String>) -> Option<String> {
    // Find the node that contains this line (the function being called from)
    let mut best: Option<(String, usize)> = None;
    
    for node_id in node_ids {
        if !node_id.starts_with(file_path) {
            continue;
        }
        
        // Extract line from node ID
        let parts: Vec<&str> = node_id.split(':').collect();
        if parts.len() >= 4 {
            if let Ok(node_line) = parts[2].parse::<usize>() {
                if node_line <= line
                    && best.as_ref().map_or(true, |(_, best_line)| node_line > *best_line) {
                        best = Some((node_id.clone(), node_line));
                    }
            }
        }
    }
    
    best.map(|(id, _)| id)
}

fn find_current_class(lines: &[&str], current_line: usize) -> String {
    // Walk backwards to find the class containing this line
    static CLASS_RE: OnceLock<Regex> = OnceLock::new();
    let re = CLASS_RE.get_or_init(|| Regex::new(r"class\s+(\w+)").unwrap());

    for i in (0..current_line).rev() {
        let line = lines[i];
        if line.contains("class ") {
            if let Some(caps) = re.captures(line) {
                return caps.get(1).map(|m| m.as_str().to_string()).unwrap_or_default();
            }
        }
    }
    "unknown".to_string()
}

// ---------------------------------------------------------------------------
// Main command
// ---------------------------------------------------------------------------

/// Build a code graph showing definitions, calls, and imports across the codebase.
///
/// Returns nodes (functions, classes, etc.), edges (calls, imports), and module info.
#[tauri::command]
pub fn tool_code_graph(path: String) -> Result<CodeGraph, String> {
    let _ws = resolve_workspace_path(&path)?;
    let workspace = std::env::current_dir()
        .map_err(|e| format!("Failed to get workspace: {e}"))?;

    let files = collect_source_files(&workspace);
    let mut all_nodes = Vec::new();
    let mut all_edges = Vec::new();
    let mut modules = Vec::new();
    let mut languages: HashMap<String, usize> = HashMap::new();
    let mut node_ids = HashSet::new();

    // Extract exports (simplified)
    static EXPORT_RE: OnceLock<Regex> = OnceLock::new();
    let export_re = EXPORT_RE.get_or_init(|| Regex::new(r"export\s+(?:default\s+)?(?:function|class|const|let|var|interface|type|enum)\s+(\w+)").unwrap());

    // First pass: collect all nodes
    for file_path in &files {
        let content = match fs::read_to_string(file_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let rel_path = file_path
            .strip_prefix(&workspace)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| file_path.to_string_lossy().to_string());

        // Track language
        let ext = Path::new(&rel_path)
            .extension()
            .map(|e| e.to_string_lossy().to_string())
            .unwrap_or_default();
        *languages.entry(ext).or_insert(0) += 1;

        let nodes = extract_nodes(&content, &rel_path);
        for node in &nodes {
            node_ids.insert(node.id.clone());
        }
        all_nodes.extend(nodes);

        // Extract imports for module info
        let imports: Vec<String> = content.lines()
            .filter_map(|line| {
                let trimmed = line.trim();
                if trimmed.starts_with("import ") || trimmed.starts_with("use ") || trimmed.starts_with("from ") {
                    Some(trimmed.to_string())
                } else {
                    None
                }
            })
            .collect();

        // Extract exports (simplified)
        let exports: Vec<String> = content.lines()
            .filter_map(|line| {
                let trimmed = line.trim();
                if trimmed.starts_with("export ") {
                    // Extract the exported name
                    export_re.captures(trimmed).and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
                } else {
                    None
                }
            })
            .collect();

        modules.push(ModuleInfo {
            path: rel_path.clone(),
            name: Path::new(&rel_path)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default(),
            exports,
            imports,
            size_bytes: content.len(),
        });
    }

    // Second pass: collect edges
    for file_path in &files {
        let content = match fs::read_to_string(file_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let rel_path = file_path
            .strip_prefix(&workspace)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| file_path.to_string_lossy().to_string());

        let edges = extract_edges(&content, &rel_path, &node_ids);
        all_edges.extend(edges);
    }

    Ok(CodeGraph {
        nodes: all_nodes,
        edges: all_edges,
        modules,
        stats: GraphStats {
            total_nodes: 0, // Will be set below
            total_edges: 0,
            total_modules: 0,
            languages,
        },
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_nodes() {
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
        let nodes = extract_nodes(ts_code, "test.ts");
        assert!(nodes.iter().any(|n| n.name == "hello" && n.kind == "function"));
        assert!(nodes.iter().any(|n| n.name == "Foo" && n.kind == "class"));
    }

    #[test]
    fn test_extract_nodes_rust() {
        let rs_code = r#"
pub fn do_something(x: i32) -> i32 {
    x + 1
}

pub struct Config {
    name: String,
}
"#;
        let nodes = extract_nodes(rs_code, "main.rs");
        assert!(nodes.iter().any(|n| n.name == "do_something" && n.kind == "function"));
        assert!(nodes.iter().any(|n| n.name == "Config" && n.kind == "struct"));
    }
}
