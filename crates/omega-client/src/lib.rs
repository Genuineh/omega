#[cfg(test)]
use std::env;

#[cfg(test)]
use reqwest::header::CONTENT_TYPE;
#[cfg(test)]
use reqwest::StatusCode;
#[cfg(test)]
use serde_json::json;

mod builder;
mod minimax;
mod types;

pub mod anthropic;
pub mod compat;

pub use anthropic::{
    parse_sse_events, AnthropicBatchResult, AnthropicCacheControl, AnthropicClient,
    AnthropicContentBlock, AnthropicCountTokensRequest, AnthropicEventStream, AnthropicMessage,
    AnthropicMessageAccumulator, AnthropicMessageBatch, AnthropicMessageBatchCreateRequest,
    AnthropicMessageBatchRequest, AnthropicMessageBatchRequestCounts, AnthropicMessageContent,
    AnthropicMessageCreateRequest, AnthropicMessageParam, AnthropicModelInfo,
    AnthropicProviderCapabilities, AnthropicProviderConfig, AnthropicStreamEvent,
    AnthropicSystemBlock, AnthropicThinkingConfig, AnthropicTokenCount, AnthropicToolChoice,
    AnthropicToolDefinition, AnthropicUsage,
};
pub use builder::ChatResponseBuilder;
pub use compat::AnthropicMessagesCompatClient;
#[cfg(test)]
pub(crate) use minimax::ANTHROPIC_VERSION;
pub use minimax::{
    MinimaxClient, MinimaxConfig, MINIMAX_CHINA_BASE_URL, MINIMAX_DEFAULT_MODEL,
    MINIMAX_GLOBAL_BASE_URL,
};
pub use types::{
    ChatEvent, ChatEventStream, ChatRequest, ChatResponse, ClientError, ContentBlock, DynLlmClient,
    LlmClient, Message, MessageContent, ProviderCapabilityError, Role, ToolDefinition, Usage,
};

pub const STOP_REASON_END_TURN: &str = "end_turn";
pub const STOP_REASON_TOOL_USE: &str = "tool_use";
pub const STOP_REASON_MAX_TOKENS: &str = "max_tokens";
pub const STOP_REASON_STOP_SEQUENCE: &str = "stop_sequence";

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
