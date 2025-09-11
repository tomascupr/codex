use std::fs::{create_dir_all, write};
use std::path::Path;
use tempfile::TempDir;

use codex_core::agents::{
    AgentRegistry, NestedAgentRunner, SubAgent, SubAgentDescription, SubAgentResult,
    discover_and_load_agents, load_agents_from_directory,
};

/// Create test agents in a directory
fn create_test_agents(agents_dir: &Path) -> Result<(), std::io::Error> {
    create_dir_all(agents_dir)?;

    // Simple agent with no tool restrictions
    let general_agent = r#"---
description: "A general purpose test agent"
---
You are a helpful general purpose assistant."#;
    write(agents_dir.join("general.md"), general_agent)?;

    // Agent with tool restrictions
    let restricted_agent = r#"---
description: "A restricted test agent"
tools: ["local_shell"]
---
You are a restricted agent that can only use shell commands."#;
    write(agents_dir.join("restricted.md"), restricted_agent)?;

    // Agent with multiple tools
    let multi_tool_agent = r#"---
description: "A multi-tool test agent"
tools: ["local_shell", "web_search", "apply_patch"]
---
You are an agent with access to multiple tools."#;
    write(agents_dir.join("multi-tool.md"), multi_tool_agent)?;

    // Agent with no tools allowed
    let no_tools_agent = r#"---
description: "An agent with no tools"
tools: []
---
You are an agent that cannot use any tools."#;
    write(agents_dir.join("no-tools.md"), no_tools_agent)?;

    Ok(())
}

#[tokio::test]
async fn test_agent_discovery_and_loading() {
    let temp_dir = TempDir::new().unwrap();
    let project_root = temp_dir.path();

    // Create project agents directory
    let project_agents_dir = project_root.join(".codex").join("agents");
    create_test_agents(&project_agents_dir).unwrap();

    // Load agents from project directory
    let registry = discover_and_load_agents(Some(project_root)).unwrap();

    assert_eq!(registry.len(), 4);
    assert!(registry.has_agent("general"));
    assert!(registry.has_agent("restricted"));
    assert!(registry.has_agent("multi-tool"));
    assert!(registry.has_agent("no-tools"));

    let agent_names = registry.list_agents();
    assert_eq!(
        agent_names,
        vec!["general", "multi-tool", "no-tools", "restricted"]
    );
}

#[test]
fn test_load_agents_from_directory() {
    let temp_dir = TempDir::new().unwrap();
    let agents_dir = temp_dir.path();

    create_test_agents(agents_dir).unwrap();

    let agents = load_agents_from_directory(agents_dir).unwrap();
    assert_eq!(agents.len(), 4);

    // Verify agent details
    let general_agent = agents.iter().find(|a| a.name == "general").unwrap();
    assert_eq!(general_agent.description, "A general purpose test agent");
    assert_eq!(general_agent.tools, None);

    let restricted_agent = agents.iter().find(|a| a.name == "restricted").unwrap();
    assert_eq!(restricted_agent.description, "A restricted test agent");
    assert_eq!(
        restricted_agent.tools,
        Some(vec!["local_shell".to_string()])
    );

    let no_tools_agent = agents.iter().find(|a| a.name == "no-tools").unwrap();
    assert_eq!(no_tools_agent.description, "An agent with no tools");
    assert_eq!(no_tools_agent.tools, Some(vec![]));
}

#[test]
fn test_agent_registry_operations() {
    let mut registry = AgentRegistry::new();
    assert!(registry.is_empty());
    assert_eq!(registry.len(), 0);

    // Add agents
    let agent1 = SubAgent {
        name: "test1".to_string(),
        description: "Test agent 1".to_string(),
        tools: Some(vec!["local_shell".to_string()]),
        body: "You are test agent 1".to_string(),
    };

    let agent2 = SubAgent {
        name: "test2".to_string(),
        description: "Test agent 2".to_string(),
        tools: None,
        body: "You are test agent 2".to_string(),
    };

    registry.insert_agent(agent1);
    registry.insert_agent(agent2);

    assert!(!registry.is_empty());
    assert_eq!(registry.len(), 2);
    assert!(registry.has_agent("test1"));
    assert!(registry.has_agent("test2"));
    assert!(!registry.has_agent("nonexistent"));

    // Test sorted listing
    let names = registry.list_agents();
    assert_eq!(names, vec!["test1", "test2"]);

    // Test retrieval
    let retrieved1 = registry.get_agent("test1").unwrap();
    assert_eq!(retrieved1.description, "Test agent 1");
    assert_eq!(retrieved1.tools, Some(vec!["local_shell".to_string()]));

    let retrieved2 = registry.get_agent("test2").unwrap();
    assert_eq!(retrieved2.description, "Test agent 2");
    assert_eq!(retrieved2.tools, None);

    // Test overwrite
    let updated_agent1 = SubAgent {
        name: "test1".to_string(),
        description: "Updated test agent 1".to_string(),
        tools: Some(vec!["local_shell".to_string(), "web_search".to_string()]),
        body: "You are the updated test agent 1".to_string(),
    };
    registry.insert_agent(updated_agent1);

    assert_eq!(registry.len(), 2); // Still 2 agents
    let updated = registry.get_agent("test1").unwrap();
    assert_eq!(updated.description, "Updated test agent 1");
    assert_eq!(
        updated.tools,
        Some(vec!["local_shell".to_string(), "web_search".to_string()])
    );
}

#[test]
fn test_nested_agent_runner_basic_operations() {
    let mut registry = AgentRegistry::new();

    let test_agent = SubAgent {
        name: "test-runner".to_string(),
        description: "A test runner agent".to_string(),
        tools: Some(vec!["local_shell".to_string()]),
        body: "You are a test runner agent".to_string(),
    };
    registry.insert_agent(test_agent);

    let runner = NestedAgentRunner::new(registry);

    // Test listing agents
    let agents = runner.list_agents();
    assert_eq!(agents, vec!["test-runner"]);

    // Test describing existing agent
    let description = runner.describe_agent("test-runner").unwrap();
    assert_eq!(description.name, "test-runner");
    assert_eq!(description.description, "A test runner agent");
    assert_eq!(description.tools, Some(vec!["local_shell".to_string()]));
    assert!(description.body.contains("test runner agent"));

    // Test describing non-existent agent
    let result = runner.describe_agent("nonexistent");
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not found"));
}

#[test]
fn test_subagent_serialization() {
    // Test SubAgentDescription serialization
    let description = SubAgentDescription {
        name: "test-agent".to_string(),
        description: "A test agent".to_string(),
        tools: Some(vec!["local_shell".to_string(), "web_search".to_string()]),
        body: "Test system prompt".to_string(),
    };

    let serialized = serde_json::to_string(&description).unwrap();
    let deserialized: SubAgentDescription = serde_json::from_str(&serialized).unwrap();

    assert_eq!(deserialized.name, "test-agent");
    assert_eq!(deserialized.description, "A test agent");
    assert_eq!(
        deserialized.tools,
        Some(vec!["local_shell".to_string(), "web_search".to_string()])
    );
    assert_eq!(deserialized.body, "Test system prompt");

    // Test SubAgentResult serialization
    let result = SubAgentResult {
        agent_name: "test-agent".to_string(),
        task: "Test task".to_string(),
        success: true,
        output: "Task completed successfully".to_string(),
        error: None,
    };

    let serialized = serde_json::to_string(&result).unwrap();
    let deserialized: SubAgentResult = serde_json::from_str(&serialized).unwrap();

    assert_eq!(deserialized.agent_name, "test-agent");
    assert_eq!(deserialized.task, "Test task");
    assert!(deserialized.success);
    assert_eq!(deserialized.output, "Task completed successfully");
    assert_eq!(deserialized.error, None);

    // Test error case
    let error_result = SubAgentResult {
        agent_name: "failing-agent".to_string(),
        task: "Failing task".to_string(),
        success: false,
        output: "".to_string(),
        error: Some("Something went wrong".to_string()),
    };

    let serialized = serde_json::to_string(&error_result).unwrap();
    let deserialized: SubAgentResult = serde_json::from_str(&serialized).unwrap();

    assert_eq!(deserialized.agent_name, "failing-agent");
    assert!(!deserialized.success);
    assert_eq!(deserialized.error, Some("Something went wrong".to_string()));
}

#[test]
fn test_invalid_agent_definitions() {
    let temp_dir = TempDir::new().unwrap();
    let agents_dir = temp_dir.path();
    create_dir_all(agents_dir).unwrap();

    // Invalid agent: missing frontmatter
    let invalid_no_frontmatter = "Just some markdown without frontmatter";
    write(agents_dir.join("invalid1.md"), invalid_no_frontmatter).unwrap();

    // Invalid agent: missing description
    let invalid_no_description = r#"---
tools: ["local_shell"]
---
Some content here."#;
    write(agents_dir.join("invalid2.md"), invalid_no_description).unwrap();

    // Invalid agent: empty description
    let invalid_empty_description = r#"---
description: ""
---
Some content here."#;
    write(agents_dir.join("invalid3.md"), invalid_empty_description).unwrap();

    // Invalid agent: empty body
    let invalid_empty_body = r#"---
description: "Valid description"
---


"#;
    write(agents_dir.join("invalid4.md"), invalid_empty_body).unwrap();

    // Invalid agent: empty tool name in allowlist
    let invalid_empty_tool = r#"---
description: "Valid description"
tools: ["local_shell", "", "web_search"]
---
Some content here."#;
    write(agents_dir.join("invalid5.md"), invalid_empty_tool).unwrap();

    // Valid agent for comparison
    let valid_agent = r#"---
description: "A valid agent"
tools: ["local_shell"]
---
You are a valid agent."#;
    write(agents_dir.join("valid.md"), valid_agent).unwrap();

    // Load agents - should only get the valid one
    let agents = load_agents_from_directory(agents_dir).unwrap();
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0].name, "valid");
    assert_eq!(agents[0].description, "A valid agent");
}

#[test]
fn test_agent_precedence() {
    let temp_dir = TempDir::new().unwrap();
    let project_root = temp_dir.path();

    // Create project agents directory with an agent
    let project_agents_dir = project_root.join(".codex").join("agents");
    create_dir_all(&project_agents_dir).unwrap();

    let project_agent = r#"---
description: "Project-specific agent"
tools: ["local_shell"]
---
I am a project-specific agent."#;
    write(project_agents_dir.join("shared-agent.md"), project_agent).unwrap();

    let project_only_agent = r#"---
description: "Project-only agent"
---
I am only in the project."#;
    write(
        project_agents_dir.join("project-only.md"),
        project_only_agent,
    )
    .unwrap();

    // Load agents from project directory
    let registry = discover_and_load_agents(Some(project_root)).unwrap();

    assert_eq!(registry.len(), 2);
    assert!(registry.has_agent("shared-agent"));
    assert!(registry.has_agent("project-only"));

    let shared_agent = registry.get_agent("shared-agent").unwrap();
    assert_eq!(shared_agent.description, "Project-specific agent");
    assert!(shared_agent.body.contains("project-specific"));

    let project_agent = registry.get_agent("project-only").unwrap();
    assert_eq!(project_agent.description, "Project-only agent");
}

#[test]
fn test_conversation_isolation() {
    // This test verifies that each sub-agent execution creates its own conversation history
    // We can test that the basic structures are set up correctly without requiring network calls

    let mut registry = AgentRegistry::new();
    let agent = SubAgent {
        name: "isolated-agent".to_string(),
        description: "An agent for testing isolation".to_string(),
        tools: Some(vec!["local_shell".to_string()]),
        body: "You are an isolated agent".to_string(),
    };
    registry.insert_agent(agent);

    let runner = NestedAgentRunner::new(registry);

    // Verify agent can be found and described
    let description = runner.describe_agent("isolated-agent").unwrap();
    assert_eq!(description.name, "isolated-agent");
    assert!(description.body.contains("isolated agent"));

    // Test that multiple describe calls return consistent results (no shared state)
    let description2 = runner.describe_agent("isolated-agent").unwrap();
    assert_eq!(description.name, description2.name);
    assert_eq!(description.description, description2.description);
    assert_eq!(description.tools, description2.tools);
    assert_eq!(description.body, description2.body);
}

#[test]
fn test_edge_cases() {
    // Test empty registry
    let empty_registry = AgentRegistry::new();
    let runner = NestedAgentRunner::new(empty_registry);
    assert_eq!(runner.list_agents().len(), 0);

    // Test loading from non-existent directory
    let non_existent_path = Path::new("/nonexistent/directory");
    let agents = load_agents_from_directory(non_existent_path).unwrap();
    assert_eq!(agents.len(), 0);

    // Test registry with many agents (sorted correctly)
    let mut registry = AgentRegistry::new();
    for i in 0..100 {
        let agent = SubAgent {
            name: format!("agent{:03}", i),
            description: format!("Agent number {}", i),
            tools: None,
            body: format!("You are agent number {}", i),
        };
        registry.insert_agent(agent);
    }

    let names = registry.list_agents();
    assert_eq!(names.len(), 100);
    assert_eq!(names[0], "agent000");
    assert_eq!(names[99], "agent099");

    // Verify sorting is correct
    for i in 1..names.len() {
        assert!(names[i - 1] <= names[i]);
    }
}

#[test]
fn test_subagent_structure_validation() {
    // Test creation of SubAgent with various configurations
    let agent_with_tools = SubAgent {
        name: "tool-agent".to_string(),
        description: "Agent with tools".to_string(),
        tools: Some(vec!["local_shell".to_string(), "web_search".to_string()]),
        body: "I can use tools".to_string(),
    };

    assert_eq!(agent_with_tools.name, "tool-agent");
    assert_eq!(agent_with_tools.tools.as_ref().unwrap().len(), 2);

    let agent_no_tools = SubAgent {
        name: "no-tool-agent".to_string(),
        description: "Agent without tools".to_string(),
        tools: None,
        body: "I cannot use tools".to_string(),
    };

    assert_eq!(agent_no_tools.name, "no-tool-agent");
    assert!(agent_no_tools.tools.is_none());

    let agent_empty_tools = SubAgent {
        name: "empty-tool-agent".to_string(),
        description: "Agent with empty tool list".to_string(),
        tools: Some(vec![]),
        body: "I have an empty tool list".to_string(),
    };

    assert_eq!(agent_empty_tools.name, "empty-tool-agent");
    assert_eq!(agent_empty_tools.tools.as_ref().unwrap().len(), 0);
}

#[test]
fn test_filesystem_integration() {
    // Test that file system operations work correctly
    let temp_dir = TempDir::new().unwrap();
    let agents_dir = temp_dir.path().join("agents");
    create_dir_all(&agents_dir).unwrap();

    // Create agent with special characters in content
    let special_agent = r#"---
description: "Agent with special characters: ñ, 中文, emoji 🤖"
tools: ["local_shell", "web_search"]
---
You are an agent that can handle unicode: ñ, 中文, and emoji 🤖.
Use tools carefully and always validate input.

## Instructions:
1. Be helpful
2. Be safe
3. Handle edge cases"#;
    write(agents_dir.join("special.md"), special_agent).unwrap();

    // Load the agent
    let agents = load_agents_from_directory(&agents_dir).unwrap();
    assert_eq!(agents.len(), 1);

    let agent = &agents[0];
    assert_eq!(agent.name, "special");
    assert!(agent.description.contains("ñ"));
    assert!(agent.description.contains("中文"));
    assert!(agent.description.contains("🤖"));
    assert!(agent.body.contains("unicode"));
    assert_eq!(
        agent.tools,
        Some(vec!["local_shell".to_string(), "web_search".to_string()])
    );
}

#[test]
fn test_yaml_parsing_edge_cases() {
    let temp_dir = TempDir::new().unwrap();
    let agents_dir = temp_dir.path();
    create_dir_all(agents_dir).unwrap();

    // Agent with complex YAML structure
    let complex_yaml_agent = r#"---
description: >
  This is a multi-line description
  that spans several lines and should
  be properly parsed by the YAML parser
tools:
  - "local_shell"
  - "web_search"
  - "apply_patch"
extra_field: "should be ignored"
---

You are a complex agent with a multi-line description.

# Your capabilities:
- Shell commands
- Web search
- Code patching

Please use these responsibly."#;
    write(agents_dir.join("complex.md"), complex_yaml_agent).unwrap();

    let agents = load_agents_from_directory(agents_dir).unwrap();
    assert_eq!(agents.len(), 1);

    let agent = &agents[0];
    assert_eq!(agent.name, "complex");
    assert!(agent.description.contains("multi-line description"));
    assert!(agent.description.contains("spans several lines"));
    assert_eq!(
        agent.tools,
        Some(vec![
            "local_shell".to_string(),
            "web_search".to_string(),
            "apply_patch".to_string()
        ])
    );
    assert!(agent.body.contains("Your capabilities"));
}
