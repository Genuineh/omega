use futures_util::StreamExt;
use httpmock::prelude::*;
use omega_client::{
    AnthropicClient, AnthropicCountTokensRequest, AnthropicMessageBatchCreateRequest,
    AnthropicMessageBatchRequest, AnthropicMessageCreateRequest, AnthropicMessageParam, ChatEvent,
    ChatRequest, LlmClient, Message, MinimaxClient, MinimaxConfig, Role, Usage,
};
use serde_json::json;

#[tokio::test]
async fn minimax_client_preserves_headers_and_response_mapping() {
    let server = MockServer::start();
    let _mock = server.mock(|when, then| {
        when.method(POST)
            .path("/anthropic/v1/messages")
            .header("x-api-key", "test-key")
            .header("anthropic-version", "2023-06-01");
        then.status(200).json_body(json!({
            "id": "msg_1",
            "model": "MiniMax-M2.5",
            "content": [{"type": "text", "text": "hello"}],
            "stop_reason": "end_turn",
            "usage": {
                "input_tokens": 10,
                "output_tokens": 5,
                "cache_creation_input_tokens": 2
            }
        }));
    });

    let client = MinimaxClient::new(MinimaxConfig::with_base_url(
        "test-key",
        "MiniMax-M2.5",
        format!("{}/anthropic", server.base_url()),
    ))
    .expect("client should build");

    let response = client
        .chat(
            omega_client::ChatRequest::new(vec![omega_client::Message::user("hello")])
                .with_system("sys"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(response.text_content(), "hello");
    assert_eq!(response.usage.expect("usage should exist").input_tokens, 10);
}

#[tokio::test]
async fn anthropic_client_supports_count_tokens_models_and_batches() {
    let server = MockServer::start();
    let config = MinimaxConfig::with_base_url("test-key", "MiniMax-M2.5", server.base_url());
    let client = AnthropicClient::new(
        "minimax",
        config.anthropic_provider_config(),
        config.provider_capabilities(),
    )
    .expect("client should build");

    let _count_tokens = server.mock(|when, then| {
        when.method(POST).path("/v1/messages/count_tokens");
        then.status(200).json_body(json!({"input_tokens": 42}));
    });
    let _models = server.mock(|when, then| {
        when.method(GET).path("/v1/models");
        then.status(200)
            .json_body(json!({"data": [{"id": "model-1", "display_name": "Model 1"}]}));
    });
    let _create_batch = server.mock(|when, then| {
        when.method(POST).path("/v1/messages/batches");
        then.status(200).json_body(json!({
            "id": "batch-1",
            "processing_status": "in_progress"
        }));
    });
    let _get_batch = server.mock(|when, then| {
        when.method(GET).path("/v1/messages/batches/batch-1");
        then.status(200).json_body(json!({
            "id": "batch-1",
            "processing_status": "ended"
        }));
    });
    let _list_batches = server.mock(|when, then| {
        when.method(GET).path("/v1/messages/batches");
        then.status(200).json_body(json!({
            "data": [{"id": "batch-1", "processing_status": "ended"}]
        }));
    });
    let _batch_results = server.mock(|when, then| {
        when.method(GET).path("/v1/messages/batches/batch-1/results");
        then.status(200).body(
            "{\"custom_id\":\"req-1\",\"result\":{\"type\":\"succeeded\"}}\n{\"custom_id\":\"req-2\",\"error\":{\"type\":\"failed\"}}\n",
        );
    });

    let token_count = client
        .messages()
        .count_tokens(AnthropicCountTokensRequest {
            model: "MiniMax-M2.5".to_string(),
            messages: vec![AnthropicMessageParam::text(Role::User, "count me")],
            system: Vec::new(),
            tools: Vec::new(),
        })
        .await
        .expect("count_tokens should succeed");
    let models = client.models().list().await.expect("models should succeed");
    let batch = client
        .message_batches()
        .create(AnthropicMessageBatchCreateRequest {
            requests: vec![AnthropicMessageBatchRequest {
                custom_id: "req-1".to_string(),
                params: AnthropicMessageCreateRequest::new(
                    "MiniMax-M2.5",
                    vec![AnthropicMessageParam::text(Role::User, "hello")],
                    64,
                ),
            }],
        })
        .await
        .expect("batch create should succeed");
    let batch_get = client
        .message_batches()
        .get("batch-1")
        .await
        .expect("batch get should succeed");
    let batch_list = client
        .message_batches()
        .list()
        .await
        .expect("batch list should succeed");
    let results = client
        .message_batches()
        .results("batch-1")
        .await
        .expect("batch results should succeed");

    assert_eq!(token_count.input_tokens, 42);
    assert_eq!(models[0].id, "model-1");
    assert_eq!(batch.id, "batch-1");
    assert_eq!(batch_get.processing_status.as_deref(), Some("ended"));
    assert_eq!(batch_list.len(), 1);
    assert_eq!(results.len(), 2);
}

#[tokio::test]
async fn minimax_llm_client_count_tokens_uses_provider_endpoint() {
    let server = MockServer::start();
    let _count_tokens = server.mock(|when, then| {
        when.method(POST).path("/anthropic/v1/messages/count_tokens");
        then.status(200).json_body(json!({"input_tokens": 57}));
    });
    let client = MinimaxClient::new(MinimaxConfig::with_base_url(
        "test-key",
        "MiniMax-M2.5",
        format!("{}/anthropic", server.base_url()),
    ))
    .expect("client should build");

    let tokens = client
        .count_tokens(ChatRequest::new(vec![Message::user("count me")]).with_system("sys"))
        .await
        .expect("count_tokens should succeed");

    assert_eq!(tokens, 57);
}

#[tokio::test]
async fn minimax_chat_stream_falls_back_to_non_stream_when_stream_start_is_missing() {
    let server = MockServer::start();
    let _stream = server.mock(|when, then| {
        when.method(POST)
            .path("/anthropic/v1/messages")
            .header("accept", "text/event-stream")
            .body_contains("\"stream\":true");
        then.status(200)
            .header("content-type", "text/event-stream")
            .body(concat!(
                "event: content_block_delta\n",
                "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"broken\"}}\n\n",
                "event: message_stop\n",
                "data: {\"type\":\"message_stop\"}\n\n"
            ));
    });
    let _fallback = server.mock(|when, then| {
        when.method(POST)
            .path("/anthropic/v1/messages")
            .json_body(json!({
                "model": "MiniMax-M2.5",
                "messages": [{"role": "user", "content": "hello"}],
                "max_tokens": 8000
            }));
        then.status(200).json_body(json!({
            "id": "msg_fallback",
            "model": "MiniMax-M2.5",
            "content": [{"type": "text", "text": "fallback ok"}],
            "stop_reason": "end_turn",
            "usage": {
                "input_tokens": 12,
                "output_tokens": 4
            }
        }));
    });

    let client = MinimaxClient::new(MinimaxConfig::with_base_url(
        "test-key",
        "MiniMax-M2.5",
        format!("{}/anthropic", server.base_url()),
    ))
    .expect("client should build");

    let mut stream = client
        .chat_stream(ChatRequest::new(vec![Message::user("hello")]))
        .await
        .expect("chat_stream should fall back successfully");
    let mut events = Vec::new();
    while let Some(event) = stream.next().await {
        events.push(event.expect("event should succeed"));
    }

    assert_eq!(
        events,
        vec![
            ChatEvent::MessageStart {
                id: "msg_fallback".to_string(),
                model: Some("MiniMax-M2.5".to_string()),
            },
            ChatEvent::TextDelta {
                text: "fallback ok".to_string(),
            },
            ChatEvent::MessageComplete {
                stop_reason: Some("end_turn".to_string()),
                usage: Some(Usage {
                    input_tokens: 12,
                    output_tokens: 4,
                    cache_creation_input_tokens: None,
                    cache_read_input_tokens: None,
                }),
            },
        ]
    );
}
