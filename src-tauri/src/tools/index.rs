use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

/// Top-level result of a workspace codebase scan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexReport {
    /// Absolute path of the workspace root.
    pub workspace_root: String,
    /// Total number of files scanned.
    pub file_count: usize,
    /// Total size of all scanned files in bytes.
    pub total_bytes: u64,
    /// Language breakdown: "TypeScript" → file count.
    pub languages: HashMap<String, usize>,
    /// Top-level package manifest info (if found).
    pub packages: Vec<PackageInfo>,
    /// Directory tree (depth-limited).
    pub tree: DirectoryNode,
    /// Important entrypoint files discovered.
    pub entrypoints: Vec<String>,
    /// Dependency graph: source file → set of imported files.
    pub imports: HashMap<String, Vec<String>>,
}

/// Metadata extracted from a package manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageInfo {
    pub kind: String, // "npm", "cargo", "python"
    pub name: String,
    pub version: Option<String>,
    pub dependencies: Vec<String>,
    pub dev_dependencies: Vec<String>,
    pub scripts: HashMap<String, String>,
}

/// A node in the directory tree (max depth 3, top 50 entries per directory).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectoryNode {
    pub name: String,
    pub is_dir: bool,
    pub size: Option<u64>,
    pub children: Vec<DirectoryNode>,
}

/// Known file extensions → language label.
fn language_for(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("ts") | Some("tsx") => "TypeScript",
        Some("js") | Some("jsx") | Some("mjs") | Some("cjs") => "JavaScript",
        Some("rs") => "Rust",
        Some("py") => "Python",
        Some("go") => "Go",
        Some("java") | Some("kt") => "JVM",
        Some("c") | Some("h") => "C",
        Some("cpp") | Some("hpp") | Some("cc") | Some("cxx") => "C++",
        Some("css") | Some("scss") | Some("less") => "CSS",
        Some("html") | Some("htm") => "HTML",
        Some("json") => "JSON",
        Some("yaml") | Some("yml") => "YAML",
        Some("md") | Some("mdx") => "Markdown",
        Some("toml") => "TOML",
        Some("sh") | Some("bash") | Some("zsh") => "Shell",
        Some("sql") => "SQL",
        Some("svg") => "SVG",
        Some("proto") => "Protobuf",
        _ => "Other",
    }
}

/// Files and directories to skip during scanning.
fn should_skip(name: &str, is_dir: bool) -> bool {
    if name.starts_with('.') {
        // Allow .meyatu, .env, .gitignore etc as files
        if is_dir && name != ".meyatu" {
            return true;
        }
        if name == ".DS_Store" {
            return true;
        }
    }
    if is_dir {
        matches!(
            name,
            "node_modules"
                | "target"
                | "dist"
                | "build"
                | ".next"
                | "__pycache__"
                | "venv"
                | ".venv"
                | "coverage"
                | ".nyc_output"
                | ".turbo"
                | ".cache"
        )
    } else {
        false
    }
}

/// Files considered "entrypoints" for the project.
fn is_entrypoint(name: &str) -> bool {
    matches!(
        name,
        "main.ts"
            | "main.tsx"
            | "index.ts"
            | "index.tsx"
            | "App.tsx"
            | "main.rs"
            | "lib.rs"
            | "mod.rs"
            | "main.py"
            | "__init__.py"
            | "main.go"
    )
}

// ---------------------------------------------------------------------------
// Tool: index_codebase
// ---------------------------------------------------------------------------

/// Scan the workspace at `path` and produce a structured index report.
///
/// The report includes file tree (depth-limited), language breakdown,
/// package manifest info, import graph, and entrypoints.
#[tauri::command]
pub fn tool_index_codebase(path: String) -> Result<String, String> {
    let workspace = super::resolve_workspace_path(&path)?;
    // For the output, represent paths relative to the workspace
    let root = workspace.to_string_lossy().to_string();

    let mut report = IndexReport {
        workspace_root: root.clone(),
        file_count: 0,
        total_bytes: 0,
        languages: HashMap::new(),
        packages: Vec::new(),
        tree: DirectoryNode {
            name: String::new(),
            is_dir: true,
            size: None,
            children: Vec::new(),
        },
        entrypoints: Vec::new(),
        imports: HashMap::new(),
    };

    scan_directory(&workspace, &workspace, 0, &mut report);

    // Sort languages by count descending for readability
    let mut langs: Vec<_> = report.languages.drain().collect();
    langs.sort_by_key(|(_lang, count)| std::cmp::Reverse(*count));
    report.languages = langs.into_iter().collect();

    // Sort entrypoints
    report.entrypoints.sort();

    // Trim import map: show only top-level import sources, keep reasonable size
    trim_import_map(&mut report.imports);

    serde_json::to_string_pretty(&report)
        .map_err(|e| format!("Failed to serialize index report: {e}"))
}

fn scan_directory(
    workspace: &Path,
    dir: &Path,
    depth: usize,
    report: &mut IndexReport,
) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    let mut children: Vec<DirectoryNode> = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);

        if should_skip(&name, is_dir) {
            continue;
        }

        let rel = path
            .strip_prefix(workspace)
            .unwrap_or(&path)
            .to_string_lossy()
            .to_string();

        if is_dir {
            let mut node = DirectoryNode {
                name: name.clone(),
                is_dir: true,
                size: None,
                children: Vec::new(),
            };
            if depth < 3 {
                scan_directory(workspace, &path, depth + 1, report);
                // Re-collect children for tree display
                if let Ok(sub_entries) = fs::read_dir(&path) {
                    let mut sub_children: Vec<DirectoryNode> = Vec::new();
                    for sub in sub_entries.flatten() {
                        let sub_name = sub.file_name().to_string_lossy().to_string();
                        let sub_is_dir =
                            sub.file_type().map(|t| t.is_dir()).unwrap_or(false);
                        if should_skip(&sub_name, sub_is_dir) {
                            continue;
                        }
                        sub_children.push(DirectoryNode {
                            name: sub_name,
                            is_dir: sub_is_dir,
                            size: None,
                            children: Vec::new(),
                        });
                    }
                    sub_children.sort_by(|a, b| {
                        b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name))
                    });
                    // Limit to top 50 entries
                    if sub_children.len() > 50 {
                        let remaining = sub_children.len() - 50;
                        sub_children.truncate(50);
                        sub_children.push(DirectoryNode {
                            name: format!("... and {} more entries", remaining),
                            is_dir: false,
                            size: None,
                            children: Vec::new(),
                        });
                    }
                    node.children = sub_children;
                }
            }
            children.push(node);
        } else {
            // File
            let metadata = fs::metadata(&path).ok();
            let size = metadata.as_ref().map(|m| m.len());
            report.file_count += 1;
            report.total_bytes += size.unwrap_or(0);

            // Language stats
            let lang = language_for(&path);
            *report.languages.entry(lang.to_string()).or_insert(0) += 1;

            // Entrypoint detection
            if is_entrypoint(&name) {
                report.entrypoints.push(rel.clone());
            }

            // Package manifest parsing
            match name.as_str() {
                "package.json" => {
                    if let Some(pkg) = parse_package_json(&path) {
                        report.packages.push(pkg);
                    }
                }
                "Cargo.toml" => {
                    if let Some(pkg) = parse_cargo_toml(&path) {
                        report.packages.push(pkg);
                    }
                }
                "requirements.txt" | "pyproject.toml" => {
                    if let Some(pkg) = parse_python_manifest(&path, &name) {
                        report.packages.push(pkg);
                    }
                }
                _ => {}
            }

            // Import scanning for source files
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                match ext {
                    "ts" | "tsx" | "js" | "jsx" | "mjs" => {
                        if let Ok(content) = fs::read_to_string(&path) {
                            let imports = extract_ts_imports(&content);
                            if !imports.is_empty() {
                                report.imports.insert(rel.clone(), imports);
                            }
                        }
                    }
                    "rs" => {
                        if let Ok(content) = fs::read_to_string(&path) {
                            let imports = extract_rs_imports(&content);
                            if !imports.is_empty() {
                                report.imports.insert(rel.clone(), imports);
                            }
                        }
                    }
                    "py" => {
                        if let Ok(content) = fs::read_to_string(&path) {
                            let imports = extract_py_imports(&content);
                            if !imports.is_empty() {
                                report.imports.insert(rel.clone(), imports);
                            }
                        }
                    }
                    "go" => {
                        if let Ok(content) = fs::read_to_string(&path) {
                            let imports = extract_go_imports(&content);
                            if !imports.is_empty() {
                                report.imports.insert(rel.clone(), imports);
                            }
                        }
                    }
                    "java" | "kt" => {
                        if let Ok(content) = fs::read_to_string(&path) {
                            let imports = extract_java_imports(&content);
                            if !imports.is_empty() {
                                report.imports.insert(rel.clone(), imports);
                            }
                        }
                    }
                    _ => {}
                }
            }

            children.push(DirectoryNode {
                name,
                is_dir: false,
                size,
                children: Vec::new(),
            });
        }
    }

    children.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name)));
    if depth == 0 {
        report.tree = DirectoryNode {
            name: dir
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
            is_dir: true,
            size: None,
            children,
        };
    }
}

// ---------------------------------------------------------------------------
// Package manifest parsers
// ---------------------------------------------------------------------------

fn parse_package_json(path: &Path) -> Option<PackageInfo> {
    let content = fs::read_to_string(path).ok()?;
    let val: serde_json::Value = serde_json::from_str(&content).ok()?;

    let name = val["name"].as_str().unwrap_or("(unnamed)").to_string();
    let version = val["version"].as_str().map(|v| v.to_string());
    let _dep_keys = ["dependencies", "devDependencies", "peerDependencies"];
    let mut deps: Vec<String> = Vec::new();
    let mut dev_deps: Vec<String> = Vec::new();
    let mut scripts: HashMap<String, String> = HashMap::new();

    if let Some(obj) = val["dependencies"].as_object() {
        for k in obj.keys() {
            deps.push(k.clone());
        }
    }
    if let Some(obj) = val["devDependencies"].as_object() {
        for k in obj.keys() {
            dev_deps.push(k.clone());
        }
    }
    if let Some(obj) = val["scripts"].as_object() {
        for (k, v) in obj {
            scripts.insert(k.clone(), v.as_str().unwrap_or("").to_string());
        }
    }

    Some(PackageInfo {
        kind: "npm".to_string(),
        name,
        version,
        dependencies: deps,
        dev_dependencies: dev_deps,
        scripts,
    })
}

fn parse_cargo_toml(path: &Path) -> Option<PackageInfo> {
    let content = fs::read_to_string(path).ok()?;
    // Lightweight TOML parsing — extract package name + deps without a full TOML lib
    let name = content
        .lines()
        .find(|l| l.trim().starts_with("name"))
        .and_then(|l| l.split('=').nth(1))
        .map(|v| v.trim().trim_matches('"').to_string())
        .unwrap_or_else(|| "(unnamed)".to_string());

    let version = content
        .lines()
        .find(|l| l.trim().starts_with("version"))
        .and_then(|l| l.split('=').nth(1))
        .map(|v| v.trim().trim_matches('"').to_string());

    let mut deps: Vec<String> = Vec::new();
    let mut in_deps = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "[dependencies]" {
            in_deps = true;
            continue;
        }
        if trimmed.starts_with('[') {
            in_deps = false;
            continue;
        }
        if in_deps && !trimmed.is_empty() && !trimmed.starts_with('#') {
            if let Some(name) = trimmed.split('=').next() {
                let dep = name.trim().trim_matches('"').to_string();
                if !dep.is_empty() {
                    deps.push(dep);
                }
            }
        }
    }

    Some(PackageInfo {
        kind: "cargo".to_string(),
        name,
        version,
        dependencies: deps,
        dev_dependencies: Vec::new(),
        scripts: HashMap::new(),
    })
}

fn parse_python_manifest(path: &Path, name: &str) -> Option<PackageInfo> {
    let content = fs::read_to_string(path).ok()?;
    let pkg_name = path
        .parent()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "(unnamed)".to_string());

    let deps: Vec<String> = if name == "requirements.txt" {
        content
            .lines()
            .filter(|l| !l.trim().is_empty() && !l.trim().starts_with('#') && !l.trim().starts_with('-'))
            .map(|l| l.split("==").next().unwrap_or(l).trim().to_string())
            .collect()
    } else {
        // pyproject.toml — lightweight extract from [project] dependencies
        let mut deps = Vec::new();
        let mut in_deps = false;
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("dependencies") {
                in_deps = true;
                continue;
            }
            if in_deps && trimmed.starts_with('[') {
                in_deps = false;
                continue;
            }
            if in_deps && trimmed.starts_with('"') {
                if let Some(d) = trimmed.trim_matches(',').trim_matches('"').split(">=").next() {
                    deps.push(d.to_string());
                }
            }
        }
        deps
    };

    Some(PackageInfo {
        kind: "python".to_string(),
        name: pkg_name,
        version: None,
        dependencies: deps,
        dev_dependencies: Vec::new(),
        scripts: HashMap::new(),
    })
}

// ---------------------------------------------------------------------------
// Import extractors
// ---------------------------------------------------------------------------

/// Extract imported modules from TypeScript/JavaScript source.
fn extract_ts_imports(content: &str) -> Vec<String> {
    let mut imports: Vec<String> = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        // import { X } from './foo'
        // import X from 'bar'
        // const X = require('bar')
        if trimmed.starts_with("import ") {
            if let Some(from_pos) = trimmed.find("from ") {
                let after = &trimmed[from_pos + 5..];
                let module = after
                    .trim_end_matches(';')
                    .trim_matches(|c| c == '\'' || c == '"');
                if !module.is_empty() {
                    imports.push(module.to_string());
                }
            }
        } else if trimmed.contains("require(") {
            if let Some(start) = trimmed.find("require(") {
                let after = &trimmed[start + 8..];
                if let Some(end) = after.find(')') {
                    let module = after[..end].trim_matches(|c| c == '\'' || c == '"');
                    if !module.is_empty() {
                        imports.push(module.to_string());
                    }
                }
            }
        }
    }
    // Deduplicate
    let mut seen: HashSet<String> = HashSet::new();
    imports.retain(|i| seen.insert(i.clone()));
    imports
}

/// Extract `use` and `mod` declarations from Rust source.
fn extract_rs_imports(content: &str) -> Vec<String> {
    let mut imports: Vec<String> = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("use ") {
            let path = trimmed
                .strip_prefix("use ")
                .unwrap()
                .trim_end_matches(';')
                .to_string();
            if !path.is_empty() {
                imports.push(path);
            }
        } else if trimmed.starts_with("mod ") && !trimmed.contains('{') {
            let module = trimmed
                .strip_prefix("mod ")
                .unwrap()
                .trim_end_matches(';')
                .to_string();
            if !module.is_empty() {
                imports.push(format!("mod {module}"));
            }
        } else if trimmed.starts_with("pub mod ") && !trimmed.contains('{') {
            let module = trimmed
                .strip_prefix("pub mod ")
                .unwrap()
                .trim_end_matches(';')
                .to_string();
            if !module.is_empty() {
                imports.push(format!("mod {module}"));
            }
        }
    }
    let mut seen: HashSet<String> = HashSet::new();
    imports.retain(|i| seen.insert(i.clone()));
    imports
}

/// Extract `import` / `from X import` from Python source.
fn extract_py_imports(content: &str) -> Vec<String> {
    let mut imports: Vec<String> = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("import ") {
            let module = trimmed
                .strip_prefix("import ")
                .unwrap()
                .split(" as ")
                .next()
                .unwrap()
                .trim()
                .to_string();
            imports.push(module);
        } else if trimmed.starts_with("from ") {
            // from X import Y
            let rest = trimmed.strip_prefix("from ").unwrap();
            if let Some(module) = rest.split(" import ").next() {
                imports.push(module.trim().to_string());
            }
        }
    }
    let mut seen: HashSet<String> = HashSet::new();
    imports.retain(|i| seen.insert(i.clone()));
    imports
}

/// Extract imported packages from Go source.
fn extract_go_imports(content: &str) -> Vec<String> {
    let mut imports: Vec<String> = Vec::new();
    let mut in_group = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("import (") {
            in_group = true;
            continue;
        }
        if in_group {
            if trimmed.starts_with(')') {
                in_group = false;
                continue;
            }
            let pkg = trimmed.trim_matches('"').trim().to_string();
            if !pkg.is_empty() {
                imports.push(pkg);
            }
        } else if trimmed.starts_with("import ") {
            let rest = trimmed.strip_prefix("import ").unwrap().trim();
            if let Some(pkg) = rest.strip_prefix('"').and_then(|s| s.split('"').next()) {
                imports.push(pkg.to_string());
            }
        }
    }
    let mut seen: HashSet<String> = HashSet::new();
    imports.retain(|i| seen.insert(i.clone()));
    imports
}

/// Extract imported classes from Java/Kotlin source.
fn extract_java_imports(content: &str) -> Vec<String> {
    let mut imports: Vec<String> = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("import ") {
            let module = trimmed
                .strip_prefix("import ")
                .unwrap()
                .trim_end_matches(';')
                .trim();
            if !module.is_empty() {
                imports.push(module.to_string());
            }
        }
    }
    let mut seen: HashSet<String> = HashSet::new();
    imports.retain(|i| seen.insert(i.clone()));
    imports
}

/// Limit import map to keep JSON output manageable — show only external references
/// and local files, limit number per file.
fn trim_import_map(imports: &mut HashMap<String, Vec<String>>) {
    for (_, sources) in imports.iter_mut() {
        sources.sort();
        sources.dedup();
        if sources.len() > 30 {
            sources.truncate(30);
        }
    }
    // Limit total import map entries to 200 files
    if imports.len() > 200 {
        let keys: Vec<String> = imports.keys().take(200).cloned().collect();
        imports.retain(|k, _| keys.contains(k));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn setup_temp_dir() -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("meyatu_index_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_language_detection() {
        assert_eq!(language_for(Path::new("foo.ts")), "TypeScript");
        assert_eq!(language_for(Path::new("bar.rs")), "Rust");
        assert_eq!(language_for(Path::new("baz.py")), "Python");
        assert_eq!(language_for(Path::new("noext")), "Other");
    }

    #[test]
    fn test_should_skip_dirs() {
        assert!(should_skip("node_modules", true));
        assert!(should_skip("target", true));
        assert!(should_skip(".git", true));
        assert!(!should_skip(".meyatu", true));
        assert!(!should_skip("src", true));
    }

    #[test]
    fn test_extract_ts_imports() {
        let content = r#"
import { foo } from './bar';
import baz from 'qux';
const x = require('y');
import type { T } from './types';
"#;
        let imports = extract_ts_imports(content);
        assert!(imports.contains(&"./bar".to_string()));
        assert!(imports.contains(&"qux".to_string()));
        assert!(imports.contains(&"y".to_string()));
        assert!(imports.contains(&"./types".to_string()));
    }

    #[test]
    fn test_extract_rs_imports() {
        let content = r#"
use std::collections::HashMap;
use crate::tools::git;
mod commands;
pub mod memory;
"#;
        let imports = extract_rs_imports(content);
        assert!(imports.contains(&"std::collections::HashMap".to_string()));
        assert!(imports.contains(&"crate::tools::git".to_string()));
        assert!(imports.contains(&"mod commands".to_string()));
        assert!(imports.contains(&"mod memory".to_string()));
    }

    #[test]
    fn test_extract_go_imports() {
        let content = r#"
import "fmt"
import "net/http"
import (
    "strings"
    "os"
)
"#;
        let imports = extract_go_imports(content);
        assert!(imports.contains(&"fmt".to_string()));
        assert!(imports.contains(&"net/http".to_string()));
        assert!(imports.contains(&"strings".to_string()));
        assert!(imports.contains(&"os".to_string()));
    }

    #[test]
    fn test_extract_java_imports() {
        let content = r#"
import java.util.List;
import java.util.ArrayList;
import org.springframework.boot.SpringApplication;
import static org.junit.Assert.*;
"#;
        let imports = extract_java_imports(content);
        assert!(imports.contains(&"java.util.List".to_string()));
        assert!(imports.contains(&"java.util.ArrayList".to_string()));
        assert!(imports.contains(&"org.springframework.boot.SpringApplication".to_string()));
    }

    #[test]
    fn test_extract_py_imports() {
        let content = r#"
import os
import json as j
from pathlib import Path
from typing import List, Dict
"#;
        let imports = extract_py_imports(content);
        assert!(imports.contains(&"os".to_string()));
        assert!(imports.contains(&"json".to_string()));
        assert!(imports.contains(&"pathlib".to_string()));
        assert!(imports.contains(&"typing".to_string()));
    }

    #[test]
    fn test_index_on_temp_dir() {
        let dir = setup_temp_dir();
        fs::write(dir.join("main.rs"), "fn main() {}").unwrap();
        fs::create_dir(dir.join("src")).unwrap();
        fs::write(dir.join("src/lib.rs"), "pub fn hello() {}").unwrap();

        let result = tool_index_codebase(dir.to_string_lossy().to_string()).unwrap();
        let report: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(report["file_count"].as_u64().unwrap() >= 2);
        assert_eq!(report["languages"]["Rust"].as_u64().unwrap(), 2);
        assert!(report["entrypoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e.as_str().unwrap().contains("main.rs")));
    }
}
