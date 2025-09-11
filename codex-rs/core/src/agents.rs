//! Sub-agents system for the codex CLI tool.
//!
//! This module provides functionality to:
//! - Load and parse agent definitions from markdown files
//! - Discover agents from both project (`.codex/agents/`) and user (`~/.codex/agents/`) directories
//! - Merge agent definitions with project agents taking precedence
//! - Validate agent definitions and tool allowlists

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::client::ModelClient;
use crate::conversation_history::ConversationHistory;
use crate::error::CodexErr;
use crate::openai_tools::{ToolsConfig, get_openai_tools};
use codex_protocol::models::{ContentItem, ResponseItem};

/// A sub-agent definition loaded from a markdown file with YAML frontmatter.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SubAgent {
    /// Name of the agent (from filename)
    pub name: String,

    /// Human-readable description of the agent's purpose
    pub description: String,

    /// Optional allowlist of tools this agent can use.
    /// If None, agent can use all available tools.
    /// If Some(empty_vec), agent can use no tools.
    /// If Some(vec_with_tools), agent can only use those tools.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<String>>,

    /// The system prompt/instructions for this agent (from markdown body)
    pub body: String,
}

/// Result of loading and merging agents from multiple sources
#[derive(Debug, Clone)]
pub struct AgentRegistry {
    /// Map of agent name -> agent definition
    agents: HashMap<String, SubAgent>,
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            agents: HashMap::new(),
        }
    }

    /// Get all available agent names
    pub fn list_agents(&self) -> Vec<String> {
        let mut names: Vec<String> = self.agents.keys().cloned().collect();
        names.sort();
        names
    }

    /// Get a specific agent by name
    pub fn get_agent(&self, name: &str) -> Option<&SubAgent> {
        self.agents.get(name)
    }

    /// Insert or update an agent in the registry
    pub fn insert_agent(&mut self, agent: SubAgent) {
        self.agents.insert(agent.name.clone(), agent);
    }

    /// Check if an agent exists
    pub fn has_agent(&self, name: &str) -> bool {
        self.agents.contains_key(name)
    }

    /// Get the number of registered agents
    pub fn len(&self) -> usize {
        self.agents.len()
    }

    /// Check if the registry is empty
    pub fn is_empty(&self) -> bool {
        self.agents.is_empty()
    }
}

/// Load agents from a directory containing markdown files
pub fn load_agents_from_directory(directory: &Path) -> Result<Vec<SubAgent>, CodexErr> {
    if !directory.exists() || !directory.is_dir() {
        return Ok(Vec::new());
    }

    let mut agents = Vec::new();

    let entries = fs::read_dir(directory).map_err(|e| {
        CodexErr::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "Failed to read agents directory '{}': {}",
                directory.display(),
                e
            ),
        ))
    })?;

    for entry in entries {
        let entry = entry.map_err(|e| {
            CodexErr::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "Failed to read directory entry in '{}': {}",
                    directory.display(),
                    e
                ),
            ))
        })?;

        let path = entry.path();

        // Only process .md files
        if !path.is_file() || path.extension().map(|s| s.to_str()) != Some(Some("md")) {
            continue;
        }

        // Extract agent name from filename (without .md extension)
        let agent_name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| {
                CodexErr::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Invalid filename for agent: {}", path.display()),
                ))
            })?
            .to_string();

        match load_agent_from_file(&path, agent_name) {
            Ok(agent) => agents.push(agent),
            Err(e) => {
                // Log warning but continue processing other agents
                tracing::warn!("Failed to load agent from '{}': {e}", path.display());
                continue;
            }
        }
    }

    Ok(agents)
}

/// Load a single agent from a markdown file
fn load_agent_from_file(file_path: &Path, agent_name: String) -> Result<SubAgent, CodexErr> {
    let content = fs::read_to_string(file_path).map_err(|e| {
        CodexErr::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Failed to read agent file '{}': {}", file_path.display(), e),
        ))
    })?;

    parse_agent_markdown(&content, agent_name)
}

/// Parse an agent definition from markdown with YAML frontmatter
fn parse_agent_markdown(content: &str, agent_name: String) -> Result<SubAgent, CodexErr> {
    let content = content.trim();

    if !content.starts_with("---") {
        return Err(CodexErr::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "Agent '{agent_name}' missing YAML frontmatter (must start with '---')"
            ),
        )));
    }

    // Find the end of frontmatter
    let search_start = 3; // Skip initial "---"
    let frontmatter_end = content[search_start..]
        .find("\n---")
        .or_else(|| content[search_start..].find("\r\n---"))
        .ok_or_else(|| {
            CodexErr::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Agent '{agent_name}' missing frontmatter closing '---'"),
            ))
        })?;

    let frontmatter_content = &content[search_start..search_start + frontmatter_end];
    let body_start = search_start + frontmatter_end + 4; // Skip past "\n---"
    let body = if body_start < content.len() {
        content[body_start..].trim()
    } else {
        ""
    };

    // Parse YAML frontmatter
    #[derive(Deserialize)]
    struct FrontMatter {
        description: String,
        #[serde(default)]
        tools: Option<Vec<String>>,
    }

    let frontmatter: FrontMatter = serde_yaml::from_str(frontmatter_content).map_err(|e| {
        CodexErr::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "Failed to parse YAML frontmatter for agent '{agent_name}': {e}"
            ),
        ))
    })?;

    // Validate required fields
    if frontmatter.description.trim().is_empty() {
        return Err(CodexErr::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Agent '{agent_name}' must have a non-empty description"),
        )));
    }

    if body.is_empty() {
        return Err(CodexErr::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "Agent '{agent_name}' must have a non-empty body (system prompt)"
            ),
        )));
    }

    // Validate tool allowlist if provided
    if let Some(ref tools) = frontmatter.tools {
        for tool in tools {
            if tool.trim().is_empty() {
                return Err(CodexErr::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Agent '{agent_name}' has empty tool name in allowlist"),
                )));
            }
        }
    }

    Ok(SubAgent {
        name: agent_name,
        description: frontmatter.description,
        tools: frontmatter.tools,
        body: body.to_string(),
    })
}

/// Discover and load agents from both project and user directories
/// Project agents take precedence over user agents with the same name
pub fn discover_and_load_agents(project_root: Option<&Path>) -> Result<AgentRegistry, CodexErr> {
    let mut registry = AgentRegistry::new();

    // Load user agents first (lower precedence)
    if let Some(home_dir) = dirs::home_dir() {
        let user_agents_dir = home_dir.join(".codex").join("agents");
        let user_agents = load_agents_from_directory(&user_agents_dir)?;

        tracing::debug!(
            "Loaded {} user agents from {}",
            user_agents.len(),
            user_agents_dir.display()
        );

        for agent in user_agents {
            registry.insert_agent(agent);
        }
    }

    // Load project agents (higher precedence - will override user agents)
    if let Some(project_root) = project_root {
        let project_agents_dir = project_root.join(".codex").join("agents");
        let project_agents = load_agents_from_directory(&project_agents_dir)?;

        tracing::debug!(
            "Loaded {} project agents from {}",
            project_agents.len(),
            project_agents_dir.display()
        );

        for agent in project_agents {
            if registry.has_agent(&agent.name) {
                tracing::debug!("Project agent '{}' overriding user agent", agent.name);
            }
            registry.insert_agent(agent);
        }
    }

    Ok(registry)
}

/// Tool filtering based on agent allowlists
pub(crate) fn filter_tools_for_agent(
    tools: &[crate::openai_tools::OpenAiTool],
    allowed_tools: Option<&[String]>,
) -> Vec<crate::openai_tools::OpenAiTool> {
    match allowed_tools {
        None => tools.to_vec(), // No restrictions - agent can use all tools
        Some(allowlist) => {
            if allowlist.is_empty() {
                // Empty allowlist means no tools allowed
                return Vec::new();
            }

            use std::collections::HashSet;
            // Build an expanded allowlist that treats different shell variants as aliases.
            // Any of these in the user-provided allowlist will enable the whole group:
            // - "shell" (function tool)
            // - "local_shell" (native local shell tool)
            // - "exec_command" / "write_stdin" (streamable shell pair)
            let mut expanded: HashSet<String> = allowlist.iter().cloned().collect();
            let shell_aliases = [
                "shell".to_string(),
                "local_shell".to_string(),
                crate::exec_command::EXEC_COMMAND_TOOL_NAME.to_string(),
                crate::exec_command::WRITE_STDIN_TOOL_NAME.to_string(),
            ];
            if allowlist.iter().any(|t| shell_aliases.contains(t)) {
                for a in shell_aliases.iter() {
                    expanded.insert(a.clone());
                }
            }

            // Filter tools based on expanded allowlist
            tools
                .iter()
                .filter(|tool| {
                    let tool_name = match tool {
                        crate::openai_tools::OpenAiTool::Function(f) => f.name.as_str(),
                        crate::openai_tools::OpenAiTool::LocalShell {} => "local_shell",
                        crate::openai_tools::OpenAiTool::WebSearch {} => "web_search",
                        crate::openai_tools::OpenAiTool::Freeform(f) => f.name.as_str(),
                    };
                    expanded.contains(tool_name)
                })
                .cloned()
                .collect()
        }
    }
}

/// Runner for executing sub-agents in isolated contexts
pub struct NestedAgentRunner {
    registry: AgentRegistry,
}

impl NestedAgentRunner {
    /// Create a new runner with the given agent registry
    pub fn new(registry: AgentRegistry) -> Self {
        Self { registry }
    }

    /// List all available agents
    pub fn list_agents(&self) -> Vec<String> {
        self.registry.list_agents()
    }

    /// Get description and details of a specific agent
    pub fn describe_agent(&self, name: &str) -> Result<SubAgentDescription, CodexErr> {
        let agent = self.registry.get_agent(name).ok_or_else(|| {
            CodexErr::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Sub-agent '{name}' not found"),
            ))
        })?;

        Ok(SubAgentDescription {
            name: agent.name.clone(),
            description: agent.description.clone(),
            tools: agent.tools.clone(),
            body: agent.body.clone(),
        })
    }

    /// Execute a sub-agent with the given task
    /// Creates an isolated conversation context and applies tool filtering enforcement
    pub(crate) async fn run_agent(
        &self,
        name: &str,
        task: &str,
        parent_tools_config: &ToolsConfig,
        _parent_client: &ModelClient,
    ) -> Result<SubAgentResult, CodexErr> {
        let agent = self.registry.get_agent(name).ok_or_else(|| {
            CodexErr::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Sub-agent '{name}' not found"),
            ))
        })?;

        tracing::info!("Starting sub-agent '{name}' with task: {task}");
        tracing::debug!("Creating isolated context for sub-agent '{name}'");

        // Filter tools based on agent's allowlist - this is the key security feature
        let available_tools = get_openai_tools(parent_tools_config, None);
        let filtered_tools = filter_tools_for_agent(&available_tools, agent.tools.as_deref());

        tracing::debug!(
            "Sub-agent '{}' has access to {} of {} available tools",
            name,
            filtered_tools.len(),
            available_tools.len()
        );

        // Create isolated conversation history
        let mut conversation_history = ConversationHistory::new();

        // Construct system prompt: agent instructions + task
        let system_prompt = format!("{}\n\nTask: {}", agent.body, task);

        // Add system message to conversation with sub-agent origin tracking
        // Note: Using "user" role instead of "system" because is_api_message() filters out system messages
        // Note: Not setting origin to avoid "Unknown parameter: 'input[0].origin'" error from OpenAI API
        let system_message = ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputText {
                text: system_prompt,
            }],
            origin: None,
        };
        conversation_history.record_items([&system_message]);

        // Create the prompt with the filtered tools and isolated conversation history
        let prompt = crate::client_common::Prompt {
            input: conversation_history.contents(),
            tools: filtered_tools.clone(),
            base_instructions_override: None,
        };

        tracing::debug!(
            "Starting model conversation for sub-agent '{}' with {} tools",
            name,
            filtered_tools.len()
        );

        // Use the parent client to stream the conversation with filtered tools
        let response_stream = _parent_client.stream(&prompt).await.map_err(|e| {
            CodexErr::Io(std::io::Error::other(
                format!(
                    "Failed to start model conversation for sub-agent '{name}': {e}"
                ),
            ))
        })?;

        // Process the response stream and accumulate results
        let mut output_text = String::new();
        let mut success = true;
        let mut error_message: Option<String> = None;
        let mut tool_calls_made = Vec::new();

        let mut stream = response_stream.rx_event;

        while let Some(event_result) = stream.recv().await {
            match event_result {
                Ok(event) => {
                    match event {
                        crate::client_common::ResponseEvent::OutputItemDone(item) => {
                            match &item {
                                ResponseItem::Message { role, content, .. }
                                    if role == "assistant" =>
                                {
                                    // Collect assistant text output
                                    for content_item in content {
                                        if let ContentItem::OutputText { text } = content_item {
                                            output_text.push_str(text);
                                        }
                                    }
                                    // Record the response item in our isolated conversation history
                                    conversation_history.record_items([&item]);
                                }
                                ResponseItem::FunctionCall { name, call_id, .. } => {
                                    // Tool calls are restricted to filtered_tools - this is enforced by the model client
                                    tool_calls_made.push(format!(
                                        "Function call: {} (id: {})",
                                        name,
                                        call_id.as_str()
                                    ));
                                    conversation_history.record_items([&item]);
                                }
                                ResponseItem::LocalShellCall { action, .. } => {
                                    tool_calls_made.push(format!("Shell call: {action:?}"));
                                    conversation_history.record_items([&item]);
                                }
                                ResponseItem::CustomToolCall { name, .. } => {
                                    tool_calls_made.push(format!("Custom tool call: {name}"));
                                    conversation_history.record_items([&item]);
                                }
                                ResponseItem::FunctionCallOutput {
                                    call_id, output, ..
                                } => {
                                    tool_calls_made.push(format!(
                                        "Tool output for {}: {} chars",
                                        call_id,
                                        output.content.len()
                                    ));
                                    conversation_history.record_items([&item]);
                                }
                                ResponseItem::Reasoning { content, .. } => {
                                    // Record reasoning but don't include in final output
                                    conversation_history.record_items([&item]);
                                    if let Some(reasoning_items) = content {
                                        for reasoning_item in reasoning_items {
                                            match reasoning_item {
                                                codex_protocol::models::ReasoningItemContent::ReasoningText { text } |
                                                codex_protocol::models::ReasoningItemContent::Text { text } => {
                                                    tracing::debug!("Sub-agent '{}' reasoning: {}", name, text.chars().take(100).collect::<String>());
                                                }
                                            }
                                        }
                                    }
                                }
                                _ => {
                                    // Record other items in conversation history
                                    conversation_history.record_items([&item]);
                                }
                            }
                        }
                        crate::client_common::ResponseEvent::Completed { response_id, .. } => {
                            tracing::debug!(
                                "Sub-agent '{}' conversation completed with response_id: {}",
                                name,
                                response_id
                            );
                            break;
                        }
                        crate::client_common::ResponseEvent::OutputTextDelta(_) => {
                            // Real-time text streaming - ignore since we get the full text in OutputItemDone
                        }
                        crate::client_common::ResponseEvent::ReasoningContentDelta(_) => {
                            // Real-time reasoning streaming - ignore since we get the full reasoning in OutputItemDone
                        }
                        _ => {
                            // Handle other events as needed
                        }
                    }
                }
                Err(e) => {
                    success = false;
                    error_message = Some(format!("Stream error in sub-agent '{name}': {e}"));
                    tracing::error!("Sub-agent '{}' failed with stream error: {}", name, e);
                    break;
                }
            }
        }

        // Build the final result
        let tool_summary = if tool_calls_made.is_empty() {
            "No tool calls were made".to_string()
        } else {
            format!("Tool calls made: {}", tool_calls_made.join(", "))
        };

        let final_output = if output_text.is_empty() {
            format!("Sub-agent '{name}' completed. {tool_summary}")
        } else {
            format!("{output_text}\n\n{tool_summary}")
        };

        tracing::info!(
            "Sub-agent '{}' execution completed. Success: {}, Tools used: {}, Output length: {} chars",
            name,
            success,
            tool_calls_made.len(),
            final_output.len()
        );

        Ok(SubAgentResult {
            agent_name: name.to_string(),
            task: task.to_string(),
            success,
            output: final_output,
            error: error_message,
        })
    }
}

/// Description of a sub-agent returned by describe_agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentDescription {
    pub name: String,
    pub description: String,
    pub tools: Option<Vec<String>>,
    pub body: String,
}

/// Result of executing a sub-agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentResult {
    pub agent_name: String,
    pub task: String,
    pub success: bool,
    pub output: String,
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::write;
    use tempfile::TempDir;

    #[test]
    fn test_parse_valid_agent_markdown() {
        let content = r#"---
description: "A helpful coding assistant"
tools: ["shell", "apply_patch"]
---

You are a coding assistant specialized in writing clean, efficient code.
Always follow best practices and write comprehensive tests.
"#;

        let agent = parse_agent_markdown(content, "test-agent".to_string()).unwrap();

        assert_eq!(agent.name, "test-agent");
        assert_eq!(agent.description, "A helpful coding assistant");
        assert_eq!(
            agent.tools,
            Some(vec!["shell".to_string(), "apply_patch".to_string()])
        );
        assert!(agent.body.contains("You are a coding assistant"));
    }

    #[test]
    fn test_parse_agent_without_tools() {
        let content = r#"---
description: "A general purpose assistant"
---

You are a general purpose assistant. Help with any task.
"#;

        let agent = parse_agent_markdown(content, "general".to_string()).unwrap();

        assert_eq!(agent.name, "general");
        assert_eq!(agent.description, "A general purpose assistant");
        assert_eq!(agent.tools, None);
        assert!(agent.body.contains("You are a general purpose assistant"));
    }

    #[test]
    fn test_parse_agent_missing_frontmatter() {
        let content = "Just some markdown without frontmatter";

        let result = parse_agent_markdown(content, "invalid".to_string());
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("missing YAML frontmatter")
        );
    }

    #[test]
    fn test_parse_agent_missing_description() {
        let content = r#"---
tools: ["shell"]
---

Some content here.
"#;

        let result = parse_agent_markdown(content, "invalid".to_string());
        assert!(result.is_err());
    }

    #[test]
    fn test_load_agents_from_directory() {
        let temp_dir = TempDir::new().unwrap();
        let agents_dir = temp_dir.path();

        // Create test agent files
        let agent1_content = r#"---
description: "Agent 1"
---
System prompt for agent 1.
"#;

        let agent2_content = r#"---
description: "Agent 2"
tools: ["shell"]
---
System prompt for agent 2.
"#;

        write(agents_dir.join("agent1.md"), agent1_content).unwrap();
        write(agents_dir.join("agent2.md"), agent2_content).unwrap();
        write(agents_dir.join("not-an-agent.txt"), "ignored").unwrap();

        let agents = load_agents_from_directory(agents_dir).unwrap();

        assert_eq!(agents.len(), 2);

        let agent_names: Vec<String> = agents.iter().map(|a| a.name.clone()).collect();
        assert!(agent_names.contains(&"agent1".to_string()));
        assert!(agent_names.contains(&"agent2".to_string()));
    }

    #[test]
    fn test_agent_registry() {
        let mut registry = AgentRegistry::new();
        assert!(registry.is_empty());

        let agent = SubAgent {
            name: "test".to_string(),
            description: "Test agent".to_string(),
            tools: None,
            body: "Test prompt".to_string(),
        };

        registry.insert_agent(agent);
        assert!(!registry.is_empty());
        assert_eq!(registry.len(), 1);
        assert!(registry.has_agent("test"));
        assert!(!registry.has_agent("nonexistent"));

        let retrieved = registry.get_agent("test").unwrap();
        assert_eq!(retrieved.name, "test");
        assert_eq!(retrieved.description, "Test agent");

        let names = registry.list_agents();
        assert_eq!(names, vec!["test".to_string()]);
    }

    #[test]
    fn test_filter_tools_for_agent() {
        use crate::openai_tools::{
            FreeformTool, FreeformToolFormat, JsonSchema, OpenAiTool, ResponsesApiTool,
        };
        use std::collections::BTreeMap;

        // Create mock tools for testing
        let tools = vec![
            OpenAiTool::Function(ResponsesApiTool {
                name: "shell".to_string(),
                description: "Execute shell commands".to_string(),
                strict: false,
                parameters: JsonSchema::Object {
                    properties: BTreeMap::new(),
                    required: None,
                    additional_properties: None,
                },
            }),
            OpenAiTool::LocalShell {},
            OpenAiTool::WebSearch {},
            OpenAiTool::Freeform(FreeformTool {
                name: "apply_patch".to_string(),
                description: "Apply code patches".to_string(),
                format: FreeformToolFormat {
                    r#type: "freeform".to_string(),
                    syntax: "patch".to_string(),
                    definition: "Apply patch".to_string(),
                },
            }),
        ];

        // Test no restrictions - agent can use all tools
        let filtered = filter_tools_for_agent(&tools, None);
        assert_eq!(filtered.len(), 4);

        // Test empty allowlist - agent can use no tools
        let filtered = filter_tools_for_agent(&tools, Some(&[]));
        assert_eq!(filtered.len(), 0);

        // Test with specific allowlist
        let filtered = filter_tools_for_agent(
            &tools,
            Some(&["shell".to_string(), "local_shell".to_string()]),
        );
        assert_eq!(filtered.len(), 2);

        // Verify the correct tools are included
        let tool_names: Vec<String> = filtered
            .iter()
            .map(|tool| match tool {
                OpenAiTool::Function(f) => f.name.clone(),
                OpenAiTool::LocalShell {} => "local_shell".to_string(),
                OpenAiTool::WebSearch {} => "web_search".to_string(),
                OpenAiTool::Freeform(f) => f.name.clone(),
            })
            .collect();
        assert!(tool_names.contains(&"shell".to_string()));
        assert!(tool_names.contains(&"local_shell".to_string()));
        assert!(!tool_names.contains(&"web_search".to_string()));
        assert!(!tool_names.contains(&"apply_patch".to_string()));

        // Test with non-existent tool in allowlist
        let filtered = filter_tools_for_agent(&tools, Some(&["nonexistent".to_string()]));
        assert_eq!(filtered.len(), 0);
    }

    #[test]
    fn test_nested_agent_runner() {
        let registry = AgentRegistry::new();
        let runner = NestedAgentRunner::new(registry);

        assert_eq!(runner.list_agents().len(), 0);

        // Test nonexistent agent
        assert!(runner.describe_agent("nonexistent").is_err());
    }

    #[test]
    fn test_parse_agent_invalid_yaml() {
        let content = r#"---
description: "Valid description"
tools: [invalid yaml structure
---

Some content here.
"#;

        let result = parse_agent_markdown(content, "invalid".to_string());
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Failed to parse YAML")
        );
    }

    #[test]
    fn test_parse_agent_empty_description() {
        let content = r#"---
description: ""
---

Some content here.
"#;

        let result = parse_agent_markdown(content, "invalid".to_string());
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("must have a non-empty description")
        );
    }

    #[test]
    fn test_parse_agent_empty_body() {
        let content = r#"---
description: "Valid description"
---


"#;

        let result = parse_agent_markdown(content, "invalid".to_string());
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("must have a non-empty body")
        );
    }

    #[test]
    fn test_parse_agent_empty_tool_in_allowlist() {
        let content = r#"---
description: "Valid description"
tools: ["shell", "", "apply_patch"]
---

Some content here.
"#;

        let result = parse_agent_markdown(content, "invalid".to_string());
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("empty tool name in allowlist")
        );
    }

    #[test]
    fn test_parse_agent_windows_line_endings() {
        let content = "---\r\ndescription: \"Windows line endings\"\r\n---\r\n\r\nYou are a Windows-compatible agent.";

        let agent = parse_agent_markdown(content, "windows-agent".to_string()).unwrap();
        assert_eq!(agent.name, "windows-agent");
        assert_eq!(agent.description, "Windows line endings");
        assert!(agent.body.contains("Windows-compatible agent"));
    }

    #[test]
    fn test_parse_agent_complex_yaml() {
        let content = r#"---
description: "Complex YAML agent"
tools:
  - "shell"
  - "apply_patch"
  - "web_search"
---

You are a complex agent with multiple tools available.
Use them wisely to accomplish tasks.
"#;

        let agent = parse_agent_markdown(content, "complex".to_string()).unwrap();
        assert_eq!(agent.name, "complex");
        assert_eq!(agent.description, "Complex YAML agent");
        assert_eq!(
            agent.tools,
            Some(vec![
                "shell".to_string(),
                "apply_patch".to_string(),
                "web_search".to_string()
            ])
        );
        assert!(agent.body.contains("complex agent"));
    }

    #[test]
    fn test_load_agents_from_nonexistent_directory() {
        use std::path::Path;

        let nonexistent_dir = Path::new("/nonexistent/directory");
        let agents = load_agents_from_directory(nonexistent_dir).unwrap();
        assert_eq!(agents.len(), 0);
    }

    #[test]
    fn test_load_agents_from_directory_with_invalid_files() {
        let temp_dir = TempDir::new().unwrap();
        let agents_dir = temp_dir.path();

        // Create valid agent
        let valid_agent = r#"---
description: "Valid agent"
---
Valid system prompt.
"#;
        write(agents_dir.join("valid.md"), valid_agent).unwrap();

        // Create invalid agent (will be skipped with warning)
        let invalid_agent = "Invalid content without frontmatter";
        write(agents_dir.join("invalid.md"), invalid_agent).unwrap();

        // Create non-markdown file (will be ignored)
        write(agents_dir.join("readme.txt"), "Not an agent file").unwrap();

        let agents = load_agents_from_directory(agents_dir).unwrap();
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].name, "valid");
    }

    #[test]
    fn test_agent_registry_operations() {
        let mut registry = AgentRegistry::new();

        // Test empty registry
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
        assert_eq!(registry.list_agents().len(), 0);

        // Add first agent
        let agent1 = SubAgent {
            name: "agent1".to_string(),
            description: "First agent".to_string(),
            tools: Some(vec!["shell".to_string()]),
            body: "System prompt 1".to_string(),
        };
        registry.insert_agent(agent1);

        assert!(!registry.is_empty());
        assert_eq!(registry.len(), 1);
        assert!(registry.has_agent("agent1"));
        assert!(!registry.has_agent("agent2"));

        // Add second agent
        let agent2 = SubAgent {
            name: "agent2".to_string(),
            description: "Second agent".to_string(),
            tools: None,
            body: "System prompt 2".to_string(),
        };
        registry.insert_agent(agent2);

        assert_eq!(registry.len(), 2);
        let names = registry.list_agents();
        assert_eq!(names, vec!["agent1".to_string(), "agent2".to_string()]); // Should be sorted

        // Test retrieval
        let retrieved1 = registry.get_agent("agent1").unwrap();
        assert_eq!(retrieved1.description, "First agent");
        assert_eq!(retrieved1.tools, Some(vec!["shell".to_string()]));

        let retrieved2 = registry.get_agent("agent2").unwrap();
        assert_eq!(retrieved2.description, "Second agent");
        assert_eq!(retrieved2.tools, None);

        // Test overwrite
        let agent1_updated = SubAgent {
            name: "agent1".to_string(),
            description: "Updated first agent".to_string(),
            tools: Some(vec!["shell".to_string(), "apply_patch".to_string()]),
            body: "Updated system prompt".to_string(),
        };
        registry.insert_agent(agent1_updated);

        assert_eq!(registry.len(), 2); // Still 2 agents
        let updated = registry.get_agent("agent1").unwrap();
        assert_eq!(updated.description, "Updated first agent");
        assert_eq!(
            updated.tools,
            Some(vec!["shell".to_string(), "apply_patch".to_string()])
        );
    }

    #[test]
    fn test_discover_and_load_agents_precedence() {
        use std::fs::create_dir_all;

        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path();

        // Create project agents directory
        let project_agents_dir = project_root.join(".codex").join("agents");
        create_dir_all(&project_agents_dir).unwrap();

        // Create a project agent
        let project_agent = r#"---
description: "Project-specific agent"
tools: ["shell"]
---
Project system prompt.
"#;
        write(project_agents_dir.join("shared-agent.md"), project_agent).unwrap();

        // We can't easily test user agents directory without mocking dirs::home_dir(),
        // so we'll test project-only scenario
        let registry = discover_and_load_agents(Some(project_root)).unwrap();

        assert_eq!(registry.len(), 1);
        assert!(registry.has_agent("shared-agent"));
        let agent = registry.get_agent("shared-agent").unwrap();
        assert_eq!(agent.description, "Project-specific agent");
        assert!(agent.body.contains("Project system prompt"));
    }

    #[test]
    fn test_subagent_description_serialization() {
        let description = SubAgentDescription {
            name: "test-agent".to_string(),
            description: "A test agent".to_string(),
            tools: Some(vec!["shell".to_string(), "apply_patch".to_string()]),
            body: "Test system prompt".to_string(),
        };

        // Test serialization
        let serialized = serde_json::to_string(&description).unwrap();
        assert!(serialized.contains("test-agent"));
        assert!(serialized.contains("A test agent"));

        // Test deserialization
        let deserialized: SubAgentDescription = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized.name, "test-agent");
        assert_eq!(deserialized.description, "A test agent");
        assert_eq!(
            deserialized.tools,
            Some(vec!["shell".to_string(), "apply_patch".to_string()])
        );
        assert_eq!(deserialized.body, "Test system prompt");
    }

    #[test]
    fn test_subagent_result_serialization() {
        let result = SubAgentResult {
            agent_name: "test-agent".to_string(),
            task: "Test task".to_string(),
            success: true,
            output: "Task completed successfully".to_string(),
            error: None,
        };

        // Test serialization
        let serialized = serde_json::to_string(&result).unwrap();
        assert!(serialized.contains("test-agent"));
        assert!(serialized.contains("Task completed"));

        // Test deserialization
        let deserialized: SubAgentResult = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized.agent_name, "test-agent");
        assert_eq!(deserialized.task, "Test task");
        assert!(deserialized.success);
        assert_eq!(deserialized.output, "Task completed successfully");
        assert_eq!(deserialized.error, None);
    }

    #[tokio::test]
    async fn test_nested_agent_runner_with_agents() {
        use crate::openai_tools::ToolsConfig;

        let mut registry = AgentRegistry::new();

        // Add a test agent
        let agent = SubAgent {
            name: "test-agent".to_string(),
            description: "A test agent for running tasks".to_string(),
            tools: Some(vec!["shell".to_string()]),
            body: "You are a helpful test agent. Complete the given task efficiently.".to_string(),
        };
        registry.insert_agent(agent);

        let runner = NestedAgentRunner::new(registry);

        // Test agent listing
        let agents = runner.list_agents();
        assert_eq!(agents, vec!["test-agent".to_string()]);

        // Test agent description
        let description = runner.describe_agent("test-agent").unwrap();
        assert_eq!(description.name, "test-agent");
        assert_eq!(description.description, "A test agent for running tasks");
        assert_eq!(description.tools, Some(vec!["shell".to_string()]));
        assert!(description.body.contains("helpful test agent"));

        // Test running agent (with mock ToolsConfig)
        let tools_config = ToolsConfig {
            shell_type: crate::openai_tools::ConfigShellToolType::DefaultShell,
            plan_tool: false,
            apply_patch_tool_type: None,
            web_search_request: false,
            include_view_image_tool: false,
            include_subagent_tools: false,
        };

        // Create a mock ModelClient for testing
        // Note: This is a minimal mock that won't actually be used in the current implementation
        // but is required for the method signature
        use crate::config::{Config, ConfigOverrides, ConfigToml};
        use crate::model_provider_info::{ModelProviderInfo, WireApi};
        use codex_protocol::config_types::{ReasoningEffort, ReasoningSummary};
        use codex_protocol::mcp_protocol::ConversationId;
        use std::path::PathBuf;
        use std::sync::Arc;

        // Create minimal test config
        let test_config_toml = ConfigToml::default();
        let mock_config = Arc::new(
            Config::load_from_base_config_with_overrides(
                test_config_toml,
                ConfigOverrides::default(),
                PathBuf::from("/tmp"),
            )
            .unwrap(),
        );

        let mock_provider = ModelProviderInfo {
            name: "test-provider".to_string(),
            base_url: Some("http://localhost:1234".to_string()), // Use localhost to avoid real API calls
            env_key: None,                                       // No env key required for tests
            env_key_instructions: None,
            wire_api: WireApi::Chat,
            query_params: None,
            http_headers: None,
            env_http_headers: None,
            request_max_retries: Some(1), // Reduce retries for faster test failure
            stream_max_retries: Some(1),
            stream_idle_timeout_ms: Some(1000),
            requires_openai_auth: false, // Don't require OpenAI auth for tests
        };

        let mock_conversation_id = ConversationId::new();
        let mock_client = ModelClient::new(
            mock_config,
            None,
            mock_provider,
            ReasoningEffort::Medium,
            ReasoningSummary::Auto,
            mock_conversation_id,
        );

        // Test running agent - this will fail because localhost:1234 is not running
        // but we're testing that the error handling works correctly
        let result = runner
            .run_agent("test-agent", "Test task", &tools_config, &mock_client)
            .await;

        // Since we can't make real API calls in tests, we expect this to fail
        // but the error should be from the network call, not from missing agent
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(
            error_msg.contains("Failed to start model conversation")
                || error_msg.contains("Connection refused")
                || error_msg.contains("network error")
        );

        // Test running nonexistent agent
        let result = runner
            .run_agent("nonexistent", "Task", &tools_config, &mock_client)
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn test_load_agent_from_file_nonexistent() {
        use std::path::Path;

        let nonexistent_file = Path::new("/nonexistent/file.md");
        let result = load_agent_from_file(nonexistent_file, "test".to_string());

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Failed to read agent file")
        );
    }
}
