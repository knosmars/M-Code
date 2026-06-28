use regex::Regex;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::tools::resolve_workspace_path;

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerfReport {
    pub file: String,
    pub issues: Vec<PerfIssue>,
    pub score: f64, // 0-100, higher is better
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerfIssue {
    pub kind: String, // n_plus_one, large_loop, memory_leak, blocking_io, etc.
    pub severity: String, // high, medium, low
    pub line: usize,
    pub message: String,
    pub suggestion: String,
    pub code_example: Option<String>,
}

// ---------------------------------------------------------------------------
// Pattern detection
// ---------------------------------------------------------------------------

fn detect_perf_issues(content: &str, file_path: &str) -> Vec<PerfIssue> {
    let mut issues = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    let ext = Path::new(file_path)
        .extension()
        .map(|e| e.to_string_lossy().to_string())
        .unwrap_or_default();

    // N+1 query pattern (SQL in loops)
    static N_PLUS_ONE_RE: OnceLock<Regex> = OnceLock::new();
    let n_plus_one_re = N_PLUS_ONE_RE.get_or_init(|| Regex::new(r"(?:for|while|loop|forEach|map|filter|reduce)\s*\(").unwrap());
    static QUERY_RE: OnceLock<Regex> = OnceLock::new();
    let query_re = QUERY_RE.get_or_init(|| Regex::new(r"(?:SELECT|INSERT|UPDATE|DELETE|query|execute|find|findOne|findAll)\s*\(").unwrap());
    static AWAIT_RE: OnceLock<Regex> = OnceLock::new();
    let await_re = AWAIT_RE.get_or_init(|| Regex::new(r"await\s+").unwrap());

    // Large array operations
    static LARGE_ARRAY_RE: OnceLock<Regex> = OnceLock::new();
    let large_array_re = LARGE_ARRAY_RE.get_or_init(|| Regex::new(r"\.sort\(\)|\.reverse\(\)|\.splice\(|\.push\(.*\.push\(").unwrap());

    // Blocking I/O in async context
    static SYNC_IO_RE: OnceLock<Regex> = OnceLock::new();
    let sync_io_re = SYNC_IO_RE.get_or_init(|| Regex::new(r"fs::read|fs::write|std::fs::|read_to_string|write_all").unwrap());

    // String concatenation in loops
    static STRING_CONCAT_RE: OnceLock<Regex> = OnceLock::new();
    let string_concat_re = STRING_CONCAT_RE.get_or_init(|| Regex::new(r#"(\w+)\s*\+=\s*["']|["'].*?\+.*?(\w+)\s*\+"#).unwrap());

    // Regex compilation in hot paths
    static REGEX_RE: OnceLock<Regex> = OnceLock::new();
    let regex_re = REGEX_RE.get_or_init(|| Regex::new(r"Regex::new\(|new RegExp\(").unwrap());

    // Unbounded recursion
    static FN_DEF_RE: OnceLock<Regex> = OnceLock::new();
    let fn_def_re = FN_DEF_RE.get_or_init(|| Regex::new(r"(?:fn|function)\s+(\w+)\s*\(").unwrap());

    // Memory leak patterns (Rust)
    static LEAK_RE: OnceLock<Regex> = OnceLock::new();
    let leak_re = LEAK_RE.get_or_init(|| Regex::new(r"Box::leak|std::mem::forget|ManuallyDrop").unwrap());

    // Unnecessary cloning
    static CLONE_RE: OnceLock<Regex> = OnceLock::new();
    let clone_re = CLONE_RE.get_or_init(|| Regex::new(r"\.clone\(\)").unwrap());

    for (i, line) in lines.iter().enumerate() {
        if n_plus_one_re.is_match(line) {
            // Check if there's a query inside the loop (look at next few lines)
            let loop_start = i;
            let mut brace_depth = 0;
            let mut found_query = false;

            for (j, l) in lines.iter().enumerate().take((i + 20).min(lines.len())).skip(i) {
                for c in l.chars() {
                    match c {
                        '{' => brace_depth += 1,
                        '}' => {
                            brace_depth -= 1;
                            if brace_depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                }

                if brace_depth > 0 && query_re.is_match(l) && await_re.is_match(l) {
                    found_query = true;
                    break;
                }

                if brace_depth == 0 && j > loop_start {
                    break;
                }
            }

            if found_query {
                issues.push(PerfIssue {
                    kind: "n_plus_one_query".to_string(),
                    severity: "high".to_string(),
                    line: i + 1,
                    message: "Potential N+1 query: database call inside a loop".to_string(),
                    suggestion: "Batch database queries outside the loop or use a single query with IN clause.".to_string(),
                    code_example: Some("// Instead of:\nfor (const id of ids) {\n  const user = await db.find(id);\n}\n\n// Use:\nconst users = await db.findAll({ where: { id: ids } });".to_string()),
                });
            }
        }

        // Large array operations
        if large_array_re.is_match(line) {
            issues.push(PerfIssue {
                kind: "large_array_operation".to_string(),
                severity: "medium".to_string(),
                line: i + 1,
                message: "Array mutation operation that may be slow on large arrays".to_string(),
                suggestion: "Consider using more efficient data structures or algorithms.".to_string(),
                code_example: None,
            });
        }

        // Blocking I/O in async context
        let async_context = lines[..i].iter().any(|l| l.contains("async"));
        if sync_io_re.is_match(line) && async_context {
            issues.push(PerfIssue {
                kind: "blocking_io_in_async".to_string(),
                severity: "high".to_string(),
                line: i + 1,
                message: "Synchronous I/O in async context blocks the event loop".to_string(),
                suggestion: "Use async file operations (tokio::fs, async-fs) instead.".to_string(),
                code_example: Some("// Instead of:\nlet content = fs::read_to_string(path)?;\n\n// Use:\nlet content = tokio::fs::read_to_string(path).await?;".to_string()),
            });
        }

        // String concatenation in loops
        if string_concat_re.is_match(line) && n_plus_one_re.is_match(line) {
            issues.push(PerfIssue {
                kind: "string_concat_in_loop".to_string(),
                severity: "medium".to_string(),
                line: i + 1,
                message: "String concatenation in a loop creates many temporary strings".to_string(),
                suggestion: "Use a StringBuilder or collect into a Vec and join.".to_string(),
                code_example: Some("// Instead of:\nlet mut result = String::new();\nfor item in items {\n    result += &item;\n}\n\n// Use:\nlet result: String = items.collect();".to_string()),
            });
        }

        // Regex compilation in hot paths
        if regex_re.is_match(line) {
            // Check if inside a loop
            for j in (0..i).rev() {
                if n_plus_one_re.is_match(lines[j]) {
                    issues.push(PerfIssue {
                        kind: "regex_compilation_in_loop".to_string(),
                        severity: "medium".to_string(),
                        line: i + 1,
                        message: "Regex compilation inside a loop is expensive".to_string(),
                        suggestion: "Compile the regex once outside the loop and reuse it.".to_string(),
                        code_example: Some("// Instead of:\nfor item in items {\n    let re = Regex::new(pattern)?;\n    // ...\n}\n\n// Use:\nlet re = Regex::new(pattern)?;\nfor item in items {\n    // use re\n}".to_string()),
                    });
                    break;
                }
            }
        }

        // Unbounded recursion (regex crate has no backreferences, so capture the
        // function name and look for a call to it in the following lines).
        if let Some(caps) = fn_def_re.captures(line) {
            let fn_name = &caps[1];
            let call_re = Regex::new(&format!(r"\b{}\s*\(", regex::escape(fn_name))).unwrap();
            let calls_itself = lines[i + 1..].iter().any(|l| call_re.is_match(l));
            if calls_itself {
                // Check for base case
                let has_base_case = lines.iter().any(|l| l.contains("if") && (l.contains("return") || l.contains("0") || l.contains("1")));
                if !has_base_case {
                    issues.push(PerfIssue {
                        kind: "unbounded_recursion".to_string(),
                        severity: "high".to_string(),
                        line: i + 1,
                        message: "Recursive function without visible base case".to_string(),
                        suggestion: "Add a base case to prevent infinite recursion.".to_string(),
                        code_example: None,
                    });
                }
            }
        }

        // Memory leak patterns (Rust)
        if ext == "rs"
            && leak_re.is_match(line) {
                issues.push(PerfIssue {
                    kind: "potential_memory_leak".to_string(),
                    severity: "high".to_string(),
                    line: i + 1,
                    message: "Code that may cause memory leaks".to_string(),
                    suggestion: "Review if this leak is intentional. Consider using RAII patterns.".to_string(),
                    code_example: None,
                });
            }

        // Unnecessary cloning
        if ext == "rs"
            && clone_re.is_match(line) {
                // Check if it's in a hot path
                let in_loop = lines[..i].iter().any(|l| n_plus_one_re.is_match(l));
                if in_loop {
                    issues.push(PerfIssue {
                        kind: "unnecessary_clone".to_string(),
                        severity: "low".to_string(),
                        line: i + 1,
                        message: "Unnecessary clone in a loop".to_string(),
                        suggestion: "Use references instead of cloning if possible.".to_string(),
                        code_example: None,
                    });
                }
            }
    }

    issues
}

// ---------------------------------------------------------------------------
// Score calculation
// ---------------------------------------------------------------------------

fn calculate_score(issues: &[PerfIssue]) -> f64 {
    let mut score: f64 = 100.0;
    for issue in issues {
        match issue.severity.as_str() {
            "high" => score -= 15.0,
            "medium" => score -= 8.0,
            "low" => score -= 3.0,
            _ => {}
        }
    }
    score.max(0.0)
}

// ---------------------------------------------------------------------------
// Main command
// ---------------------------------------------------------------------------

/// Analyze code for performance issues and provide optimization suggestions.
///
/// Detects common performance anti-patterns like N+1 queries, blocking I/O,
/// unnecessary cloning, and provides specific suggestions for improvement.
#[tauri::command]
pub fn tool_perf_analyze(
    path: String,
    file_path: Option<String>,
) -> Result<Vec<PerfReport>, String> {
    let _ws = resolve_workspace_path(&path)?;
    let workspace = std::env::current_dir()
        .map_err(|e| format!("Failed to get workspace: {e}"))?;

    let mut reports = Vec::new();

    if let Some(ref target_file) = file_path {
        // Analyze a specific file
        let full_path = workspace.join(target_file);
        let content = fs::read_to_string(&full_path)
            .map_err(|e| format!("Failed to read file: {e}"))?;

        let issues = detect_perf_issues(&content, target_file);
        let score = calculate_score(&issues);
        let summary = format!(
            "Found {} performance issues (score: {:.0}/100)",
            issues.len(),
            score
        );

        reports.push(PerfReport {
            file: target_file.clone(),
            issues,
            score,
            summary,
        });
    } else {
        // Analyze all source files
        let source_files = collect_source_files(&workspace);
        for file_path in &source_files {
            let content = match fs::read_to_string(file_path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let rel_path = file_path
                .strip_prefix(&workspace)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| file_path.to_string_lossy().to_string());

            let issues = detect_perf_issues(&content, &rel_path);
            if !issues.is_empty() {
                let score = calculate_score(&issues);
                let summary = format!(
                    "Found {} performance issues (score: {:.0}/100)",
                    issues.len(),
                    score
                );

                reports.push(PerfReport {
                    file: rel_path,
                    issues,
                    score,
                    summary,
                });
            }
        }
    }

    Ok(reports)
}

// ---------------------------------------------------------------------------
// File collection (same as other tools)
// ---------------------------------------------------------------------------

const SKIP_DIRS: &[&str] = &[
    "node_modules", "target", "dist", "build", ".next", "__pycache__",
    "venv", ".venv", ".git", ".cache", "coverage",
];

const SOURCE_EXTS: &[&str] = &[
    ".ts", ".tsx", ".js", ".jsx", ".rs", ".py", ".go", ".java", ".kt",
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
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_n_plus_one() {
        let code = r#"
async function getUsers(ids: number[]) {
    for (const id of ids) {
        const user = await db.find(id);
        console.log(user);
    }
}
"#;
        let issues = detect_perf_issues(code, "test.ts");
        assert!(issues.iter().any(|i| i.kind == "n_plus_one_query"));
    }

    #[test]
    fn test_detect_blocking_io() {
        let code = r#"
async function readFile() {
    const content = fs::read_to_string("file.txt");
    return content;
}
"#;
        let issues = detect_perf_issues(code, "test.rs");
        assert!(issues.iter().any(|i| i.kind == "blocking_io_in_async"));
    }

    #[test]
    fn test_score_calculation() {
        let issues = vec![
            PerfIssue {
                kind: "n_plus_one_query".to_string(),
                severity: "high".to_string(),
                line: 1,
                message: "test".to_string(),
                suggestion: "test".to_string(),
                code_example: None,
            },
        ];
        let score = calculate_score(&issues);
        assert_eq!(score, 85.0); // 100 - 15
    }
}
