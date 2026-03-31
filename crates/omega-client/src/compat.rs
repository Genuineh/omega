use futures_util::{stream, StreamExt};
use tracing::warn;

use crate::anthropic::{
    AnthropicClient, AnthropicContentBlock, AnthropicCountTokensRequest, AnthropicEventStream,
    AnthropicMessage, AnthropicMessageContent, AnthropicMessageCreateRequest,
    AnthropicMessageParam, AnthropicStreamEvent, AnthropicSystemBlock, AnthropicToolDefinition,
};
use crate::{
    ChatEvent, ChatEventStream, ChatRequest, ChatResponse, ClientError, ContentBlock, Message,
    MessageContent, ToolDefinition, Usage,
};

#[derive(Debug, Clone)]
pub struct AnthropicMessagesCompatClient {
    client: AnthropicClient,
}

impl AnthropicMessagesCompatClient {
    pub fn new(client: AnthropicClient) -> Self {
        Self { client }
    }

    pub fn provider_name(&self) -> &'static str {
        self.client.provider_name()
    }

    pub fn client(&self) -> &AnthropicClient {
        &self.client
    }

    #[cfg(test)]
    pub(crate) fn messages_endpoint(&self) -> String {
        self.client.messages_endpoint()
    }

    #[cfg(test)]
    pub(crate) fn build_headers(
        &self,
        betas: &[String],
    ) -> Result<reqwest::header::HeaderMap, ClientError> {
        self.client.build_headers(betas)
    }

    #[cfg(test)]
    pub(crate) fn build_body(
        &self,
        request: ChatRequest,
    ) -> Result<serde_json::Value, ClientError> {
        let anthropic_request =
            chat_request_to_anthropic_request(request, &self.client.config().default_model);
        self.client.messages().body_value(&anthropic_request)
    }

    pub async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, ClientError> {
        let anthropic_request =
            chat_request_to_anthropic_request(request, &self.client.config().default_model);
        let response = self.client.messages().create(anthropic_request).await?;
        Ok(anthropic_message_to_chat_response(response))
    }

    pub async fn count_tokens(&self, request: ChatRequest) -> Result<u32, ClientError> {
        let anthropic_request =
            chat_request_to_count_tokens_request(request, &self.client.config().default_model);
        let response = self
            .client
            .messages()
            .count_tokens(anthropic_request)
            .await?;
        Ok(response.input_tokens)
    }

    pub async fn chat_stream(&self, request: ChatRequest) -> Result<ChatEventStream, ClientError> {
        let fallback_request = request.clone();
        let anthropic_request =
            chat_request_to_anthropic_request(request, &self.client.config().default_model);
        match self
            .client
            .messages()
            .create_stream(anthropic_request)
            .await
        {
            Ok(stream) => Ok(map_anthropic_stream(stream)),
            Err(ClientError::Stream(stream_error)) => {
                warn!(
                    provider = self.provider_name(),
                    stream_error = %stream_error,
                    "streaming response invalid; falling back to non-stream chat"
                );
                let response = self
                    .chat(fallback_request)
                    .await
                    .map_err(|fallback_error| {
                        ClientError::Stream(format!(
                            "{stream_error}; non-stream fallback failed: {fallback_error}"
                        ))
                    })?;
                Ok(Box::pin(stream::iter(
                    response.to_events().into_iter().map(Ok),
                )))
            }
            Err(error) => Err(error),
        }
    }
}

pub(crate) fn chat_request_to_anthropic_request(
    request: ChatRequest,
    default_model: &str,
) -> AnthropicMessageCreateRequest {
    let last_assistant_index = request
        .messages
        .iter()
        .rposition(|message| message.role == crate::Role::Assistant);
    let messages = request
        .messages
        .into_iter()
        .enumerate()
        .map(|(index, message)| {
            message_to_anthropic(
                message,
                request.cache_last_assistant_turn && Some(index) == last_assistant_index,
            )
        })
        .collect();
    let mut anthropic_request =
        AnthropicMessageCreateRequest::new(default_model.to_string(), messages, request.max_tokens);

    if let Some(system) = request.system {
        anthropic_request.system = vec![AnthropicSystemBlock::text(system)];
    }
    anthropic_request.system.extend(
        request
            .system_blocks
            .into_iter()
            .map(system_block_to_anthropic),
    );
    let tool_len = request.tools.len();
    anthropic_request.tools = request
        .tools
        .into_iter()
        .enumerate()
        .map(|(index, tool)| tool_to_anthropic(tool, index + 1 == tool_len))
        .collect();

    anthropic_request
}

fn chat_request_to_count_tokens_request(
    request: ChatRequest,
    default_model: &str,
) -> AnthropicCountTokensRequest {
    let last_assistant_index = request
        .messages
        .iter()
        .rposition(|message| message.role == crate::Role::Assistant);
    let messages = request
        .messages
        .into_iter()
        .enumerate()
        .map(|(index, message)| {
            message_to_anthropic(
                message,
                request.cache_last_assistant_turn && Some(index) == last_assistant_index,
            )
        })
        .collect();
    let mut system = Vec::new();
    if let Some(system_text) = request.system {
        system.push(AnthropicSystemBlock::text(system_text));
    }
    system.extend(
        request
            .system_blocks
            .into_iter()
            .map(system_block_to_anthropic),
    );
    let tool_len = request.tools.len();
    let tools = request
        .tools
        .into_iter()
        .enumerate()
        .map(|(index, tool)| tool_to_anthropic(tool, index + 1 == tool_len))
        .collect();

    AnthropicCountTokensRequest {
        model: default_model.to_string(),
        messages,
        system,
        tools,
    }
}

fn message_to_anthropic(message: Message, cache_last_text_block: bool) -> AnthropicMessageParam {
    match message.content {
        MessageContent::Text(text) => {
            if cache_last_text_block && message.role == crate::Role::Assistant {
                AnthropicMessageParam {
                    role: message.role,
                    content: AnthropicMessageContent::Blocks(vec![AnthropicContentBlock::Text {
                        text,
                        cache_control: Some(crate::AnthropicCacheControl::ephemeral()),
                        citations: Vec::new(),
                    }]),
                }
            } else {
                AnthropicMessageParam {
                    role: message.role,
                    content: AnthropicMessageContent::Text(text),
                }
            }
        }
        MessageContent::Blocks(blocks) => AnthropicMessageParam {
            role: message.role,
            content: AnthropicMessageContent::Blocks(content_blocks_to_anthropic(
                blocks,
                cache_last_text_block,
            )),
        },
    }
}

fn system_block_to_anthropic(block: crate::SystemBlock) -> AnthropicSystemBlock {
    AnthropicSystemBlock {
        kind: "text".to_string(),
        text: block.text,
        cache_control: block
            .cache_control
            .map(|cache_control| crate::AnthropicCacheControl {
                kind: cache_control.kind,
            }),
        citations: Vec::new(),
    }
}

fn content_blocks_to_anthropic(
    blocks: Vec<ContentBlock>,
    cache_last_text_block: bool,
) -> Vec<AnthropicContentBlock> {
    let last_text_index = if cache_last_text_block {
        blocks
            .iter()
            .rposition(|block| matches!(block, ContentBlock::Text { .. }))
    } else {
        None
    };
    blocks
        .into_iter()
        .enumerate()
        .map(|(index, block)| content_block_to_anthropic(block, Some(index) == last_text_index))
        .collect()
}

fn content_block_to_anthropic(block: ContentBlock, cache_control: bool) -> AnthropicContentBlock {
    match block {
        ContentBlock::Text { text } => AnthropicContentBlock::Text {
            text,
            cache_control: cache_control.then(crate::AnthropicCacheControl::ephemeral),
            citations: Vec::new(),
        },
        ContentBlock::Thinking {
            thinking,
            signature,
        } => AnthropicContentBlock::Thinking {
            thinking,
            signature,
        },
        ContentBlock::ToolUse { id, name, input } => {
            AnthropicContentBlock::ToolUse { id, name, input }
        }
        ContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => AnthropicContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        },
    }
}

fn tool_to_anthropic(tool: ToolDefinition, cache_control: bool) -> AnthropicToolDefinition {
    AnthropicToolDefinition {
        name: tool.name,
        description: tool.description,
        input_schema: tool.input_schema,
        cache_control: cache_control.then(crate::AnthropicCacheControl::ephemeral),
        strict: None,
    }
}

pub(crate) fn anthropic_message_to_chat_response(message: AnthropicMessage) -> ChatResponse {
    ChatResponse {
        id: message.id,
        model: message.model,
        content: message
            .content
            .into_iter()
            .map(anthropic_content_block_to_chat)
            .collect(),
        stop_reason: message.stop_reason,
        usage: message.usage.map(|usage| Usage {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            cache_creation_input_tokens: usage.cache_creation_input_tokens,
            cache_read_input_tokens: usage.cache_read_input_tokens,
        }),
    }
}

fn anthropic_content_block_to_chat(block: AnthropicContentBlock) -> ContentBlock {
    match block {
        AnthropicContentBlock::Text { text, .. } => ContentBlock::Text { text },
        AnthropicContentBlock::Thinking {
            thinking,
            signature,
        } => ContentBlock::Thinking {
            thinking,
            signature,
        },
        AnthropicContentBlock::ToolUse { id, name, input } => {
            ContentBlock::ToolUse { id, name, input }
        }
        AnthropicContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => ContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        },
    }
}

fn map_anthropic_stream(stream: AnthropicEventStream) -> ChatEventStream {
    Box::pin(stream.map(|event| event.map(anthropic_stream_event_to_chat_event)))
}

fn anthropic_stream_event_to_chat_event(event: AnthropicStreamEvent) -> ChatEvent {
    match event {
        AnthropicStreamEvent::MessageStart { id, model } => ChatEvent::MessageStart { id, model },
        AnthropicStreamEvent::TextDelta { text } => ChatEvent::TextDelta { text },
        AnthropicStreamEvent::ThinkingDelta {
            thinking,
            signature,
        } => ChatEvent::ThinkingDelta {
            thinking,
            signature,
        },
        AnthropicStreamEvent::ToolUse { id, name, input } => ChatEvent::ToolUse { id, name, input },
        AnthropicStreamEvent::MessageComplete { stop_reason, usage } => {
            ChatEvent::MessageComplete {
                stop_reason,
                usage: usage.map(|usage| Usage {
                    input_tokens: usage.input_tokens,
                    output_tokens: usage.output_tokens,
                    cache_creation_input_tokens: usage.cache_creation_input_tokens,
                    cache_read_input_tokens: usage.cache_read_input_tokens,
                }),
            }
        }
    }
}
