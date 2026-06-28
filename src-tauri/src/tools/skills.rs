use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillDef {
    pub name: String,
    pub description: String,
    pub prompt: String,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub category: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillSummary {
    pub name: String,
    pub description: String,
    pub tools: Vec<String>,
    pub category: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillsOutput {
    pub skills: Vec<SkillSummary>,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillLoadOutput {
    pub name: String,
    pub description: String,
    pub prompt: String,
    pub tools: Vec<String>,
    pub category: String,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn scan_skills_dir(skills_dir: &Path) -> Vec<SkillDef> {
    let mut skills = Vec::new();
    if !skills_dir.is_dir() {
        return skills;
    }
    if let Ok(entries) = fs::read_dir(skills_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "yml" || e == "yaml") {
                if let Ok(raw) = fs::read_to_string(&path) {
                    if let Ok(skill) = serde_yaml::from_str::<SkillDef>(&raw) {
                        skills.push(skill);
                    }
                }
            }
        }
    }
    skills
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

/// List all skills from .meyatu/skills/ directory (YAML files).
#[tauri::command]
pub fn tool_skills_list(path: String) -> Result<SkillsOutput, String> {
    let root = super::resolve_workspace_path(&path)?;
    let skills_dir = root.join(".meyatu").join("skills");

    let defs = scan_skills_dir(&skills_dir);
    let summaries: Vec<SkillSummary> = defs
        .iter()
        .map(|s| SkillSummary {
            name: s.name.clone(),
            description: s.description.clone(),
            tools: s.tools.clone(),
            category: s.category.clone(),
        })
        .collect();

    Ok(SkillsOutput {
        count: summaries.len(),
        skills: summaries,
    })
}

/// Load a specific skill by name from .meyatu/skills/ directory.
#[tauri::command]
pub fn tool_skills_load(path: String, name: String) -> Result<SkillLoadOutput, String> {
    let root = super::resolve_workspace_path(&path)?;
    let skills_dir = root.join(".meyatu").join("skills");

    let defs = scan_skills_dir(&skills_dir);
    let skill = defs
        .into_iter()
        .find(|s| s.name == name)
        .ok_or_else(|| format!("Skill not found: {name}"))?;

    Ok(SkillLoadOutput {
        name: skill.name,
        description: skill.description,
        prompt: skill.prompt,
        tools: skill.tools,
        category: skill.category,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn skill_deserialization() {
        let yaml = r#"
name: rust-expert
description: "Expert Rust programming"
prompt: "You are a Rust expert."
tools:
  - read_file
  - grep
category: backend
"#;
        let skill: SkillDef = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(skill.name, "rust-expert");
        assert_eq!(skill.tools.len(), 2);
        assert_eq!(skill.category, "backend");
    }

    #[test]
    fn skill_deserialization_minimal() {
        let yaml = r#"
name: minimal
description: "Minimal skill"
prompt: "Be helpful."
"#;
        let skill: SkillDef = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(skill.tools.len(), 0);
        assert_eq!(skill.category, "");
    }

    #[test]
    fn scan_empty_dir() {
        let dir = std::env::temp_dir().join("meyatu_test_skills_empty");
        let _ = fs::create_dir_all(&dir);
        let skills = scan_skills_dir(&dir);
        assert!(skills.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_dir_with_yaml() {
        let dir = std::env::temp_dir().join("meyatu_test_skills_scan");
        let _ = fs::create_dir_all(&dir);
        let mut file = fs::File::create(dir.join("test.yml")).unwrap();
        writeln!(file, "name: test\n").unwrap();
        writeln!(file, "description: Test\n").unwrap();
        writeln!(file, "prompt: Test prompt\n").unwrap();
        drop(file);

        let skills = scan_skills_dir(&dir);
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "test");

        let _ = fs::remove_dir_all(&dir);
    }
}
