use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum MatchKind {
    Definition,
    Import,
    Reference,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MatchEntry {
    line: usize,
    kind: MatchKind,
    content: String,
    context_before: Vec<String>,
    context_after: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FileResult {
    file: String,
    matches: Vec<MatchEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SearchReport {
    query: String,
    total_matches: usize,
    files_searched: usize,
    results: Vec<FileResult>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Files and directories to skip during scanning.
fn should_skip(name: &str, is_dir: bool) -> bool {
    if name.starts_with('.') {
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

/// Check if a file is a source file we should search.
fn is_source_file(name: &str) -> bool {
    let exts = [
        ".ts", ".tsx", ".js", ".jsx", ".rs", ".py", ".go", ".java", ".c", ".cpp", ".h", ".hpp",
    ];
    exts.iter().any(|ext| name.ends_with(ext))
}

/// Classify a line by its kind.
fn classify_line(line: &str) -> MatchKind {
    let trimmed = line.trim();
    // Definition patterns
    let def_patterns = [
        "function ", "class ", "struct ", "enum ", "interface ", "type ", "trait ", "impl ", "fn ",
        "def ", "const ", "let ", "var ",
    ];
    for pat in &def_patterns {
        if trimmed.starts_with(pat) {
            return MatchKind::Definition;
        }
    }
    // Import patterns
    let import_patterns = ["import ", "use ", "require", "from ", "mod "];
    for pat in &import_patterns {
        if trimmed.starts_with(pat) {
            return MatchKind::Import;
        }
    }
    MatchKind::Reference
}

// ---------------------------------------------------------------------------
// Tool: search_codebase
// ---------------------------------------------------------------------------

/// Search the workspace at `path` for occurrences of `query`.
///
/// Returns a JSON report with classified matches (definition, import,
/// reference), contextual lines, and file metadata.  Enforces limits:
/// max 50 files, max 10 matches per file, max 200 total matches.
#[tauri::command]
pub fn tool_search_codebase(query: String, path: String) -> Result<String, String> {
    let workspace = super::resolve_workspace_path(&path)?;
    let query_lower = query.to_lowercase();
    let is_path_query = query.contains('/') || query.contains('\\') || query.rfind('.').is_some_and(|i| {
        let ext = &query[i..];
        [".ts", ".tsx", ".js", ".jsx", ".rs", ".py", ".go", ".java", ".c", ".cpp", ".h", ".hpp"].contains(&ext)
    });

    let mut report = SearchReport {
        query: query.clone(),
        total_matches: 0,
        files_searched: 0,
        results: Vec::new(),
    };

    search_directory(
        &workspace,
        &workspace,
        &query_lower,
        is_path_query,
        &mut report,
    )?;

    // Sort results: definitions first, then imports, then references
    for file_result in &mut report.results {
        file_result.matches.sort_by(|a, b| {
            let ord_a = match a.kind {
                MatchKind::Definition => 0,
                MatchKind::Import => 1,
                MatchKind::Reference => 2,
            };
            let ord_b = match b.kind {
                MatchKind::Definition => 0,
                MatchKind::Import => 1,
                MatchKind::Reference => 2,
            };
            ord_a.cmp(&ord_b).then(a.line.cmp(&b.line))
        });
    }

    // If query looks like a file path, prioritize files whose path contains it
    if is_path_query {
        let query_path_lower = query.to_lowercase();
        report.results.sort_by(|a, b| {
            let a_contains = a.file.to_lowercase().contains(&query_path_lower);
            let b_contains = b.file.to_lowercase().contains(&query_path_lower);
            b_contains
                .cmp(&a_contains)
                .then(a.file.cmp(&b.file))
        });
    }

    serde_json::to_string_pretty(&report)
        .map_err(|e| format!("Failed to serialize search report: {e}"))
}

fn search_directory(
    workspace: &Path,
    dir: &Path,
    query_lower: &str,
    _is_path_query: bool,
    report: &mut SearchReport,
) -> Result<(), String> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);

        if should_skip(&name, is_dir) {
            continue;
        }

        if is_dir {
            search_directory(workspace, &path, query_lower, _is_path_query, report)?;
            if report.total_matches >= 200 || report.results.len() >= 50 {
                return Ok(());
            }
        } else if is_source_file(&name) {
            if report.results.len() >= 50 {
                return Ok(());
            }

            report.files_searched += 1;

            let content = match fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let rel = path
                .strip_prefix(workspace)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string();

            let lines: Vec<&str> = content.lines().collect();
            let mut file_matches: Vec<MatchEntry> = Vec::new();

            for (idx, line) in lines.iter().enumerate() {
                if line.to_lowercase().contains(query_lower) {
                    let kind = classify_line(line);
                    let line_num = idx + 1;

                    let context_before: Vec<String> = (idx.saturating_sub(2)..idx)
                        .filter_map(|i| lines.get(i))
                        .map(|s| s.to_string())
                        .collect();

                    let context_after: Vec<String> = ((idx + 1)..(idx + 3))
                        .filter_map(|i| lines.get(i))
                        .map(|s| s.to_string())
                        .collect();

                    file_matches.push(MatchEntry {
                        line: line_num,
                        kind,
                        content: line.to_string(),
                        context_before,
                        context_after,
                    });

                    report.total_matches += 1;
                    if file_matches.len() >= 10 || report.total_matches >= 200 {
                        break;
                    }
                }
            }

            if !file_matches.is_empty() {
                report.results.push(FileResult {
                    file: rel,
                    matches: file_matches,
                });
            }

            if report.total_matches >= 200 || report.results.len() >= 50 {
                return Ok(());
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup_temp_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("meyatu_search_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_basic_string_matching() {
        let dir = setup_temp_dir();
        fs::write(
            dir.join("foo.ts"),
            "const x = 1;\nfunction bar() {}\nconst y = 2;\n",
        )
        .unwrap();

        let result =
            tool_search_codebase("bar".to_string(), dir.to_string_lossy().to_string()).unwrap();
        let report: SearchReport = serde_json::from_str(&result).unwrap();
        assert_eq!(report.total_matches, 1);
        assert_eq!(report.results.len(), 1);
        assert_eq!(report.results[0].file, "foo.ts");
        assert_eq!(report.results[0].matches[0].content, "function bar() {}");
    }

    #[test]
    fn test_match_classification() {
        let dir = setup_temp_dir();
        fs::write(
            dir.join("test.rs"),
            "use foo;\nfn foo() {}\ncall_foo();\n",
        )
        .unwrap();

        let result =
            tool_search_codebase("foo".to_string(), dir.to_string_lossy().to_string()).unwrap();
        let report: SearchReport = serde_json::from_str(&result).unwrap();
        assert_eq!(report.results.len(), 1);

        let file = &report.results[0];
        assert_eq!(file.matches.len(), 3);

        let kinds: Vec<_> = file.matches.iter().map(|m| &m.kind).collect();
        assert!(matches!(kinds[0], MatchKind::Definition));
        assert!(matches!(kinds[1], MatchKind::Import));
        assert!(matches!(kinds[2], MatchKind::Reference));
    }

    #[test]
    fn test_result_limits() {
        let dir = setup_temp_dir();

        // 60 files to test the 50-file limit
        for i in 0..60 {
            fs::write(
                dir.join(format!("file{}.ts", i)),
                format!("const match_{} = 1;\n", i),
            )
            .unwrap();
        }

        let result =
            tool_search_codebase("match".to_string(), dir.to_string_lossy().to_string()).unwrap();
        let report: SearchReport = serde_json::from_str(&result).unwrap();
        assert!(report.results.len() <= 50);
        assert!(report.total_matches <= 200);

        // One big file to test the 10-matches-per-file limit
        let mut big_file = String::new();
        for i in 0..20 {
            big_file.push_str(&format!("const match_{} = 1;\n", i));
        }
        fs::write(dir.join("big.ts"), big_file).unwrap();

        let result2 =
            tool_search_codebase("match".to_string(), dir.to_string_lossy().to_string()).unwrap();
        let report2: SearchReport = serde_json::from_str(&result2).unwrap();
        if let Some(big) = report2.results.iter().find(|r| r.file == "big.ts") {
            assert!(big.matches.len() <= 10);
        }
    }

    #[test]
    fn test_case_insensitive_search() {
        let dir = setup_temp_dir();
        fs::write(dir.join("test.ts"), "const Foo = 1;\nconst foo = 2;\nconst FOO = 3;\n").unwrap();

        let result =
            tool_search_codebase("FOO".to_string(), dir.to_string_lossy().to_string()).unwrap();
        let report: SearchReport = serde_json::from_str(&result).unwrap();
        assert_eq!(report.total_matches, 3);
    }

    #[test]
    fn test_path_query_prioritization() {
        let dir = setup_temp_dir();
        fs::write(dir.join("alpha.ts"), "const x = 'beta.ts';\n").unwrap();
        fs::write(dir.join("beta.ts"), "const y = 'beta.ts';\n").unwrap();

        let result =
            tool_search_codebase("beta.ts".to_string(), dir.to_string_lossy().to_string()).unwrap();
        let report: SearchReport = serde_json::from_str(&result).unwrap();
        assert!(!report.results.is_empty());
        // beta.ts should appear first because its path contains the query
        assert_eq!(report.results[0].file, "beta.ts");
    }

    #[test]
    fn test_context_lines() {
        let dir = setup_temp_dir();
        fs::write(
            dir.join("ctx.rs"),
            "line one\nline two\nline three\ntarget line\nline five\nline six\nline seven\n",
        )
        .unwrap();

        let result =
            tool_search_codebase("target".to_string(), dir.to_string_lossy().to_string()).unwrap();
        let report: SearchReport = serde_json::from_str(&result).unwrap();
        assert_eq!(report.results.len(), 1);
        let m = &report.results[0].matches[0];
        assert_eq!(m.content, "target line");
        assert_eq!(m.context_before.len(), 2);
        assert_eq!(m.context_before[0], "line two");
        assert_eq!(m.context_before[1], "line three");
        assert_eq!(m.context_after.len(), 2);
        assert_eq!(m.context_after[0], "line five");
        assert_eq!(m.context_after[1], "line six");
    }
}
