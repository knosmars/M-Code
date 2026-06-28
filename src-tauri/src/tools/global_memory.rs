use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use crate::tools::resolve_workspace_path;

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalMemory {
    pub projects: Vec<ProjectMemory>,
    pub shared_patterns: Vec<SharedPattern>,
    pub stats: GlobalStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMemory {
    pub name: String,
    pub path: String,
    pub language: String,
    pub framework: Option<String>,
    pub patterns: Vec<String>,
    pub last_active: String,
    pub notes: Vec<MemoryNote>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryNote {
    pub id: String,
    pub content: String,
    pub category: String, // best_practice, gotcha, tip, architecture
    pub tags: Vec<String>,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedPattern {
    pub pattern: String,
    pub description: String,
    pub language: Option<String>,
    pub framework: Option<String>,
    pub examples: Vec<String>,
    pub source_project: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalStats {
    pub total_projects: usize,
    pub total_notes: usize,
    pub total_patterns: usize,
    pub languages: HashMap<String, usize>,
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

fn get_global_memory_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".meyatu").join("global_memory.json")
}

fn load_global_memory() -> GlobalMemory {
    let path = get_global_memory_path();
    if path.exists() {
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(memory) = serde_json::from_str(&content) {
                return memory;
            }
        }
    }
    GlobalMemory {
        projects: Vec::new(),
        shared_patterns: Vec::new(),
        stats: GlobalStats {
            total_projects: 0,
            total_notes: 0,
            total_patterns: 0,
            languages: HashMap::new(),
        },
    }
}

fn save_global_memory(memory: &GlobalMemory) -> Result<(), String> {
    let path = get_global_memory_path();
    let dir = path.parent().ok_or("Invalid memory path")?;
    fs::create_dir_all(dir).map_err(|e| format!("Failed to create directory: {e}"))?;
    let content = serde_json::to_string_pretty(memory)
        .map_err(|e| format!("Failed to serialize memory: {e}"))?;
    fs::write(path, content).map_err(|e| format!("Failed to write memory: {e}"))
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// Register a project in the global memory system.
#[tauri::command]
pub fn tool_global_register_project(
    path: String,
    name: String,
    language: String,
    framework: Option<String>,
) -> Result<ProjectMemory, String> {
    let _ws = resolve_workspace_path(&path)?;
    let workspace = std::env::current_dir()
        .map_err(|e| format!("Failed to get workspace: {e}"))?;

    let mut memory = load_global_memory();

    // Check if project already exists
    let ws_str = workspace.to_string_lossy().to_string();
    let project_idx = memory.projects.iter().position(|p| p.path == ws_str);
    if let Some(idx) = project_idx {
        memory.projects[idx].last_active = chrono::Utc::now().to_rfc3339();
        memory.projects[idx].language = language;
        memory.projects[idx].framework = framework;
        save_global_memory(&memory)?;
        return Ok(memory.projects[idx].clone());
    }

    // Create new project
    let project = ProjectMemory {
        name,
        path: ws_str,
        language: language.clone(),
        framework,
        patterns: Vec::new(),
        last_active: chrono::Utc::now().to_rfc3339(),
        notes: Vec::new(),
    };

    memory.projects.push(project.clone());
    *memory.stats.languages.entry(language).or_insert(0) += 1;
    memory.stats.total_projects = memory.projects.len();

    save_global_memory(&memory)?;
    Ok(project)
}

/// Add a note to a project's memory.
#[tauri::command]
pub fn tool_global_add_note(
    path: String,
    content: String,
    category: String,
    tags: Vec<String>,
) -> Result<MemoryNote, String> {
    let _ws = resolve_workspace_path(&path)?;
    let workspace = std::env::current_dir()
        .map_err(|e| format!("Failed to get workspace: {e}"))?;

    let mut memory = load_global_memory();
    let ws_str = workspace.to_string_lossy().to_string();

    let project = memory.projects.iter_mut().find(|p| p.path == ws_str)
        .ok_or("Project not registered. Call tool_global_register_project first.")?;

    let note = MemoryNote {
        id: uuid::Uuid::new_v4().to_string(),
        content,
        category,
        tags,
        timestamp: chrono::Utc::now().to_rfc3339(),
    };

    project.notes.push(note.clone());
    memory.stats.total_notes = memory.projects.iter().map(|p| p.notes.len()).sum();

    save_global_memory(&memory)?;
    Ok(note)
}

/// Search across all project memories.
#[tauri::command]
pub fn tool_global_search(
    query: String,
    language: Option<String>,
    category: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<MemoryNote>, String> {
    let memory = load_global_memory();
    let max_results = limit.unwrap_or(20);
    let query_lower = query.to_lowercase();

    let mut results: Vec<MemoryNote> = memory.projects
        .iter()
        .filter(|p| {
            if let Some(ref lang) = language {
                &p.language == lang
            } else {
                true
            }
        })
        .flat_map(|p| p.notes.iter())
        .filter(|n| {
            if let Some(ref cat) = category {
                &n.category != cat
            } else {
                true
            }
        })
        .filter(|n| {
            n.content.to_lowercase().contains(&query_lower)
                || n.tags.iter().any(|t| t.to_lowercase().contains(&query_lower))
        })
        .cloned()
        .collect();

    results.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    results.truncate(max_results);

    Ok(results)
}

/// Get global memory statistics.
#[tauri::command]
pub fn tool_global_stats() -> Result<GlobalStats, String> {
    let memory = load_global_memory();
    Ok(memory.stats)
}

/// Add a shared pattern learned across projects.
#[tauri::command]
pub fn tool_global_add_pattern(
    path: String,
    pattern: String,
    description: String,
    language: Option<String>,
    framework: Option<String>,
    examples: Vec<String>,
) -> Result<SharedPattern, String> {
    let _ws = resolve_workspace_path(&path)?;
    let workspace = std::env::current_dir()
        .map_err(|e| format!("Failed to get workspace: {e}"))?;

    let mut memory = load_global_memory();

    let shared = SharedPattern {
        pattern,
        description,
        language,
        framework,
        examples,
        source_project: workspace.to_string_lossy().to_string(),
    };

    memory.shared_patterns.push(shared.clone());
    memory.stats.total_patterns = memory.shared_patterns.len();

    save_global_memory(&memory)?;
    Ok(shared)
}

/// Get all shared patterns, optionally filtered by language/framework.
#[tauri::command]
pub fn tool_global_patterns(
    language: Option<String>,
    framework: Option<String>,
) -> Result<Vec<SharedPattern>, String> {
    let memory = load_global_memory();

    let patterns: Vec<SharedPattern> = memory.shared_patterns
        .into_iter()
        .filter(|p| {
            if let Some(ref lang) = language {
                p.language.as_ref() != Some(lang)
            } else {
                true
            }
        })
        .filter(|p| {
            if let Some(ref fw) = framework {
                p.framework.as_ref() != Some(fw)
            } else {
                true
            }
        })
        .collect();

    Ok(patterns)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_global_memory_creation() {
        let memory = GlobalMemory {
            projects: Vec::new(),
            shared_patterns: Vec::new(),
            stats: GlobalStats {
                total_projects: 0,
                total_notes: 0,
                total_patterns: 0,
                languages: HashMap::new(),
            },
        };

        assert_eq!(memory.projects.len(), 0);
        assert_eq!(memory.shared_patterns.len(), 0);
    }

    #[test]
    fn test_memory_note_creation() {
        let note = MemoryNote {
            id: "test-1".to_string(),
            content: "Always use Result for error handling".to_string(),
            category: "best_practice".to_string(),
            tags: vec!["rust".to_string(), "error-handling".to_string()],
            timestamp: "2024-01-01T00:00:00Z".to_string(),
        };

        assert_eq!(note.category, "best_practice");
        assert_eq!(note.tags.len(), 2);
    }
}
