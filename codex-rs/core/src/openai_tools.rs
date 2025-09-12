use serde::Deserialize;
use serde::Serialize;
use serde_json::Value as JsonValue;
use serde_json::json;
use std::collections::BTreeMap;
use std::collections::HashMap;

use crate::model_family::ModelFamily;
use crate::plan_tool::PLAN_TOOL;
use crate::protocol::AskForApproval;
use crate::protocol::SandboxPolicy;
use crate::tool_apply_patch::ApplyPatchToolType;
use crate::tool_apply_patch::create_apply_patch_freeform_tool;
use crate::tool_apply_patch::create_apply_patch_json_tool;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ResponsesApiTool {
    pub(crate) name: String,
    pub(crate) description: String,
    /// TODO: Validation. When strict is set to true, the JSON schema,
    /// `required` and `additional_properties` must be present. All fields in
    /// `properties` must be present in `required`.
    pub(crate) strict: bool,
    pub(crate) parameters: JsonSchema,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FreeformTool {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) format: FreeformToolFormat,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FreeformToolFormat {
    pub(crate) r#type: String,
    pub(crate) syntax: String,
    pub(crate) definition: String,
}

/// When serialized as JSON, this produces a valid "Tool" in the OpenAI
/// Responses API.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "type")]
pub(crate) enum OpenAiTool {
    #[serde(rename = "function")]
    Function(ResponsesApiTool),
    #[serde(rename = "local_shell")]
    LocalShell {},
    // TODO: Understand why we get an error on web_search although the API docs say it's supported.
    // https://platform.openai.com/docs/guides/tools-web-search?api-mode=responses#:~:text=%7B%20type%3A%20%22web_search%22%20%7D%2C
    #[serde(rename = "web_search")]
    WebSearch {},
    #[serde(rename = "custom")]
    Freeform(FreeformTool),
}

#[derive(Debug, Clone)]
pub enum ConfigShellToolType {
    DefaultShell,
    ShellWithRequest { sandbox_policy: SandboxPolicy },
    LocalShell,
    StreamableShell,
}

#[derive(Debug, Clone)]
pub(crate) struct ToolsConfig {
    pub shell_type: ConfigShellToolType,
    pub plan_tool: bool,
    pub apply_patch_tool_type: Option<ApplyPatchToolType>,
    pub web_search_request: bool,
    pub include_view_image_tool: bool,
    pub include_subagent_tools: bool,
}

pub(crate) struct ToolsConfigParams<'a> {
    pub(crate) model_family: &'a ModelFamily,
    pub(crate) approval_policy: AskForApproval,
    pub(crate) sandbox_policy: SandboxPolicy,
    pub(crate) include_plan_tool: bool,
    pub(crate) include_apply_patch_tool: bool,
    pub(crate) include_web_search_request: bool,
    pub(crate) use_streamable_shell_tool: bool,
    pub(crate) include_view_image_tool: bool,
    pub(crate) include_subagent_tools: bool,
}

impl ToolsConfig {
    pub fn new(params: &ToolsConfigParams) -> Self {
        let ToolsConfigParams {
            model_family,
            approval_policy,
            sandbox_policy,
            include_plan_tool,
            include_apply_patch_tool,
            include_web_search_request,
            use_streamable_shell_tool,
            include_view_image_tool,
            include_subagent_tools,
        } = params;
        let mut shell_type = if *use_streamable_shell_tool {
            ConfigShellToolType::StreamableShell
        } else if model_family.uses_local_shell_tool {
            ConfigShellToolType::LocalShell
        } else {
            ConfigShellToolType::DefaultShell
        };
        if matches!(approval_policy, AskForApproval::OnRequest) && !use_streamable_shell_tool {
            shell_type = ConfigShellToolType::ShellWithRequest {
                sandbox_policy: sandbox_policy.clone(),
            }
        }

        let apply_patch_tool_type = match model_family.apply_patch_tool_type {
            Some(ApplyPatchToolType::Freeform) => Some(ApplyPatchToolType::Freeform),
            Some(ApplyPatchToolType::Function) => Some(ApplyPatchToolType::Function),
            None => {
                if *include_apply_patch_tool {
                    Some(ApplyPatchToolType::Freeform)
                } else {
                    None
                }
            }
        };

        Self {
            shell_type,
            plan_tool: *include_plan_tool,
            apply_patch_tool_type,
            web_search_request: *include_web_search_request,
            include_view_image_tool: *include_view_image_tool,
            include_subagent_tools: *include_subagent_tools,
        }
    }
}

/// Generic JSON‑Schema subset needed for our tool definitions
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub(crate) enum JsonSchema {
    Boolean {
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    String {
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    /// MCP schema allows "number" | "integer" for Number
    #[serde(alias = "integer")]
    Number {
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    Array {
        items: Box<JsonSchema>,

        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    Object {
        properties: BTreeMap<String, JsonSchema>,
        #[serde(skip_serializing_if = "Option::is_none")]
        required: Option<Vec<String>>,
        #[serde(
            rename = "additionalProperties",
            skip_serializing_if = "Option::is_none"
        )]
        additional_properties: Option<bool>,
    },
}

fn create_subagent_list_tool() -> OpenAiTool {
    // No properties needed - this tool takes no parameters
    let properties = BTreeMap::new();

    OpenAiTool::Function(ResponsesApiTool {
        name: "subagent_list".to_string(),
        description: "List available sub-agents with their names and descriptions".to_string(),
        strict: false,
        parameters: JsonSchema::Object {
            properties,
            required: Some(vec![]),
            additional_properties: Some(false),
        },
    })
}

fn create_subagent_describe_tool() -> OpenAiTool {
    let mut properties = BTreeMap::new();
    properties.insert(
        "name".to_string(),
        JsonSchema::String {
            description: Some("Name of the sub-agent to describe".to_string()),
        },
    );

    OpenAiTool::Function(ResponsesApiTool {
        name: "subagent_describe".to_string(),
        description:
            "Get detailed information about a specific sub-agent including tools and prompt"
                .to_string(),
        strict: false,
        parameters: JsonSchema::Object {
            properties,
            required: Some(vec!["name".to_string()]),
            additional_properties: Some(false),
        },
    })
}

fn create_subagent_run_tool() -> OpenAiTool {
    let mut properties = BTreeMap::new();
    properties.insert(
        "name".to_string(),
        JsonSchema::String {
            description: Some("Name of the sub-agent to run".to_string()),
        },
    );
    properties.insert(
        "task".to_string(),
        JsonSchema::String {
            description: Some("Task to execute with the sub-agent".to_string()),
        },
    );
    properties.insert(
        "model".to_string(),
        JsonSchema::String {
            description: Some("Optional model override for the sub-agent".to_string()),
        },
    );

    OpenAiTool::Function(ResponsesApiTool {
        name: "subagent_run".to_string(),
        description: "Execute a sub-agent with a specific task".to_string(),
        strict: false,
        parameters: JsonSchema::Object {
            properties,
            required: Some(vec!["name".to_string(), "task".to_string()]),
            additional_properties: Some(false),
        },
    })
}

fn create_shell_tool() -> OpenAiTool {
    let mut properties = BTreeMap::new();
    properties.insert(
        "command".to_string(),
        JsonSchema::Array {
            items: Box::new(JsonSchema::String { description: None }),
            description: Some("The command to execute".to_string()),
        },
    );
    properties.insert(
        "workdir".to_string(),
        JsonSchema::String {
            description: Some("The working directory to execute the command in".to_string()),
        },
    );
    properties.insert(
        "timeout_ms".to_string(),
        JsonSchema::Number {
            description: Some("The timeout for the command in milliseconds".to_string()),
        },
    );

    OpenAiTool::Function(ResponsesApiTool {
        name: "shell".to_string(),
        description: "Runs a shell command and returns its output".to_string(),
        strict: false,
        parameters: JsonSchema::Object {
            properties,
            required: Some(vec!["command".to_string()]),
            additional_properties: Some(false),
        },
    })
}

fn create_shell_tool_for_sandbox(sandbox_policy: &SandboxPolicy) -> OpenAiTool {
    let mut properties = BTreeMap::new();
    properties.insert(
        "command".to_string(),
        JsonSchema::Array {
            items: Box::new(JsonSchema::String { description: None }),
            description: Some("The command to execute".to_string()),
        },
    );
    properties.insert(
        "workdir".to_string(),
        JsonSchema::String {
            description: Some("The working directory to execute the command in".to_string()),
        },
    );
    properties.insert(
        "timeout_ms".to_string(),
        JsonSchema::Number {
            description: Some("The timeout for the command in milliseconds".to_string()),
        },
    );

    if matches!(sandbox_policy, SandboxPolicy::WorkspaceWrite { .. }) {
        properties.insert(
        "with_escalated_permissions".to_string(),
        JsonSchema::Boolean {
            description: Some("Whether to request escalated permissions. Set to true if command needs to be run without sandbox restrictions".to_string()),
        },
    );
        properties.insert(
        "justification".to_string(),
        JsonSchema::String {
            description: Some("Only set if with_escalated_permissions is true. 1-sentence explanation of why we want to run this command.".to_string()),
        },
    );
    }

    let description = match sandbox_policy {
        SandboxPolicy::WorkspaceWrite {
            network_access,
            writable_roots,
            ..
        } => {
            format!(
                r#"
The shell tool is used to execute shell commands.
- When invoking the shell tool, your call will be running in a sandbox, and some shell commands will require escalated privileges:
  - Types of actions that require escalated privileges:
    - Writing files other than those in the writable roots
      - writable roots:
{}{}
  - Examples of commands that require escalated privileges:
    - git commit
    - npm install or pnpm install
    - cargo build
    - cargo test
- When invoking a command that will require escalated privileges:
  - Provide the with_escalated_permissions parameter with the boolean value true
  - Include a short, 1 sentence explanation for why we need to run with_escalated_permissions in the justification parameter."#,
                writable_roots.iter().map(|wr| format!("        - {}", wr.to_string_lossy())).collect::<Vec<String>>().join("\n"),
                if !network_access {
                    "\n    - Commands that require network access\n"
                } else {
                    ""
                }
            )
        }
        SandboxPolicy::DangerFullAccess => {
            "Runs a shell command and returns its output.".to_string()
        }
        SandboxPolicy::ReadOnly => {
            r#"
The shell tool is used to execute shell commands.
- When invoking the shell tool, your call will be running in a sandbox, and some shell commands (including apply_patch) will require escalated permissions:
  - Types of actions that require escalated privileges:
    - Writing files
    - Applying patches
  - Examples of commands that require escalated privileges:
    - apply_patch
    - git commit
    - npm install or pnpm install
    - cargo build
    - cargo test
- When invoking a command that will require escalated privileges:
  - Provide the with_escalated_permissions parameter with the boolean value true
  - Include a short, 1 sentence explanation for why we need to run with_escalated_permissions in the justification parameter"#.to_string()
        }
    };

    OpenAiTool::Function(ResponsesApiTool {
        name: "shell".to_string(),
        description,
        strict: false,
        parameters: JsonSchema::Object {
            properties,
            required: Some(vec!["command".to_string()]),
            additional_properties: Some(false),
        },
    })
}

fn create_view_image_tool() -> OpenAiTool {
    // Support only local filesystem path.
    let mut properties = BTreeMap::new();
    properties.insert(
        "path".to_string(),
        JsonSchema::String {
            description: Some("Local filesystem path to an image file".to_string()),
        },
    );

    OpenAiTool::Function(ResponsesApiTool {
        name: "view_image".to_string(),
        description:
            "Attach a local image (by filesystem path) to the conversation context for this turn."
                .to_string(),
        strict: false,
        parameters: JsonSchema::Object {
            properties,
            required: Some(vec!["path".to_string()]),
            additional_properties: Some(false),
        },
    })
}
/// TODO(dylan): deprecate once we get rid of json tool
#[derive(Serialize, Deserialize)]
pub(crate) struct ApplyPatchToolArgs {
    pub(crate) input: String,
}

/// Arguments for subagent_list tool - no parameters needed
#[derive(Serialize, Deserialize)]
pub(crate) struct SubAgentListArgs {
    // No fields needed - this is an empty struct for consistency
}

/// Arguments for subagent_describe tool
#[derive(Serialize, Deserialize)]
pub(crate) struct SubAgentDescribeArgs {
    /// Name of the sub-agent to describe
    pub(crate) name: String,
}

/// Arguments for subagent_run tool
#[derive(Serialize, Deserialize)]
pub(crate) struct SubAgentRunArgs {
    /// Name of the sub-agent to run
    pub(crate) name: String,
    /// Task to execute with the sub-agent
    pub(crate) task: String,
    /// Optional model override for the sub-agent
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) model: Option<String>,
}

/// Returns JSON values that are compatible with Function Calling in the
/// Responses API:
/// https://platform.openai.com/docs/guides/function-calling?api-mode=responses
pub fn create_tools_json_for_responses_api(
    tools: &Vec<OpenAiTool>,
) -> crate::error::Result<Vec<serde_json::Value>> {
    let mut tools_json = Vec::new();

    for tool in tools {
        let json = serde_json::to_value(tool)?;
        tools_json.push(json);
    }

    Ok(tools_json)
}
/// Returns JSON values that are compatible with Function Calling in the
/// Chat Completions API:
/// https://platform.openai.com/docs/guides/function-calling?api-mode=chat
pub(crate) fn create_tools_json_for_chat_completions_api(
    tools: &Vec<OpenAiTool>,
) -> crate::error::Result<Vec<serde_json::Value>> {
    // We start with the JSON for the Responses API and than rewrite it to match
    // the chat completions tool call format.
    let responses_api_tools_json = create_tools_json_for_responses_api(tools)?;
    let tools_json = responses_api_tools_json
        .into_iter()
        .filter_map(|mut tool| {
            if tool.get("type") != Some(&serde_json::Value::String("function".to_string())) {
                return None;
            }

            if let Some(map) = tool.as_object_mut() {
                // Remove "type" field as it is not needed in chat completions.
                map.remove("type");
                Some(json!({
                    "type": "function",
                    "function": map,
                }))
            } else {
                None
            }
        })
        .collect::<Vec<serde_json::Value>>();
    Ok(tools_json)
}

pub(crate) fn mcp_tool_to_openai_tool(
    fully_qualified_name: String,
    tool: mcp_types::Tool,
) -> Result<ResponsesApiTool, serde_json::Error> {
    let mcp_types::Tool {
        description,
        mut input_schema,
        ..
    } = tool;

    // OpenAI models mandate the "properties" field in the schema. The Agents
    // SDK fixed this by inserting an empty object for "properties" if it is not
    // already present https://github.com/openai/openai-agents-python/issues/449
    // so here we do the same.
    if input_schema.properties.is_none() {
        input_schema.properties = Some(serde_json::Value::Object(serde_json::Map::new()));
    }

    // Serialize to a raw JSON value so we can sanitize schemas coming from MCP
    // servers. Some servers omit the top-level or nested `type` in JSON
    // Schemas (e.g. using enum/anyOf), or use unsupported variants like
    // `integer`. Our internal JsonSchema is a small subset and requires
    // `type`, so we coerce/sanitize here for compatibility.
    let mut serialized_input_schema = serde_json::to_value(input_schema)?;
    sanitize_json_schema(&mut serialized_input_schema);
    let input_schema = serde_json::from_value::<JsonSchema>(serialized_input_schema)?;

    Ok(ResponsesApiTool {
        name: fully_qualified_name,
        description: description.unwrap_or_default(),
        strict: false,
        parameters: input_schema,
    })
}

/// Sanitize a JSON Schema (as serde_json::Value) so it can fit our limited
/// JsonSchema enum. This function:
/// - Ensures every schema object has a "type". If missing, infers it from
///   common keywords (properties => object, items => array, enum/const/format => string)
///   and otherwise defaults to "string".
/// - Fills required child fields (e.g. array items, object properties) with
///   permissive defaults when absent.
fn sanitize_json_schema(value: &mut JsonValue) {
    match value {
        JsonValue::Bool(_) => {
            // JSON Schema boolean form: true/false. Coerce to an accept-all string.
            *value = json!({ "type": "string" });
        }
        JsonValue::Array(arr) => {
            for v in arr.iter_mut() {
                sanitize_json_schema(v);
            }
        }
        JsonValue::Object(map) => {
            // First, recursively sanitize known nested schema holders
            if let Some(props) = map.get_mut("properties")
                && let Some(props_map) = props.as_object_mut()
            {
                for (_k, v) in props_map.iter_mut() {
                    sanitize_json_schema(v);
                }
            }
            if let Some(items) = map.get_mut("items") {
                sanitize_json_schema(items);
            }
            // Some schemas use oneOf/anyOf/allOf - sanitize their entries
            for combiner in ["oneOf", "anyOf", "allOf", "prefixItems"] {
                if let Some(v) = map.get_mut(combiner) {
                    sanitize_json_schema(v);
                }
            }

            // Normalize/ensure type
            let mut ty = map
                .get("type")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            // If type is an array (union), pick first supported; else leave to inference
            if ty.is_none()
                && let Some(JsonValue::Array(types)) = map.get("type")
            {
                for t in types {
                    if let Some(tt) = t.as_str()
                        && matches!(
                            tt,
                            "object" | "array" | "string" | "number" | "integer" | "boolean"
                        )
                    {
                        ty = Some(tt.to_string());
                        break;
                    }
                }
            }

            // Infer type if still missing
            if ty.is_none() {
                if map.contains_key("properties")
                    || map.contains_key("required")
                    || map.contains_key("additionalProperties")
                {
                    ty = Some("object".to_string());
                } else if map.contains_key("items") || map.contains_key("prefixItems") {
                    ty = Some("array".to_string());
                } else if map.contains_key("enum")
                    || map.contains_key("const")
                    || map.contains_key("format")
                {
                    ty = Some("string".to_string());
                } else if map.contains_key("minimum")
                    || map.contains_key("maximum")
                    || map.contains_key("exclusiveMinimum")
                    || map.contains_key("exclusiveMaximum")
                    || map.contains_key("multipleOf")
                {
                    ty = Some("number".to_string());
                }
            }
            // If we still couldn't infer, default to string
            let ty = ty.unwrap_or_else(|| "string".to_string());
            map.insert("type".to_string(), JsonValue::String(ty.to_string()));

            // Ensure object schemas have properties map
            if ty == "object" {
                if !map.contains_key("properties") {
                    map.insert(
                        "properties".to_string(),
                        JsonValue::Object(serde_json::Map::new()),
                    );
                }
                // If additionalProperties is an object schema, sanitize it too.
                // Leave booleans as-is, since JSON Schema allows boolean here.
                if let Some(ap) = map.get_mut("additionalProperties") {
                    let is_bool = matches!(ap, JsonValue::Bool(_));
                    if !is_bool {
                        sanitize_json_schema(ap);
                    }
                }
            }

            // Ensure array schemas have items
            if ty == "array" && !map.contains_key("items") {
                map.insert("items".to_string(), json!({ "type": "string" }));
            }
        }
        _ => {}
    }
}

/// Returns a list of OpenAiTools based on the provided config and MCP tools.
/// Note that the keys of mcp_tools should be fully qualified names. See
/// [`McpConnectionManager`] for more details.
pub(crate) fn get_openai_tools(
    config: &ToolsConfig,
    mcp_tools: Option<HashMap<String, mcp_types::Tool>>,
) -> Vec<OpenAiTool> {
    let mut tools: Vec<OpenAiTool> = Vec::new();

    match &config.shell_type {
        ConfigShellToolType::DefaultShell => {
            tools.push(create_shell_tool());
        }
        ConfigShellToolType::ShellWithRequest { sandbox_policy } => {
            tools.push(create_shell_tool_for_sandbox(sandbox_policy));
        }
        ConfigShellToolType::LocalShell => {
            tools.push(OpenAiTool::LocalShell {});
        }
        ConfigShellToolType::StreamableShell => {
            tools.push(OpenAiTool::Function(
                crate::exec_command::create_exec_command_tool_for_responses_api(),
            ));
            tools.push(OpenAiTool::Function(
                crate::exec_command::create_write_stdin_tool_for_responses_api(),
            ));
        }
    }

    if config.plan_tool {
        tools.push(PLAN_TOOL.clone());
    }

    if let Some(apply_patch_tool_type) = &config.apply_patch_tool_type {
        match apply_patch_tool_type {
            ApplyPatchToolType::Freeform => {
                tools.push(create_apply_patch_freeform_tool());
            }
            ApplyPatchToolType::Function => {
                tools.push(create_apply_patch_json_tool());
            }
        }
    }

    if config.web_search_request {
        tools.push(OpenAiTool::WebSearch {});
    }

    // Include the view_image tool so the agent can attach images to context.
    if config.include_view_image_tool {
        tools.push(create_view_image_tool());
    }

    // Include sub-agent tools when enabled
    if config.include_subagent_tools {
        tools.push(create_subagent_list_tool());
        tools.push(create_subagent_describe_tool());
        tools.push(create_subagent_run_tool());
    }

    if let Some(mcp_tools) = mcp_tools {
        // Ensure deterministic ordering to maximize prompt cache hits.
        // HashMap iteration order is non-deterministic, so sort by fully-qualified tool name.
        let mut entries: Vec<(String, mcp_types::Tool)> = mcp_tools.into_iter().collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));

        for (name, tool) in entries.into_iter() {
            match mcp_tool_to_openai_tool(name.clone(), tool.clone()) {
                Ok(converted_tool) => tools.push(OpenAiTool::Function(converted_tool)),
                Err(e) => {
                    tracing::error!("Failed to convert {name:?} MCP tool to OpenAI tool: {e:?}");
                }
            }
        }
    }

    tools
}

#[cfg(test)]
mod tests {
    use crate::model_family::find_family_for_model;
    use mcp_types::ToolInputSchema;
    use pretty_assertions::assert_eq;

    use super::*;

    fn assert_eq_tool_names(tools: &[OpenAiTool], expected_names: &[&str]) {
        let tool_names = tools
            .iter()
            .map(|tool| match tool {
                OpenAiTool::Function(ResponsesApiTool { name, .. }) => name,
                OpenAiTool::LocalShell {} => "local_shell",
                OpenAiTool::WebSearch {} => "web_search",
                OpenAiTool::Freeform(FreeformTool { name, .. }) => name,
            })
            .collect::<Vec<_>>();

        assert_eq!(
            tool_names.len(),
            expected_names.len(),
            "tool_name mismatch, {tool_names:?}, {expected_names:?}",
        );
        for (name, expected_name) in tool_names.iter().zip(expected_names.iter()) {
            assert_eq!(
                name, expected_name,
                "tool_name mismatch, {name:?}, {expected_name:?}"
            );
        }
    }

    #[test]
    fn test_get_openai_tools() {
        let model_family = find_family_for_model("codex-mini-latest")
            .expect("codex-mini-latest should be a valid model family");
        let config = ToolsConfig::new(&ToolsConfigParams {
            model_family: &model_family,
            approval_policy: AskForApproval::Never,
            sandbox_policy: SandboxPolicy::ReadOnly,
            include_plan_tool: true,
            include_apply_patch_tool: false,
            include_web_search_request: true,
            use_streamable_shell_tool: false,
            include_view_image_tool: true,
            include_subagent_tools: false,
        });
        let tools = get_openai_tools(&config, Some(HashMap::new()));

        assert_eq_tool_names(
            &tools,
            &["local_shell", "update_plan", "web_search", "view_image"],
        );
    }

    #[test]
    fn test_get_openai_tools_default_shell() {
        let model_family = find_family_for_model("o3").expect("o3 should be a valid model family");
        let config = ToolsConfig::new(&ToolsConfigParams {
            model_family: &model_family,
            approval_policy: AskForApproval::Never,
            sandbox_policy: SandboxPolicy::ReadOnly,
            include_plan_tool: true,
            include_apply_patch_tool: false,
            include_web_search_request: true,
            use_streamable_shell_tool: false,
            include_view_image_tool: true,
            include_subagent_tools: false,
        });
        let tools = get_openai_tools(&config, Some(HashMap::new()));

        assert_eq_tool_names(
            &tools,
            &["shell", "update_plan", "web_search", "view_image"],
        );
    }

    #[test]
    fn test_get_openai_tools_mcp_tools() {
        let model_family = find_family_for_model("o3").expect("o3 should be a valid model family");
        let config = ToolsConfig::new(&ToolsConfigParams {
            model_family: &model_family,
            approval_policy: AskForApproval::Never,
            sandbox_policy: SandboxPolicy::ReadOnly,
            include_plan_tool: false,
            include_apply_patch_tool: false,
            include_web_search_request: true,
            use_streamable_shell_tool: false,
            include_view_image_tool: true,
            include_subagent_tools: false,
        });
        let tools = get_openai_tools(
            &config,
            Some(HashMap::from([(
                "test_server/do_something_cool".to_string(),
                mcp_types::Tool {
                    name: "do_something_cool".to_string(),
                    input_schema: ToolInputSchema {
                        properties: Some(serde_json::json!({
                            "string_argument": {
                                "type": "string",
                            },
                            "number_argument": {
                                "type": "number",
                            },
                            "object_argument": {
                                "type": "object",
                                "properties": {
                                    "string_property": { "type": "string" },
                                    "number_property": { "type": "number" },
                                },
                                "required": [
                                    "string_property",
                                    "number_property",
                                ],
                                "additionalProperties": Some(false),
                            },
                        })),
                        required: None,
                        r#type: "object".to_string(),
                    },
                    output_schema: None,
                    title: None,
                    annotations: None,
                    description: Some("Do something cool".to_string()),
                },
            )])),
        );

        assert_eq_tool_names(
            &tools,
            &[
                "shell",
                "web_search",
                "view_image",
                "test_server/do_something_cool",
            ],
        );

        assert_eq!(
            tools[3],
            OpenAiTool::Function(ResponsesApiTool {
                name: "test_server/do_something_cool".to_string(),
                parameters: JsonSchema::Object {
                    properties: BTreeMap::from([
                        (
                            "string_argument".to_string(),
                            JsonSchema::String { description: None }
                        ),
                        (
                            "number_argument".to_string(),
                            JsonSchema::Number { description: None }
                        ),
                        (
                            "object_argument".to_string(),
                            JsonSchema::Object {
                                properties: BTreeMap::from([
                                    (
                                        "string_property".to_string(),
                                        JsonSchema::String { description: None }
                                    ),
                                    (
                                        "number_property".to_string(),
                                        JsonSchema::Number { description: None }
                                    ),
                                ]),
                                required: Some(vec![
                                    "string_property".to_string(),
                                    "number_property".to_string(),
                                ]),
                                additional_properties: Some(false),
                            },
                        ),
                    ]),
                    required: None,
                    additional_properties: None,
                },
                description: "Do something cool".to_string(),
                strict: false,
            })
        );
    }

    #[test]
    fn test_get_openai_tools_mcp_tools_sorted_by_name() {
        let model_family = find_family_for_model("o3").expect("o3 should be a valid model family");
        let config = ToolsConfig::new(&ToolsConfigParams {
            model_family: &model_family,
            approval_policy: AskForApproval::Never,
            sandbox_policy: SandboxPolicy::ReadOnly,
            include_plan_tool: false,
            include_apply_patch_tool: false,
            include_web_search_request: false,
            use_streamable_shell_tool: false,
            include_view_image_tool: true,
            include_subagent_tools: false,
        });

        // Intentionally construct a map with keys that would sort alphabetically.
        let tools_map: HashMap<String, mcp_types::Tool> = HashMap::from([
            (
                "test_server/do".to_string(),
                mcp_types::Tool {
                    name: "a".to_string(),
                    input_schema: ToolInputSchema {
                        properties: Some(serde_json::json!({})),
                        required: None,
                        r#type: "object".to_string(),
                    },
                    output_schema: None,
                    title: None,
                    annotations: None,
                    description: Some("a".to_string()),
                },
            ),
            (
                "test_server/something".to_string(),
                mcp_types::Tool {
                    name: "b".to_string(),
                    input_schema: ToolInputSchema {
                        properties: Some(serde_json::json!({})),
                        required: None,
                        r#type: "object".to_string(),
                    },
                    output_schema: None,
                    title: None,
                    annotations: None,
                    description: Some("b".to_string()),
                },
            ),
            (
                "test_server/cool".to_string(),
                mcp_types::Tool {
                    name: "c".to_string(),
                    input_schema: ToolInputSchema {
                        properties: Some(serde_json::json!({})),
                        required: None,
                        r#type: "object".to_string(),
                    },
                    output_schema: None,
                    title: None,
                    annotations: None,
                    description: Some("c".to_string()),
                },
            ),
        ]);

        let tools = get_openai_tools(&config, Some(tools_map));
        // Expect shell first, followed by MCP tools sorted by fully-qualified name.
        assert_eq_tool_names(
            &tools,
            &[
                "shell",
                "view_image",
                "test_server/cool",
                "test_server/do",
                "test_server/something",
            ],
        );
    }

    #[test]
    fn test_mcp_tool_property_missing_type_defaults_to_string() {
        let model_family = find_family_for_model("o3").expect("o3 should be a valid model family");
        let config = ToolsConfig::new(&ToolsConfigParams {
            model_family: &model_family,
            approval_policy: AskForApproval::Never,
            sandbox_policy: SandboxPolicy::ReadOnly,
            include_plan_tool: false,
            include_apply_patch_tool: false,
            include_web_search_request: true,
            use_streamable_shell_tool: false,
            include_view_image_tool: true,
            include_subagent_tools: false,
        });

        let tools = get_openai_tools(
            &config,
            Some(HashMap::from([(
                "dash/search".to_string(),
                mcp_types::Tool {
                    name: "search".to_string(),
                    input_schema: ToolInputSchema {
                        properties: Some(serde_json::json!({
                            "query": {
                                "description": "search query"
                            }
                        })),
                        required: None,
                        r#type: "object".to_string(),
                    },
                    output_schema: None,
                    title: None,
                    annotations: None,
                    description: Some("Search docs".to_string()),
                },
            )])),
        );

        assert_eq_tool_names(
            &tools,
            &["shell", "web_search", "view_image", "dash/search"],
        );

        assert_eq!(
            tools[3],
            OpenAiTool::Function(ResponsesApiTool {
                name: "dash/search".to_string(),
                parameters: JsonSchema::Object {
                    properties: BTreeMap::from([(
                        "query".to_string(),
                        JsonSchema::String {
                            description: Some("search query".to_string())
                        }
                    )]),
                    required: None,
                    additional_properties: None,
                },
                description: "Search docs".to_string(),
                strict: false,
            })
        );
    }

    #[test]
    fn test_mcp_tool_integer_normalized_to_number() {
        let model_family = find_family_for_model("o3").expect("o3 should be a valid model family");
        let config = ToolsConfig::new(&ToolsConfigParams {
            model_family: &model_family,
            approval_policy: AskForApproval::Never,
            sandbox_policy: SandboxPolicy::ReadOnly,
            include_plan_tool: false,
            include_apply_patch_tool: false,
            include_web_search_request: true,
            use_streamable_shell_tool: false,
            include_view_image_tool: true,
            include_subagent_tools: false,
        });

        let tools = get_openai_tools(
            &config,
            Some(HashMap::from([(
                "dash/paginate".to_string(),
                mcp_types::Tool {
                    name: "paginate".to_string(),
                    input_schema: ToolInputSchema {
                        properties: Some(serde_json::json!({
                            "page": { "type": "integer" }
                        })),
                        required: None,
                        r#type: "object".to_string(),
                    },
                    output_schema: None,
                    title: None,
                    annotations: None,
                    description: Some("Pagination".to_string()),
                },
            )])),
        );

        assert_eq_tool_names(
            &tools,
            &["shell", "web_search", "view_image", "dash/paginate"],
        );
        assert_eq!(
            tools[3],
            OpenAiTool::Function(ResponsesApiTool {
                name: "dash/paginate".to_string(),
                parameters: JsonSchema::Object {
                    properties: BTreeMap::from([(
                        "page".to_string(),
                        JsonSchema::Number { description: None }
                    )]),
                    required: None,
                    additional_properties: None,
                },
                description: "Pagination".to_string(),
                strict: false,
            })
        );
    }

    #[test]
    fn test_mcp_tool_array_without_items_gets_default_string_items() {
        let model_family = find_family_for_model("o3").expect("o3 should be a valid model family");
        let config = ToolsConfig::new(&ToolsConfigParams {
            model_family: &model_family,
            approval_policy: AskForApproval::Never,
            sandbox_policy: SandboxPolicy::ReadOnly,
            include_plan_tool: false,
            include_apply_patch_tool: false,
            include_web_search_request: true,
            use_streamable_shell_tool: false,
            include_view_image_tool: true,
            include_subagent_tools: false,
        });

        let tools = get_openai_tools(
            &config,
            Some(HashMap::from([(
                "dash/tags".to_string(),
                mcp_types::Tool {
                    name: "tags".to_string(),
                    input_schema: ToolInputSchema {
                        properties: Some(serde_json::json!({
                            "tags": { "type": "array" }
                        })),
                        required: None,
                        r#type: "object".to_string(),
                    },
                    output_schema: None,
                    title: None,
                    annotations: None,
                    description: Some("Tags".to_string()),
                },
            )])),
        );

        assert_eq_tool_names(&tools, &["shell", "web_search", "view_image", "dash/tags"]);
        assert_eq!(
            tools[3],
            OpenAiTool::Function(ResponsesApiTool {
                name: "dash/tags".to_string(),
                parameters: JsonSchema::Object {
                    properties: BTreeMap::from([(
                        "tags".to_string(),
                        JsonSchema::Array {
                            items: Box::new(JsonSchema::String { description: None }),
                            description: None
                        }
                    )]),
                    required: None,
                    additional_properties: None,
                },
                description: "Tags".to_string(),
                strict: false,
            })
        );
    }

    #[test]
    fn test_mcp_tool_anyof_defaults_to_string() {
        let model_family = find_family_for_model("o3").expect("o3 should be a valid model family");
        let config = ToolsConfig::new(&ToolsConfigParams {
            model_family: &model_family,
            approval_policy: AskForApproval::Never,
            sandbox_policy: SandboxPolicy::ReadOnly,
            include_plan_tool: false,
            include_apply_patch_tool: false,
            include_web_search_request: true,
            use_streamable_shell_tool: false,
            include_view_image_tool: true,
            include_subagent_tools: false,
        });

        let tools = get_openai_tools(
            &config,
            Some(HashMap::from([(
                "dash/value".to_string(),
                mcp_types::Tool {
                    name: "value".to_string(),
                    input_schema: ToolInputSchema {
                        properties: Some(serde_json::json!({
                            "value": { "anyOf": [ { "type": "string" }, { "type": "number" } ] }
                        })),
                        required: None,
                        r#type: "object".to_string(),
                    },
                    output_schema: None,
                    title: None,
                    annotations: None,
                    description: Some("AnyOf Value".to_string()),
                },
            )])),
        );

        assert_eq_tool_names(&tools, &["shell", "web_search", "view_image", "dash/value"]);
        assert_eq!(
            tools[3],
            OpenAiTool::Function(ResponsesApiTool {
                name: "dash/value".to_string(),
                parameters: JsonSchema::Object {
                    properties: BTreeMap::from([(
                        "value".to_string(),
                        JsonSchema::String { description: None }
                    )]),
                    required: None,
                    additional_properties: None,
                },
                description: "AnyOf Value".to_string(),
                strict: false,
            })
        );
    }

    #[test]
    fn test_shell_tool_for_sandbox_workspace_write() {
        let sandbox_policy = SandboxPolicy::WorkspaceWrite {
            writable_roots: vec!["workspace".into()],
            network_access: false,
            exclude_tmpdir_env_var: false,
            exclude_slash_tmp: false,
        };
        let tool = super::create_shell_tool_for_sandbox(&sandbox_policy);
        let OpenAiTool::Function(ResponsesApiTool {
            description, name, ..
        }) = &tool
        else {
            panic!("expected function tool");
        };
        assert_eq!(name, "shell");

        let expected = r#"
The shell tool is used to execute shell commands.
- When invoking the shell tool, your call will be running in a sandbox, and some shell commands will require escalated privileges:
  - Types of actions that require escalated privileges:
    - Writing files other than those in the writable roots
      - writable roots:
        - workspace
    - Commands that require network access

  - Examples of commands that require escalated privileges:
    - git commit
    - npm install or pnpm install
    - cargo build
    - cargo test
- When invoking a command that will require escalated privileges:
  - Provide the with_escalated_permissions parameter with the boolean value true
  - Include a short, 1 sentence explanation for why we need to run with_escalated_permissions in the justification parameter."#;
        assert_eq!(description, expected);
    }

    #[test]
    fn test_shell_tool_for_sandbox_readonly() {
        let tool = super::create_shell_tool_for_sandbox(&SandboxPolicy::ReadOnly);
        let OpenAiTool::Function(ResponsesApiTool {
            description, name, ..
        }) = &tool
        else {
            panic!("expected function tool");
        };
        assert_eq!(name, "shell");

        let expected = r#"
The shell tool is used to execute shell commands.
- When invoking the shell tool, your call will be running in a sandbox, and some shell commands (including apply_patch) will require escalated permissions:
  - Types of actions that require escalated privileges:
    - Writing files
    - Applying patches
  - Examples of commands that require escalated privileges:
    - apply_patch
    - git commit
    - npm install or pnpm install
    - cargo build
    - cargo test
- When invoking a command that will require escalated privileges:
  - Provide the with_escalated_permissions parameter with the boolean value true
  - Include a short, 1 sentence explanation for why we need to run with_escalated_permissions in the justification parameter"#;
        assert_eq!(description, expected);
    }

    #[test]
    fn test_shell_tool_for_sandbox_danger_full_access() {
        let tool = super::create_shell_tool_for_sandbox(&SandboxPolicy::DangerFullAccess);
        let OpenAiTool::Function(ResponsesApiTool {
            description, name, ..
        }) = &tool
        else {
            panic!("expected function tool");
        };
        assert_eq!(name, "shell");

        assert_eq!(description, "Runs a shell command and returns its output.");
    }

    #[test]
    fn test_get_openai_tools_with_subagent_tools() {
        let model_family = find_family_for_model("o3").expect("o3 should be a valid model family");
        let config = ToolsConfig::new(&ToolsConfigParams {
            model_family: &model_family,
            approval_policy: AskForApproval::Never,
            sandbox_policy: SandboxPolicy::ReadOnly,
            include_plan_tool: false,
            include_apply_patch_tool: false,
            include_web_search_request: false,
            use_streamable_shell_tool: false,
            include_view_image_tool: false,
            include_subagent_tools: true,
        });
        let tools = get_openai_tools(&config, Some(HashMap::new()));

        assert_eq_tool_names(
            &tools,
            &[
                "shell",
                "subagent_list",
                "subagent_describe",
                "subagent_run",
            ],
        );
    }

    #[test]
    fn test_subagent_tool_schemas() {
        // Test subagent_list tool
        let list_tool = create_subagent_list_tool();
        if let OpenAiTool::Function(ResponsesApiTool {
            name,
            description,
            parameters,
            ..
        }) = &list_tool
        {
            assert_eq!(name, "subagent_list");
            assert_eq!(
                description,
                "List available sub-agents with their names and descriptions"
            );
            if let JsonSchema::Object { required, .. } = parameters {
                assert_eq!(required, &Some(vec![]));
            } else {
                panic!("Expected Object schema for subagent_list");
            }
        } else {
            panic!("Expected Function tool for subagent_list");
        }

        // Test subagent_describe tool
        let describe_tool = create_subagent_describe_tool();
        if let OpenAiTool::Function(ResponsesApiTool {
            name,
            description,
            parameters,
            ..
        }) = &describe_tool
        {
            assert_eq!(name, "subagent_describe");
            assert_eq!(
                description,
                "Get detailed information about a specific sub-agent including tools and prompt"
            );
            if let JsonSchema::Object {
                required,
                properties,
                ..
            } = parameters
            {
                assert_eq!(required, &Some(vec!["name".to_string()]));
                assert!(properties.contains_key("name"));
            } else {
                panic!("Expected Object schema for subagent_describe");
            }
        } else {
            panic!("Expected Function tool for subagent_describe");
        }

        // Test subagent_run tool
        let run_tool = create_subagent_run_tool();
        if let OpenAiTool::Function(ResponsesApiTool {
            name,
            description,
            parameters,
            ..
        }) = &run_tool
        {
            assert_eq!(name, "subagent_run");
            assert_eq!(description, "Execute a sub-agent with a specific task");
            if let JsonSchema::Object {
                required,
                properties,
                ..
            } = parameters
            {
                assert_eq!(
                    required,
                    &Some(vec!["name".to_string(), "task".to_string()])
                );
                assert!(properties.contains_key("name"));
                assert!(properties.contains_key("task"));
                assert!(properties.contains_key("model")); // Optional parameter should still be in properties
            } else {
                panic!("Expected Object schema for subagent_run");
            }
        } else {
            panic!("Expected Function tool for subagent_run");
        }
    }

    #[test]
    fn test_tools_config_subagent_tools_enabled() {
        let model_family = find_family_for_model("o3").expect("o3 should be a valid model family");
        let config = ToolsConfig::new(&ToolsConfigParams {
            model_family: &model_family,
            approval_policy: AskForApproval::Never,
            sandbox_policy: SandboxPolicy::ReadOnly,
            include_plan_tool: false,
            include_apply_patch_tool: false,
            include_web_search_request: false,
            use_streamable_shell_tool: false,
            include_view_image_tool: false,
            include_subagent_tools: true,
        });

        assert!(config.include_subagent_tools);

        let tools = get_openai_tools(&config, None);
        let tool_names: Vec<String> = tools
            .iter()
            .map(|tool| match tool {
                OpenAiTool::Function(f) => f.name.clone(),
                OpenAiTool::LocalShell {} => "local_shell".to_string(),
                OpenAiTool::WebSearch {} => "web_search".to_string(),
                OpenAiTool::Freeform(f) => f.name.clone(),
            })
            .collect();

        assert!(tool_names.contains(&"subagent_list".to_string()));
        assert!(tool_names.contains(&"subagent_describe".to_string()));
        assert!(tool_names.contains(&"subagent_run".to_string()));
    }

    #[test]
    fn test_tools_config_subagent_tools_disabled() {
        let model_family = find_family_for_model("o3").expect("o3 should be a valid model family");
        let config = ToolsConfig::new(&ToolsConfigParams {
            model_family: &model_family,
            approval_policy: AskForApproval::Never,
            sandbox_policy: SandboxPolicy::ReadOnly,
            include_plan_tool: false,
            include_apply_patch_tool: false,
            include_web_search_request: false,
            use_streamable_shell_tool: false,
            include_view_image_tool: false,
            include_subagent_tools: false,
        });

        assert!(!config.include_subagent_tools);

        let tools = get_openai_tools(&config, None);
        let tool_names: Vec<String> = tools
            .iter()
            .map(|tool| match tool {
                OpenAiTool::Function(f) => f.name.clone(),
                OpenAiTool::LocalShell {} => "local_shell".to_string(),
                OpenAiTool::WebSearch {} => "web_search".to_string(),
                OpenAiTool::Freeform(f) => f.name.clone(),
            })
            .collect();

        assert!(!tool_names.contains(&"subagent_list".to_string()));
        assert!(!tool_names.contains(&"subagent_describe".to_string()));
        assert!(!tool_names.contains(&"subagent_run".to_string()));
    }

    #[test]
    fn test_subagent_args_serialization() {
        // Test SubAgentListArgs
        let list_args = SubAgentListArgs {};
        let serialized = serde_json::to_string(&list_args).unwrap();
        assert_eq!(serialized, "{}");
        let _deserialized: SubAgentListArgs = serde_json::from_str(&serialized).unwrap();
        // Just ensure it deserializes without error

        // Test SubAgentDescribeArgs
        let describe_args = SubAgentDescribeArgs {
            name: "test-agent".to_string(),
        };
        let serialized = serde_json::to_string(&describe_args).unwrap();
        assert!(serialized.contains("test-agent"));
        let deserialized: SubAgentDescribeArgs = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized.name, "test-agent");

        // Test SubAgentRunArgs with model
        let run_args_with_model = SubAgentRunArgs {
            name: "code-agent".to_string(),
            task: "Review this code".to_string(),
            model: Some("gpt-4".to_string()),
        };
        let serialized = serde_json::to_string(&run_args_with_model).unwrap();
        assert!(serialized.contains("code-agent"));
        assert!(serialized.contains("Review this code"));
        assert!(serialized.contains("gpt-4"));
        let deserialized: SubAgentRunArgs = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized.name, "code-agent");
        assert_eq!(deserialized.task, "Review this code");
        assert_eq!(deserialized.model, Some("gpt-4".to_string()));

        // Test SubAgentRunArgs without model
        let run_args_no_model = SubAgentRunArgs {
            name: "simple-agent".to_string(),
            task: "Simple task".to_string(),
            model: None,
        };
        let serialized = serde_json::to_string(&run_args_no_model).unwrap();
        assert!(serialized.contains("simple-agent"));
        assert!(serialized.contains("Simple task"));
        assert!(!serialized.contains("model")); // Should be omitted when None
        let deserialized: SubAgentRunArgs = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized.name, "simple-agent");
        assert_eq!(deserialized.task, "Simple task");
        assert_eq!(deserialized.model, None);
    }

    #[test]
    fn test_create_tools_json_with_subagent_tools() {
        // Test Responses API format
        let tools = vec![
            create_subagent_list_tool(),
            create_subagent_describe_tool(),
            create_subagent_run_tool(),
        ];

        let responses_json = create_tools_json_for_responses_api(&tools).unwrap();
        assert_eq!(responses_json.len(), 3);

        // Verify subagent_list tool
        let list_tool = &responses_json[0];
        assert_eq!(list_tool["type"], "function");
        assert_eq!(list_tool["name"], "subagent_list");
        assert!(
            list_tool["description"]
                .as_str()
                .unwrap()
                .contains("List available sub-agents")
        );

        // Verify subagent_describe tool
        let describe_tool = &responses_json[1];
        assert_eq!(describe_tool["type"], "function");
        assert_eq!(describe_tool["name"], "subagent_describe");
        assert!(
            describe_tool["parameters"]["required"]
                .as_array()
                .unwrap()
                .contains(&serde_json::Value::String("name".to_string()))
        );

        // Verify subagent_run tool
        let run_tool = &responses_json[2];
        assert_eq!(run_tool["type"], "function");
        assert_eq!(run_tool["name"], "subagent_run");
        let required = run_tool["parameters"]["required"].as_array().unwrap();
        assert!(required.contains(&serde_json::Value::String("name".to_string())));
        assert!(required.contains(&serde_json::Value::String("task".to_string())));

        // Test Chat Completions API format
        let chat_json = create_tools_json_for_chat_completions_api(&tools).unwrap();
        assert_eq!(chat_json.len(), 3);

        for tool in &chat_json {
            assert_eq!(tool["type"], "function");
            assert!(tool["function"].is_object());
            assert!(!tool["function"]["type"].is_string()); // type field should be removed from function object
        }
    }

    #[test]
    fn test_subagent_tool_parameter_validation() {
        // Test that subagent_list has no required parameters
        let list_tool = create_subagent_list_tool();
        if let OpenAiTool::Function(ResponsesApiTool { parameters, .. }) = &list_tool
            && let JsonSchema::Object {
                required,
                properties,
                ..
            } = parameters
        {
            assert_eq!(required, &Some(vec![]));
            assert!(properties.is_empty());
        }

        // Test that subagent_describe requires name parameter
        let describe_tool = create_subagent_describe_tool();
        if let OpenAiTool::Function(ResponsesApiTool { parameters, .. }) = &describe_tool
            && let JsonSchema::Object {
                required,
                properties,
                ..
            } = parameters
        {
            assert_eq!(required, &Some(vec!["name".to_string()]));
            assert_eq!(properties.len(), 1);
            assert!(properties.contains_key("name"));
            if let JsonSchema::String { description } = &properties["name"] {
                assert!(
                    description
                        .as_ref()
                        .unwrap()
                        .contains("Name of the sub-agent")
                );
            }
        }

        // Test that subagent_run requires name and task, but not model
        let run_tool = create_subagent_run_tool();
        if let OpenAiTool::Function(ResponsesApiTool { parameters, .. }) = &run_tool
            && let JsonSchema::Object {
                required,
                properties,
                ..
            } = parameters
        {
            assert_eq!(
                required,
                &Some(vec!["name".to_string(), "task".to_string()])
            );
            assert_eq!(properties.len(), 3); // name, task, model
            assert!(properties.contains_key("name"));
            assert!(properties.contains_key("task"));
            assert!(properties.contains_key("model"));

            // Model should not be required
            assert!(!required.as_ref().unwrap().contains(&"model".to_string()));
        }
    }

    #[test]
    fn test_tools_config_params_construction() {
        let model_family = find_family_for_model("o3").expect("o3 should be a valid model family");

        // Test with all options enabled
        let params_all_enabled = ToolsConfigParams {
            model_family: &model_family,
            approval_policy: AskForApproval::OnRequest,
            sandbox_policy: SandboxPolicy::WorkspaceWrite {
                writable_roots: vec![],
                network_access: true,
                exclude_tmpdir_env_var: false,
                exclude_slash_tmp: false,
            },
            include_plan_tool: true,
            include_apply_patch_tool: true,
            include_web_search_request: true,
            use_streamable_shell_tool: true,
            include_view_image_tool: true,
            include_subagent_tools: true,
        };

        let config = ToolsConfig::new(&params_all_enabled);
        assert!(config.include_subagent_tools);
        assert!(config.plan_tool);
        assert!(config.web_search_request);
        assert!(config.include_view_image_tool);

        // Test with subagent tools disabled
        let params_subagent_disabled = ToolsConfigParams {
            model_family: &model_family,
            approval_policy: AskForApproval::Never,
            sandbox_policy: SandboxPolicy::ReadOnly,
            include_plan_tool: false,
            include_apply_patch_tool: false,
            include_web_search_request: false,
            use_streamable_shell_tool: false,
            include_view_image_tool: false,
            include_subagent_tools: false,
        };

        let config = ToolsConfig::new(&params_subagent_disabled);
        assert!(!config.include_subagent_tools);
        assert!(!config.plan_tool);
        assert!(!config.web_search_request);
        assert!(!config.include_view_image_tool);
    }
}
