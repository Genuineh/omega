use anyhow::{anyhow, Result};
use futures_util::StreamExt;
use omega_client::{ChatRequest, ChatResponse, ContentBlock, SystemBlock, ToolDefinition};
use omega_tools::{ToolDispatcher, ToolErrorKind, ToolResult};
use tokio::sync::watch;
use tracing::{error, info, instrument};
use uuid::Uuid;

use crate::helpers::{todo_input_has_open_items, tool_not_visible_error, tool_result_block};
use crate::{ChatEvent, ChatResponseBuilder, DynLlmClient, Message};

/// Core agent that implements the LLM ↔ tool execution loop.
///
/// Mirrors the Python reference: `learn-claude-code/agents/s01_agent_loop.py`
const DEFAULT_MAX_ITERATIONS: u32 = 100;

pub struct Agent {
    client: DynLlmClient,
    dispatcher: ToolDispatcher,
    messages: Vec<Message>,
    system: String,
    system_blocks: Vec<SystemBlock>,
    tool_definitions: Vec<ToolDefinition>,
    all_tool_definitions: Vec<ToolDefinition>,
    max_tokens: u32,
    max_iterations: u32,
}

impl Agent {
    pub fn new(client: DynLlmClient, system: String, dispatcher: ToolDispatcher) -> Result<Self> {
        let tool_definitions: Vec<ToolDefinition> = dispatcher
            .to_schemas()
            .into_iter()
            .map(|value| {
                serde_json::from_value(value)
                    .map_err(|error| anyhow!("invalid tool schema: {error}"))
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(Self {
            client,
            dispatcher,
            messages: Vec::new(),
            system,
            system_blocks: Vec::new(),
            all_tool_definitions: tool_definitions.clone(),
            tool_definitions,
            max_tokens: 8_000,
            max_iterations: DEFAULT_MAX_ITERATIONS,
        })
    }

    pub fn add_user_message(&mut self, content: &str) {
        self.messages.push(Message::user(content));
    }

    pub async fn run_single_response(&mut self) -> Result<String> {
        self.run_single_response_with_events(|_| {}).await
    }

    pub async fn run_single_response_with_events<F>(&mut self, on_chat_event: F) -> Result<String>
    where
        F: FnMut(&ChatEvent),
    {
        let request = self
            .base_request()
            .with_cache_last_assistant_turn(true)
            .with_max_tokens(self.max_tokens);

        let response = self
            .stream_chat_response(request, on_chat_event, None, None)
            .await?;

        if response.is_tool_use() {
            return Err(anyhow!(
                "model requested tools during a no-tools workflow phase"
            ));
        }

        self.messages
            .push(Message::assistant(response.content.clone()));
        Ok(response.text_content())
    }

    pub async fn run_loop(&mut self) -> Result<String> {
        self.run_loop_with(|_, _, _, _| {}).await
    }

    #[instrument(
        skip(self, on_tool_call),
        fields(
            agent_loop.session_id,
            agent_loop.iteration,
            agent_loop.message_count,
            agent_loop.stop_reason
        )
    )]
    pub async fn run_loop_with<F>(&mut self, on_tool_call: F) -> Result<String>
    where
        F: FnMut(&str, &str, &serde_json::Value, &ToolResult),
    {
        self.run_loop_with_events(on_tool_call, |_| {}).await
    }

    pub async fn run_loop_with_events<F, E>(
        &mut self,
        on_tool_call: F,
        on_chat_event: E,
    ) -> Result<String>
    where
        F: FnMut(&str, &str, &serde_json::Value, &ToolResult),
        E: FnMut(&ChatEvent),
    {
        self.run_loop_with_events_until_turn_change(on_tool_call, on_chat_event, None, None)
            .await
    }

    pub async fn run_loop_with_events_until_turn_change<F, E>(
        &mut self,
        mut on_tool_call: F,
        mut on_chat_event: E,
        mut turn_guard: Option<&mut watch::Receiver<u64>>,
        active_turn_id: Option<u64>,
    ) -> Result<String>
    where
        F: FnMut(&str, &str, &serde_json::Value, &ToolResult),
        E: FnMut(&ChatEvent),
    {
        let session_id = Uuid::new_v4().to_string();
        let mut rounds_since_todo = 0usize;
        let mut todo_has_open_items = false;
        tracing::Span::current().record("agent_loop.session_id", &session_id);

        info!(agent_loop.started = true, agent_loop.session_id = %session_id);

        let mut iterations = 0u32;
        loop {
            iterations += 1;
            if iterations > self.max_iterations {
                error!(agent_loop.session_id = %session_id, agent_loop.iterations = iterations);
                return Err(anyhow!(
                    "agent loop exceeded {} iterations",
                    self.max_iterations
                ));
            }

            let _agent_loop_span = tracing::info_span!(
                "agent_loop",
                agent_loop.iteration = iterations,
                agent_loop.message_count = self.messages.len()
            );
            let _guard = _agent_loop_span.enter();

            let request = self
                .base_request()
                .with_cache_last_assistant_turn(true)
                .with_tools(self.tool_definitions.clone())
                .with_max_tokens(self.max_tokens);

            let response = self
                .stream_chat_response(
                    request,
                    &mut on_chat_event,
                    turn_guard.as_deref_mut(),
                    active_turn_id,
                )
                .await?;

            if let Some(ref stop_reason) = response.stop_reason {
                tracing::Span::current().record("agent_loop.stop_reason", stop_reason.as_str());
            }

            self.messages
                .push(Message::assistant(response.content.clone()));

            if !response.is_tool_use() {
                info!(
                    agent_loop.session_id = %session_id,
                    agent_loop.iterations = iterations,
                    agent_loop.final_message_count = self.messages.len(),
                    agent_loop.completed = true
                );
                return Ok(response.text_content());
            }

            let mut results = Vec::new();
            let mut updated_todo = false;
            for block in &response.content {
                if let ContentBlock::ToolUse { id, name, input } = block {
                    let result = if !self.is_tool_visible(name) {
                        let err_result = self.dispatcher.error_result(
                            name,
                            tool_not_visible_error(name),
                            ToolErrorKind::Policy,
                        );
                        on_tool_call(id, name, input, &err_result);
                        tool_result_block(id, &err_result)
                    } else {
                        match self.dispatcher.dispatch(name, input.clone()) {
                            Ok(result) => {
                                if name == "todo" && !result.is_error() {
                                    updated_todo = true;
                                    if let Some(has_open_items) = todo_input_has_open_items(input) {
                                        todo_has_open_items = has_open_items;
                                    }
                                }
                                on_tool_call(id, name, input, &result);
                                tool_result_block(id, &result)
                            }
                            Err(error) => {
                                let err_result = self.dispatcher.error_result(
                                    name,
                                    error.to_string(),
                                    ToolErrorKind::Execution,
                                );
                                on_tool_call(id, name, input, &err_result);
                                tool_result_block(id, &err_result)
                            }
                        }
                    };
                    results.push(result);
                }
            }

            let todo_visible = self.is_tool_visible("todo") || self.is_tool_visible("todo_write");
            if todo_visible && todo_has_open_items {
                rounds_since_todo = if updated_todo {
                    0
                } else {
                    rounds_since_todo.saturating_add(1)
                };

                if rounds_since_todo >= 3 {
                    results.insert(
                        0,
                        ContentBlock::text("<reminder>Update your todos.</reminder>"),
                    );
                }
            } else if updated_todo || !todo_visible {
                rounds_since_todo = 0;
            }

            self.messages.push(Message::tool_results(results));
        }
    }

    async fn stream_chat_response<F>(
        &self,
        request: ChatRequest,
        mut on_chat_event: F,
        mut turn_guard: Option<&mut watch::Receiver<u64>>,
        active_turn_id: Option<u64>,
    ) -> Result<ChatResponse>
    where
        F: FnMut(&ChatEvent),
    {
        let mut stream = self
            .client
            .chat_stream(request)
            .await
            .map_err(|error| anyhow!("{error}"))?;
        let mut builder = ChatResponseBuilder::new();

        loop {
            if turn_guard
                .as_ref()
                .zip(active_turn_id)
                .is_some_and(|(guard, turn_id)| *guard.borrow() != turn_id)
            {
                return Err(anyhow!("agent turn canceled"));
            }

            let next_event = match turn_guard.as_mut().zip(active_turn_id) {
                Some((guard, turn_id)) => {
                    tokio::select! {
                        changed = guard.changed() => {
                            match changed {
                                Ok(()) if *guard.borrow() != turn_id => {
                                    return Err(anyhow!("agent turn canceled"));
                                }
                                Ok(()) => {
                                    continue;
                                }
                                Err(_) => stream.next().await,
                            }
                        }
                        event = stream.next() => event,
                    }
                }
                None => stream.next().await,
            };

            let Some(event) = next_event else {
                break;
            };

            let event = event.map_err(|error| anyhow!("{error}"))?;
            on_chat_event(&event);
            builder
                .push_event(event)
                .map_err(|error| anyhow!("{error}"))?;
        }

        builder.finish().map_err(|error| anyhow!("{error}"))
    }

    fn base_request(&self) -> ChatRequest {
        let request =
            ChatRequest::new(self.messages.clone()).with_system_blocks(self.system_blocks.clone());
        if self.system.is_empty() {
            request
        } else {
            request.with_system(&self.system)
        }
    }

    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    pub fn set_messages(&mut self, messages: Vec<Message>) {
        self.messages = messages;
    }

    pub fn set_max_tokens(&mut self, max_tokens: u32) {
        self.max_tokens = max_tokens;
    }

    pub fn set_max_iterations(&mut self, max_iterations: u32) {
        self.max_iterations = max_iterations;
    }

    pub fn set_system(&mut self, system: String) {
        self.system = system;
        self.system_blocks.clear();
    }

    pub fn set_system_blocks(&mut self, system_blocks: Vec<SystemBlock>) {
        self.system.clear();
        self.system_blocks = system_blocks;
    }

    pub fn set_visible_tools(&mut self, names: Option<&[&str]>) -> Vec<String> {
        let tool_definitions = match names {
            Some(names) => self
                .dispatcher
                .to_schemas_filtered(names)
                .into_iter()
                .map(|value| {
                    serde_json::from_value(value)
                        .map_err(|error| anyhow!("invalid tool schema: {error}"))
                })
                .collect::<Result<Vec<ToolDefinition>>>()
                .expect("dispatcher generated invalid filtered tool schema"),
            None => self.all_tool_definitions.clone(),
        };

        let names = tool_definitions
            .iter()
            .map(|definition| definition.name.clone())
            .collect::<Vec<_>>();
        self.tool_definitions = tool_definitions;
        names
    }

    pub fn visible_tool_names(&self) -> Vec<&str> {
        self.tool_definitions
            .iter()
            .map(|definition| definition.name.as_str())
            .collect()
    }

    fn is_tool_visible(&self, name: &str) -> bool {
        if self
            .tool_definitions
            .iter()
            .any(|definition| definition.name == name)
        {
            return true;
        }

        self.dispatcher.manifest_for(name).is_some_and(|manifest| {
            self.tool_definitions
                .iter()
                .any(|definition| definition.name == manifest.id)
        })
    }
}
