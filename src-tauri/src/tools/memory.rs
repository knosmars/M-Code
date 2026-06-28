use std::fs;

/// Read project memory from `.meyatu/memory.md`. Returns empty string if file doesn't exist.
#[tauri::command]
pub fn tool_memory_read(path: String) -> Result<String, String> {
    let workspace = super::resolve_workspace_path(&path)?;
    let memory_dir = workspace.join(".meyatu");
    let memory_file = memory_dir.join("memory.md");

    if !memory_file.exists() {
        return Ok(String::new());
    }

    fs::read_to_string(&memory_file)
        .map_err(|e| format!("Failed to read memory: {e}"))
}

/// Write to `.meyatu/memory.md` (creates the file + directory if missing).
///
/// `mode`:
/// - `"append"` (default): adds a new timestamped section — quick capture.
/// - `"replace"`: overwrites the whole file with `content` — lets the model
///   curate its own memory (merge duplicates, drop the obsolete, regroup) so it
///   doesn't grow into noise over many sessions.
#[tauri::command]
pub fn tool_memory_write(
    path: String,
    content: String,
    mode: Option<String>,
) -> Result<(), String> {
    if content.trim().is_empty() {
        return Err("Memory content must not be empty".into());
    }

    let workspace = super::resolve_workspace_path(&path)?;
    let memory_dir = workspace.join(".meyatu");
    fs::create_dir_all(&memory_dir).map_err(|e| format!("Failed to create .meyatu dir: {e}"))?;

    let memory_file = memory_dir.join("memory.md");
    let body = content.trim();

    let new_content = if mode.as_deref() == Some("replace") {
        // The model curated the entire memory — overwrite. Keep a top heading.
        if body.starts_with("# ") {
            format!("{body}\n")
        } else {
            format!("# Project Memory\n\n{body}\n")
        }
    } else {
        let existing = if memory_file.exists() {
            fs::read_to_string(&memory_file).unwrap_or_default()
        } else {
            String::from("# Project Memory\n\n")
        };
        format!("{existing}\n## {}\n\n{body}\n", chrono_now())
    };

    fs::write(&memory_file, new_content).map_err(|e| format!("Failed to write memory: {e}"))
}

/// Search project memory for relevant sections using TF-IDF-style keyword matching.
///
/// Splits `.meyatu/memory.md` into `## ` sections, scores each against `query`,
/// and returns the top-N most relevant as JSON.
#[tauri::command]
pub fn tool_memory_search(
    path: String,
    query: String,
    limit: Option<usize>,
) -> Result<String, String> {
    let workspace = super::resolve_workspace_path(&path)?;
    let memory_file = workspace.join(".meyatu").join("memory.md");

    let content = if memory_file.exists() {
        fs::read_to_string(&memory_file).map_err(|e| format!("Failed to read memory: {e}"))?
    } else {
        String::new()
    };

    if content.trim().is_empty() {
        return Ok(serde_json::json!({
            "query": query,
            "results": [],
            "total_sections": 0,
            "returned": 0
        }).to_string());
    }

    let sections = split_into_sections(&content);
    let total_sections = sections.len();

    if query.trim().is_empty() {
        return Ok(serde_json::json!({
            "query": query,
            "results": [],
            "total_sections": total_sections,
            "returned": 0
        }).to_string());
    }

    let mut scored: Vec<(String, String, f64)> = sections
        .into_iter()
        .map(|(heading, body)| {
            let full_section = format!("{heading}\n{body}");
            let score = compute_relevance(&query, &full_section);
            (heading, body, score)
        })
        .filter(|(_, _, score)| *score > 0.0)
        .collect();

    scored.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));

    let max_results = limit.unwrap_or(5);
    let returned = scored.len().min(max_results);
    let top = &scored[..returned];

    let results: Vec<serde_json::Value> = top
        .iter()
        .map(|(heading, body, score)| {
            serde_json::json!({
                "heading": heading,
                "content": body.trim(),
                "score": (score * 100.0).round() / 100.0
            })
        })
        .collect();

    Ok(serde_json::json!({
        "query": query,
        "results": results,
        "total_sections": total_sections,
        "returned": returned
    }).to_string())
}

/// Split memory content into `(heading, body)` sections separated by `## ` headings.
fn split_into_sections(content: &str) -> Vec<(String, String)> {
    let mut sections = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    let mut current_heading = String::new();
    let mut current_body = String::new();

    for line in &lines {
        if line.starts_with("## ") {
            if !current_heading.is_empty() {
                sections.push((
                    current_heading.clone(),
                    current_body.clone(),
                ));
            }
            current_heading = line.to_string();
            current_body.clear();
        } else {
            current_body.push_str(line);
            current_body.push('\n');
        }
    }

    if !current_heading.is_empty() {
        sections.push((current_heading, current_body));
    }

    sections
}

/// Compute a relevance score between `query` and `document`.
///
/// Uses term frequency with bonuses for exact matches, partial matches,
/// and title matches.
fn compute_relevance(query: &str, document: &str) -> f64 {
    let query_lower = query.to_lowercase();
    let query_terms: Vec<&str> = query_lower
        .split_whitespace()
        .filter(|w| w.len() > 2)
        .collect();

    let doc_lower = document.to_lowercase();
    let doc_terms: Vec<&str> = doc_lower
        .split_whitespace()
        .filter(|w| w.len() > 2)
        .collect();

    if query_terms.is_empty() || doc_terms.is_empty() {
        return 0.0;
    }

    let mut score = 0.0;
    for qt in &query_terms {
        let tf = doc_terms.iter().filter(|t| **t == *qt).count() as f64
            / doc_terms.len().max(1) as f64;

        // Exact match bonus
        if doc_lower.contains(qt) {
            score += tf * 2.0;
        }

        // Partial match (substring)
        for dt in &doc_terms {
            if dt.contains(qt) || qt.contains(dt) {
                score += tf * 0.5;
            }
        }
    }

    // Title match boost
    if let Some(first_line) = document.lines().next() {
        if let Some(stripped) = first_line.strip_prefix("## ") {
            let title = stripped.to_lowercase();
            for qt in &query_terms {
                if title.contains(qt) {
                    score *= 1.5;
                }
            }
        }
    }

    score
}

fn chrono_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // Convert UNIX seconds to UTC date parts — no chrono dependency
    const SECS_PER_DAY: u64 = 86400;
    let days_since_epoch = secs / SECS_PER_DAY;

    // Civil date algorithm (Howard Hinnant)
    let z = days_since_epoch + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };

    let time = secs % SECS_PER_DAY;
    let hours = time / 3600;
    let minutes = (time % 3600) / 60;
    format!("{year:04}-{m:02}-{d:02} {hours:02}:{minutes:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Create an isolated temp workspace so tests that write `.meyatu/memory.md`
    /// don't race against each other on a shared `.` directory.
    fn temp_workspace() -> String {
        let dir = std::env::temp_dir().join(format!(
            "meyatu_mem_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.to_string_lossy().into_owned()
    }

    #[test]
    fn memory_write_empty_content_rejected() {
        let result = tool_memory_write(".".into(), "".into(), None);
        assert!(result.is_err());
    }

    #[test]
    fn memory_read_returns_ok_on_new_workspace() {
        // Should return empty string if no .meyatu/memory.md exists
        let result = tool_memory_read(".".into());
        assert!(result.is_ok());
    }

    #[test]
    fn memory_roundtrip() {
        let ws = temp_workspace();
        let _ = tool_memory_write(ws.clone(), "Always use strict TypeScript".into(), None);
        let result = tool_memory_read(ws);
        assert!(result.is_ok());
        assert!(result.unwrap().contains("Always use strict TypeScript"));
    }

    #[test]
    fn memory_replace_overwrites_whole_file() {
        // append something, then replace — the appended text must be gone.
        let ws = temp_workspace();
        let _ = tool_memory_write(ws.clone(), "OLD_APPENDED_NOTE".into(), None);
        let _ = tool_memory_write(
            ws.clone(),
            "# Project Memory\n\n## Conventions\n- curated single source".into(),
            Some("replace".into()),
        );
        let out = tool_memory_read(ws).unwrap();
        assert!(out.contains("curated single source"));
        assert!(!out.contains("OLD_APPENDED_NOTE"));
    }

    // --- memory_search tests ---

    #[test]
    fn memory_search_returns_relevant_sections() {
        let content = "# Project Memory\n\n## Conventions\n- Always use strict TypeScript\n- Enable noImplicitAny\n\n## Build Rules\n- Run tsc --noEmit before commit\n\n## Styling\n- Use Tailwind CSS for all components\n";
        let sections = split_into_sections(content);
        assert_eq!(sections.len(), 3);

        let score = compute_relevance("TypeScript strict", &format!("{}\n{}", sections[0].0, sections[0].1));
        assert!(score > 0.0, "Expected positive score for matching section");
    }

    #[test]
    fn memory_search_title_match_boost() {
        let content = "# Project Memory\n\n## TypeScript Conventions\n- Always use strict mode\n\n## Build Rules\n- Run tsc before commit\n";
        let sections = split_into_sections(content);

        let title_score = compute_relevance("TypeScript", &format!("{}\n{}", sections[0].0, sections[0].1));
        let body_score = compute_relevance("TypeScript", &format!("{}\n{}", sections[1].0, sections[1].1));

        assert!(
            title_score > body_score,
            "Title match should score higher than body-only match"
        );
    }

    #[test]
    fn memory_search_empty_query_returns_empty() {
        let content = "# Project Memory\n\n## Conventions\n- Always use strict TypeScript\n";
        let sections = split_into_sections(content);
        assert_eq!(sections.len(), 1);

        let score = compute_relevance("", &format!("{}\n{}", sections[0].0, sections[0].1));
        assert_eq!(score, 0.0, "Empty query should return zero score");
    }

    #[test]
    fn memory_search_limit_respected() {
        let content = "# Project Memory\n\n## Section One\n- content one\n\n## Section Two\n- content two\n\n## Section Three\n- content three\n\n## Section Four\n- content four\n\n## Section Five\n- content five\n\n## Section Six\n- content six\n";
        let sections = split_into_sections(content);
        assert_eq!(sections.len(), 6);

        let query = "content";
        let mut scored: Vec<(String, String, f64)> = sections
            .into_iter()
            .map(|(h, b)| {
                let full = format!("{h}\n{b}");
                let s = compute_relevance(query, &full);
                (h, b, s)
            })
            .filter(|(_, _, s)| *s > 0.0)
            .collect();
        scored.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));

        let limit = 3;
        let returned = scored.len().min(limit);
        assert_eq!(returned, 3, "Limit should cap the number of returned results");
    }

    #[test]
    fn memory_search_no_match_returns_empty() {
        let content = "# Project Memory\n\n## Conventions\n- Always use strict TypeScript\n\n## Build Rules\n- Run tsc before commit\n";
        let sections = split_into_sections(content);

        let score = compute_relevance("python django", &format!("{}\n{}", sections[0].0, sections[0].1));
        assert_eq!(score, 0.0, "No match should return zero score");
    }
}
