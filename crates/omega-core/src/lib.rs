use std::path::PathBuf;

use anyhow::{anyhow, Result};
use futures_util::StreamExt;
use omega_client::{ChatRequest, ChatResponse, ContentBlock, ToolDefinition};
use omega_skills::LoadSkillHandler;
use omega_todo::{SharedTodoManager, TodoToolHandler};
use omega_tools::{ToolDispatcher, ToolErrorKind, ToolResult};
use omega_tools_builtin::{
    default_bash_allowed_commands as builtin_default_bash_allowed_commands,
    default_batch_max_requests as builtin_default_batch_max_requests, ApplyPatchHandler,
    BashHandler, BatchHandler, CreateFileHandler, EditHandler, GlobSearchHandler,
    GrepSearchHandler, ListDirHandler, ReadHandler, WriteHandler,
};
use tracing::{error, info, instrument};
use uuid::Uuid;

// Re-export construction types for downstream crates (e.g. omega-tui).
pub use omega_client::{ChatEvent, ChatResponseBuilder};
pub use omega_client::{
    ChatEventStream, ClientError, DynLlmClient, LlmClient, Message, MinimaxClient, MinimaxConfig,
    STOP_REASON_END_TURN, STOP_REASON_TOOL_USE,
};
pub use omega_todo::{
    SharedTodoManager as CoreSharedTodoManager, TodoItem, TodoManager, TodoStatus,
    TodoToolHandler as CoreTodoToolHandler,
};
pub use omega_tools::{ToolErrorKind as CoreToolErrorKind, ToolResult as CoreToolResult};

/// Core agent that implements the LLM ↔ tool execution loop.
///
/// Mirrors the Python reference: `learn-claude-code/agents/s01_agent_loop.py`
const DEFAULT_MAX_ITERATIONS: u32 = 100;

fn tool_not_visible_error(name: &str) -> String {
    format!("Error: Tool '{name}' is not available in this workflow step")
}

fn tool_result_block(tool_use_id: &str, result: &ToolResult) -> ContentBlock {
    ContentBlock::ToolResult {
        tool_use_id: tool_use_id.to_string(),
        content: serde_json::Value::String(result.output.clone()),
        is_error: result.is_error().then_some(true),
    }
}

fn todo_input_has_open_items(input: &serde_json::Value) -> Option<bool> {
    let items = input.get("items")?.clone();
    let items: Vec<TodoItem> = serde_json::from_value(items).ok()?;
    Some(
        items
            .iter()
            .any(|item| item.status != TodoStatus::Completed),
    )
}

pub struct Agent {
    client: DynLlmClient,
    dispatcher: ToolDispatcher,
    messages: Vec<Message>,
    system: String,
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
            .map(|v| serde_json::from_value(v).map_err(|e| anyhow!("invalid tool schema: {e}")))
            .collect::<Result<Vec<_>>>()?;

        Ok(Self {
            client,
            dispatcher,
            messages: Vec::new(),
            system,
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
        let request = ChatRequest::new(self.messages.clone())
            .with_system(&self.system)
            .with_max_tokens(self.max_tokens);

        let response = self.stream_chat_response(request, on_chat_event).await?;

        if response.is_tool_use() {
            return Err(anyhow!(
                "model requested tools during a no-tools workflow phase"
            ));
        }

        self.messages
            .push(Message::assistant(response.content.clone()));
        Ok(response.text_content())
    }

    /// Run the agent loop without tool-call callbacks.
    pub async fn run_loop(&mut self) -> Result<String> {
        self.run_loop_with(|_, _, _, _| {}).await
    }

    /// Run the agent loop, calling `on_tool_call(tool_use_id, name, input, result)` after
    /// each tool execution so the caller can display progress.
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
        mut on_tool_call: F,
        mut on_chat_event: E,
    ) -> Result<String>
    where
        F: FnMut(&str, &str, &serde_json::Value, &ToolResult),
        E: FnMut(&ChatEvent),
    {
        let session_id = Uuid::new_v4().to_string();
        let todo_enabled = self.dispatcher.has_tool("todo");
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

            // Create agent_loop span for each iteration
            let _agent_loop_span = tracing::info_span!(
                "agent_loop",
                agent_loop.iteration = iterations,
                agent_loop.message_count = self.messages.len()
            );
            let _guard = _agent_loop_span.enter();

            let request = ChatRequest::new(self.messages.clone())
                .with_system(&self.system)
                .with_tools(self.tool_definitions.clone())
                .with_max_tokens(self.max_tokens);

            let response = self
                .stream_chat_response(request, &mut on_chat_event)
                .await?;

            // Record stop_reason
            if let Some(ref stop_reason) = response.stop_reason {
                tracing::Span::current().record("agent_loop.stop_reason", stop_reason.as_str());
            }

            // Append assistant turn
            self.messages
                .push(Message::assistant(response.content.clone()));

            // If the model didn't call a tool, we're done
            if !response.is_tool_use() {
                info!(
                    agent_loop.session_id = %session_id,
                    agent_loop.iterations = iterations,
                    agent_loop.final_message_count = self.messages.len(),
                    agent_loop.completed = true
                );
                return Ok(response.text_content());
            }

            // Execute each tool call, collect results
            let mut results = Vec::new();
            let mut updated_todo = false;
            for block in &response.content {
                if let ContentBlock::ToolUse { id, name, input } = block {
                    let result = if !self.is_tool_visible(name) {
                        let err_result =
                            ToolResult::error(tool_not_visible_error(name), ToolErrorKind::Policy);
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
                            Err(e) => {
                                let err_result =
                                    ToolResult::error(e.to_string(), ToolErrorKind::Execution);
                                on_tool_call(id, name, input, &err_result);
                                tool_result_block(id, &err_result)
                            }
                        }
                    };
                    results.push(result);
                }
            }

            if todo_enabled && todo_has_open_items {
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
            } else if updated_todo {
                rounds_since_todo = 0;
            }

            self.messages.push(Message::tool_results(results));
        }
    }

    async fn stream_chat_response<F>(
        &self,
        request: ChatRequest,
        mut on_chat_event: F,
    ) -> Result<ChatResponse>
    where
        F: FnMut(&ChatEvent),
    {
        let mut stream = self
            .client
            .chat_stream(request)
            .await
            .map_err(|e| anyhow!("{e}"))?;
        let mut builder = ChatResponseBuilder::new();

        while let Some(event) = stream.next().await {
            let event = event.map_err(|e| anyhow!("{e}"))?;
            on_chat_event(&event);
            builder.push_event(event).map_err(|e| anyhow!("{e}"))?;
        }

        builder.finish().map_err(|e| anyhow!("{e}"))
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
        self.tool_definitions
            .iter()
            .any(|definition| definition.name == name)
    }
}

/// Create a ToolDispatcher with all built-in tools.
pub fn create_default_tools(root: PathBuf) -> ToolDispatcher {
    create_default_tools_with_todo_manager(
        root,
        std::sync::Arc::new(std::sync::Mutex::new(TodoManager::new())),
    )
}

pub fn default_bash_allowed_commands() -> Vec<String> {
    builtin_default_bash_allowed_commands()
}

pub fn default_batch_max_requests() -> usize {
    builtin_default_batch_max_requests()
}

pub fn create_default_tools_with_todo_manager(
    root: PathBuf,
    todo_manager: SharedTodoManager,
) -> ToolDispatcher {
    create_default_tools_with_todo_manager_and_tool_limits(
        root,
        todo_manager,
        default_bash_allowed_commands(),
        default_batch_max_requests(),
    )
}

pub fn create_default_tools_with_todo_manager_and_bash_allowlist(
    root: PathBuf,
    todo_manager: SharedTodoManager,
    bash_allowed_commands: Vec<String>,
) -> ToolDispatcher {
    create_default_tools_with_todo_manager_and_tool_limits(
        root,
        todo_manager,
        bash_allowed_commands,
        default_batch_max_requests(),
    )
}

pub fn create_default_tools_with_todo_manager_and_tool_limits(
    root: PathBuf,
    todo_manager: SharedTodoManager,
    bash_allowed_commands: Vec<String>,
    batch_max_requests: usize,
) -> ToolDispatcher {
    let mut dispatcher = ToolDispatcher::new();
    dispatcher.register(Box::new(BashHandler::with_allowed_commands(
        root.clone(),
        bash_allowed_commands,
    )));
    dispatcher.register(Box::new(BatchHandler::with_max_requests(
        root.clone(),
        batch_max_requests,
    )));
    dispatcher.register(Box::new(ListDirHandler::new(root.clone())));
    dispatcher.register(Box::new(GlobSearchHandler::new(root.clone())));
    dispatcher.register(Box::new(GrepSearchHandler::new(root.clone())));
    dispatcher.register(Box::new(ReadHandler::new(root.clone())));
    dispatcher.register(Box::new(CreateFileHandler::new(root.clone())));
    dispatcher.register(Box::new(WriteHandler::new(root.clone())));
    dispatcher.register(Box::new(EditHandler::new(root.clone())));
    dispatcher.register(Box::new(ApplyPatchHandler::new(root.clone())));
    if let Ok(handler) = LoadSkillHandler::from_repo_root(&root) {
        dispatcher.register(Box::new(handler));
    }
    dispatcher.register(Box::new(TodoToolHandler::with_manager(todo_manager)));
    dispatcher
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
