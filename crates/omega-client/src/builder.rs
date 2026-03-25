use crate::{ChatEvent, ChatResponse, ClientError, ContentBlock, Usage};

#[derive(Debug, Default)]
pub struct ChatResponseBuilder {
    id: Option<String>,
    model: Option<String>,
    content: Vec<ContentBlock>,
    stop_reason: Option<String>,
    usage: Option<Usage>,
    current_text: Option<String>,
    current_thinking: Option<(String, Option<String>)>,
}

impl ChatResponseBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_event(&mut self, event: ChatEvent) -> Result<(), ClientError> {
        match event {
            ChatEvent::MessageStart { id, model } => {
                if self.id.is_some() {
                    return Err(ClientError::Stream(
                        "chat stream emitted multiple message_start events".to_string(),
                    ));
                }
                self.id = Some(id);
                self.model = model;
            }
            ChatEvent::TextDelta { text } => {
                self.flush_thinking_block();
                self.current_text
                    .get_or_insert_with(String::new)
                    .push_str(&text);
            }
            ChatEvent::ThinkingDelta {
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
            ChatEvent::ToolUse { id, name, input } => {
                self.flush_open_block();
                self.content.push(ContentBlock::ToolUse { id, name, input });
            }
            ChatEvent::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                self.flush_open_block();
                self.content.push(ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                });
            }
            ChatEvent::MessageComplete { stop_reason, usage } => {
                self.stop_reason = stop_reason;
                self.usage = usage;
            }
        }

        Ok(())
    }

    pub fn finish(mut self) -> Result<ChatResponse, ClientError> {
        self.flush_open_block();
        let id = self.id.ok_or_else(|| {
            ClientError::Stream("chat stream finished without message_start".to_string())
        })?;

        Ok(ChatResponse {
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
                self.content.push(ContentBlock::Text { text });
            }
        }
    }

    fn flush_thinking_block(&mut self) {
        if let Some((thinking, signature)) = self.current_thinking.take() {
            if !thinking.is_empty() {
                self.content.push(ContentBlock::Thinking {
                    thinking,
                    signature,
                });
            }
        }
    }
}
