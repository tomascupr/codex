#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs::File;
use std::fs::{self};
use std::io::Write;
use std::path::Path;

use tempfile::TempDir;
use time::OffsetDateTime;
use time::PrimitiveDateTime;
use time::format_description::FormatItem;
use time::macros::format_description;
use uuid::Uuid;

use crate::config::Config;
use crate::rollout::list::ConversationItem;

fn create_test_config() -> Config {
    Config {
        model: "test-model".to_string(),
        model_family: crate::model_family::ModelFamily {
            slug: "test-model".to_string(),
            family: "test".to_string(),
            needs_special_apply_patch_instructions: false,
            supports_reasoning_summaries: false,
            reasoning_summary_format: crate::config_types::ReasoningSummaryFormat::None,
            uses_local_shell_tool: false,
            apply_patch_tool_type: None,
        },
        model_context_window: Some(128000),
        model_max_output_tokens: Some(4096),
        model_provider_id: "test".to_string(),
        model_provider: crate::model_provider_info::ModelProviderInfo {
            name: "test-provider".to_string(),
            base_url: None,
            env_key: None,
            env_key_instructions: None,
            wire_api: crate::model_provider_info::WireApi::Chat,
            query_params: None,
            http_headers: None,
            env_http_headers: None,
            request_max_retries: None,
            stream_max_retries: None,
            stream_idle_timeout_ms: None,
            requires_openai_auth: false,
        },
        approval_policy: crate::protocol::AskForApproval::Never,
        sandbox_policy: crate::protocol::SandboxPolicy::ReadOnly,
        shell_environment_policy: crate::config_types::ShellEnvironmentPolicy::default(),
        hide_agent_reasoning: false,
        show_raw_agent_reasoning: false,
        user_instructions: None,
        base_instructions: None,
        notify: None,
        cwd: std::env::current_dir().unwrap_or_else(|_| std::env::temp_dir()),
        mcp_servers: std::collections::HashMap::new(),
        model_providers: std::collections::HashMap::new(),
        project_doc_max_bytes: 32768,
        codex_home: std::env::temp_dir(),
        history: crate::config_types::History::default(),
        file_opener: crate::config_types::UriBasedFileOpener::None,
        tui: crate::config_types::Tui::default(),
        codex_linux_sandbox_exe: None,
        model_reasoning_effort: codex_protocol::config_types::ReasoningEffort::Medium,
        model_reasoning_summary: codex_protocol::config_types::ReasoningSummary::Auto,
        model_verbosity: Some(codex_protocol::config_types::Verbosity::Medium),
        chatgpt_base_url: "https://chatgpt.com".to_string(),
        experimental_resume: None,
        include_plan_tool: false,
        include_apply_patch_tool: false,
        tools_web_search_request: false,
        responses_originator_header: "test".to_string(),
        preferred_auth_method: codex_protocol::mcp_protocol::AuthMode::ApiKey,
        use_experimental_streamable_shell_tool: false,
        include_view_image_tool: false,
        include_subagent_tools: false,
        disable_paste_burst: false,
    }
}
use crate::rollout::list::ConversationsPage;
use crate::rollout::list::Cursor;
use crate::rollout::list::get_conversation;
use crate::rollout::list::get_conversations;

fn write_session_file(
    root: &Path,
    ts_str: &str,
    uuid: Uuid,
    num_records: usize,
) -> std::io::Result<(OffsetDateTime, Uuid)> {
    let format: &[FormatItem] =
        format_description!("[year]-[month]-[day]T[hour]-[minute]-[second]");
    let dt = PrimitiveDateTime::parse(ts_str, format)
        .unwrap()
        .assume_utc();
    let dir = root
        .join("sessions")
        .join(format!("{:04}", dt.year()))
        .join(format!("{:02}", u8::from(dt.month())))
        .join(format!("{:02}", dt.day()));
    fs::create_dir_all(&dir)?;

    let filename = format!("rollout-{ts_str}-{uuid}.jsonl");
    let file_path = dir.join(filename);
    let mut file = File::create(file_path)?;

    let meta = serde_json::json!({
        "timestamp": ts_str,
        "id": uuid.to_string()
    });
    writeln!(file, "{meta}")?;

    for i in 0..num_records {
        let rec = serde_json::json!({
            "record_type": "response",
            "index": i
        });
        writeln!(file, "{rec}")?;
    }
    Ok((dt, uuid))
}

#[tokio::test]
async fn test_list_conversations_latest_first() {
    let temp = TempDir::new().unwrap();
    let home = temp.path();

    // Fixed UUIDs for deterministic expectations
    let u1 = Uuid::from_u128(1);
    let u2 = Uuid::from_u128(2);
    let u3 = Uuid::from_u128(3);

    // Create three sessions across three days
    write_session_file(home, "2025-01-01T12-00-00", u1, 3).unwrap();
    write_session_file(home, "2025-01-02T12-00-00", u2, 3).unwrap();
    write_session_file(home, "2025-01-03T12-00-00", u3, 3).unwrap();

    let page = get_conversations(home, 10, None).await.unwrap();

    // Build expected objects
    let p1 = home
        .join("sessions")
        .join("2025")
        .join("01")
        .join("03")
        .join(format!("rollout-2025-01-03T12-00-00-{u3}.jsonl"));
    let p2 = home
        .join("sessions")
        .join("2025")
        .join("01")
        .join("02")
        .join(format!("rollout-2025-01-02T12-00-00-{u2}.jsonl"));
    let p3 = home
        .join("sessions")
        .join("2025")
        .join("01")
        .join("01")
        .join(format!("rollout-2025-01-01T12-00-00-{u1}.jsonl"));

    let head_3 = vec![
        serde_json::json!({"timestamp": "2025-01-03T12-00-00", "id": u3.to_string()}),
        serde_json::json!({"record_type": "response", "index": 0}),
        serde_json::json!({"record_type": "response", "index": 1}),
        serde_json::json!({"record_type": "response", "index": 2}),
    ];
    let head_2 = vec![
        serde_json::json!({"timestamp": "2025-01-02T12-00-00", "id": u2.to_string()}),
        serde_json::json!({"record_type": "response", "index": 0}),
        serde_json::json!({"record_type": "response", "index": 1}),
        serde_json::json!({"record_type": "response", "index": 2}),
    ];
    let head_1 = vec![
        serde_json::json!({"timestamp": "2025-01-01T12-00-00", "id": u1.to_string()}),
        serde_json::json!({"record_type": "response", "index": 0}),
        serde_json::json!({"record_type": "response", "index": 1}),
        serde_json::json!({"record_type": "response", "index": 2}),
    ];

    let expected_cursor: Cursor =
        serde_json::from_str(&format!("\"2025-01-01T12-00-00|{u1}\"")).unwrap();

    let expected = ConversationsPage {
        items: vec![
            ConversationItem {
                path: p1,
                head: head_3,
            },
            ConversationItem {
                path: p2,
                head: head_2,
            },
            ConversationItem {
                path: p3,
                head: head_1,
            },
        ],
        next_cursor: Some(expected_cursor),
        num_scanned_files: 3,
        reached_scan_cap: false,
    };

    assert_eq!(page, expected);
}

#[tokio::test]
async fn test_pagination_cursor() {
    let temp = TempDir::new().unwrap();
    let home = temp.path();

    // Fixed UUIDs for deterministic expectations
    let u1 = Uuid::from_u128(11);
    let u2 = Uuid::from_u128(22);
    let u3 = Uuid::from_u128(33);
    let u4 = Uuid::from_u128(44);
    let u5 = Uuid::from_u128(55);

    // Oldest to newest
    write_session_file(home, "2025-03-01T09-00-00", u1, 1).unwrap();
    write_session_file(home, "2025-03-02T09-00-00", u2, 1).unwrap();
    write_session_file(home, "2025-03-03T09-00-00", u3, 1).unwrap();
    write_session_file(home, "2025-03-04T09-00-00", u4, 1).unwrap();
    write_session_file(home, "2025-03-05T09-00-00", u5, 1).unwrap();

    let page1 = get_conversations(home, 2, None).await.unwrap();
    let p5 = home
        .join("sessions")
        .join("2025")
        .join("03")
        .join("05")
        .join(format!("rollout-2025-03-05T09-00-00-{u5}.jsonl"));
    let p4 = home
        .join("sessions")
        .join("2025")
        .join("03")
        .join("04")
        .join(format!("rollout-2025-03-04T09-00-00-{u4}.jsonl"));
    let head_5 = vec![
        serde_json::json!({"timestamp": "2025-03-05T09-00-00", "id": u5.to_string()}),
        serde_json::json!({"record_type": "response", "index": 0}),
    ];
    let head_4 = vec![
        serde_json::json!({"timestamp": "2025-03-04T09-00-00", "id": u4.to_string()}),
        serde_json::json!({"record_type": "response", "index": 0}),
    ];
    let expected_cursor1: Cursor =
        serde_json::from_str(&format!("\"2025-03-04T09-00-00|{u4}\"")).unwrap();
    let expected_page1 = ConversationsPage {
        items: vec![
            ConversationItem {
                path: p5,
                head: head_5,
            },
            ConversationItem {
                path: p4,
                head: head_4,
            },
        ],
        next_cursor: Some(expected_cursor1.clone()),
        num_scanned_files: 3, // scanned 05, 04, and peeked at 03 before breaking
        reached_scan_cap: false,
    };
    assert_eq!(page1, expected_page1);

    let page2 = get_conversations(home, 2, page1.next_cursor.as_ref())
        .await
        .unwrap();
    let p3 = home
        .join("sessions")
        .join("2025")
        .join("03")
        .join("03")
        .join(format!("rollout-2025-03-03T09-00-00-{u3}.jsonl"));
    let p2 = home
        .join("sessions")
        .join("2025")
        .join("03")
        .join("02")
        .join(format!("rollout-2025-03-02T09-00-00-{u2}.jsonl"));
    let head_3 = vec![
        serde_json::json!({"timestamp": "2025-03-03T09-00-00", "id": u3.to_string()}),
        serde_json::json!({"record_type": "response", "index": 0}),
    ];
    let head_2 = vec![
        serde_json::json!({"timestamp": "2025-03-02T09-00-00", "id": u2.to_string()}),
        serde_json::json!({"record_type": "response", "index": 0}),
    ];
    let expected_cursor2: Cursor =
        serde_json::from_str(&format!("\"2025-03-02T09-00-00|{u2}\"")).unwrap();
    let expected_page2 = ConversationsPage {
        items: vec![
            ConversationItem {
                path: p3,
                head: head_3,
            },
            ConversationItem {
                path: p2,
                head: head_2,
            },
        ],
        next_cursor: Some(expected_cursor2.clone()),
        num_scanned_files: 5, // scanned 05, 04 (anchor), 03, 02, and peeked at 01
        reached_scan_cap: false,
    };
    assert_eq!(page2, expected_page2);

    let page3 = get_conversations(home, 2, page2.next_cursor.as_ref())
        .await
        .unwrap();
    let p1 = home
        .join("sessions")
        .join("2025")
        .join("03")
        .join("01")
        .join(format!("rollout-2025-03-01T09-00-00-{u1}.jsonl"));
    let head_1 = vec![
        serde_json::json!({"timestamp": "2025-03-01T09-00-00", "id": u1.to_string()}),
        serde_json::json!({"record_type": "response", "index": 0}),
    ];
    let expected_cursor3: Cursor =
        serde_json::from_str(&format!("\"2025-03-01T09-00-00|{u1}\"")).unwrap();
    let expected_page3 = ConversationsPage {
        items: vec![ConversationItem {
            path: p1,
            head: head_1,
        }],
        next_cursor: Some(expected_cursor3.clone()),
        num_scanned_files: 5, // scanned 05, 04 (anchor), 03, 02 (anchor), 01
        reached_scan_cap: false,
    };
    assert_eq!(page3, expected_page3);
}

#[tokio::test]
async fn test_get_conversation_contents() {
    let temp = TempDir::new().unwrap();
    let home = temp.path();

    let uuid = Uuid::new_v4();
    let ts = "2025-04-01T10-30-00";
    write_session_file(home, ts, uuid, 2).unwrap();

    let page = get_conversations(home, 1, None).await.unwrap();
    let path = &page.items[0].path;

    let content = get_conversation(path).await.unwrap();

    // Page equality (single item)
    let expected_path = home
        .join("sessions")
        .join("2025")
        .join("04")
        .join("01")
        .join(format!("rollout-2025-04-01T10-30-00-{uuid}.jsonl"));
    let expected_head = vec![
        serde_json::json!({"timestamp": ts, "id": uuid.to_string()}),
        serde_json::json!({"record_type": "response", "index": 0}),
        serde_json::json!({"record_type": "response", "index": 1}),
    ];
    let expected_cursor: Cursor = serde_json::from_str(&format!("\"{ts}|{uuid}\"")).unwrap();
    let expected_page = ConversationsPage {
        items: vec![ConversationItem {
            path: expected_path.clone(),
            head: expected_head,
        }],
        next_cursor: Some(expected_cursor),
        num_scanned_files: 1,
        reached_scan_cap: false,
    };
    assert_eq!(page, expected_page);

    // Entire file contents equality
    let meta = serde_json::json!({"timestamp": ts, "id": uuid.to_string()});
    let rec0 = serde_json::json!({"record_type": "response", "index": 0});
    let rec1 = serde_json::json!({"record_type": "response", "index": 1});
    let expected_content = format!("{meta}\n{rec0}\n{rec1}\n");
    assert_eq!(content, expected_content);
}

#[tokio::test]
async fn test_stable_ordering_same_second_pagination() {
    let temp = TempDir::new().unwrap();
    let home = temp.path();

    let ts = "2025-07-01T00-00-00";
    let u1 = Uuid::from_u128(1);
    let u2 = Uuid::from_u128(2);
    let u3 = Uuid::from_u128(3);

    write_session_file(home, ts, u1, 0).unwrap();
    write_session_file(home, ts, u2, 0).unwrap();
    write_session_file(home, ts, u3, 0).unwrap();

    let page1 = get_conversations(home, 2, None).await.unwrap();

    let p3 = home
        .join("sessions")
        .join("2025")
        .join("07")
        .join("01")
        .join(format!("rollout-2025-07-01T00-00-00-{u3}.jsonl"));
    let p2 = home
        .join("sessions")
        .join("2025")
        .join("07")
        .join("01")
        .join(format!("rollout-2025-07-01T00-00-00-{u2}.jsonl"));
    let head = |u: Uuid| -> Vec<serde_json::Value> {
        vec![serde_json::json!({"timestamp": ts, "id": u.to_string()})]
    };
    let expected_cursor1: Cursor = serde_json::from_str(&format!("\"{ts}|{u2}\"")).unwrap();
    let expected_page1 = ConversationsPage {
        items: vec![
            ConversationItem {
                path: p3,
                head: head(u3),
            },
            ConversationItem {
                path: p2,
                head: head(u2),
            },
        ],
        next_cursor: Some(expected_cursor1.clone()),
        num_scanned_files: 3, // scanned u3, u2, peeked u1
        reached_scan_cap: false,
    };
    assert_eq!(page1, expected_page1);

    let page2 = get_conversations(home, 2, page1.next_cursor.as_ref())
        .await
        .unwrap();
    let p1 = home
        .join("sessions")
        .join("2025")
        .join("07")
        .join("01")
        .join(format!("rollout-2025-07-01T00-00-00-{u1}.jsonl"));
    let expected_cursor2: Cursor = serde_json::from_str(&format!("\"{ts}|{u1}\"")).unwrap();
    let expected_page2 = ConversationsPage {
        items: vec![ConversationItem {
            path: p1,
            head: head(u1),
        }],
        next_cursor: Some(expected_cursor2.clone()),
        num_scanned_files: 3, // scanned u3, u2 (anchor), u1
        reached_scan_cap: false,
    };
    assert_eq!(page2, expected_page2);
}

#[test]
fn test_subagent_events_are_persisted() {
    use crate::rollout::policy::is_persisted_response_item;
    use codex_protocol::models::ResponseItem;
    use codex_protocol::protocol::Origin;

    // Test that SubAgentStart events are persisted
    let subagent_start = ResponseItem::SubAgentStart {
        name: "code-reviewer".to_string(),
        description: "Reviews code for quality".to_string(),
        origin: Some(Origin::Main),
    };
    assert!(is_persisted_response_item(&subagent_start));

    // Test that SubAgentEnd events are persisted
    let subagent_end = ResponseItem::SubAgentEnd {
        name: "code-reviewer".to_string(),
        success: true,
        origin: Some(Origin::Main),
    };
    assert!(is_persisted_response_item(&subagent_end));

    // Test failed sub-agent end event
    let subagent_end_failed = ResponseItem::SubAgentEnd {
        name: "analyzer".to_string(),
        success: false,
        origin: Some(Origin::SubAgent {
            name: "parent-agent".to_string(),
        }),
    };
    assert!(is_persisted_response_item(&subagent_end_failed));
}

#[test]
fn test_subagent_event_serialization_roundtrip() {
    use codex_protocol::models::ResponseItem;
    use codex_protocol::protocol::Origin;

    // Test SubAgentStart roundtrip
    let start_event = ResponseItem::SubAgentStart {
        name: "test-agent".to_string(),
        description: "A test sub-agent".to_string(),
        origin: Some(Origin::Main),
    };

    let serialized = serde_json::to_string(&start_event).unwrap();
    let deserialized: ResponseItem = serde_json::from_str(&serialized).unwrap();

    match (start_event, deserialized) {
        (
            ResponseItem::SubAgentStart {
                name: n1,
                description: d1,
                origin: o1,
            },
            ResponseItem::SubAgentStart {
                name: n2,
                description: d2,
                origin: o2,
            },
        ) => {
            assert_eq!(n1, n2);
            assert_eq!(d1, d2);
            assert_eq!(o1, o2);
        }
        _ => panic!("Expected SubAgentStart events"),
    }

    // Test SubAgentEnd roundtrip
    let end_event = ResponseItem::SubAgentEnd {
        name: "test-agent".to_string(),
        success: false,
        origin: Some(Origin::SubAgent {
            name: "parent".to_string(),
        }),
    };

    let serialized = serde_json::to_string(&end_event).unwrap();
    let deserialized: ResponseItem = serde_json::from_str(&serialized).unwrap();

    match (end_event, deserialized) {
        (
            ResponseItem::SubAgentEnd {
                name: n1,
                success: s1,
                origin: o1,
            },
            ResponseItem::SubAgentEnd {
                name: n2,
                success: s2,
                origin: o2,
            },
        ) => {
            assert_eq!(n1, n2);
            assert_eq!(s1, s2);
            assert_eq!(o1, o2);
        }
        _ => panic!("Expected SubAgentEnd events"),
    }
}

#[tokio::test]
async fn test_rollout_recorder_with_subagent_events() {
    use crate::rollout::recorder::RolloutRecorder;
    use crate::rollout::recorder::RolloutRecorderParams;
    use codex_protocol::models::ResponseItem;
    use codex_protocol::protocol::Origin;
    use tempfile::TempDir;
    use uuid::Uuid;

    let temp_dir = TempDir::new().unwrap();
    let codex_home = temp_dir.path().to_path_buf();

    let session_id = Uuid::new_v4();
    let params = RolloutRecorderParams::new(
        codex_protocol::mcp_protocol::ConversationId(session_id),
        Some("test-project".to_string()),
    );

    let mut config = create_test_config();
    config.codex_home = codex_home;
    let recorder = RolloutRecorder::new(&config, params).await.unwrap();

    // Record a sequence of sub-agent events
    let events = vec![
        ResponseItem::Message {
            id: None,
            role: "assistant".to_string(),
            content: vec![codex_protocol::models::ContentItem::OutputText {
                text: "Starting sub-agent execution".to_string(),
            }],
            origin: Some(Origin::Main),
        },
        ResponseItem::SubAgentStart {
            name: "code-analyzer".to_string(),
            description: "Analyzes code structure and patterns".to_string(),
            origin: Some(Origin::Main),
        },
        ResponseItem::Message {
            id: None,
            role: "assistant".to_string(),
            content: vec![codex_protocol::models::ContentItem::OutputText {
                text: "Analyzing code...".to_string(),
            }],
            origin: Some(Origin::SubAgent {
                name: "code-analyzer".to_string(),
            }),
        },
        ResponseItem::SubAgentEnd {
            name: "code-analyzer".to_string(),
            success: true,
            origin: Some(Origin::Main),
        },
        ResponseItem::Message {
            id: None,
            role: "assistant".to_string(),
            content: vec![codex_protocol::models::ContentItem::OutputText {
                text: "Sub-agent execution completed".to_string(),
            }],
            origin: Some(Origin::Main),
        },
    ];

    // Record all events
    recorder.record_items(&events).await.unwrap();

    // Flush to ensure everything is written
    drop(recorder);

    // Verify that sub-agent events are properly persisted
    // We can't easily test file contents without exposing more internals,
    // but we can verify the recording doesn't fail and the policy includes these events
    for event in &events {
        match event {
            ResponseItem::SubAgentStart { .. } | ResponseItem::SubAgentEnd { .. } => {
                assert!(crate::rollout::policy::is_persisted_response_item(event));
            }
            ResponseItem::Message { .. } => {
                assert!(crate::rollout::policy::is_persisted_response_item(event));
            }
            _ => {}
        }
    }
}

#[test]
fn test_subagent_events_with_different_origins() {
    use crate::rollout::policy::is_persisted_response_item;
    use codex_protocol::models::ResponseItem;
    use codex_protocol::protocol::Origin;

    // Test sub-agent events from main origin
    let main_start = ResponseItem::SubAgentStart {
        name: "main-agent".to_string(),
        description: "Started from main".to_string(),
        origin: Some(Origin::Main),
    };
    assert!(is_persisted_response_item(&main_start));

    // Test sub-agent events from sub-agent origin (nested)
    let nested_start = ResponseItem::SubAgentStart {
        name: "nested-agent".to_string(),
        description: "Started from another sub-agent".to_string(),
        origin: Some(Origin::SubAgent {
            name: "parent-agent".to_string(),
        }),
    };
    assert!(is_persisted_response_item(&nested_start));

    // Test sub-agent events without origin
    let no_origin_end = ResponseItem::SubAgentEnd {
        name: "orphan-agent".to_string(),
        success: true,
        origin: None,
    };
    assert!(is_persisted_response_item(&no_origin_end));
}

#[test]
fn test_subagent_event_json_structure() {
    use codex_protocol::models::ResponseItem;
    use codex_protocol::protocol::Origin;
    use serde_json::Value;

    // Test SubAgentStart JSON structure
    let start_event = ResponseItem::SubAgentStart {
        name: "formatter".to_string(),
        description: "Formats code according to style guide".to_string(),
        origin: Some(Origin::Main),
    };

    let json_value: Value = serde_json::to_value(&start_event).unwrap();

    // Verify JSON structure
    if let Value::Object(obj) = json_value {
        assert_eq!(
            obj.get("type").unwrap().as_str().unwrap(),
            "sub_agent_start"
        );

        assert_eq!(obj.get("name").unwrap().as_str().unwrap(), "formatter");
        assert_eq!(
            obj.get("description").unwrap().as_str().unwrap(),
            "Formats code according to style guide"
        );
        assert!(obj.contains_key("origin"));
    } else {
        panic!("Expected JSON object");
    }

    // Test SubAgentEnd JSON structure
    let end_event = ResponseItem::SubAgentEnd {
        name: "formatter".to_string(),
        success: true,
        origin: Some(Origin::SubAgent {
            name: "coordinator".to_string(),
        }),
    };

    let json_value: Value = serde_json::to_value(&end_event).unwrap();

    if let Value::Object(obj) = json_value {
        assert_eq!(obj.get("type").unwrap().as_str().unwrap(), "sub_agent_end");

        assert_eq!(obj.get("name").unwrap().as_str().unwrap(), "formatter");
        assert!(obj.get("success").unwrap().as_bool().unwrap());

        // Check nested sub-agent origin structure
        if let Some(Value::Object(origin_obj)) = obj.get("origin") {
            if let Some(Value::Object(sub_agent_obj)) = origin_obj.get("sub_agent") {
                assert_eq!(
                    sub_agent_obj.get("name").unwrap().as_str().unwrap(),
                    "coordinator"
                );
            } else {
                panic!("Expected sub_agent origin object");
            }
        } else {
            panic!("Expected origin object");
        }
    } else {
        panic!("Expected JSON object");
    }
}
