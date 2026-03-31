use std::collections::VecDeque;
use std::sync::Mutex;

use async_trait::async_trait;
use futures_util::stream;

use crate::{
    ChatEvent, ChatEventStream, ChatRequest, ChatResponse, ChatResponseBuilder, ClientError,
    LlmClient,
};

#[derive(Debug)]
pub enum ScriptedLlmStep {
    Response(ChatResponse),
    Stream(Vec<Result<ChatEvent, ClientError>>),
    Failure(ClientError),
    CountTokens(Result<u32, ClientError>),
}

#[derive(Debug, Default)]
struct ScriptedLlmState {
    chat_steps: VecDeque<ScriptedLlmStep>,
    count_token_steps: VecDeque<Result<u32, ClientError>>,
    chat_requests: Vec<ChatRequest>,
    count_token_requests: Vec<ChatRequest>,
}

#[derive(Debug)]
pub struct ScriptedLlmClient {
    provider_name: &'static str,
    state: Mutex<ScriptedLlmState>,
}

impl ScriptedLlmClient {
    pub fn builder() -> ScriptedLlmClientBuilder {
        ScriptedLlmClientBuilder::default()
    }

    pub fn from_responses<I>(responses: I) -> Self
    where
        I: IntoIterator<Item = ChatResponse>,
    {
        let mut builder = Self::builder();
        for response in responses {
            builder = builder.push_response(response);
        }
        builder.build()
    }

    pub fn recorded_requests(&self) -> Vec<ChatRequest> {
        self.state.lock().unwrap().chat_requests.clone()
    }

    pub fn recorded_count_token_requests(&self) -> Vec<ChatRequest> {
        self.state.lock().unwrap().count_token_requests.clone()
    }

    pub fn recorded_systems(&self) -> Vec<Option<String>> {
        self.recorded_requests()
            .into_iter()
            .map(|request| {
                let mut sections = Vec::new();
                if let Some(system) = request.system {
                    if !system.is_empty() {
                        sections.push(system);
                    }
                }
                sections.extend(
                    request
                        .system_blocks
                        .into_iter()
                        .map(|block| block.text)
                        .filter(|text| !text.is_empty()),
                );

                if sections.is_empty() {
                    None
                } else {
                    Some(sections.join("\n\n"))
                }
            })
            .collect()
    }

    pub fn recorded_max_tokens(&self) -> Vec<u32> {
        self.recorded_requests()
            .into_iter()
            .map(|request| request.max_tokens)
            .collect()
    }

    pub fn remaining_steps(&self) -> usize {
        let state = self.state.lock().unwrap();
        state.chat_steps.len() + state.count_token_steps.len()
    }

    fn next_chat_step(&self, request: ChatRequest) -> Result<ScriptedLlmStep, ClientError> {
        let mut state = self.state.lock().unwrap();
        state.chat_requests.push(request);
        state.chat_steps.pop_front().ok_or_else(|| {
            ClientError::Stream("scripted llm client exhausted scripted steps".to_string())
        })
    }

    fn next_count_step(
        &self,
        request: ChatRequest,
    ) -> Result<Result<u32, ClientError>, ClientError> {
        let mut state = self.state.lock().unwrap();
        state.count_token_requests.push(request);
        state.count_token_steps.pop_front().ok_or_else(|| {
            ClientError::Stream(
                "scripted llm client exhausted scripted count_tokens steps".to_string(),
            )
        })
    }
}

#[async_trait]
impl LlmClient for ScriptedLlmClient {
    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, ClientError> {
        match self.next_chat_step(request)? {
            ScriptedLlmStep::Response(response) => Ok(response),
            ScriptedLlmStep::Stream(events) => scripted_events_to_response(events),
            ScriptedLlmStep::Failure(error) => Err(error),
            ScriptedLlmStep::CountTokens(_) => Err(ClientError::Stream(
                "scripted llm client received count_tokens step during chat".to_string(),
            )),
        }
    }

    async fn chat_stream(&self, request: ChatRequest) -> Result<ChatEventStream, ClientError> {
        match self.next_chat_step(request)? {
            ScriptedLlmStep::Response(response) => Ok(Box::pin(stream::iter(
                response.to_events().into_iter().map(Ok),
            ))),
            ScriptedLlmStep::Stream(events) => Ok(Box::pin(stream::iter(events.into_iter()))),
            ScriptedLlmStep::Failure(error) => Err(error),
            ScriptedLlmStep::CountTokens(_) => Err(ClientError::Stream(
                "scripted llm client received count_tokens step during chat_stream".to_string(),
            )),
        }
    }

    async fn count_tokens(&self, request: ChatRequest) -> Result<u32, ClientError> {
        self.next_count_step(request)?
    }

    fn provider_name(&self) -> &'static str {
        self.provider_name
    }
}

#[derive(Debug, Default)]
pub struct ScriptedLlmClientBuilder {
    provider_name: Option<&'static str>,
    chat_steps: VecDeque<ScriptedLlmStep>,
    count_token_steps: VecDeque<Result<u32, ClientError>>,
}

impl ScriptedLlmClientBuilder {
    pub fn with_provider_name(mut self, provider_name: &'static str) -> Self {
        self.provider_name = Some(provider_name);
        self
    }

    pub fn push_response(mut self, response: ChatResponse) -> Self {
        self.chat_steps
            .push_back(ScriptedLlmStep::Response(response));
        self
    }

    pub fn push_stream<I>(mut self, events: I) -> Self
    where
        I: IntoIterator<Item = ChatEvent>,
    {
        self.chat_steps.push_back(ScriptedLlmStep::Stream(
            events.into_iter().map(Ok).collect(),
        ));
        self
    }

    pub fn push_stream_results<I>(mut self, events: I) -> Self
    where
        I: IntoIterator<Item = Result<ChatEvent, ClientError>>,
    {
        self.chat_steps
            .push_back(ScriptedLlmStep::Stream(events.into_iter().collect()));
        self
    }

    pub fn push_failure(mut self, error: ClientError) -> Self {
        self.chat_steps.push_back(ScriptedLlmStep::Failure(error));
        self
    }

    pub fn push_count_tokens(mut self, count: u32) -> Self {
        self.count_token_steps.push_back(Ok(count));
        self
    }

    pub fn push_count_tokens_error(mut self, error: ClientError) -> Self {
        self.count_token_steps.push_back(Err(error));
        self
    }

    pub fn build(self) -> ScriptedLlmClient {
        ScriptedLlmClient {
            provider_name: self.provider_name.unwrap_or("scripted"),
            state: Mutex::new(ScriptedLlmState {
                chat_steps: self.chat_steps,
                count_token_steps: self.count_token_steps,
                chat_requests: Vec::new(),
                count_token_requests: Vec::new(),
            }),
        }
    }
}

#[derive(Debug, Clone)]
pub struct IdleLlmClient {
    provider_name: &'static str,
    panic_message: &'static str,
}

impl IdleLlmClient {
    pub const fn new(panic_message: &'static str) -> Self {
        Self {
            provider_name: "idle",
            panic_message,
        }
    }

    pub const fn with_provider_name(
        provider_name: &'static str,
        panic_message: &'static str,
    ) -> Self {
        Self {
            provider_name,
            panic_message,
        }
    }
}

impl Default for IdleLlmClient {
    fn default() -> Self {
        Self::new("chat should not be called in this test")
    }
}

#[async_trait]
impl LlmClient for IdleLlmClient {
    async fn chat(&self, _request: ChatRequest) -> Result<ChatResponse, ClientError> {
        panic!("{}", self.panic_message);
    }

    fn provider_name(&self) -> &'static str {
        self.provider_name
    }
}

fn scripted_events_to_response(
    events: Vec<Result<ChatEvent, ClientError>>,
) -> Result<ChatResponse, ClientError> {
    let mut builder = ChatResponseBuilder::new();
    for event in events {
        builder.push_event(event?)?;
    }
    builder.finish()
}

#[cfg(test)]
mod tests {
    use futures_util::StreamExt;
    use serde_json::json;

    use crate::{
        ChatEvent, ChatRequest, ClientError, ContentBlock, LlmClient, Message, STOP_REASON_END_TURN,
    };

    use super::{scripted_events_to_response, ScriptedLlmClient, ScriptedLlmClientBuilder};

    #[test]
    fn scripted_client_records_request_metadata() {
        let client = ScriptedLlmClient::from_responses(vec![crate::ChatResponse {
            id: "msg-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("ok")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        }]);

        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime
            .block_on(client.chat(ChatRequest::new(vec![Message::user("hi")]).with_system("sys")))
            .unwrap();

        assert_eq!(client.recorded_systems(), vec![Some("sys".to_string())]);
        assert_eq!(client.recorded_max_tokens(), vec![8_000]);
        assert_eq!(client.remaining_steps(), 0);
    }

    #[test]
    fn scripted_events_can_drive_stream_and_sync_paths() {
        let sync_events = vec![
            Ok(ChatEvent::MessageStart {
                id: "msg-1".to_string(),
                model: Some("test-model".to_string()),
            }),
            Ok(ChatEvent::TextDelta {
                text: "hello ".to_string(),
            }),
            Ok(ChatEvent::ToolUse {
                id: "tool-1".to_string(),
                name: "bash".to_string(),
                input: json!({"command": "pwd"}),
            }),
            Ok(ChatEvent::MessageComplete {
                stop_reason: Some(crate::STOP_REASON_TOOL_USE.to_string()),
                usage: None,
            }),
        ];
        let stream_events = vec![
            Ok(ChatEvent::MessageStart {
                id: "msg-1".to_string(),
                model: Some("test-model".to_string()),
            }),
            Ok(ChatEvent::TextDelta {
                text: "hello ".to_string(),
            }),
            Ok(ChatEvent::ToolUse {
                id: "tool-1".to_string(),
                name: "bash".to_string(),
                input: json!({"command": "pwd"}),
            }),
            Ok(ChatEvent::MessageComplete {
                stop_reason: Some(crate::STOP_REASON_TOOL_USE.to_string()),
                usage: None,
            }),
        ];
        let expected = scripted_events_to_response(sync_events).unwrap();

        let sync_client = ScriptedLlmClientBuilder::default()
            .push_stream_results(stream_events)
            .build();
        let stream_client = ScriptedLlmClientBuilder::default()
            .push_stream(vec![
                ChatEvent::MessageStart {
                    id: "msg-1".to_string(),
                    model: Some("test-model".to_string()),
                },
                ChatEvent::TextDelta {
                    text: "hello ".to_string(),
                },
                ChatEvent::ToolUse {
                    id: "tool-1".to_string(),
                    name: "bash".to_string(),
                    input: json!({"command": "pwd"}),
                },
                ChatEvent::MessageComplete {
                    stop_reason: Some(crate::STOP_REASON_TOOL_USE.to_string()),
                    usage: None,
                },
            ])
            .build();
        let request = ChatRequest::new(vec![Message::user("hi")]);
        let runtime = tokio::runtime::Runtime::new().unwrap();

        let sync_response = runtime.block_on(sync_client.chat(request.clone())).unwrap();
        assert_eq!(sync_response, expected);

        let streamed = runtime.block_on(async {
            stream_client
                .chat_stream(request)
                .await
                .unwrap()
                .collect::<Vec<_>>()
                .await
        });
        assert_eq!(streamed.len(), 4);
        assert!(streamed.into_iter().all(|event| event.is_ok()));
    }

    #[test]
    fn scripted_client_supports_count_tokens_and_records_requests() {
        let client = ScriptedLlmClientBuilder::default()
            .push_count_tokens(123)
            .push_response(crate::ChatResponse {
                id: "msg-1".to_string(),
                model: Some("test-model".to_string()),
                content: vec![ContentBlock::text("ok")],
                stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                usage: None,
            })
            .build();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let request = ChatRequest::new(vec![Message::user("hi")]);

        let count = runtime
            .block_on(client.count_tokens(request.clone()))
            .unwrap();
        let response = runtime.block_on(client.chat(request.clone())).unwrap();

        assert_eq!(count, 123);
        assert_eq!(response.text_content(), "ok");
        assert_eq!(
            client.recorded_count_token_requests(),
            vec![request.clone()]
        );
        assert_eq!(client.recorded_requests(), vec![request]);
    }

    #[test]
    fn scripted_client_supports_midstream_failure() {
        let client = ScriptedLlmClientBuilder::default()
            .push_stream_results(vec![
                Ok(ChatEvent::MessageStart {
                    id: "msg-1".to_string(),
                    model: Some("test-model".to_string()),
                }),
                Err(ClientError::Stream("socket closed".to_string())),
            ])
            .build();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let request = ChatRequest::new(vec![Message::user("hi")]);

        let error = runtime.block_on(client.chat(request)).unwrap_err();

        assert_eq!(error.to_string(), "stream processing failed: socket closed");
    }
}
