use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

use codex_protocol::custom_commands::CustomCommandArgSpec;
use codex_protocol::custom_commands::CustomCommandId;
use codex_protocol::custom_commands::CustomCommandSpec;
use codex_protocol::custom_commands::CustomCommandVisibility;

use crate::error::CodexErr;

/// Indicates where a command definition was loaded from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandSource {
    User(PathBuf),
    Project(PathBuf),
}

/// Fully loaded custom command with source metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct CustomCommand {
    pub spec: CustomCommandSpec,
    pub path: PathBuf,
    pub source: CommandSource,
}

impl CustomCommand {
    pub fn id(&self) -> &str {
        &self.spec.id.0
    }
}

/// Normalize a name into a stable `CustomCommandId` string.
///
/// Lowercase, convert spaces/underscores to hyphens, drop other non-alphanumeric
/// characters (except hyphen), collapse runs of hyphens, and trim leading/trailing hyphens.
pub fn normalize_id(name: &str) -> String {
    let lowered = name.to_lowercase();
    let mut out = String::with_capacity(lowered.len());
    let mut prev_hyphen = false;
    for ch in lowered.chars() {
        let mapped = match ch {
            'a'..='z' | '0'..='9' => {
                prev_hyphen = false;
                Some(ch)
            }
            ' ' | '_' | '-' => {
                if !prev_hyphen {
                    prev_hyphen = true;
                    Some('-')
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some(m) = mapped {
            out.push(m);
        }
    }
    // Trim leading/trailing '-'
    let trimmed = out.trim_matches('-').to_string();
    // Avoid empty ids
    if trimmed.is_empty() {
        "unnamed".to_string()
    } else {
        trimmed
    }
}

/// Load commands from a directory containing markdown files with YAML frontmatter.
/// Returns successfully loaded commands; invalid files are logged and skipped.
pub fn load_commands_from_directory(
    directory: &Path,
    source: CommandSource,
) -> Result<Vec<CustomCommand>, CodexErr> {
    if !directory.exists() || !directory.is_dir() {
        return Ok(Vec::new());
    }

    let mut commands = Vec::new();

    let entries = fs::read_dir(directory).map_err(|e| {
        CodexErr::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "Failed to read commands directory '{}': {e}",
                directory.display()
            ),
        ))
    })?;

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!(
                    "Failed to read directory entry in '{}': {}",
                    directory.display(),
                    e
                );
                continue;
            }
        };
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }

        match load_command_from_file(&path, source.clone()) {
            Ok(cmd) => commands.push(cmd),
            Err(e) => {
                tracing::warn!("Failed to load command from '{}': {e}", path.display());
            }
        }
    }

    Ok(commands)
}

/// Load a single command from a markdown file.
fn load_command_from_file(path: &Path, source: CommandSource) -> Result<CustomCommand, CodexErr> {
    let content = fs::read_to_string(path).map_err(|e| {
        CodexErr::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Failed to read command file '{}': {e}", path.display()),
        ))
    })?;

    let (frontmatter, body) = parse_frontmatter(&content, path)?;

    // Compute canonical name from filename; allow optional name in frontmatter
    let filename_name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| {
            CodexErr::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Invalid filename for command: {}", path.display()),
            ))
        })?
        .to_string();

    let declared_name = frontmatter.name.unwrap_or_else(|| filename_name.clone());
    if declared_name != filename_name {
        tracing::warn!(
            "Frontmatter name '{}' differs from filename '{}'; using filename",
            declared_name,
            filename_name
        );
    }

    if frontmatter.description.trim().is_empty() {
        return Err(CodexErr::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Command '{filename_name}' must have a non-empty description"),
        )));
    }
    if body.trim().is_empty() {
        return Err(CodexErr::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Command '{filename_name}' must have a non-empty template body"),
        )));
    }

    let id = normalize_id(&filename_name);

    let spec = CustomCommandSpec {
        id: CustomCommandId(id),
        name: filename_name,
        aliases: frontmatter.aliases.unwrap_or_default(),
        description: frontmatter.description,
        template: body,
        args: frontmatter.args.unwrap_or_default(),
        tags: frontmatter.tags.unwrap_or_default(),
        version: frontmatter.version,
        disabled: frontmatter.disabled.unwrap_or(false),
        visibility: frontmatter.visibility.unwrap_or_default(),
    };

    Ok(CustomCommand {
        spec,
        path: path.to_path_buf(),
        source,
    })
}

/// YAML frontmatter for a command file.
#[derive(Debug, Clone, serde::Deserialize)]
struct CommandFrontmatter {
    #[serde(default)]
    name: Option<String>,
    description: String,
    #[serde(default)]
    aliases: Option<Vec<String>>,
    #[serde(default)]
    args: Option<Vec<CustomCommandArgSpec>>,
    #[serde(default)]
    tags: Option<Vec<String>>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    disabled: Option<bool>,
    #[serde(default)]
    visibility: Option<CustomCommandVisibility>,
}

/// Parse '---' frontmatter and return (frontmatter, body)
fn parse_frontmatter(content: &str, path: &Path) -> Result<(CommandFrontmatter, String), CodexErr> {
    let normalized = content.replace("\r\n", "\n");
    let lines: Vec<&str> = normalized.trim().lines().collect();
    if lines.first().map(|l| l.trim()) != Some("---") {
        return Err(CodexErr::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "Command file '{}' missing YAML frontmatter (must start with '---')",
                path.display()
            ),
        )));
    }

    let mut fm_end_idx: Option<usize> = None;
    for (i, line) in lines.iter().enumerate().skip(1) {
        if line.trim() == "---" {
            fm_end_idx = Some(i);
            break;
        }
    }
    let fm_end_idx = fm_end_idx.ok_or_else(|| {
        CodexErr::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "Command file '{}' missing frontmatter closing '---'",
                path.display()
            ),
        ))
    })?;

    let frontmatter_content = lines[1..fm_end_idx].join("\n");
    let body = if fm_end_idx + 1 < lines.len() {
        lines[fm_end_idx + 1..].join("\n").trim().to_string()
    } else {
        String::new()
    };

    let frontmatter: CommandFrontmatter =
        serde_yaml::from_str(&frontmatter_content).map_err(|e| {
            CodexErr::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "Failed to parse YAML frontmatter for command '{}': {e}",
                    path.display()
                ),
            ))
        })?;

    Ok((frontmatter, body))
}

/// Discover commands from user and project directories, with project taking precedence.
///
/// - User path: `$HOME/.codex/commands`
/// - Project path: `<project_root>/.codex/commands`
///
/// Later (project) definitions override earlier (user) ones by id.
pub fn discover_and_load_commands(
    project_root: Option<&Path>,
) -> Result<Vec<CustomCommand>, CodexErr> {
    let mut by_id: HashMap<String, CustomCommand> = HashMap::new();

    // Load user commands first (lower precedence)
    if let Some(home) = dirs::home_dir() {
        let user_dir = home.join(".codex").join("commands");
        let cmds = load_commands_from_directory(&user_dir, CommandSource::User(user_dir.clone()))?;
        for c in cmds {
            by_id.insert(c.id().to_string(), c);
        }
    }

    // Load project commands (higher precedence)
    if let Some(root) = project_root {
        let proj_dir = root.join(".codex").join("commands");
        let cmds =
            load_commands_from_directory(&proj_dir, CommandSource::Project(proj_dir.clone()))?;
        for c in cmds {
            by_id.insert(c.id().to_string(), c);
        }
    }

    // Return sorted by id for determinism
    let mut out: Vec<CustomCommand> = by_id.into_values().collect();
    out.sort_by(|a, b| a.id().cmp(b.id()));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn id_normalization_basic() {
        assert_eq!(normalize_id("Hello World"), "hello-world");
        assert_eq!(normalize_id("A__b  C"), "a-b-c");
        assert_eq!(normalize_id("___"), "unnamed");
        assert_eq!(normalize_id("foo@bar!baz"), "foobarbaz");
    }

    #[test]
    fn load_single_command_from_dir() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("commands");
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("greet.md");
        fs::write(
            &file,
            r#"---
description: Greet a user
aliases: ["hi", "hello"]
args:
  - name: name
    required: false
---
Hello $1!
"#,
        )
        .unwrap();

        let cmds = load_commands_from_directory(&dir, CommandSource::User(dir.clone())).unwrap();
        assert_eq!(cmds.len(), 1);
        let cmd = &cmds[0];
        assert_eq!(cmd.spec.id.0, "greet");
        assert_eq!(cmd.spec.name, "greet");
        assert_eq!(cmd.spec.description, "Greet a user");
        assert_eq!(cmd.spec.aliases, vec!["hi", "hello"]);
        assert_eq!(cmd.spec.template.trim(), "Hello $1!");
        assert!(!cmd.spec.disabled);
    }

    #[test]
    fn invalid_files_are_skipped() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("commands");
        fs::create_dir_all(&dir).unwrap();
        // Missing frontmatter
        fs::write(dir.join("bad1.md"), "no frontmatter").unwrap();
        // Empty body
        fs::write(dir.join("bad2.md"), "---\ndescription: test\n---\n\n\n").unwrap();
        // Valid
        fs::write(dir.join("ok.md"), "---\ndescription: ok\n---\nbody\n").unwrap();

        let cmds = load_commands_from_directory(&dir, CommandSource::User(dir.clone())).unwrap();
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].spec.id.0, "ok");
    }

    #[test]
    fn project_overrides_user() {
        let tmp = TempDir::new().unwrap();
        let home = tmp.path().join("home");
        let project = tmp.path().join("project");
        fs::create_dir_all(home.join(".codex/commands")).unwrap();
        fs::create_dir_all(project.join(".codex/commands")).unwrap();

        // Simulate HOME for this test by writing to the real HOME is not ideal.
        // Instead, load user via direct function and then project override via discover function.
        // We'll write both and then call the discover using project root, but we cannot
        // override dirs::home_dir() easily here. So validate override behavior via direct maps.

        // User command
        fs::write(
            home.join(".codex/commands/greet.md"),
            "---\ndescription: user desc\n---\nUser template\n",
        )
        .unwrap();
        // Project command overrides same id
        fs::write(
            project.join(".codex/commands/greet.md"),
            "---\ndescription: project desc\n---\nProject template\n",
        )
        .unwrap();

        // Load both explicitly to simulate precedence
        let mut by_id: HashMap<String, CustomCommand> = HashMap::new();
        for c in load_commands_from_directory(
            &home.join(".codex/commands"),
            CommandSource::User(home.join(".codex/commands")),
        )
        .unwrap()
        {
            by_id.insert(c.id().to_string(), c);
        }
        for c in load_commands_from_directory(
            &project.join(".codex/commands"),
            CommandSource::Project(project.join(".codex/commands")),
        )
        .unwrap()
        {
            by_id.insert(c.id().to_string(), c);
        }

        let mut list: Vec<CustomCommand> = by_id.into_values().collect();
        list.sort_by(|a, b| a.id().cmp(b.id()));
        assert_eq!(list.len(), 1);
        let cmd = &list[0];
        assert_eq!(cmd.spec.id.0, "greet");
        assert_eq!(cmd.spec.description, "project desc");
        assert_eq!(cmd.spec.template.trim(), "Project template");
    }
}
