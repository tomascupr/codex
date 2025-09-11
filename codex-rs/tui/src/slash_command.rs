use strum::IntoEnumIterator;
use strum_macros::AsRefStr;
use strum_macros::EnumIter;
use strum_macros::EnumString;
use strum_macros::IntoStaticStr;

/// Commands that can be invoked by starting a message with a leading slash.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString, EnumIter, AsRefStr, IntoStaticStr,
)]
#[strum(serialize_all = "kebab-case")]
pub enum SlashCommand {
    // DO NOT ALPHA-SORT! Enum order is presentation order in the popup, so
    // more frequently used commands should be listed first.
    Model,
    Approvals,
    New,
    Init,
    Compact,
    Diff,
    Mention,
    Agents,
    Agent,
    Status,
    Mcp,
    Logout,
    Quit,
    #[cfg(debug_assertions)]
    TestApproval,
}

impl SlashCommand {
    /// User-visible description shown in the popup.
    pub fn description(self) -> &'static str {
        match self {
            SlashCommand::New => "start a new chat during a conversation",
            SlashCommand::Init => "create an AGENTS.md file with instructions for Codex",
            SlashCommand::Compact => "summarize conversation to prevent hitting the context limit",
            SlashCommand::Quit => "exit Codex",
            SlashCommand::Diff => "show git diff (including untracked files)",
            SlashCommand::Mention => "mention a file",
            SlashCommand::Agents => "list available sub-agents",
            SlashCommand::Agent => {
                "run a sub-agent with a specific task (usage: /agent <name> <task>)"
            }
            SlashCommand::Status => "show current session configuration and token usage",
            SlashCommand::Model => "choose what model and reasoning effort to use",
            SlashCommand::Approvals => "choose what Codex can do without approval",
            SlashCommand::Mcp => "list configured MCP tools",
            SlashCommand::Logout => "log out of Codex",
            #[cfg(debug_assertions)]
            SlashCommand::TestApproval => "test approval request",
        }
    }

    /// Command string without the leading '/'. Provided for compatibility with
    /// existing code that expects a method named `command()`.
    pub fn command(self) -> &'static str {
        self.into()
    }

    /// Whether this command can be run while a task is in progress.
    pub fn available_during_task(self) -> bool {
        match self {
            SlashCommand::New
            | SlashCommand::Init
            | SlashCommand::Compact
            | SlashCommand::Model
            | SlashCommand::Approvals
            | SlashCommand::Logout => false,
            SlashCommand::Diff
            | SlashCommand::Mention
            | SlashCommand::Agents
            | SlashCommand::Agent
            | SlashCommand::Status
            | SlashCommand::Mcp
            | SlashCommand::Quit => true,

            #[cfg(debug_assertions)]
            SlashCommand::TestApproval => true,
        }
    }
}

/// Return all built-in commands in a Vec paired with their command string.
pub fn built_in_slash_commands() -> Vec<(&'static str, SlashCommand)> {
    SlashCommand::iter().map(|c| (c.command(), c)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_agents_slash_command() {
        // Test that agents command exists and has correct properties
        let agents_cmd = SlashCommand::Agents;
        assert_eq!(agents_cmd.command(), "agents");
        assert_eq!(agents_cmd.description(), "list available sub-agents");
        assert!(agents_cmd.available_during_task());
    }

    #[test]
    fn test_agent_slash_command() {
        // Test that agent command exists and has correct properties
        let agent_cmd = SlashCommand::Agent;
        assert_eq!(agent_cmd.command(), "agent");
        assert_eq!(
            agent_cmd.description(),
            "run a sub-agent with a specific task (usage: /agent <name> <task>)"
        );
        assert!(agent_cmd.available_during_task());
    }

    #[test]
    fn test_slash_command_parsing() {
        // Test that commands can be parsed from strings
        assert_eq!(
            SlashCommand::from_str("agents").unwrap(),
            SlashCommand::Agents
        );
        assert_eq!(
            SlashCommand::from_str("agent").unwrap(),
            SlashCommand::Agent
        );

        // Test kebab-case serialization
        assert_eq!(SlashCommand::Agents.as_ref(), "agents");
        assert_eq!(SlashCommand::Agent.as_ref(), "agent");
    }

    #[test]
    fn test_built_in_slash_commands_includes_subagent_commands() {
        let commands = built_in_slash_commands();
        let command_names: Vec<&str> = commands.iter().map(|(name, _)| *name).collect();

        assert!(command_names.contains(&"agents"));
        assert!(command_names.contains(&"agent"));

        // Find the actual command structs
        let agents_entry = commands.iter().find(|(name, _)| *name == "agents").unwrap();
        let agent_entry = commands.iter().find(|(name, _)| *name == "agent").unwrap();

        assert_eq!(agents_entry.1, SlashCommand::Agents);
        assert_eq!(agent_entry.1, SlashCommand::Agent);
    }

    #[test]
    fn test_slash_command_availability() {
        // Test that sub-agent commands can be run during task execution
        assert!(SlashCommand::Agents.available_during_task());
        assert!(SlashCommand::Agent.available_during_task());

        // Compare with commands that can't be run during tasks
        assert!(!SlashCommand::New.available_during_task());
        assert!(!SlashCommand::Model.available_during_task());
    }

    #[test]
    fn test_enum_iteration_includes_subagent_commands() {
        let all_commands: Vec<SlashCommand> = SlashCommand::iter().collect();

        assert!(all_commands.contains(&SlashCommand::Agents));
        assert!(all_commands.contains(&SlashCommand::Agent));

        // Verify the commands appear in the expected order (before Status)
        let agents_pos = all_commands
            .iter()
            .position(|&c| c == SlashCommand::Agents)
            .unwrap();
        let agent_pos = all_commands
            .iter()
            .position(|&c| c == SlashCommand::Agent)
            .unwrap();
        let status_pos = all_commands
            .iter()
            .position(|&c| c == SlashCommand::Status)
            .unwrap();

        assert!(agents_pos < status_pos);
        assert!(agent_pos < status_pos);
        assert_eq!(agent_pos, agents_pos + 1); // Agent should come right after Agents
    }

    #[test]
    fn test_slash_command_descriptions() {
        // Test specific descriptions for sub-agent commands
        assert!(SlashCommand::Agents.description().contains("sub-agents"));
        assert!(SlashCommand::Agent.description().contains("sub-agent"));
        assert!(SlashCommand::Agent.description().contains("usage:"));
        assert!(SlashCommand::Agent.description().contains("/agent"));

        // Ensure descriptions are not empty
        for cmd in SlashCommand::iter() {
            assert!(
                !cmd.description().is_empty(),
                "Command {:?} has empty description",
                cmd
            );
        }
    }
}
