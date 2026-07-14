use std::sync::Arc;

use async_trait::async_trait;
use futures_util::StreamExt;

use super::*;

#[test]
fn minimax_endpoints_match_region() {
    let global = MinimaxConfig::international("key", MINIMAX_DEFAULT_MODEL);
    let mainland = MinimaxConfig::china_mainland("key", MINIMAX_DEFAULT_MODEL);

    assert_eq!(global.base_url, MINIMAX_GLOBAL_BASE_URL);
    assert_eq!(mainland.base_url, MINIMAX_CHINA_BASE_URL);
}

#[test]
fn minimax_config_with_custom_base_url() {
    let cfg = MinimaxConfig::with_base_url("key", "model-x", "https://example.com/api");
    assert_eq!(cfg.base_url, "https://example.com/api");
    assert_eq!(cfg.model, "model-x");
    assert_eq!(cfg.anthropic_version, ANTHROPIC_VERSION);
    assert_eq!(
        cfg.request_throttle_interval,
        std::time::Duration::from_millis(100)
    );
    assert_eq!(cfg.max_concurrent_requests, 1);
    assert_eq!(
        cfg.rate_limit_retry_delay,
        std::time::Duration::from_secs(10)
    );
}

#[test]
fn minimax_client_new_returns_ok() {
    let result = MinimaxClient::new(MinimaxConfig::international("key", MINIMAX_DEFAULT_MODEL));
    assert!(result.is_ok());
}

#[test]
fn minimax_client_builds_messages_endpoint() {
    let client = MinimaxClient::new(MinimaxConfig::international("key", MINIMAX_DEFAULT_MODEL))
        .expect("client should build");

    assert_eq!(
        client.messages_endpoint(),
        "https://api.minimax.io/anthropic/v1/messages"
    );
}

#[test]
fn minimax_client_messages_endpoint_strips_trailing_slash() {
    let client = MinimaxClient::new(MinimaxConfig::with_base_url(
        "k",
        "m",
        "https://example.com/",
    ))
    .expect("client should build");

    assert_eq!(
        client.messages_endpoint(),
        "https://example.com/v1/messages"
    );
}

#[test]
fn minimax_client_provider_name() {
    let client = MinimaxClient::new(MinimaxConfig::international("key", MINIMAX_DEFAULT_MODEL))
        .expect("client should build");
    assert_eq!(client.provider_name(), "minimax");
}

#[test]
fn from_env_fails_without_api_key() {
    env::remove_var("OMEGA_API_KEY");
    env::remove_var("OMEGA_MINIMAX_API_KEY");
    env::remove_var("ANTHROPIC_API_KEY");
    let result = MinimaxConfig::from_env();
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.to_string().contains("OMEGA_API_KEY"),
        "error should mention env var: {err}"
    );
}

#[test]
fn from_env_accepts_anthropic_fallbacks() {
    env::remove_var("OMEGA_API_KEY");
    env::remove_var("OMEGA_MINIMAX_API_KEY");
    env::remove_var("OMEGA_MODEL_ID");
    env::remove_var("OMEGA_BASE_URL");
    env::set_var("ANTHROPIC_API_KEY", "anthropic-key");
    env::set_var("ANTHROPIC_MODEL", "claude-compatible");
    env::set_var("ANTHROPIC_BASE_URL", "https://anthropic.example.com");
    env::set_var("ANTHROPIC_PROVIDER_REQUEST_THROTTLE_MS", "250");
    env::set_var("ANTHROPIC_PROVIDER_MAX_CONCURRENT_REQUESTS", "3");
    env::set_var("ANTHROPIC_PROVIDER_RATE_LIMIT_RETRY_DELAY_MS", "15000");

    let config = MinimaxConfig::from_env().expect("env fallback should load config");

    assert_eq!(config.api_key, "anthropic-key");
    assert_eq!(config.model, "claude-compatible");
    assert_eq!(config.base_url, "https://anthropic.example.com");
    assert_eq!(
        config.request_throttle_interval,
        std::time::Duration::from_millis(250)
    );
    assert_eq!(config.max_concurrent_requests, 3);
    assert_eq!(
        config.rate_limit_retry_delay,
        std::time::Duration::from_secs(15)
    );

    env::remove_var("ANTHROPIC_API_KEY");
    env::remove_var("ANTHROPIC_MODEL");
    env::remove_var("ANTHROPIC_BASE_URL");
    env::remove_var("ANTHROPIC_PROVIDER_REQUEST_THROTTLE_MS");
    env::remove_var("ANTHROPIC_PROVIDER_MAX_CONCURRENT_REQUESTS");
    env::remove_var("ANTHROPIC_PROVIDER_RATE_LIMIT_RETRY_DELAY_MS");
}

#[test]
fn from_env_rejects_zero_max_concurrent_requests() {
    env::set_var("OMEGA_API_KEY", "omega-key");
    env::set_var("OMEGA_PROVIDER_MAX_CONCURRENT_REQUESTS", "0");

    let error = MinimaxConfig::from_env().expect_err("zero concurrency must be rejected");

    assert!(error
        .to_string()
        .contains("OMEGA_PROVIDER_MAX_CONCURRENT_REQUESTS must be greater than 0"));

    env::remove_var("OMEGA_API_KEY");
    env::remove_var("OMEGA_PROVIDER_MAX_CONCURRENT_REQUESTS");
}

#[test]
fn message_user_creates_text_content() {
    let msg = Message::user("hello");
    assert_eq!(msg.role, Role::User);
    assert_eq!(msg.content, MessageContent::Text("hello".into()));
}

#[test]
fn message_assistant_creates_blocks_content() {
    let msg = Message::assistant(vec![ContentBlock::text("hi")]);
    assert_eq!(msg.role, Role::Assistant);
    match &msg.content {
        MessageContent::Blocks(blocks) => {
            assert_eq!(blocks.len(), 1);
            assert!(matches!(&blocks[0], ContentBlock::Text { text } if text == "hi"));
        }
        _ => panic!("expected Blocks"),
    }
}

#[test]
fn message_tool_results_serialize_as_blocks() {
    let message = Message::tool_results(vec![ContentBlock::tool_result("tool-1", "done")]);
    let value = serde_json::to_value(message).expect("message should serialize");

    assert_eq!(value["role"], "user");
    assert_eq!(value["content"][0]["type"], "tool_result");
    assert_eq!(value["content"][0]["tool_use_id"], "tool-1");
    assert_eq!(value["content"][0]["content"], "done");
}

#[test]
fn content_block_text_roundtrip() {
    let block = ContentBlock::text("hello");
    let json = serde_json::to_value(&block).unwrap();
    assert_eq!(json["type"], "text");
    assert_eq!(json["text"], "hello");

    let back: ContentBlock = serde_json::from_value(json).unwrap();
    assert_eq!(back, block);
}

#[test]
fn content_block_tool_use_roundtrip() {
    let block = ContentBlock::tool_use("id-1", "bash", json!({"command": "ls"}));
    let json = serde_json::to_value(&block).unwrap();
    assert_eq!(json["type"], "tool_use");
    assert_eq!(json["id"], "id-1");
    assert_eq!(json["name"], "bash");
    assert_eq!(json["input"]["command"], "ls");

    let back: ContentBlock = serde_json::from_value(json).unwrap();
    assert_eq!(back, block);
}

#[test]
fn content_block_tool_result_roundtrip() {
    let block = ContentBlock::tool_result("id-1", "output");
    let json = serde_json::to_value(&block).unwrap();
    assert_eq!(json["type"], "tool_result");
    assert_eq!(json["tool_use_id"], "id-1");
    assert_eq!(json["content"], "output");
    assert!(
        json.get("is_error").is_none(),
        "is_error should be skipped when None"
    );

    let back: ContentBlock = serde_json::from_value(json).unwrap();
    assert_eq!(back, block);
}

#[test]
fn content_block_tool_result_error() {
    let block = ContentBlock::tool_result_error("id-1", "fail");
    let json = serde_json::to_value(&block).unwrap();
    assert_eq!(json["is_error"], true);
}

#[test]
fn chat_request_defaults() {
    let req = ChatRequest::new(vec![Message::user("hi")]);
    assert!(req.system.is_none());
    assert!(req.tools.is_empty());
    assert_eq!(req.max_tokens, 8_000);
}

#[test]
fn chat_request_builder_chain() {
    let tool = ToolDefinition {
        name: "bash".into(),
        description: "run shell".into(),
        input_schema: json!({"type": "object"}),
    };
    let req = ChatRequest::new(vec![Message::user("hi")])
        .with_system("You are helpful.")
        .with_tools(vec![tool.clone()])
        .with_max_tokens(4_000);

    assert_eq!(req.system.as_deref(), Some("You are helpful."));
    assert_eq!(req.tools.len(), 1);
    assert_eq!(req.tools[0].name, "bash");
    assert_eq!(req.max_tokens, 4_000);
}

#[test]
fn chat_request_serializes_correctly() {
    let req = ChatRequest::new(vec![Message::user("hi")]).with_system("sys");
    let json = serde_json::to_value(&req).unwrap();
    assert_eq!(json["system"], "sys");
    assert_eq!(json["messages"][0]["role"], "user");
    assert_eq!(json["messages"][0]["content"], "hi");
    assert_eq!(json["max_tokens"], 8000);
    assert!(json.get("tools").is_none() || json["tools"].as_array().unwrap().is_empty());
}

#[test]
fn build_body_maps_cache_markers_for_system_tools_and_last_assistant_turn() {
    let client = MinimaxClient::new(MinimaxConfig::international("key", "model-a"))
        .expect("client should build");
    let tool = ToolDefinition {
        name: "bash".into(),
        description: "run shell".into(),
        input_schema: json!({"type": "object"}),
    };
    let req = ChatRequest::new(vec![
        Message::assistant(vec![ContentBlock::text("previous answer")]),
        Message::user("continue"),
    ])
    .with_system_blocks(vec![
        SystemBlock::text("stable instructions")
            .with_cache_control(PromptCacheControl::ephemeral()),
        SystemBlock::text("summary context").with_cache_control(PromptCacheControl::ephemeral()),
        SystemBlock::text("dynamic workflow prompt"),
    ])
    .with_tools(vec![tool])
    .with_cache_last_assistant_turn(true);

    let json = client.build_body(req).expect("body should build");

    assert_eq!(json["system"][0]["cache_control"]["type"], "ephemeral");
    assert_eq!(json["system"][1]["cache_control"]["type"], "ephemeral");
    assert!(json["system"][2].get("cache_control").is_none());
    assert_eq!(json["tools"][0]["cache_control"]["type"], "ephemeral");
    assert_eq!(
        json["messages"][0]["content"][0]["cache_control"]["type"],
        "ephemeral"
    );
}

#[test]
fn chat_response_deserialize_end_turn() {
    let json = json!({
        "id": "msg_01",
        "model": "MiniMax-M2.5",
        "content": [{"type": "text", "text": "Hello!"}],
        "stop_reason": "end_turn",
        "usage": {"input_tokens": 10, "output_tokens": 5}
    });
    let resp: ChatResponse = serde_json::from_value(json).unwrap();
    assert_eq!(resp.id, "msg_01");
    assert_eq!(resp.model.as_deref(), Some("MiniMax-M2.5"));
    assert!(!resp.is_tool_use());
    assert_eq!(resp.text_content(), "Hello!");
    assert!(resp.tool_use_blocks().is_empty());

    let usage = resp.usage.expect("usage should be present");
    assert_eq!(usage.input_tokens, 10);
    assert_eq!(usage.output_tokens, 5);
}

#[test]
fn chat_response_deserialize_tool_use() {
    let json = json!({
        "id": "msg_02",
        "content": [
            {"type": "text", "text": "Let me run that."},
            {"type": "tool_use", "id": "tu_1", "name": "bash", "input": {"command": "ls"}}
        ],
        "stop_reason": "tool_use"
    });
    let resp: ChatResponse = serde_json::from_value(json).unwrap();
    assert!(resp.is_tool_use());
    assert_eq!(resp.text_content(), "Let me run that.");
    assert_eq!(resp.tool_use_blocks().len(), 1);
}

#[test]
fn chat_response_missing_usage_defaults_to_none() {
    let json = json!({
        "id": "msg_03",
        "content": [{"type": "text", "text": "ok"}],
        "stop_reason": "end_turn"
    });
    let resp: ChatResponse = serde_json::from_value(json).unwrap();
    assert!(resp.usage.is_none());
}

#[test]
fn chat_response_missing_model_defaults_to_none() {
    let json = json!({
        "id": "msg_04",
        "content": [],
        "stop_reason": "end_turn"
    });
    let resp: ChatResponse = serde_json::from_value(json).unwrap();
    assert!(resp.model.is_none());
}

#[test]
fn chat_response_multiple_text_blocks_concatenated() {
    let resp = ChatResponse {
        id: "msg_05".into(),
        model: None,
        content: vec![ContentBlock::text("hello "), ContentBlock::text("world")],
        stop_reason: Some(STOP_REASON_END_TURN.into()),
        usage: None,
    };
    assert_eq!(resp.text_content(), "hello world");
}

#[test]
fn chat_response_roundtrips_through_events() {
    let response = ChatResponse {
        id: "msg_stream".to_string(),
        model: Some("mock".to_string()),
        content: vec![
            ContentBlock::Thinking {
                thinking: "plan".to_string(),
                signature: Some("sig-1".to_string()),
            },
            ContentBlock::text("hello"),
            ContentBlock::tool_use("tool-1", "bash", json!({"command": "pwd"})),
            ContentBlock::text("done"),
        ],
        stop_reason: Some(STOP_REASON_TOOL_USE.to_string()),
        usage: Some(Usage {
            input_tokens: 11,
            output_tokens: 7,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        }),
    };

    let mut builder = ChatResponseBuilder::new();
    for event in response.to_events() {
        builder.push_event(event).unwrap();
    }

    assert_eq!(builder.finish().unwrap(), response);
}

#[test]
fn chat_response_builder_merges_sequential_deltas() {
    let mut builder = ChatResponseBuilder::new();
    builder
        .push_event(ChatEvent::MessageStart {
            id: "msg-1".to_string(),
            model: Some("mock".to_string()),
        })
        .unwrap();
    builder
        .push_event(ChatEvent::ThinkingDelta {
            thinking: "plan".to_string(),
            signature: None,
        })
        .unwrap();
    builder
        .push_event(ChatEvent::ThinkingDelta {
            thinking: " more".to_string(),
            signature: Some("sig".to_string()),
        })
        .unwrap();
    builder
        .push_event(ChatEvent::TextDelta {
            text: "hello".to_string(),
        })
        .unwrap();
    builder
        .push_event(ChatEvent::TextDelta {
            text: " world".to_string(),
        })
        .unwrap();
    builder
        .push_event(ChatEvent::MessageComplete {
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        })
        .unwrap();

    let response = builder.finish().unwrap();

    assert_eq!(
        response.content,
        vec![
            ContentBlock::Thinking {
                thinking: "plan more".to_string(),
                signature: Some("sig".to_string()),
            },
            ContentBlock::text("hello world"),
        ]
    );
}

#[tokio::test]
async fn default_chat_stream_replays_chat_response_events() {
    struct StreamingCompatClient;

    #[async_trait]
    impl LlmClient for StreamingCompatClient {
        async fn chat(&self, _request: ChatRequest) -> Result<ChatResponse, ClientError> {
            Ok(ChatResponse {
                id: "msg-stream".to_string(),
                model: Some("mock".to_string()),
                content: vec![
                    ContentBlock::Thinking {
                        thinking: "draft".to_string(),
                        signature: None,
                    },
                    ContentBlock::text("answer"),
                ],
                stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                usage: None,
            })
        }

        fn provider_name(&self) -> &'static str {
            "streaming-compat"
        }
    }

    let client = StreamingCompatClient;
    let mut stream = client
        .chat_stream(ChatRequest::new(vec![Message::user("hi")]))
        .await
        .unwrap();
    let mut events = Vec::new();

    while let Some(event) = stream.next().await {
        events.push(event.unwrap());
    }

    assert_eq!(
        events,
        vec![
            ChatEvent::MessageStart {
                id: "msg-stream".to_string(),
                model: Some("mock".to_string()),
            },
            ChatEvent::ThinkingDelta {
                thinking: "draft".to_string(),
                signature: None,
            },
            ChatEvent::TextDelta {
                text: "answer".to_string(),
            },
            ChatEvent::MessageComplete {
                stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                usage: None,
            },
        ]
    );
}

#[test]
fn build_body_minimal_request() {
    let client = MinimaxClient::new(MinimaxConfig::international("key", "model-a"))
        .expect("client should build");
    let req = ChatRequest::new(vec![Message::user("hi")]);
    let body = client.build_body(req).unwrap();

    assert_eq!(body["model"], "model-a");
    assert_eq!(body["max_tokens"], 8000);
    assert!(body.get("system").is_none());
    assert!(body.get("tools").is_none());
}

#[test]
fn build_body_with_system_and_tools() {
    let client = MinimaxClient::new(MinimaxConfig::international("key", "model-a"))
        .expect("client should build");
    let tool = ToolDefinition {
        name: "bash".into(),
        description: "run".into(),
        input_schema: json!({"type": "object"}),
    };
    let req = ChatRequest::new(vec![Message::user("hi")])
        .with_system("sys prompt")
        .with_tools(vec![tool]);
    let body = client.build_body(req).unwrap();

    assert_eq!(body["system"][0]["type"], "text");
    assert_eq!(body["system"][0]["text"], "sys prompt");
    assert_eq!(body["tools"][0]["name"], "bash");
}

#[test]
fn build_headers_contains_required_keys() {
    let client = MinimaxClient::new(MinimaxConfig::international("test-key", "model"))
        .expect("client should build");
    let headers = client.build_headers().unwrap();

    assert_eq!(headers.get(CONTENT_TYPE).unwrap(), "application/json");
    assert_eq!(headers.get("x-api-key").unwrap(), "test-key");
    assert_eq!(headers.get("anthropic-version").unwrap(), ANTHROPIC_VERSION);
}

#[test]
fn tool_definition_roundtrip() {
    let tool = ToolDefinition {
        name: "bash".into(),
        description: "Run shell command".into(),
        input_schema: json!({
            "type": "object",
            "properties": {"command": {"type": "string"}},
            "required": ["command"]
        }),
    };
    let json = serde_json::to_value(&tool).unwrap();
    let back: ToolDefinition = serde_json::from_value(json).unwrap();
    assert_eq!(back, tool);
}

#[test]
fn client_error_display() {
    let err = ClientError::Config("missing key".into());
    assert_eq!(err.to_string(), "configuration error: missing key");

    let err = ClientError::Api {
        status: StatusCode::UNAUTHORIZED,
        body: "bad token".into(),
    };
    assert!(err.to_string().contains("401"));
    assert!(err.to_string().contains("bad token"));
}

#[test]
fn stop_reason_constants_match_api_values() {
    assert_eq!(STOP_REASON_END_TURN, "end_turn");
    assert_eq!(STOP_REASON_TOOL_USE, "tool_use");
    assert_eq!(STOP_REASON_MAX_TOKENS, "max_tokens");
    assert_eq!(STOP_REASON_STOP_SEQUENCE, "stop_sequence");
}

#[test]
fn minimax_client_can_be_wrapped_as_dyn() {
    let client = MinimaxClient::new(MinimaxConfig::international("key", MINIMAX_DEFAULT_MODEL))
        .expect("client should build");
    let _dyn_client: DynLlmClient = Arc::new(client);
}
