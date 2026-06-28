use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::tools::resolve_workspace_path;

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocIndex {
    pub total_files: usize,
    pub total_sections: usize,
    pub files: Vec<DocFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocFile {
    pub path: String,
    pub title: String,
    pub sections: Vec<DocSection>,
    pub size_bytes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocSection {
    pub heading: String,
    pub level: usize,
    pub content: String,
    pub line_start: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocSearchResult {
    pub query: String,
    pub total_matches: usize,
    pub results: Vec<DocMatch>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocMatch {
    pub file: String,
    pub section: String,
    pub line: usize,
    pub snippet: String,
    pub score: f64,
}

// ---------------------------------------------------------------------------
// File discovery
// ---------------------------------------------------------------------------

const DOC_EXTS: &[&str] = &[".md", ".mdx", ".txt", ".rst", ".adoc"];

const SKIP_DIRS: &[&str] = &[
    "node_modules", "target", "dist", "build", ".next", "__pycache__",
    "venv", ".venv", ".git", ".cache",
];

fn collect_doc_files(root: &Path) -> Vec<PathBuf> {
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
                    if DOC_EXTS.contains(&ext_str.as_str()) {
                        files.push(path);
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Markdown parsing
// ---------------------------------------------------------------------------

fn parse_markdown(content: &str) -> Vec<DocSection> {
    let mut sections = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    let mut current_heading = String::new();
    let mut current_level = 0;
    let mut current_content = Vec::new();
    let mut line_start = 1;

    for (i, line) in lines.iter().enumerate() {
        if let Some(level) = heading_level(line) {
            // Save previous section
            if !current_heading.is_empty() || !current_content.is_empty() {
                sections.push(DocSection {
                    heading: current_heading.clone(),
                    level: current_level,
                    content: current_content.join("\n").trim().to_string(),
                    line_start,
                });
            }
            current_heading = line.trim_start_matches('#').trim().to_string();
            current_level = level;
            current_content.clear();
            line_start = i + 1;
        } else {
            current_content.push(line.to_string());
        }
    }

    // Save last section
    if !current_heading.is_empty() || !current_content.is_empty() {
        sections.push(DocSection {
            heading: current_heading,
            level: current_level,
            content: current_content.join("\n").trim().to_string(),
            line_start,
        });
    }

    sections
}

fn heading_level(line: &str) -> Option<usize> {
    let trimmed = line.trim();
    if trimmed.starts_with('#') {
        let level = trimmed.chars().take_while(|&c| c == '#').count();
        if level <= 6 && trimmed.len() > level && trimmed.as_bytes()[level] == b' ' {
            Some(level)
        } else {
            None
        }
    } else {
        None
    }
}

fn extract_title(content: &str, path: &Path) -> String {
    // Try to find first heading
    for line in content.lines().take(20) {
        if let Some(level) = heading_level(line) {
            if level <= 2 {
                return line.trim_start_matches('#').trim().to_string();
            }
        }
    }
    // Fallback to filename
    path.file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "Untitled".to_string())
}

// ---------------------------------------------------------------------------
// Search scoring
// ---------------------------------------------------------------------------

fn score_match(query: &str, text: &str) -> f64 {
    let query_lower = query.to_lowercase();
    let text_lower = text.to_lowercase();

    if text_lower == query_lower {
        return 100.0;
    }

    if text_lower.contains(&query_lower) {
        return 80.0;
    }

    // Partial word match
    let query_words: Vec<&str> = query_lower.split_whitespace().collect();
    let text_words: Vec<&str> = text_lower.split_whitespace().collect();

    let mut matched = 0;
    for qw in &query_words {
        for tw in &text_words {
            if tw.contains(qw) {
                matched += 1;
                break;
            }
        }
    }

    if matched > 0 {
        return 50.0 * (matched as f64 / query_words.len() as f64);
    }

    0.0
}

// ---------------------------------------------------------------------------
// Index command
// ---------------------------------------------------------------------------

/// Scan and index all markdown/documentation files in the workspace.
///
/// Returns a structured index with file paths, titles, sections, and content.
#[tauri::command]
pub fn tool_doc_index(path: String) -> Result<DocIndex, String> {
    let _ws = resolve_workspace_path(&path)?;
    let workspace = std::env::current_dir()
        .map_err(|e| format!("Failed to get workspace: {e}"))?;

    let files = collect_doc_files(&workspace);
    let mut doc_files = Vec::new();
    let mut total_sections = 0;

    for file_path in &files {
        let content = match fs::read_to_string(file_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let rel_path = file_path
            .strip_prefix(&workspace)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| file_path.to_string_lossy().to_string());

        let title = extract_title(&content, file_path);
        let sections = parse_markdown(&content);
        total_sections += sections.len();

        doc_files.push(DocFile {
            path: rel_path,
            title,
            sections,
            size_bytes: content.len(),
        });
    }

    Ok(DocIndex {
        total_files: doc_files.len(),
        total_sections,
        files: doc_files,
    })
}

// ---------------------------------------------------------------------------
// Search command
// ---------------------------------------------------------------------------

/// Search through indexed documentation files.
///
/// Returns matching sections with relevance scores.
#[tauri::command]
pub fn tool_doc_search(
    path: String,
    query: String,
    limit: Option<usize>,
) -> Result<DocSearchResult, String> {
    let _ws = resolve_workspace_path(&path)?;
    let workspace = std::env::current_dir()
        .map_err(|e| format!("Failed to get workspace: {e}"))?;

    let files = collect_doc_files(&workspace);
    let max_results = limit.unwrap_or(20);
    let mut matches = Vec::new();

    for file_path in &files {
        let content = match fs::read_to_string(file_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let rel_path = file_path
            .strip_prefix(&workspace)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| file_path.to_string_lossy().to_string());

        let sections = parse_markdown(&content);

        for section in &sections {
            let score = score_match(&query, &section.heading) + 
                       score_match(&query, &section.content) * 0.5;

            if score > 0.0 {
                // Extract snippet around the match
                let snippet = extract_snippet(&section.content, &query, 200);

                matches.push(DocMatch {
                    file: rel_path.clone(),
                    section: section.heading.clone(),
                    line: section.line_start,
                    snippet,
                    score,
                });
            }
        }
    }

    // Sort by score descending
    matches.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    matches.truncate(max_results);

    Ok(DocSearchResult {
        query,
        total_matches: matches.len(),
        results: matches,
    })
}

fn extract_snippet(text: &str, query: &str, max_len: usize) -> String {
    let query_lower = query.to_lowercase();
    let text_lower = text.to_lowercase();

    if let Some(pos) = text_lower.find(&query_lower) {
        let start = pos.saturating_sub(max_len / 2);
        let end = (pos + query.len() + max_len / 2).min(text.len());
        let snippet = &text[start..end];
        
        if start > 0 {
            format!("...{}", snippet)
        } else if end < text.len() {
            format!("{}...", snippet)
        } else {
            snippet.to_string()
        }
    } else {
        text.chars().take(max_len).collect()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heading_level() {
        assert_eq!(heading_level("# Title"), Some(1));
        assert_eq!(heading_level("## Section"), Some(2));
        assert_eq!(heading_level("### Subsection"), Some(3));
        assert_eq!(heading_level("Not a heading"), None);
        assert_eq!(heading_level("#NoSpace"), None);
    }

    #[test]
    fn test_parse_markdown() {
        let md = r#"# Main Title

Some intro text.

## Section 1

Content of section 1.

## Section 2

More content here.
"#;
        let sections = parse_markdown(md);
        assert_eq!(sections.len(), 3); // Title + 2 sections
        assert_eq!(sections[0].heading, "Main Title");
        assert_eq!(sections[0].level, 1);
        assert_eq!(sections[1].heading, "Section 1");
        assert_eq!(sections[1].level, 2);
    }

    #[test]
    fn test_score_match() {
        assert!(score_match("hello", "hello world") > 50.0);
        assert!(score_match("hello", "world hello") > 50.0);
        assert_eq!(score_match("xyz", "hello world"), 0.0);
    }
}
