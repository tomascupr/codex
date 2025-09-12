# Sub‑Agents Implementation Plan (10x Rust TDD)

This is the detailed, test‑driven plan to implement Sub‑Agents in Codex CLI.
Work proceeds in milestones (M1 → M5). Each milestone includes tests first, then implementation, then formatting and per‑crate tests. When shared crates are touched (protocol/core), the full suite is run after per‑crate tests.

Status legend: [ ] pending, [x] done, [~] in progress

## M1 — Protocol + Tool Surface + Config Plumbing [x]

Scope:
- Protocol events for subagent start/end.
- New conversation param to include subagent tools.
- Tool definitions: subagent_list, subagent_describe, subagent_run.
- ToolsConfig flag to include subagent tools; feed through conversation/session creation.

TDD checklist:
- Protocol serde round‑trip tests for SubAgentStart/End.
- Ensure NewConversationParams supports include_subagent_tools.
- openai_tools schema tests: subagent_* tool presence/absence by flag; deterministic ordering.

Exit criteria:
- `cargo test -p codex-protocol` and `cargo test -p codex-core` pass. [done]
- `just fmt` clean; `just fix` succeeds. [done]

Notes:
- Added EventMsg::SubAgentStart/SubAgentEnd with serde tests.
- Added NewConversationParams.include_subagent_tools and plumbed through config + TUI/exec/mcp.
- Exposed subagent_* tools behind include_subagent_tools flag.

## M2 — Agent Authoring, Discovery, Allowlist [x]

Scope:
- `core/src/agents.rs`: `SubAgent`, `AgentRegistry`.
- YAML frontmatter parser with filename‑as‑canonical name.
- Discovery from `~/.codex/agents` and `<repo>/.codex/agents` with project‑overrides precedence.
- Allowlist filtering with alias expansion (shell/local_shell/exec_command/write_stdin group).

TDD checklist:
- Parser: missing frontmatter, empty body, description required, name mismatch warns.
- Discovery precedence test using temp dirs.
- Allowlist filtering and alias matrix tests.

Exit criteria:
- `cargo test -p codex-core` passes. [done]

Notes:
- Implemented `core/src/agents.rs` with frontmatter parser, directory loader, allowlist filtering with shell alias expansion, and registry precedence helpers. Added unit tests.

## M3 — Nested Runner + Manager + Loop Wiring [~]

Scope:
- `NestedAgentRunner` (isolated prompt, filtered tools, optional model override, no recursion).
- `SubAgentManager::{handle_subagent_list, handle_subagent_describe, handle_subagent_run}`.
- Integrate into `codex.rs` function call dispatch; emit SubAgentStart/End events.
- Recursion depth limit = 1.

TDD checklist:
- Function‑call routing tests: list/describe/run happy path + arg errors.
- E2E with SSE fixture: nested run aggregates assistant output; errors surface. [tool calls: deferred; see Notes]
- Recursion prevention test.

Exit criteria:
- `cargo test -p codex-core` passes; then run full suite. [done]

Notes:
- Implemented subagent_list, subagent_describe, and subagent_run handlers in core/src/codex.rs.
- Subagent runs use a nested prompt with the agent body + Task appended and a filtered tool set (allowlist + subagent tools pruned to prevent recursion).
- For now, nested runs stream via drain_to_completed (no tool calls executed inside subagent to avoid recursive async cycles). We will switch to a fully tooled nested loop in a follow-up patch (M3b) by refactoring try_run_turn to avoid self-recursive futures (e.g., via boxed futures or a dedicated runner that doesn’t re-enter handle_function_call).

## M4 — Rollout Persistence + TUI Surfacing [ ]

Scope:
- Persist start/end events; ordering with nested tool calls.
- TUI: minimal lines for start/end using ratatui Stylize helpers.

TDD checklist:
- Rollout JSONL contains SubAgentStart/End around nested execution.
- TUI snapshot test includes markers.

Exit criteria:
- `cargo test -p codex-core`, `cargo test -p codex-tui` pass; then full suite.

## M5 — Docs + Examples + Polish [ ]

Scope:
- Finalize `docs/subagents.md` with quickstart; add example `~/.codex/agents/*.md`.
- Config docs for toggle and precedence.

TDD checklist:
- Doc lint/build scripts (if any) succeed.

Exit criteria:
- Build, lint, tests all green.

---

## Parity Checklist with Claude Code

- Authoring via Markdown + YAML frontmatter (description, optional tools allowlist). [ ]
- Discovery from user and project scopes; project overrides user. [ ]
- Tool allowlist with alias groups for shell family. [ ]
- Tools exposed: subagent_list, subagent_describe, subagent_run. [ ]
- Nested run with isolated prompt and filtered tools. [ ]
- Recursion prevention via disabled subagents (depth limit 1). [ ]
- Optional per‑run model override. [ ]
- Local shell mapping to `function: shell` in Chat payloads. [ ]
- Start/End events persisted for audit/UI. [ ]
- Config toggle + MCP passthrough. [ ]

---

## Runbook per Milestone

1) Write tests first (protocol/core/tui as applicable).
2) Implement code to pass tests.
3) Run `just fmt`.
4) Run per‑crate tests for modified crates.
5) If modified shared crates (protocol/core), run `just fix` and `cargo test --all-features`.
6) Update this plan: mark milestone done and proceed.
