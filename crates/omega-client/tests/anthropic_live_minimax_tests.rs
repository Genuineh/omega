use futures_util::StreamExt;
use omega_client::{
    AnthropicCacheControl, AnthropicClient, AnthropicMessageCreateRequest, AnthropicMessageParam,
    AnthropicSystemBlock, ChatRequest, LlmClient, Message, MinimaxClient, MinimaxConfig, Role,
};

fn live_config() -> Option<MinimaxConfig> {
    MinimaxConfig::from_env().ok()
}

#[tokio::test]
#[ignore]
async fn live_minimax_chat_roundtrip() {
    let Some(config) = live_config() else {
        return;
    };
    let client = MinimaxClient::new(config).expect("client should build");

    let response = client
        .chat(ChatRequest::new(vec![Message::user("Reply with OK only")]).with_max_tokens(32))
        .await
        .expect("live chat should succeed");

    assert!(!response.text_content().trim().is_empty());
}

#[tokio::test]
#[ignore]
async fn live_minimax_prompt_cache_roundtrip() {
    let Some(config) = live_config() else {
        return;
    };
    let client = AnthropicClient::new(
        "minimax",
        config.anthropic_provider_config(),
        config.provider_capabilities(),
    )
    .expect("client should build");
    let mut request = AnthropicMessageCreateRequest::new(
        config.model.clone(),
        vec![AnthropicMessageParam::text(Role::User, "Say cache test")],
        64,
    );
    request.system = vec![AnthropicSystemBlock {
        kind: "text".to_string(),
        text: "Cached system".to_string(),
        cache_control: Some(AnthropicCacheControl::ephemeral()),
        citations: Vec::new(),
    }];

    let first = client
        .messages()
        .create(request.clone())
        .await
        .expect("first cached call should succeed");
    let second = client
        .messages()
        .create(request)
        .await
        .expect("second cached call should succeed");

    assert!(first.usage.is_some());
    assert!(second.usage.is_some());
}

#[tokio::test]
#[ignore]
async fn live_minimax_streaming_roundtrip() {
    let Some(config) = live_config() else {
        return;
    };
    let client = MinimaxClient::new(config).expect("client should build");
    let mut stream = client
        .chat_stream(
            ChatRequest::new(vec![Message::user("Reply with one short sentence")])
                .with_max_tokens(64),
        )
        .await
        .expect("stream should start");
    let mut event_count = 0usize;
    while let Some(event) = stream.next().await {
        event.expect("stream event should succeed");
        event_count += 1;
    }

    assert!(event_count > 0);
}
