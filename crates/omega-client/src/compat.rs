use futures_util::StreamExt;

use crate::anthropic::{
    AnthropicClient, AnthropicContentBlock, AnthropicEventStream, AnthropicMessage,
    AnthropicMessageContent, AnthropicMessageCreateRequest, AnthropicMessageParam,
    AnthropicStreamEvent, AnthropicSystemBlock, AnthropicToolDefinition,
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

    pub async fn chat_stream(&self, request: ChatRequest) -> Result<ChatEventStream, ClientError> {
        let anthropic_request =
            chat_request_to_anthropic_request(request, &self.client.config().default_model);
        let stream = self
            .client
            .messages()
            .create_stream(anthropic_request)
            .await?;
        Ok(map_anthropic_stream(stream))
    }
}

pub(crate) fn chat_request_to_anthropic_request(
    request: ChatRequest,
    default_model: &str,
) -> AnthropicMessageCreateRequest {
    let mut anthropic_request = AnthropicMessageCreateRequest::new(
        default_model.to_string(),
        request
            .messages
            .into_iter()
            .map(message_to_anthropic)
            .collect(),
        request.max_tokens,
    );

    if let Some(system) = request.system {
        anthropic_request.system = vec![AnthropicSystemBlock::text(system)];
    }
    if !request.tools.is_empty() {
        anthropic_request.tools = request.tools.into_iter().map(tool_to_anthropic).collect();
    }

    anthropic_request
}

fn message_to_anthropic(message: Message) -> AnthropicMessageParam {
    match message.content {
        MessageContent::Text(text) => AnthropicMessageParam {
            role: message.role,
            content: AnthropicMessageContent::Text(text),
        },
        MessageContent::Blocks(blocks) => AnthropicMessageParam {
            role: message.role,
            content: AnthropicMessageContent::Blocks(
                blocks.into_iter().map(content_block_to_anthropic).collect(),
            ),
        },
    }
}

fn content_block_to_anthropic(block: ContentBlock) -> AnthropicContentBlock {
    match block {
        ContentBlock::Text { text } => AnthropicContentBlock::text(text),
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

fn tool_to_anthropic(tool: ToolDefinition) -> AnthropicToolDefinition {
    AnthropicToolDefinition {
        name: tool.name,
        description: tool.description,
        input_schema: tool.input_schema,
        cache_control: None,
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
                }),
            }
        }
    }
}
