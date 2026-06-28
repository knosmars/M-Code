use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    pub id: String,
    pub category: String,
    pub severity: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExecutionConfig {
    #[serde(default = "default_permission")]
    pub default_permission: String,
    #[serde(default = "default_max_iterations")]
    pub max_iterations: u32,
    #[serde(default = "default_verify")]
    pub verify_before_completion: bool,
}

fn default_permission() -> String {
    "prompt".into()
}
fn default_max_iterations() -> u32 {
    25
}
fn default_verify() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentsRulesFile {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub rules: Vec<Rule>,
    #[serde(default)]
    pub execution: ExecutionConfig,
}

fn default_version() -> u32 {
    1
}

impl Default for AgentsRulesFile {
    fn default() -> Self {
        Self {
            version: 1,
            rules: vec![],
            execution: ExecutionConfig {
                default_permission: default_permission(),
                max_iterations: default_max_iterations(),
                verify_before_completion: default_verify(),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentsRulesOutput {
    pub rules: Vec<Rule>,
    pub execution: ExecutionConfig,
    pub system_prompt: String,
}

fn generate_system_prompt(rules: &[Rule], exec: &ExecutionConfig) -> String {
    let mut lines: Vec<String> = vec![];
    lines.push("## Project Rules (from .meyatu/agents.yml)".into());
    lines.push(String::new());

    let strict: Vec<&Rule> = rules.iter().filter(|r| r.severity == "strict").collect();
    let suggest: Vec<&Rule> = rules.iter().filter(|r| r.severity == "suggest" || r.severity == "suggestion").collect();
    let policy: Vec<&Rule> = rules.iter().filter(|r| r.severity == "policy").collect();

    if !strict.is_empty() {
        lines.push("**STRICT RULES** — must be followed without exception:".into());
        for (i, r) in strict.iter().enumerate() {
            lines.push(format!("{}. [{}] {}", i + 1, r.category, r.description));
        }
        lines.push(String::new());
    }

    if !suggest.is_empty() {
        lines.push("**SUGGESTED RULES** — follow when practical:".into());
        for (i, r) in suggest.iter().enumerate() {
            lines.push(format!("{}. [{}] {}", i + 1, r.category, r.description));
        }
        lines.push(String::new());
    }

    if !policy.is_empty() {
        lines.push("**POLICY NOTES**:".into());
        for r in &policy {
            lines.push(format!("- [{}] {}", r.category, r.description));
        }
        lines.push(String::new());
    }

    lines.push(format!(
        "Execution config: permission={}, max_iterations={}, verify_before_completion={}",
        exec.default_permission, exec.max_iterations, exec.verify_before_completion
    ));

    lines.join("\n")
}

#[tauri::command]
pub fn tool_agents_rules_read(path: String) -> Result<AgentsRulesOutput, String> {
    let workspace = super::resolve_workspace_path(&path)?;
    let rules_file = workspace.join(".meyatu").join("agents.yml");

    let rules: AgentsRulesFile = if rules_file.exists() {
        let content = fs::read_to_string(&rules_file)
            .map_err(|e| format!("Failed to read .meyatu/agents.yml: {e}"))?;
        serde_yaml::from_str(&content)
            .map_err(|e| format!("Invalid YAML in .meyatu/agents.yml: {e}"))?
    } else {
        AgentsRulesFile::default()
    };

    let system_prompt = generate_system_prompt(&rules.rules, &rules.execution);

    Ok(AgentsRulesOutput {
        rules: rules.rules,
        execution: rules.execution,
        system_prompt,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_rules_returns_defaults() {
        let rules = AgentsRulesFile::default();
        assert_eq!(rules.version, 1);
        assert!(rules.rules.is_empty());
        assert_eq!(rules.execution.default_permission, "prompt");
        assert_eq!(rules.execution.max_iterations, 25);
        assert!(rules.execution.verify_before_completion);
    }

    #[test]
    fn system_prompt_includes_strict() {
        let rules = vec![Rule {
            id: "no-any".into(),
            category: "type_safety".into(),
            severity: "strict".into(),
            description: "Never use `any` type.".into(),
        }];
        let exec = ExecutionConfig {
            default_permission: "prompt".into(),
            max_iterations: 25,
            verify_before_completion: true,
        };
        let prompt = generate_system_prompt(&rules, &exec);
        assert!(prompt.contains("STRICT RULES"));
        assert!(prompt.contains("Never use `any` type."));
    }

    #[test]
    fn system_prompt_includes_suggest() {
        let rules = vec![Rule {
            id: "prefer-func".into(),
            category: "code_style".into(),
            severity: "suggest".into(),
            description: "Prefer functional components.".into(),
        }];
        let exec = ExecutionConfig {
            default_permission: "prompt".into(),
            max_iterations: 25,
            verify_before_completion: true,
        };
        let prompt = generate_system_prompt(&rules, &exec);
        assert!(prompt.contains("SUGGESTED RULES"));
        assert!(prompt.contains("Prefer functional components."));
    }

    #[test]
    fn parse_valid_yaml() {
        let yaml = r#"
version: 1
rules:
  - id: naming
    category: code_style
    severity: strict
    description: "Use camelCase"
execution:
  default_permission: auto_approve
  max_iterations: 10
  verify_before_completion: false
"#;
        let rules: AgentsRulesFile = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(rules.rules.len(), 1);
        assert_eq!(rules.rules[0].id, "naming");
        assert_eq!(rules.execution.default_permission, "auto_approve");
        assert_eq!(rules.execution.max_iterations, 10);
        assert!(!rules.execution.verify_before_completion);
    }

    #[test]
    fn missing_rules_file_returns_empty() {
        let tmp = std::env::temp_dir().join("meyatu_test_missing_rules");
        let _ = std::fs::create_dir_all(&tmp);
        let result = tool_agents_rules_read(tmp.to_string_lossy().to_string());
        let _ = std::fs::remove_dir_all(&tmp);
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.rules.is_empty());
        assert_eq!(output.execution.default_permission, "prompt");
    }
}
