# Sub‑Agents in Codex CLI

This document explains the sub‑agent feature implemented in this fork: how agents are authored and discovered, how they are exposed to the model as tools, and how execution is performed and persisted. It also includes code pointers and examples to help you extend or debug the system.

## Overview

- Sub‑agents are named, reusable “mini agents” with their own system prompt and an optional tool allowlist.
- They are defined as Markdown files with YAML frontmatter, discovered from user and project locations.
- The main agent calls them through three function tools: `subagent_list`, `subagent_describe`, and `subagent_run`.
- Runs execute in an isolated nested context with its own prompt and filtered tools; recursion is prevented by disabling sub‑agent tools inside sub‑agents.
- Start/end events are emitted and persisted for auditability and UI.

Key modules:
- `codex-rs/core/src/agents.rs` — loading/merging sub‑agents, nested runner, and execution result types.
- `codex-rs/core/src/openai_tools.rs` — tool definitions for `subagent_*` and shell mapping.
- `codex-rs/core/src/codex.rs` — wiring sub‑agent tools into the main conversation loop and high‑level manager.
- `codex-rs/protocol/src/protocol.rs` — SubAgentStart/End events and Origin tagging.

## Authoring Agents

Create Markdown files with YAML frontmatter. The filename (without `.md`) is the canonical agent name; a different `name:` in the frontmatter is ignored with a warning.

Example: `~/.codex/agents/docs-writer.md`

```markdown
---
# Optional name: frontmatter name differing from filename will be ignored (filename wins)
name: docs-writer
# Required short description
description: "Writes documentation with code pointers and examples"
# Optional allowlist; empty list means no tools; omit to allow all
tools: ["shell", "apply_patch", "web_search"]
---
You are a technical writer agent.
- Prefer concise, actionable sections.
- Include verified file paths and minimal code excerpts.
```

Frontmatter schema (subset):
- `description: string` (required)
- `tools: string[]` (optional)

Parser/validation: `parse_agent_markdown(..)` in `agents.rs` enforces frontmatter markers and non‑empty body; see errors like “missing YAML frontmatter” and “must have a non‑empty body”.

### Implementation snippet (parser)

```rust
// core/src/agents.rs
#[derive(Deserialize)]
struct FrontMatter {
    #[serde(default)]
    name: Option<String>,
    description: String,
    #[serde(default)]
    tools: Option<Vec<String>>,
}

pub fn parse_agent_markdown(content: &str, agent_name: String) -> Result<SubAgent, CodexErr> {
    // Locate opening/closing '---' and split
    // ...
    let frontmatter: FrontMatter = serde_yaml::from_str(&frontmatter_content)?;
    if let Some(ref declared) = frontmatter.name && declared.trim() != agent_name {
        tracing::warn!("Frontmatter name '{}' differs from filename '{}'; using filename", declared, agent_name);
    }
    // Validate description/body, then build SubAgent
    Ok(SubAgent { name: agent_name, description: frontmatter.description, tools: frontmatter.tools, body })
}
```

## Discovery and Precedence

Agents are loaded from both user and project scopes:
- User: `~/.codex/agents/`
- Project: `<repo>/.codex/agents/`

Project agents override user agents with the same name. See:
- `discover_and_load_agents(..)` and `load_agents_from_directory(..)` in `agents.rs`.

### Implementation snippet (discovery)

```rust
// core/src/agents.rs
pub fn discover_and_load_agents(project_root: Option<&Path>) -> Result<AgentRegistry, CodexErr> {
    let mut registry = AgentRegistry::new();
    if let Some(home_dir) = dirs::home_dir() {
        for agent in load_agents_from_directory(&home_dir.join(".codex/agents"))? {
            registry.insert_agent(agent);
        }
    }
    if let Some(project_root) = project_root {
        for agent in load_agents_from_directory(&project_root.join(".codex/agents"))? {
            if registry.has_agent(&agent.name) { tracing::debug!("Project agent '{}' overriding user agent", agent.name); }
            registry.insert_agent(agent);
        }
    }
    Ok(registry)
}
```

## Tool Allowlists and Aliases

When an agent specifies `tools: [...]`, the available tool set for that agent is filtered:
- Implementation: `filter_tools_for_agent(..)` in `agents.rs`.
- Shell aliases: specifying any of `"shell"`, `"local_shell"`, `"exec_command"`, or `"write_stdin"` enables the entire shell family so authors don’t need to memorize internal names.

```rust
// Pseudocode: expands shell aliases and filters tools
let filtered = filter_tools_for_agent(&available_tools, agent_desc.tools.as_deref());
```

### Implementation snippet (allowlist + aliases)

```rust
// core/src/agents.rs
pub(crate) fn filter_tools_for_agent(tools: &[OpenAiTool], allowed: Option<&[String]>) -> Vec<OpenAiTool> {
    match allowed {
        None => tools.to_vec(),
        Some(allowlist) if allowlist.is_empty() => Vec::new(),
        Some(allowlist) => {
            use std::collections::HashSet;
            let mut expanded: HashSet<String> = allowlist.iter().cloned().collect();
            let shell_aliases = ["shell", "local_shell", EXEC_COMMAND_TOOL_NAME, WRITE_STDIN_TOOL_NAME];
            if allowlist.iter().any(|t| shell_aliases.contains(&t.as_str())) {
                for a in shell_aliases { expanded.insert(a.to_string()); }
            }
            tools.iter().filter(|tool| {
                let name = match tool { OpenAiTool::Function(f) => f.name.as_str(), OpenAiTool::LocalShell {} => "local_shell", OpenAiTool::WebSearch {} => "web_search", OpenAiTool::Freeform(f) => f.name.as_str() };
                expanded.contains(name)
            }).cloned().collect()
        }
    }
}
```

## Exposed Tools (Function Tools)

Three tools expose sub‑agent functionality to the model (defined in `openai_tools.rs`):

- `subagent_list()` — returns the list of available agent names.
- `subagent_describe(name)` — returns full metadata/body for the named agent.
- `subagent_run(name, task, model?)` — executes the agent with a task.

Creation helpers (snippets):

```rust
// openai_tools.rs
fn create_subagent_list_tool() -> OpenAiTool { /* name: "subagent_list" */ }
fn create_subagent_describe_tool() -> OpenAiTool { /* name: "subagent_describe" */ }
fn create_subagent_run_tool() -> OpenAiTool { /* name: "subagent_run" */ }
```

Wiring into the conversation loop (snippet from `codex.rs`):

```rust
match name.as_str() {
    "subagent_list" => SubAgentManager::new(&sess.agent_registry)
        .handle_subagent_list(call_id).await,
    "subagent_describe" => SubAgentManager::new(&sess.agent_registry)
        .handle_subagent_describe(arguments, call_id).await,
    "subagent_run" => SubAgentManager::new(&sess.agent_registry)
        .handle_subagent_run(arguments, call_id, sess, turn_context, &sub_id).await,
    // ... other tools
}
```

Feature flag: tools are injected only when `include_subagent_tools` is enabled (see Configuration below). Inside a running sub‑agent, the nested context sets `include_subagent_tools = false` to prevent recursion.

## Execution Flow (NestedAgentRunner)

The `NestedAgentRunner` executes sub‑agents in an isolated flow:

1. `describe_agent(name)` loads the prompt/body and optional allowlist.
2. Compose a system prompt: `"{body}\n\nTask: {task}"`.
3. Compute available tools for the nested context and filter by allowlist:
   ```rust
   let available = get_openai_tools(&nested_tools_config, Some(sess.mcp_connection_manager.list_all_tools()));
   let tools = filter_tools_for_agent(&available, agent_desc.tools.as_deref());
   ```
4. Start streaming with `ModelClient::stream(&Prompt { input, tools, .. })`.
5. Process events:
   - `ResponseItem::Message` — aggregate assistant output.
   - `ResponseItem::FunctionCall` / `LocalShellCall` — execute, record outputs, append to transcript.
   - `ResponseItem::Reasoning` — recorded for traceability (not surfaced in final output).
6. On completion, build `SubAgentResult { success, output, error }` and return.

Important details:
- Local shell calls in Chat Completions payloads are mapped to a standard function tool call named `"shell"`. The generated request has `tool_calls[0].type == "function"` with `function.name == "shell"`.
- Nested sub‑agents are explicitly blocked with a clear failure output: “Sub‑agents are not enabled in this nested context”.

### Implementation snippets (manager + mapping)

Manager entry points:

```rust
// core/src/codex.rs (SubAgentManager)
async fn handle_subagent_run(&self, arguments: String, call_id: String, sess: &Session, turn_context: &TurnContext, sub_id: &str) -> ResponseInputItem {
    #[derive(serde::Deserialize)] struct RunArgs { name: String, task: String, #[serde(default)] model: Option<String> }
    let args = serde_json::from_str::<RunArgs>(&arguments)
        .map_err(|e| format!("failed to parse function arguments: {e}"));
    // Emit SubAgentStart event + record
    // Describe agent; on error:
    // ResponseInputItem::FunctionCallOutput { success: Some(false), content: format!("Sub-agent execution failed: {e}") }
    // Build nested tools_config with include_subagent_tools=false and run model loop…
}
```

Chat payload mapping for LocalShellCall → function `shell`:

```rust
// core/src/chat_completions.rs
ResponseItem::LocalShellCall { id, call_id, action, .. } => {
    let tc_id = call_id.as_deref().or(id.as_deref()).unwrap_or("");
    let args_value = match action { LocalShellAction::Exec(a) => json!({ "command": a.command, "workdir": a.working_directory, "timeout_ms": a.timeout_ms }) };
    messages.push(json!({
        "role": "assistant",
        "content": null,
        "tool_calls": [{
            "id": tc_id,
            "type": "function",
            "function": { "name": "shell", "arguments": serde_json::to_string(&args_value).unwrap_or_default() }
        }]
    }));
}
```

## Start/End Events and Persistence

For UI/auditing, the main process records start/end markers:

- Start: `ResponseItem::SubAgentStart { name, description, origin: Some(Origin::Main) }`
- End: `ResponseItem::SubAgentEnd { name, success, origin: Some(Origin::Main) }`

These also emit `EventMsg::SubAgentStart`/`EventMsg::SubAgentEnd` and are persisted via the rollout recorder. See `protocol.rs` for event types and `rollout/tests.rs` for persistence tests.

### Implementation snippet (events)

```rust
// core/src/codex.rs
let start_event = Event { id: sub_id.to_string(), msg: EventMsg::SubAgentStart(SubAgentStartEvent { name: args.name.clone(), description: args.task.clone() }) };
let end_event   = Event { id: sub_id.to_string(), msg: EventMsg::SubAgentEnd(SubAgentEndEvent   { name: args.name.clone(), success }) };
```

## Error Handling and Messages

- Unknown agent: the manager returns a `FunctionCallOutput` with `success: false` and content containing “Sub‑agent execution failed: …”.
- Invalid tool arguments: the manager returns `failed to parse function arguments: …`.
- Model start/stream errors: captured and included in `SubAgentResult.error`.

## Configuration

- In‑memory toggle (derived from config): `ToolsConfig { include_subagent_tools: bool, .. }`.
- Over MCP: `protocol::mcp_protocol::Tools { subagent_tools: Option<bool>, .. }`.
- CLI override example (enable):
  ```
  codex -c tools.subagent_tools=true
  ```

## File Layout and Key Types

- Definitions and loading:
  - `core/src/agents.rs` — `SubAgent`, `AgentRegistry`, `discover_and_load_agents`, alias expansion.
- Tooling and exposure:
  - `core/src/openai_tools.rs` — `create_subagent_*_tool()` and schemas.
- Orchestration:
  - `core/src/codex.rs` — `SubAgentManager::{handle_subagent_list,handle_subagent_describe,handle_subagent_run}`.
- Protocol/Events:
  - `protocol/src/protocol.rs` — `SubAgentStartEvent`, `SubAgentEndEvent`, `Origin` variants.

### Implementation snippet (result type)

```rust
// core/src/agents.rs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentResult { pub agent_name: String, pub task: String, pub success: bool, pub output: String, pub error: Option<String> }
```

## Example: Running a Sub‑Agent

Call from the model or via a crafted tool call:

```json
{
  "type": "function",
  "function": {
    "name": "subagent_run",
    "arguments": "{\n  \"name\": \"docs-writer\",\n  \"task\": \"Write a guide for the new API\"\n}"
  }
}
```

The manager will emit start/end events and return a `FunctionCallOutput` containing a JSON `SubAgentResult` string with fields: `agent_name`, `task`, `success`, `output`, and optional `error`.

## Testing and Diagnostics

- Unit tests: agent parsing, tool schemas, and end‑to‑end runner behavior live in:
  - `core/src/openai_tools.rs` (schema tests)
  - `core/src/codex.rs` (manager tests for list/describe/run paths)
  - `core/src/rollout/tests.rs` (persistence of SubAgent events)
- Chat payload tests: `core/tests/chat_completions_payload.rs` validate reasoning anchoring and shell mapping (`tool_calls[0].type == "function"`, `name == "shell"`).

To enable verbose logs during a run, use tracing flags (see `docs/advanced.md`).

## Future Work

- Optional per‑agent model overrides (schema is present; integration TBD).
- Richer tool alias groups per agent.
- Surface reasoning summaries from sub‑agents in the main transcript (currently they are recorded only for debugging).

---

# Design critique and refactor opportunities

This section calls out places to further “Rust‑up” the design and improve long‑term maintainability.

## 1) Strongly‑typed identifiers and enums

- Today: agent names and tool names are `String`s. Allowlist items are also `String`s.
- Risk: typos drift silently; mapping tables must duplicate string literals.
- Direction: introduce small newtypes and enums:
  - `struct AgentName(String);` (Display/FromStr)
  - `enum BuiltinTool { Shell, LocalShell, ExecCommand, WriteStdin, WebSearch, Freeform(String) }`
  - Implement `From<&OpenAiTool> for BuiltinTool` and vice‑versa to centralize mapping.

Benefits: compile‑time coverage in match arms; fewer stringly‑typed branches.

## 2) Frontmatter parsing robustness

- Today: manual `---` scanning and splitting is simple and explicit.
- Risk: tricky edge cases (leading BOM, trailing whitespace, fenced code blocks preceding frontmatter).
- Direction: small parser helper crate or adopt a lightweight frontmatter parser; keep strict error messages. Preserve the current behavior (filename wins over frontmatter `name`).

## 3) Recursion control and policy surface

- Today: nested runs set `include_subagent_tools = false` to prevent recursion.
- Direction: add `depth: u8` to nested runner with a configurable max (e.g., `subagent_max_depth = 1`). Return a structured error when exceeded.

## 4) Structured outputs instead of JSON string payloads

- Today: `SubAgentResult` is returned as a JSON string inside `FunctionCallOutputPayload.content`.
- Direction: when emitting MCP responses or protocol events, prefer structured fields (e.g., `structured_content` where available) to avoid string‑of‑JSON round‑trips.

## 5) Error handling ergonomics

- Today: custom `CodexErr` is used; some call‑sites wrap IO errors with custom messages.
- Direction: ensure all sub‑agent code implements `thiserror` for `CodexErr` variants; consider `anyhow::Context` at boundaries for richer traces while keeping precise error types within the crate.

## 6) Allocation hygiene and data movement

- Hot paths build many `String`s; several `to_string()` calls could become `Cow<'_, str>` or borrowed `&str` where lifetimes are straightforward.
- Consider replacing `Vec<ResponseItem>` concatenations with small adapters or iterators where possible.

## 7) Test coverage and property tests

- Add tests for:
  - “Project overrides user” precedence
  - Tool allowlist alias matrix
  - Recursion prevention at depth > 1
  - Round‑trip of `SubAgentResult` through protocol structured fields
- Consider proptests for markdown/frontmatter edge cases.

## 8) Modularity and crate boundaries

- Consider extracting `agents` into a small `codex-agents` crate to enable reuse and faster unit builds, with a clear interface for discovery/parsing and execution.

## 9) Observability

- Add `tracing` spans around nested runs (`subagent.name`, `task`, tool counts, durations). Provide `RUST_LOG` examples in this doc for easier field debugging.

## 10) Configuration surface

- Add per‑agent model overrides (already modeled in args) into `NestedAgentRunner::run` with a safe allowlist.
- Expose `subagent_tools` toggle in profiles and document CLI overrides along with policy defaults.

