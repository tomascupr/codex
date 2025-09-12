use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::error::CodexErr;
use crate::error::Result as CodexResult;
use crate::exec_command::EXEC_COMMAND_TOOL_NAME;
use crate::exec_command::WRITE_STDIN_TOOL_NAME;
use crate::openai_tools::OpenAiTool;
use crate::openai_tools::ResponsesApiTool;
use serde::Deserialize;
use tracing::warn;

#[derive(Debug, Clone, PartialEq)]
pub struct SubAgent {
    pub name: String,
    pub description: String,
    pub tools: Option<Vec<String>>, // allowlist
    pub body: String,
}

#[derive(Default, Debug)]
pub struct AgentRegistry {
    agents: BTreeMap<String, SubAgent>,
}

impl AgentRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert_agent(&mut self, agent: SubAgent) {
        self.agents.insert(agent.name.clone(), agent);
    }

    #[allow(dead_code)]
    pub fn get(&self, name: &str) -> Option<&SubAgent> {
        self.agents.get(name)
    }

    #[allow(dead_code)]
    pub fn names(&self) -> Vec<String> {
        self.agents.keys().cloned().collect()
    }

    #[allow(dead_code)]
    pub fn has_agent(&self, name: &str) -> bool {
        self.agents.contains_key(name)
    }
}

#[derive(Deserialize)]
struct FrontMatter {
    #[serde(default)]
    name: Option<String>,
    description: String,
    #[serde(default)]
    tools: Option<Vec<String>>,
}

pub fn parse_agent_markdown(content: &str, agent_name: String) -> CodexResult<SubAgent> {
    // Require frontmatter delimited by lines containing only '---'.
    let mut lines = content.lines();
    let first = lines.next().unwrap_or("").trim();
    if first != "---" {
        return Err(std::io::Error::other("missing YAML frontmatter at start of file").into());
    }

    // Collect frontmatter lines until the closing '---'.
    let mut fm = String::new();
    for line in &mut lines {
        if line.trim() == "---" {
            break;
        }
        fm.push_str(line);
        fm.push('\n');
    }
    // The rest is the body.
    let body: String = lines.collect::<Vec<&str>>().join("\n");

    let fm: FrontMatter = serde_yaml::from_str(&fm)
        .map_err(|e| std::io::Error::other(format!("frontmatter parse error: {e}")))?;
    if let Some(declared) = fm.name.as_deref()
        && declared.trim() != agent_name
    {
        warn!(
            "Frontmatter name '{}' differs from filename '{}'; using filename",
            declared, agent_name
        );
    }
    let body_trimmed = body.trim();
    if body_trimmed.is_empty() {
        return Err(std::io::Error::other("agent markdown must have a non-empty body").into());
    }

    Ok(SubAgent {
        name: agent_name,
        description: fm.description,
        tools: fm.tools,
        body: body_trimmed.to_string(),
    })
}

fn load_agents_from_directory(dir: &Path) -> CodexResult<Vec<SubAgent>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut agents = Vec::new();
    for entry in fs::read_dir(dir).map_err(CodexErr::Io)? {
        let entry = entry.map_err(CodexErr::Io)?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        if name.is_empty() {
            continue;
        }
        let contents = fs::read_to_string(&path).map_err(CodexErr::Io)?;
        let agent = parse_agent_markdown(&contents, name)?;
        agents.push(agent);
    }
    Ok(agents)
}

pub fn discover_and_load_agents(project_root: Option<&Path>) -> CodexResult<AgentRegistry> {
    let mut registry = AgentRegistry::new();

    if let Some(home_dir) = dirs::home_dir() {
        let user_dir = home_dir.join(".codex/agents");
        for agent in load_agents_from_directory(&user_dir)? {
            registry.insert_agent(agent);
        }
    }

    if let Some(root) = project_root {
        let project_dir = root.join(".codex/agents");
        for agent in load_agents_from_directory(&project_dir)? {
            registry.insert_agent(agent);
        }
    }

    Ok(registry)
}

/// Filter available OpenAI tools using an optional allowlist, expanding shell aliases.
pub fn filter_tools_for_agent(tools: &[OpenAiTool], allowed: Option<&[String]>) -> Vec<OpenAiTool> {
    match allowed {
        None => tools.to_vec(),
        Some([]) => Vec::new(),
        Some(allowlist) => {
            use std::collections::HashSet;
            let mut expanded: HashSet<String> = allowlist.iter().cloned().collect();
            let shell_aliases = [
                "shell",
                "local_shell",
                EXEC_COMMAND_TOOL_NAME,
                WRITE_STDIN_TOOL_NAME,
            ];
            if allowlist
                .iter()
                .any(|t| shell_aliases.iter().any(|a| a == t))
            {
                for a in shell_aliases {
                    expanded.insert(a.to_string());
                }
            }

            tools
                .iter()
                .filter(|tool| match tool_name(tool) {
                    Some(name) => expanded.contains(name),
                    None => false,
                })
                .cloned()
                .collect()
        }
    }
}

fn tool_name(tool: &OpenAiTool) -> Option<&str> {
    match tool {
        OpenAiTool::Function(ResponsesApiTool { name, .. }) => Some(name.as_str()),
        OpenAiTool::LocalShell {} => Some("local_shell"),
        OpenAiTool::WebSearch {} => Some("web_search"),
        OpenAiTool::Freeform(f) => Some(f.name.as_str()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exec_command::create_exec_command_tool_for_responses_api;
    use crate::exec_command::create_write_stdin_tool_for_responses_api;
    use pretty_assertions::assert_eq;
    use std::collections::HashMap;
    use tempfile::TempDir;

    #[test]
    fn parses_basic_agent_markdown() {
        let md = "---\ndescription: example\ntools: [shell]\n---\nYou are an agent.";
        let agent = parse_agent_markdown(md, "docs-writer".to_string()).unwrap();
        assert_eq!(agent.name, "docs-writer");
        assert_eq!(agent.description, "example");
        assert_eq!(agent.tools, Some(vec!["shell".to_string()]));
        assert_eq!(agent.body, "You are an agent.");
    }

    #[test]
    fn load_agents_from_dir_reads_md_files() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path();
        fs::create_dir_all(dir).unwrap();
        let a = dir.join("a.md");
        fs::write(&a, "---\ndescription: A\n---\nBody A").unwrap();
        let b = dir.join("b.md");
        fs::write(&b, "---\ndescription: B\n---\nBody B").unwrap();
        let agents = load_agents_from_directory(dir).unwrap();
        assert_eq!(agents.len(), 2);
        let names: HashMap<_, _> = agents
            .iter()
            .map(|a| (a.name.as_str(), &a.description))
            .collect();
        assert!(names.contains_key("a"));
        assert!(names.contains_key("b"));
    }

    #[test]
    fn filter_tools_with_aliases() {
        let tools = vec![
            OpenAiTool::LocalShell {},
            OpenAiTool::Function(create_exec_command_tool_for_responses_api()),
            OpenAiTool::Function(create_write_stdin_tool_for_responses_api()),
        ];
        let filtered = filter_tools_for_agent(&tools, Some(&["shell".to_string()]));
        // Expect all shell-group tools present
        assert_eq!(filtered.len(), 3);
        let names: Vec<&str> = filtered
            .iter()
            .map(|t| super::tool_name(t).unwrap())
            .collect();
        assert!(names.contains(&"local_shell"));
        assert!(names.contains(&EXEC_COMMAND_TOOL_NAME));
        assert!(names.contains(&WRITE_STDIN_TOOL_NAME));
    }

    #[test]
    fn registry_overrides_by_project() {
        let mut reg = AgentRegistry::new();
        reg.insert_agent(SubAgent {
            name: "docs".into(),
            description: "user".into(),
            tools: None,
            body: "user body".into(),
        });
        // project insert should override
        reg.insert_agent(SubAgent {
            name: "docs".into(),
            description: "project".into(),
            tools: None,
            body: "proj body".into(),
        });
        let a = reg.get("docs").unwrap();
        assert_eq!(a.description, "project");
    }
}
