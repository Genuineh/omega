use futures_util::StreamExt;
use httpmock::prelude::*;
use omega_client::{
    AnthropicClient, AnthropicMessageAccumulator, AnthropicMessageCreateRequest,
    AnthropicMessageParam, AnthropicProviderCapabilities, AnthropicProviderConfig,
    AnthropicStreamEvent, AnthropicUsage, Role,
};

fn test_client(base_url: String) -> AnthropicClient {
    let config = AnthropicProviderConfig::new("test-key", "model-a", base_url, "2023-06-01");
    AnthropicClient::new(
        "mock-anthropic",
        config,
        AnthropicProviderCapabilities::minimax(),
    )
    .expect("client should build")
}

#[tokio::test]
async fn create_stream_parses_sse_into_typed_events_and_accumulates() {
    let server = MockServer::start();
    let sse_body = concat!(
        "event: message_start\n",
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"model\":\"model-a\",\"usage\":{\"input_tokens\":11}}}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"plan\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"sig-1\"}}\n\n",
        "event: content_block_stop\n",
        "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"tool-1\",\"name\":\"bash\",\"input\":{}}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"command\\\":\\\"pwd\\\"}\"}}\n\n",
        "event: content_block_stop\n",
        "data: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":2,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":2,\"delta\":{\"type\":\"text_delta\",\"text\":\"done\"}}\n\n",
        "event: content_block_stop\n",
        "data: {\"type\":\"content_block_stop\",\"index\":2}\n\n",
        "event: message_delta\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":7,\"cache_read_input_tokens\":5}}\n\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n\n"
    );

    let _mock = server.mock(|when, then| {
        when.method(POST).path("/v1/messages");
        then.status(200)
            .header("content-type", "text/event-stream")
            .body(sse_body);
    });

    let client = test_client(server.base_url());
    let request = AnthropicMessageCreateRequest::new(
        "model-a",
        vec![AnthropicMessageParam::text(Role::User, "hello")],
        256,
    );

    let mut stream = client
        .messages()
        .create_stream(request)
        .await
        .expect("stream should parse");
    let mut events = Vec::new();
    while let Some(event) = stream.next().await {
        events.push(event.expect("event should decode"));
    }

    assert_eq!(
        events,
        vec![
            AnthropicStreamEvent::MessageStart {
                id: "msg_1".to_string(),
                model: Some("model-a".to_string()),
            },
            AnthropicStreamEvent::ThinkingDelta {
                thinking: "plan".to_string(),
                signature: None,
            },
            AnthropicStreamEvent::ThinkingDelta {
                thinking: String::new(),
                signature: Some("sig-1".to_string()),
            },
            AnthropicStreamEvent::ToolUse {
                id: "tool-1".to_string(),
                name: "bash".to_string(),
                input: serde_json::json!({"command": "pwd"}),
            },
            AnthropicStreamEvent::TextDelta {
                text: "done".to_string(),
            },
            AnthropicStreamEvent::MessageComplete {
                stop_reason: Some("tool_use".to_string()),
                usage: Some(AnthropicUsage {
                    input_tokens: 11,
                    output_tokens: 7,
                    cache_creation_input_tokens: None,
                    cache_read_input_tokens: Some(5),
                }),
            },
        ]
    );

    let mut accumulator = AnthropicMessageAccumulator::new();
    for event in events {
        accumulator
            .push_event(event)
            .expect("accumulator should accept event");
    }
    let message = accumulator.finish().expect("message should finish");

    assert_eq!(message.id, "msg_1");
    assert_eq!(message.content.len(), 3);
    assert_eq!(message.stop_reason.as_deref(), Some("tool_use"));
    assert_eq!(
        message
            .usage
            .expect("usage should exist")
            .cache_read_input_tokens,
        Some(5)
    );
}
