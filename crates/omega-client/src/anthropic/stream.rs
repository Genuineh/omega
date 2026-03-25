use std::pin::Pin;

use futures_util::stream::Stream;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ClientError;

use super::types::{AnthropicContentBlock, AnthropicMessage, AnthropicUsage};

pub type AnthropicEventStream =
    Pin<Box<dyn Stream<Item = Result<AnthropicStreamEvent, ClientError>> + Send>>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AnthropicStreamEvent {
    MessageStart {
        id: String,
        #[serde(default)]
        model: Option<String>,
    },
    TextDelta {
        text: String,
    },
    ThinkingDelta {
        thinking: String,
        #[serde(default)]
        signature: Option<String>,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    MessageComplete {
        #[serde(default)]
        stop_reason: Option<String>,
        #[serde(default)]
        usage: Option<AnthropicUsage>,
    },
}

#[derive(Debug, Default)]
pub struct AnthropicMessageAccumulator {
    id: Option<String>,
    model: Option<String>,
    content: Vec<AnthropicContentBlock>,
    stop_reason: Option<String>,
    usage: Option<AnthropicUsage>,
    current_text: Option<String>,
    current_thinking: Option<(String, Option<String>)>,
}

impl AnthropicMessageAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_event(&mut self, event: AnthropicStreamEvent) -> Result<(), ClientError> {
        match event {
            AnthropicStreamEvent::MessageStart { id, model } => {
                if self.id.is_some() {
                    return Err(ClientError::Stream(
                        "anthropic stream emitted multiple message_start events".to_string(),
                    ));
                }
                self.id = Some(id);
                self.model = model;
            }
            AnthropicStreamEvent::TextDelta { text } => {
                self.flush_thinking_block();
                self.current_text
                    .get_or_insert_with(String::new)
                    .push_str(&text);
            }
            AnthropicStreamEvent::ThinkingDelta {
                thinking,
                signature,
            } => {
                self.flush_text_block();
                let current = self
                    .current_thinking
                    .get_or_insert_with(|| (String::new(), None));
                current.0.push_str(&thinking);
                if signature.is_some() {
                    current.1 = signature;
                }
            }
            AnthropicStreamEvent::ToolUse { id, name, input } => {
                self.flush_open_block();
                self.content
                    .push(AnthropicContentBlock::ToolUse { id, name, input });
            }
            AnthropicStreamEvent::MessageComplete { stop_reason, usage } => {
                self.stop_reason = stop_reason;
                self.usage = usage;
            }
        }

        Ok(())
    }

    pub fn finish(mut self) -> Result<AnthropicMessage, ClientError> {
        self.flush_open_block();
        let id = self.id.ok_or_else(|| {
            ClientError::Stream("anthropic stream finished without message_start".to_string())
        })?;

        Ok(AnthropicMessage {
            id,
            model: self.model,
            content: self.content,
            stop_reason: self.stop_reason,
            usage: self.usage,
        })
    }

    fn flush_open_block(&mut self) {
        self.flush_text_block();
        self.flush_thinking_block();
    }

    fn flush_text_block(&mut self) {
        if let Some(text) = self.current_text.take() {
            if !text.is_empty() {
                self.content.push(AnthropicContentBlock::text(text));
            }
        }
    }

    fn flush_thinking_block(&mut self) {
        if let Some((thinking, signature)) = self.current_thinking.take() {
            if !thinking.is_empty() {
                self.content.push(AnthropicContentBlock::Thinking {
                    thinking,
                    signature,
                });
            }
        }
    }
}

#[derive(Default)]
struct StreamParseState {
    pending_tool_use: Option<PendingToolUse>,
    stop_reason: Option<String>,
    usage: Option<AnthropicUsage>,
}

struct PendingToolUse {
    id: String,
    name: String,
    input_value: Option<Value>,
    partial_json: String,
}

pub fn parse_sse_events(body: &str) -> Result<Vec<AnthropicStreamEvent>, ClientError> {
    let mut events = Vec::new();
    let mut state = StreamParseState::default();

    for frame in body.split("\n\n") {
        let payload = extract_sse_data(frame);
        if payload.is_empty() || payload == "[DONE]" {
            continue;
        }

        let value: Value = serde_json::from_str(&payload).map_err(|error| {
            ClientError::Stream(format!(
                "failed to parse SSE payload: {error}: {payload}; {}",
                stream_body_diagnostics(body)
            ))
        })?;
        consume_sse_value(&value, &mut state, &mut events)?;
    }

    Ok(events)
}

pub(super) fn validate_stream_event_sequence(
    events: &[AnthropicStreamEvent],
    body: &str,
) -> Result<(), ClientError> {
    let start_count = events
        .iter()
        .filter(|event| matches!(event, AnthropicStreamEvent::MessageStart { .. }))
        .count();

    if start_count == 0 {
        return Err(ClientError::Stream(format!(
            "stream missing initial message_start; {}",
            stream_body_diagnostics(body)
        )));
    }

    if !matches!(
        events.first(),
        Some(AnthropicStreamEvent::MessageStart { .. })
    ) {
        return Err(ClientError::Stream(format!(
            "stream started with {:?} instead of message_start; {}",
            event_name(events.first()),
            stream_body_diagnostics(body)
        )));
    }

    if start_count > 1 {
        return Err(ClientError::Stream(format!(
            "stream emitted multiple message_start events; {}",
            stream_body_diagnostics(body)
        )));
    }

    Ok(())
}

pub(super) fn annotate_stream_error(
    provider_name: &str,
    path: &str,
    error: ClientError,
) -> ClientError {
    match error {
        ClientError::Stream(message) => {
            ClientError::Stream(format!("{provider_name} {path}: {message}"))
        }
        other => other,
    }
}

pub(super) fn parse_json_lines<T: DeserializeOwned>(body: &str) -> Result<Vec<T>, ClientError> {
    body.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str(line).map_err(|error| {
                ClientError::Decode(format!("failed to decode jsonl line: {error}"))
            })
        })
        .collect()
}

fn event_name(event: Option<&AnthropicStreamEvent>) -> &'static str {
    match event {
        Some(AnthropicStreamEvent::MessageStart { .. }) => "message_start",
        Some(AnthropicStreamEvent::TextDelta { .. }) => "text_delta",
        Some(AnthropicStreamEvent::ThinkingDelta { .. }) => "thinking_delta",
        Some(AnthropicStreamEvent::ToolUse { .. }) => "tool_use",
        Some(AnthropicStreamEvent::MessageComplete { .. }) => "message_complete",
        None => "no_event",
    }
}

fn stream_body_diagnostics(body: &str) -> String {
    let payloads = body
        .split("\n\n")
        .map(extract_sse_data)
        .filter(|payload| !payload.is_empty() && payload != "[DONE]")
        .collect::<Vec<_>>();
    let event_types = payloads
        .iter()
        .filter_map(|payload| {
            serde_json::from_str::<Value>(payload)
                .ok()
                .and_then(|value| {
                    value
                        .get("type")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
        })
        .take(6)
        .collect::<Vec<_>>();
    let preview = payloads
        .iter()
        .take(2)
        .map(|payload| preview_stream_fragment(payload, 120))
        .collect::<Vec<_>>()
        .join(" | ");

    format!(
        "frame_count={}, event_types={:?}, body_preview={} bytes:{}",
        payloads.len(),
        event_types,
        if preview.is_empty() {
            "<empty>"
        } else {
            preview.as_str()
        },
        body.len()
    )
}

fn preview_stream_fragment(text: &str, limit: usize) -> String {
    let mut chars = text.chars();
    let preview = chars.by_ref().take(limit).collect::<String>();
    let preview = preview.replace('\n', "\\n");
    if chars.next().is_some() {
        format!("{}...", preview)
    } else {
        preview
    }
}

fn extract_sse_data(frame: &str) -> String {
    frame
        .lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .map(str::trim)
        .collect::<Vec<_>>()
        .join("\n")
}

fn consume_sse_value(
    value: &Value,
    state: &mut StreamParseState,
    events: &mut Vec<AnthropicStreamEvent>,
) -> Result<(), ClientError> {
    let event_type = value
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| ClientError::Stream("SSE payload missing type".to_string()))?;

    match event_type {
        "ping" => {}
        "message_start" => {
            let message = value
                .get("message")
                .ok_or_else(|| ClientError::Stream("message_start missing message".to_string()))?;
            let id = message
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| ClientError::Stream("message_start missing id".to_string()))?;
            let model = message
                .get("model")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
            if let Some(usage) = parse_usage_field(message.get("usage"))? {
                state.usage = Some(usage);
            }
            events.push(AnthropicStreamEvent::MessageStart {
                id: id.to_string(),
                model,
            });
        }
        "content_block_start" => {
            let block = value.get("content_block").ok_or_else(|| {
                ClientError::Stream("content_block_start missing content_block".to_string())
            })?;
            match block.get("type").and_then(Value::as_str) {
                Some("text") => {
                    if let Some(text) = block.get("text").and_then(Value::as_str) {
                        if !text.is_empty() {
                            events.push(AnthropicStreamEvent::TextDelta {
                                text: text.to_string(),
                            });
                        }
                    }
                }
                Some("thinking") => {
                    if let Some(thinking) = block.get("thinking").and_then(Value::as_str) {
                        if !thinking.is_empty() {
                            events.push(AnthropicStreamEvent::ThinkingDelta {
                                thinking: thinking.to_string(),
                                signature: block
                                    .get("signature")
                                    .and_then(Value::as_str)
                                    .map(ToOwned::to_owned),
                            });
                        }
                    }
                }
                Some("tool_use") => {
                    let id = block
                        .get("id")
                        .and_then(Value::as_str)
                        .ok_or_else(|| ClientError::Stream("tool_use missing id".to_string()))?;
                    let name = block
                        .get("name")
                        .and_then(Value::as_str)
                        .ok_or_else(|| ClientError::Stream("tool_use missing name".to_string()))?;
                    state.pending_tool_use = Some(PendingToolUse {
                        id: id.to_string(),
                        name: name.to_string(),
                        input_value: block.get("input").cloned(),
                        partial_json: String::new(),
                    });
                }
                Some(other) => {
                    return Err(ClientError::Stream(format!(
                        "unsupported content_block_start type: {other}"
                    )))
                }
                None => {
                    return Err(ClientError::Stream(
                        "content_block_start missing content_block.type".to_string(),
                    ))
                }
            }
        }
        "content_block_delta" => {
            let delta = value.get("delta").ok_or_else(|| {
                ClientError::Stream("content_block_delta missing delta".to_string())
            })?;
            match delta.get("type").and_then(Value::as_str) {
                Some("text_delta") => {
                    let text = delta.get("text").and_then(Value::as_str).ok_or_else(|| {
                        ClientError::Stream("text_delta missing text".to_string())
                    })?;
                    events.push(AnthropicStreamEvent::TextDelta {
                        text: text.to_string(),
                    });
                }
                Some("thinking_delta") => {
                    let thinking =
                        delta
                            .get("thinking")
                            .and_then(Value::as_str)
                            .ok_or_else(|| {
                                ClientError::Stream("thinking_delta missing thinking".to_string())
                            })?;
                    events.push(AnthropicStreamEvent::ThinkingDelta {
                        thinking: thinking.to_string(),
                        signature: None,
                    });
                }
                Some("signature_delta") => {
                    let signature =
                        delta
                            .get("signature")
                            .and_then(Value::as_str)
                            .ok_or_else(|| {
                                ClientError::Stream("signature_delta missing signature".to_string())
                            })?;
                    events.push(AnthropicStreamEvent::ThinkingDelta {
                        thinking: String::new(),
                        signature: Some(signature.to_string()),
                    });
                }
                Some("input_json_delta") => {
                    let partial_json = delta
                        .get("partial_json")
                        .and_then(Value::as_str)
                        .ok_or_else(|| {
                            ClientError::Stream("input_json_delta missing partial_json".to_string())
                        })?;
                    let tool_use = state.pending_tool_use.as_mut().ok_or_else(|| {
                        ClientError::Stream(
                            "received input_json_delta without an open tool_use block".to_string(),
                        )
                    })?;
                    tool_use.partial_json.push_str(partial_json);
                }
                Some(other) => {
                    return Err(ClientError::Stream(format!(
                        "unsupported content_block_delta type: {other}"
                    )))
                }
                None => {
                    return Err(ClientError::Stream(
                        "content_block_delta missing delta.type".to_string(),
                    ))
                }
            }
        }
        "content_block_stop" => {
            if let Some(tool_use) = state.pending_tool_use.take() {
                let input = if !tool_use.partial_json.is_empty() {
                    serde_json::from_str(&tool_use.partial_json).map_err(|error| {
                        ClientError::Stream(format!(
                            "failed to parse tool_use partial_json: {error}"
                        ))
                    })?
                } else {
                    tool_use
                        .input_value
                        .unwrap_or(Value::Object(Default::default()))
                };
                events.push(AnthropicStreamEvent::ToolUse {
                    id: tool_use.id,
                    name: tool_use.name,
                    input,
                });
            }
        }
        "message_delta" => {
            if let Some(delta) = value.get("delta") {
                if let Some(stop_reason) = delta.get("stop_reason").and_then(Value::as_str) {
                    state.stop_reason = Some(stop_reason.to_string());
                }
            }
            if let Some(usage) = parse_usage_field(value.get("usage"))? {
                let current = state.usage.get_or_insert_with(AnthropicUsage::default);
                current.merge_from(&usage);
            }
        }
        "message_stop" => {
            events.push(AnthropicStreamEvent::MessageComplete {
                stop_reason: state.stop_reason.take(),
                usage: state.usage.take(),
            });
        }
        other => {
            return Err(ClientError::Stream(format!(
                "unsupported anthropic SSE event type: {other}"
            )))
        }
    }

    Ok(())
}

fn parse_usage_field(value: Option<&Value>) -> Result<Option<AnthropicUsage>, ClientError> {
    match value {
        None => Ok(None),
        Some(Value::Null) => Ok(None),
        Some(value) => serde_json::from_value(value.clone())
            .map(Some)
            .map_err(|error| ClientError::Stream(format!("failed to parse usage: {error}"))),
    }
}
