use omega_client::{
    AnthropicCacheControl, AnthropicContentBlock, AnthropicMessageCreateRequest,
    AnthropicMessageParam, AnthropicSystemBlock, AnthropicThinkingConfig, AnthropicToolChoice,
    AnthropicToolDefinition, Role,
};
use serde_json::json;

#[test]
fn anthropic_request_serializes_system_blocks_tools_and_cache_markers() {
    let mut request = AnthropicMessageCreateRequest::new(
        "model-x",
        vec![
            AnthropicMessageParam::text(Role::User, "hello"),
            AnthropicMessageParam::blocks(
                Role::Assistant,
                vec![AnthropicContentBlock::Text {
                    text: "cached context".to_string(),
                    cache_control: Some(AnthropicCacheControl::ephemeral()),
                    citations: Vec::new(),
                }],
            ),
        ],
        4096,
    );
    request.system = vec![AnthropicSystemBlock {
        kind: "text".to_string(),
        text: "system".to_string(),
        cache_control: Some(AnthropicCacheControl::ephemeral()),
        citations: Vec::new(),
    }];
    request.tools = vec![AnthropicToolDefinition {
        name: "read_file".to_string(),
        description: "Read a file".to_string(),
        input_schema: json!({"type": "object"}),
        cache_control: Some(AnthropicCacheControl::ephemeral()),
        strict: Some(true),
    }];
    request.tool_choice = Some(AnthropicToolChoice::Auto);
    request.thinking = Some(AnthropicThinkingConfig::Enabled {
        budget_tokens: 1024,
    });
    request.betas = vec!["prompt-caching-2024-07-31".to_string()];

    let value = serde_json::to_value(&request).expect("request should serialize");

    assert_eq!(value["model"], "model-x");
    assert_eq!(value["system"][0]["type"], "text");
    assert_eq!(value["system"][0]["cache_control"]["type"], "ephemeral");
    assert_eq!(value["tools"][0]["cache_control"]["type"], "ephemeral");
    assert_eq!(value["tool_choice"]["type"], "auto");
    assert_eq!(value["thinking"]["type"], "enabled");
    assert_eq!(
        value["messages"][1]["content"][0]["cache_control"]["type"],
        "ephemeral"
    );
    assert!(request.contains_cache_control());
}

#[test]
fn anthropic_request_omits_false_stream_flag() {
    let request = AnthropicMessageCreateRequest::new(
        "model-x",
        vec![AnthropicMessageParam::text(Role::User, "hello")],
        128,
    );

    let value = serde_json::to_value(&request).expect("request should serialize");

    assert!(value.get("stream").is_none());
}
