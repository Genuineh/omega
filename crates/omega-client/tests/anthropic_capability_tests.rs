use omega_client::{
    AnthropicCacheControl, AnthropicClient, AnthropicCountTokensRequest,
    AnthropicMessageCreateRequest, AnthropicMessageParam, AnthropicProviderCapabilities,
    AnthropicProviderConfig, AnthropicSystemBlock, ClientError, Role,
};

fn limited_client(capabilities: AnthropicProviderCapabilities) -> AnthropicClient {
    let config = AnthropicProviderConfig::new(
        "test-key",
        "model-a",
        "https://api.example.com",
        "2023-06-01",
    );
    AnthropicClient::new("limited", config, capabilities).expect("client should build")
}

#[tokio::test]
async fn create_rejects_prompt_caching_when_provider_does_not_support_it() {
    let client = limited_client(AnthropicProviderCapabilities {
        tools: true,
        thinking: true,
        prompt_caching: false,
        streaming: true,
        count_tokens: true,
        models: true,
        message_batches: true,
    });
    let mut request = AnthropicMessageCreateRequest::new(
        "model-a",
        vec![AnthropicMessageParam::text(Role::User, "hello")],
        128,
    );
    request.system = vec![AnthropicSystemBlock {
        kind: "text".to_string(),
        text: "system".to_string(),
        cache_control: Some(AnthropicCacheControl::ephemeral()),
        citations: Vec::new(),
    }];

    let error = client
        .messages()
        .create(request)
        .await
        .expect_err("prompt caching should be rejected");

    match error {
        ClientError::UnsupportedCapability(error) => {
            assert_eq!(error.provider, "limited");
            assert_eq!(error.operation, "messages.prompt_caching");
        }
        other => panic!("unexpected error: {other}"),
    }
}

#[tokio::test]
async fn count_tokens_models_and_batches_return_capability_errors() {
    let client = limited_client(AnthropicProviderCapabilities {
        tools: true,
        thinking: true,
        prompt_caching: true,
        streaming: false,
        count_tokens: false,
        models: false,
        message_batches: false,
    });

    let count_error = client
        .messages()
        .count_tokens(AnthropicCountTokensRequest {
            model: "model-a".to_string(),
            messages: vec![AnthropicMessageParam::text(Role::User, "hello")],
            system: Vec::new(),
            tools: Vec::new(),
        })
        .await
        .expect_err("count_tokens should be rejected");
    let models_error = client
        .models()
        .list()
        .await
        .expect_err("models should be rejected");
    let batches_error = client
        .message_batches()
        .list()
        .await
        .expect_err("batches should be rejected");

    assert!(matches!(count_error, ClientError::UnsupportedCapability(_)));
    assert!(matches!(
        models_error,
        ClientError::UnsupportedCapability(_)
    ));
    assert!(matches!(
        batches_error,
        ClientError::UnsupportedCapability(_)
    ));
}
