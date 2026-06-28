use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub name: String,
    pub description: String,
    pub system_prompt: String,
    pub tools: Vec<String>,
    pub category: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentsFile {
    pub agents: Vec<AgentConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentsListOutput {
    pub agents: Vec<AgentConfig>,
}

#[tauri::command]
pub fn tool_agents_list(path: String) -> Result<String, String> {
    let ws = super::resolve_workspace_path(&path)?;
    let agents_path = ws.join(".meyatu").join("agents.yml");

    if !agents_path.exists() {
        return Ok(
            serde_json::to_string(&AgentsListOutput { agents: vec![] }).unwrap_or_default(),
        );
    }

    let content = std::fs::read_to_string(&agents_path)
        .map_err(|e| format!("Failed to read agents.yml: {e}"))?;

    let file: AgentsFile =
        serde_yaml::from_str(&content).map_err(|e| format!("Invalid agents.yml: {e}"))?;

    Ok(serde_json::to_string(&AgentsListOutput { agents: file.agents }).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_agent_config() {
        let yaml = r#"
agents:
  - name: explorer
    description: Searches codebase for patterns
    system_prompt: "You are a code explorer."
    tools:
      - read_file
      - grep
      - glob
      - list_dir
    category: explore
  - name: reviewer
    description: Reviews code for quality
    system_prompt: "You are a code reviewer."
    tools:
      - read_file
      - grep
    category: review
"#;
        let file: AgentsFile = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(file.agents.len(), 2);
        assert_eq!(file.agents[0].name, "explorer");
        assert_eq!(file.agents[0].tools.len(), 4);
        assert_eq!(file.agents[1].name, "reviewer");
        assert_eq!(file.agents[1].tools.len(), 2);
    }

    #[test]
    fn deserialize_minimal_agent() {
        let yaml = r#"
agents:
  - name: minimal
    description: Minimal agent
    system_prompt: ""
    tools: []
"#;
        let file: AgentsFile = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(file.agents.len(), 1);
        assert_eq!(file.agents[0].name, "minimal");
        assert!(file.agents[0].tools.is_empty());
    }

    #[test]
    fn missing_file_returns_empty_list() {
        // Use "." which resolves to the cargo workspace directory
        let dir = std::env::current_dir().unwrap();
        let testfile = dir.join(".meyatu").join("nonexistent_agents_test.yml");
        assert!(!testfile.exists(), "test file should not exist");
        let result = tool_agents_list(dir.to_string_lossy().to_string()).unwrap();
        let output: AgentsListOutput = serde_json::from_str(&result).unwrap();
        assert!(output.agents.is_empty());
    }
}
