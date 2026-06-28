use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::tools::resolve_workspace_path;

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewFeedback {
    pub id: String,
    pub file: String,
    pub line: Option<usize>,
    pub comment: String,
    pub category: String, // style, bug, performance, architecture, naming, etc.
    pub severity: String, // critical, major, minor, suggestion
    pub timestamp: String,
    pub resolved: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewStore {
    pub feedbacks: Vec<ReviewFeedback>,
    pub stats: ReviewStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewStats {
    pub total: usize,
    pub unresolved: usize,
    pub by_category: std::collections::HashMap<String, usize>,
    pub by_severity: std::collections::HashMap<String, usize>,
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

fn get_reviews_path(workspace: &Path) -> PathBuf {
    workspace.join(".meyatu").join("reviews.json")
}

fn load_reviews(workspace: &Path) -> Vec<ReviewFeedback> {
    let path = get_reviews_path(workspace);
    if path.exists() {
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(reviews) = serde_json::from_str(&content) {
                return reviews;
            }
        }
    }
    Vec::new()
}

fn save_reviews(workspace: &Path, reviews: &[ReviewFeedback]) -> Result<(), String> {
    let path = get_reviews_path(workspace);
    let dir = path.parent().ok_or("Invalid reviews path")?;
    fs::create_dir_all(dir).map_err(|e| format!("Failed to create directory: {e}"))?;
    let content = serde_json::to_string_pretty(reviews)
        .map_err(|e| format!("Failed to serialize reviews: {e}"))?;
    fs::write(path, content).map_err(|e| format!("Failed to write reviews: {e}"))
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// Store a code review feedback item.
#[tauri::command]
pub fn tool_review_add(
    path: String,
    file: String,
    line: Option<usize>,
    comment: String,
    category: String,
    severity: String,
) -> Result<ReviewFeedback, String> {
    let _ws = resolve_workspace_path(&path)?;
    let workspace = std::env::current_dir()
        .map_err(|e| format!("Failed to get workspace: {e}"))?;

    let feedback = ReviewFeedback {
        id: uuid::Uuid::new_v4().to_string(),
        file,
        line,
        comment,
        category,
        severity,
        timestamp: chrono::Utc::now().to_rfc3339(),
        resolved: false,
    };

    let mut reviews = load_reviews(&workspace);
    reviews.push(feedback.clone());
    save_reviews(&workspace, &reviews)?;

    Ok(feedback)
}

/// List all review feedback items, optionally filtered by file or category.
#[tauri::command]
pub fn tool_review_list(
    path: String,
    file: Option<String>,
    category: Option<String>,
    unresolved_only: Option<bool>,
) -> Result<ReviewStore, String> {
    let _ws = resolve_workspace_path(&path)?;
    let workspace = std::env::current_dir()
        .map_err(|e| format!("Failed to get workspace: {e}"))?;

    let reviews = load_reviews(&workspace);
    let mut filtered: Vec<ReviewFeedback> = reviews
        .into_iter()
        .filter(|r| {
            if let Some(ref f) = file {
                if &r.file != f {
                    return false;
                }
            }
            if let Some(ref c) = category {
                if &r.category != c {
                    return false;
                }
            }
            if unresolved_only.unwrap_or(false) && r.resolved {
                return false;
            }
            true
        })
        .collect();

    // Sort by timestamp descending
    filtered.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

    let stats = compute_stats(&filtered);

    Ok(ReviewStore {
        feedbacks: filtered,
        stats,
    })
}

/// Mark a review feedback item as resolved.
#[tauri::command]
pub fn tool_review_resolve(
    path: String,
    review_id: String,
) -> Result<ReviewFeedback, String> {
    let _ws = resolve_workspace_path(&path)?;
    let workspace = std::env::current_dir()
        .map_err(|e| format!("Failed to get workspace: {e}"))?;

    let mut reviews = load_reviews(&workspace);
    let feedback = reviews.iter_mut().find(|r| r.id == review_id)
        .ok_or_else(|| format!("Review not found: {}", review_id))?;

    feedback.resolved = true;
    let result = feedback.clone();
    save_reviews(&workspace, &reviews)?;

    Ok(result)
}

/// Delete a review feedback item.
#[tauri::command]
pub fn tool_review_delete(
    path: String,
    review_id: String,
) -> Result<bool, String> {
    let _ws = resolve_workspace_path(&path)?;
    let workspace = std::env::current_dir()
        .map_err(|e| format!("Failed to get workspace: {e}"))?;

    let mut reviews = load_reviews(&workspace);
    let initial_len = reviews.len();
    reviews.retain(|r| r.id != review_id);

    if reviews.len() < initial_len {
        save_reviews(&workspace, &reviews)?;
        Ok(true)
    } else {
        Err(format!("Review not found: {}", review_id))
    }
}

/// Get review statistics and patterns.
#[tauri::command]
pub fn tool_review_stats(path: String) -> Result<ReviewStats, String> {
    let _ws = resolve_workspace_path(&path)?;
    let workspace = std::env::current_dir()
        .map_err(|e| format!("Failed to get workspace: {e}"))?;

    let reviews = load_reviews(&workspace);
    Ok(compute_stats(&reviews))
}

fn compute_stats(reviews: &[ReviewFeedback]) -> ReviewStats {
    let mut by_category = std::collections::HashMap::new();
    let mut by_severity = std::collections::HashMap::new();

    for review in reviews {
        *by_category.entry(review.category.clone()).or_insert(0) += 1;
        *by_severity.entry(review.severity.clone()).or_insert(0) += 1;
    }

    ReviewStats {
        total: reviews.len(),
        unresolved: reviews.iter().filter(|r| !r.resolved).count(),
        by_category,
        by_severity,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_review_feedback_creation() {
        let feedback = ReviewFeedback {
            id: "test-1".to_string(),
            file: "src/main.rs".to_string(),
            line: Some(42),
            comment: "This function is too long".to_string(),
            category: "style".to_string(),
            severity: "minor".to_string(),
            timestamp: "2024-01-01T00:00:00Z".to_string(),
            resolved: false,
        };

        assert_eq!(feedback.file, "src/main.rs");
        assert!(!feedback.resolved);
    }

    #[test]
    fn test_compute_stats() {
        let reviews = vec![
            ReviewFeedback {
                id: "1".to_string(),
                file: "a.rs".to_string(),
                line: None,
                comment: "test".to_string(),
                category: "style".to_string(),
                severity: "minor".to_string(),
                timestamp: "2024-01-01T00:00:00Z".to_string(),
                resolved: false,
            },
            ReviewFeedback {
                id: "2".to_string(),
                file: "b.rs".to_string(),
                line: None,
                comment: "test".to_string(),
                category: "bug".to_string(),
                severity: "critical".to_string(),
                timestamp: "2024-01-02T00:00:00Z".to_string(),
                resolved: true,
            },
        ];

        let stats = compute_stats(&reviews);
        assert_eq!(stats.total, 2);
        assert_eq!(stats.unresolved, 1);
        assert_eq!(stats.by_category.get("style"), Some(&1));
        assert_eq!(stats.by_category.get("bug"), Some(&1));
    }
}
