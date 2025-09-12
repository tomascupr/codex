#![expect(clippy::expect_used)]

use std::path::PathBuf;

use codex_core::AuthManager;
use codex_core::CodexAuth;
use codex_core::ConversationManager;
use codex_core::ModelProviderInfo;
use codex_core::NewConversation;
use codex_core::built_in_model_providers;
use codex_core::protocol::EventMsg;
use codex_core::protocol::InputItem;
use codex_core::protocol::Op;
use core_test_support::load_sse_fixture_with_id_from_str;
use tempfile::TempDir;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

/// End-to-end: model calls subagent_run; nested run produces assistant text; end events emitted.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn subagent_run_yields_assistant_output_and_events() {
    // Mock Responses API server.
    let server = MockServer::start().await;

    // SSE 1: function call to subagent_run, then completed.
    let item_fc = serde_json::json!({
        "type": "function_call",
        "name": "subagent_run",
        "arguments": "{\"name\":\"docs\",\"task\":\"say hi\"}",
        "call_id": "call_main_1"
    })
    .to_string();
    let sse_fc = format!("event: response.output_item.done\ndata: {item_fc}\n\n");
    let completed_main = serde_json::json!({
        "type": "response.completed",
        "response": { "id": "resp_main" }
    })
    .to_string();
    let sse_completed_main = format!("event: response.completed\ndata: {completed_main}\n\n");

    // SSE 2: nested run returns a final assistant message and completes.
    let item_msg = serde_json::json!({
        "type": "message",
        "role": "assistant",
        "content": [{"type":"output_text", "text":"hi from agent\n"}]
    })
    .to_string();
    let sse_msg = format!("event: response.output_item.done\ndata: {item_msg}\n\n");
    let completed_nested = serde_json::json!({
        "type": "response.completed",
        "response": { "id": "resp_nested" }
    })
    .to_string();
    let sse_completed_nested = format!("event: response.completed\ndata: {completed_nested}\n\n");

    // First request → function call to subagent_run
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(format!("{sse_fc}{sse_completed_main}"), "text/event-stream"),
        )
        .expect(1)
        .mount(&server)
        .await;

    // Second request (nested) → assistant message
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(format!("{sse_msg}{sse_completed_nested}"), "text/event-stream"),
        )
        .expect(1)
        .mount(&server)
        .await;

    // Configure provider to point to mock server.
    let mut provider = built_in_model_providers()["openai"].clone();
    provider.base_url = Some(format!("{}/v1", server.uri()));
    provider.requires_openai_auth = false;

    // Prepare Codex config in a temp home and working dir with a docs agent.
    let codex_home = TempDir::new().expect("temp home");
    let workdir = TempDir::new().expect("workdir");
    let agents_dir = workdir.path().join(".codex/agents");
    std::fs::create_dir_all(&agents_dir).expect("mkdir agents");
    let doc_agent = r#"---
description: simple test agent
tools: ["shell"]
---
You are a simple agent.
"#;
    std::fs::write(agents_dir.join("docs.md"), doc_agent).expect("write agent");

    // Build config and override cwd/model provider and include_subagent_tools to true.
    let auth_manager = AuthManager::shared(codex_home.path().to_path_buf());
    let conversation_manager = ConversationManager::new(auth_manager);
    let mut config = core_test_support::load_default_config_for_test(&codex_home);
    config.model_provider = provider;
    config.cwd = workdir.path().to_path_buf();
    config.include_subagent_tools = true;

    let NewConversation { conversation: codex, .. } = conversation_manager
        .new_conversation(config)
        .await
        .expect("new conversation");

    codex
        .submit(Op::UserInput {
            items: vec![InputItem::Text {
                text: "start".to_string(),
            }],
        })
        .await
        .expect("submit");

    // Expect SubAgentStart and SubAgentEnd, then TaskComplete.
    let start = core_test_support::wait_for_event(&codex, |ev| match ev {
        EventMsg::SubAgentStart(s) => s.name == "docs",
        _ => false,
    })
    .await;
    match start {
        EventMsg::SubAgentStart(s) => assert_eq!(s.name, "docs"),
        _ => panic!("unexpected event"),
    }

    let end = core_test_support::wait_for_event(&codex, |ev| matches!(ev, EventMsg::SubAgentEnd(_)))
        .await;
    match end {
        EventMsg::SubAgentEnd(e) => assert!(e.success),
        _ => panic!("unexpected event"),
    }

    let complete = core_test_support::wait_for_event(&codex, |ev| matches!(ev, EventMsg::TaskComplete(_)))
        .await;
    matches!(complete, EventMsg::TaskComplete(_));
}

