use serde::Deserialize;
use serde::Serialize;
use std::path::PathBuf;
use ts_rs::TS;

#[derive(Serialize, Deserialize, Debug, Clone, TS)]
pub struct Agent {
    pub name: String,
    pub path: PathBuf,
    pub description: String,
    pub tools: Option<Vec<String>>,
    pub prompt: String,
}

/// YAML frontmatter structure for agent files
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AgentFrontmatter {
    pub name: String,
    pub description: String,
    pub tools: Option<Vec<String>>,
}
