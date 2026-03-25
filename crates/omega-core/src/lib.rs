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
mod tests {
    use super::*;
    use async_trait::async_trait;
    use omega_client::ChatEvent;
    use omega_client::{
        ChatResponse, ClientError, MessageContent, Usage, STOP_REASON_END_TURN,
        STOP_REASON_TOOL_USE,
    };
    use std::sync::{Arc, Mutex};

    type RecordedToolCall = (String, String, String, Option<String>, String);

    // ── Mock LLM client ───────────────────────────────────────────────

    struct MockLlmClient {
        responses: Mutex<Vec<ChatResponse>>,
    }

    impl MockLlmClient {
        fn new(responses: Vec<ChatResponse>) -> Self {
            Self {
                responses: Mutex::new(responses),
            }
        }
    }

    #[async_trait]
    impl LlmClient for MockLlmClient {
        async fn chat(&self, _request: ChatRequest) -> Result<ChatResponse, ClientError> {
            let mut responses = self.responses.lock().unwrap();
            assert!(!responses.is_empty(), "MockLlmClient: no more responses");
            Ok(responses.remove(0))
        }

        fn provider_name(&self) -> &'static str {
            "mock"
        }
    }

    // ── Helpers ───────────────────────────────────────────────────────

    fn text_response(text: &str) -> ChatResponse {
        ChatResponse {
            id: "msg_test".to_string(),
            model: Some("mock".to_string()),
            content: vec![ContentBlock::text(text)],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: Some(Usage {
                input_tokens: 10,
                output_tokens: 5,
            }),
        }
    }

    fn tool_use_response(tool_id: &str, name: &str, input: serde_json::Value) -> ChatResponse {
        ChatResponse {
            id: "msg_test".to_string(),
            model: Some("mock".to_string()),
            content: vec![ContentBlock::tool_use(tool_id, name, input)],
            stop_reason: Some(STOP_REASON_TOOL_USE.to_string()),
            usage: Some(Usage {
                input_tokens: 10,
                output_tokens: 5,
            }),
        }
    }

    fn make_agent(responses: Vec<ChatResponse>) -> Agent {
        let client: DynLlmClient = Arc::new(MockLlmClient::new(responses));
        let tmp = std::env::temp_dir().join("omega-core-test");
        let _ = std::fs::create_dir_all(&tmp);
        let dispatcher = create_default_tools(tmp);
        Agent::new(client, "Test system prompt.".to_string(), dispatcher).unwrap()
    }

    // ── Tests ─────────────────────────────────────────────────────────

    #[tokio::test]
    async fn simple_text_response_terminates() {
        let mut agent = make_agent(vec![text_response("Hello!")]);
        agent.add_user_message("hi");
        let result = agent.run_loop().await.unwrap();
        assert_eq!(result, "Hello!");
    }

    #[tokio::test]
    async fn single_response_uses_system_without_tools() {
        struct RecordingClient {
            systems: Mutex<Vec<Option<String>>>,
        }

        #[async_trait]
        impl LlmClient for RecordingClient {
            async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, ClientError> {
                self.systems.lock().unwrap().push(request.system.clone());
                assert!(request.tools.is_empty());
                Ok(text_response("planned"))
            }

            fn provider_name(&self) -> &'static str {
                "recording"
            }
        }

        let client = Arc::new(RecordingClient {
            systems: Mutex::new(Vec::new()),
        });
        let dispatcher = create_default_tools(std::env::temp_dir());
        let mut agent = Agent::new(client.clone(), "phase prompt".to_string(), dispatcher).unwrap();
        agent.add_user_message("go");

        let result = agent.run_single_response().await.unwrap();

        assert_eq!(result, "planned");
        assert_eq!(
            client.systems.lock().unwrap().as_slice(),
            &[Some("phase prompt".to_string())]
        );
    }

    #[tokio::test]
    async fn messages_recorded() {
        let mut agent = make_agent(vec![text_response("done")]);
        agent.add_user_message("query");
        agent.run_loop().await.unwrap();
        // user + assistant = 2
        assert_eq!(agent.messages().len(), 2);
    }

    #[tokio::test]
    async fn tool_use_then_text() {
        let mut agent = make_agent(vec![
            tool_use_response("t1", "bash", serde_json::json!({"command": "echo hello"})),
            text_response("Done!"),
        ]);
        agent.add_user_message("run echo");
        let result = agent.run_loop().await.unwrap();
        assert_eq!(result, "Done!");
        // user + assistant(tool_use) + user(tool_result) + assistant(text) = 4
        assert_eq!(agent.messages().len(), 4);
    }

    #[tokio::test]
    async fn unknown_tool_returns_feedback() {
        let mut agent = make_agent(vec![
            tool_use_response("t1", "nonexistent", serde_json::json!({})),
            text_response("Tool not found."),
        ]);
        agent.add_user_message("try unknown");
        let result = agent.run_loop().await.unwrap();
        assert_eq!(result, "Tool not found.");
    }

    #[tokio::test]
    async fn callback_receives_tool_output() {
        let calls: Arc<Mutex<Vec<RecordedToolCall>>> = Arc::new(Mutex::new(Vec::new()));
        let calls_clone = calls.clone();

        let client: DynLlmClient = Arc::new(MockLlmClient::new(vec![
            tool_use_response(
                "t1",
                "bash",
                serde_json::json!({"command": "echo callback_test"}),
            ),
            text_response("ok"),
        ]));
        let tmp = std::env::temp_dir().join("omega-core-cb-test");
        let _ = std::fs::create_dir_all(&tmp);
        let dispatcher = create_default_tools(tmp);
        let mut agent = Agent::new(client, "Test".to_string(), dispatcher).unwrap();
        agent.add_user_message("go");

        agent
            .run_loop_with(|tool_use_id, name, _input, output| {
                calls_clone.lock().unwrap().push((
                    tool_use_id.to_string(),
                    name.to_string(),
                    output.output.clone(),
                    output.preview.clone(),
                    output.metadata["command"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                ));
            })
            .await
            .unwrap();

        let recorded = calls.lock().unwrap();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].0, "t1");
        assert_eq!(recorded[0].1, "bash");
        assert!(recorded[0].2.contains("callback_test"));
        assert!(recorded[0]
            .3
            .as_deref()
            .is_some_and(|preview| preview.contains("callback_test")));
        assert_eq!(recorded[0].4, "echo callback_test");
    }

    #[tokio::test]
    async fn response_event_callback_receives_text_and_completion() {
        let events: Arc<Mutex<Vec<ChatEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let events_clone = events.clone();

        let client: DynLlmClient = Arc::new(MockLlmClient::new(vec![ChatResponse {
            id: "msg_test".to_string(),
            model: Some("mock".to_string()),
            content: vec![
                ContentBlock::Thinking {
                    thinking: "draft".to_string(),
                    signature: None,
                },
                ContentBlock::text("Hello!"),
            ],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: Some(Usage {
                input_tokens: 10,
                output_tokens: 5,
            }),
        }]));
        let tmp = std::env::temp_dir().join("omega-core-event-cb-test");
        let _ = std::fs::create_dir_all(&tmp);
        let dispatcher = create_default_tools(tmp);
        let mut agent = Agent::new(client, "Test".to_string(), dispatcher).unwrap();
        agent.add_user_message("go");

        let result = agent
            .run_single_response_with_events(|event| {
                events_clone.lock().unwrap().push(event.clone());
            })
            .await
            .unwrap();

        assert_eq!(result, "Hello!");
        assert_eq!(
            events.lock().unwrap().as_slice(),
            &[
                ChatEvent::MessageStart {
                    id: "msg_test".to_string(),
                    model: Some("mock".to_string()),
                },
                ChatEvent::ThinkingDelta {
                    thinking: "draft".to_string(),
                    signature: None,
                },
                ChatEvent::TextDelta {
                    text: "Hello!".to_string(),
                },
                ChatEvent::MessageComplete {
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: Some(Usage {
                        input_tokens: 10,
                        output_tokens: 5,
                    }),
                },
            ]
        );
    }

    #[test]
    fn create_default_tools_includes_bash() {
        let dispatcher = create_default_tools(std::env::temp_dir());
        assert!(dispatcher.has_tool("apply_patch"));
        assert!(dispatcher.has_tool("bash"));
        assert!(dispatcher.has_tool("batch"));
        assert!(dispatcher.has_tool("create_file"));
        assert!(dispatcher.has_tool("list_dir"));
        assert!(dispatcher.has_tool("glob_search"));
        assert!(dispatcher.has_tool("grep_search"));
        assert!(dispatcher.has_tool("load_skill"));
        assert!(dispatcher.has_tool("todo"));
    }

    #[test]
    fn tool_definitions_deserialize() {
        let dispatcher = create_default_tools(std::env::temp_dir());
        let schemas = dispatcher.to_schemas();
        let defs: Vec<ToolDefinition> = schemas
            .into_iter()
            .map(|v| serde_json::from_value(v).unwrap())
            .collect();
        assert_eq!(defs.len(), 12);
        let names: Vec<&str> = defs.iter().map(|def| def.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "apply_patch",
                "bash",
                "batch",
                "create_file",
                "edit_file",
                "glob_search",
                "grep_search",
                "list_dir",
                "load_skill",
                "read_file",
                "todo",
                "write_file"
            ]
        );
    }

    #[tokio::test]
    async fn injects_todo_reminder_after_three_rounds_without_todo() {
        let mut agent = make_agent(vec![
            tool_use_response(
                "t0",
                "todo",
                serde_json::json!({
                    "items": [
                        {"id": "1", "text": "Plan", "status": "completed"},
                        {"id": "2", "text": "Code", "status": "in_progress", "activeForm": "coding"}
                    ]
                }),
            ),
            tool_use_response("t1", "bash", serde_json::json!({"command": "echo step1"})),
            tool_use_response("t2", "bash", serde_json::json!({"command": "echo step2"})),
            tool_use_response("t3", "bash", serde_json::json!({"command": "echo step3"})),
            text_response("Done."),
        ]);
        agent.add_user_message("multi step");

        let result = agent.run_loop().await.unwrap();

        assert_eq!(result, "Done.");
        assert_eq!(agent.messages().len(), 10);

        let reminder_message = &agent.messages()[8];
        let MessageContent::Blocks(blocks) = &reminder_message.content else {
            panic!("expected tool result blocks");
        };

        assert!(matches!(
            &blocks[0],
            ContentBlock::Text { text } if text == "<reminder>Update your todos.</reminder>"
        ));
        assert!(matches!(&blocks[1], ContentBlock::ToolResult { .. }));
    }

    #[tokio::test]
    async fn todo_tool_resets_reminder_counter() {
        let mut agent = make_agent(vec![
            tool_use_response("t1", "bash", serde_json::json!({"command": "echo step1"})),
            tool_use_response("t2", "bash", serde_json::json!({"command": "echo step2"})),
            tool_use_response(
                "t3",
                "todo",
                serde_json::json!({
                    "items": [
                        {"id": "1", "text": "Plan", "status": "completed"},
                        {"id": "2", "text": "Code", "status": "in_progress", "activeForm": "coding"}
                    ]
                }),
            ),
            text_response("Done."),
        ]);
        agent.add_user_message("multi step");

        let result = agent.run_loop().await.unwrap();

        assert_eq!(result, "Done.");

        let todo_message = &agent.messages()[6];
        let MessageContent::Blocks(blocks) = &todo_message.content else {
            panic!("expected tool result blocks");
        };

        assert_eq!(blocks.len(), 1);
        assert!(matches!(&blocks[0], ContentBlock::ToolResult { .. }));
    }

    #[tokio::test]
    async fn does_not_inject_reminder_before_any_todo_exists() {
        let mut agent = make_agent(vec![
            tool_use_response("t1", "bash", serde_json::json!({"command": "echo step1"})),
            tool_use_response("t2", "bash", serde_json::json!({"command": "echo step2"})),
            tool_use_response("t3", "bash", serde_json::json!({"command": "echo step3"})),
            text_response("Done."),
        ]);
        agent.add_user_message("multi step");

        let result = agent.run_loop().await.unwrap();

        assert_eq!(result, "Done.");

        let last_tool_message = &agent.messages()[6];
        let MessageContent::Blocks(blocks) = &last_tool_message.content else {
            panic!("expected tool result blocks");
        };

        assert_eq!(blocks.len(), 1);
        assert!(matches!(&blocks[0], ContentBlock::ToolResult { .. }));
    }

    #[tokio::test]
    async fn completed_todos_do_not_trigger_reminders() {
        let mut agent = make_agent(vec![
            tool_use_response(
                "t0",
                "todo",
                serde_json::json!({
                    "items": [
                        {"id": "1", "text": "Done", "status": "completed"}
                    ]
                }),
            ),
            tool_use_response("t1", "bash", serde_json::json!({"command": "echo step1"})),
            tool_use_response("t2", "bash", serde_json::json!({"command": "echo step2"})),
            tool_use_response("t3", "bash", serde_json::json!({"command": "echo step3"})),
            text_response("Done."),
        ]);
        agent.add_user_message("multi step");

        let result = agent.run_loop().await.unwrap();

        assert_eq!(result, "Done.");

        let last_tool_message = &agent.messages()[8];
        let MessageContent::Blocks(blocks) = &last_tool_message.content else {
            panic!("expected tool result blocks");
        };

        assert_eq!(blocks.len(), 1);
        assert!(matches!(&blocks[0], ContentBlock::ToolResult { .. }));
    }

    #[tokio::test]
    async fn failed_todo_update_does_not_reset_open_todo_reminder_counter() {
        let mut agent = make_agent(vec![
            tool_use_response(
                "t0",
                "todo",
                serde_json::json!({
                    "items": [
                        {"id": "1", "text": "Plan", "status": "completed"},
                        {"id": "2", "text": "Code", "status": "in_progress", "activeForm": "coding"}
                    ]
                }),
            ),
            tool_use_response(
                "t1",
                "todo",
                serde_json::json!({
                    "items": [
                        {"id": "1", "text": "Broken 1", "status": "in_progress"},
                        {"id": "2", "text": "Broken 2", "status": "in_progress"}
                    ]
                }),
            ),
            tool_use_response("t2", "bash", serde_json::json!({"command": "echo step1"})),
            tool_use_response("t3", "bash", serde_json::json!({"command": "echo step2"})),
            text_response("Done."),
        ]);
        agent.add_user_message("multi step");

        let result = agent.run_loop().await.unwrap();

        assert_eq!(result, "Done.");

        let last_tool_message = &agent.messages()[8];
        let MessageContent::Blocks(blocks) = &last_tool_message.content else {
            panic!("expected tool result blocks");
        };

        assert_eq!(blocks.len(), 2);
        assert!(matches!(
            &blocks[0],
            ContentBlock::Text { text } if text == "<reminder>Update your todos.</reminder>"
        ));
        assert!(matches!(&blocks[1], ContentBlock::ToolResult { .. }));
    }

    #[tokio::test]
    async fn multiple_tool_calls_in_one_response() {
        // Model calls two tools at once
        let multi_tool = ChatResponse {
            id: "msg_test".to_string(),
            model: Some("mock".to_string()),
            content: vec![
                ContentBlock::tool_use("t1", "bash", serde_json::json!({"command": "echo one"})),
                ContentBlock::tool_use("t2", "bash", serde_json::json!({"command": "echo two"})),
            ],
            stop_reason: Some(STOP_REASON_TOOL_USE.to_string()),
            usage: None,
        };

        let mut agent = make_agent(vec![multi_tool, text_response("Both done.")]);
        agent.add_user_message("run two commands");
        let result = agent.run_loop().await.unwrap();
        assert_eq!(result, "Both done.");
        // user + assistant(2 tool_use) + user(2 tool_result) + assistant(text) = 4
        assert_eq!(agent.messages().len(), 4);
    }

    #[tokio::test]
    async fn multi_turn_tool_loop() {
        // Model calls tool → result → calls tool again → result → text
        let mut agent = make_agent(vec![
            tool_use_response("t1", "bash", serde_json::json!({"command": "echo step1"})),
            tool_use_response("t2", "bash", serde_json::json!({"command": "echo step2"})),
            text_response("All steps complete."),
        ]);
        agent.add_user_message("multi step");
        let result = agent.run_loop().await.unwrap();
        assert_eq!(result, "All steps complete.");
        // user + (assistant+user)*2 + assistant = 6
        assert_eq!(agent.messages().len(), 6);
    }

    #[test]
    fn set_max_tokens() {
        let client: DynLlmClient = Arc::new(MockLlmClient::new(vec![]));
        let dispatcher = create_default_tools(std::env::temp_dir());
        let mut agent = Agent::new(client, "test".to_string(), dispatcher).unwrap();
        agent.set_max_tokens(4096);
        // No panic; just verifies the setter works
    }

    #[test]
    fn set_visible_tools_filters_model_visible_tool_subset() {
        let client: DynLlmClient = Arc::new(MockLlmClient::new(vec![]));
        let dispatcher = create_default_tools(std::env::temp_dir());
        let mut agent = Agent::new(client, "test".to_string(), dispatcher).unwrap();

        let visible = agent.set_visible_tools(Some(&["todo", "bash", "missing"]));

        assert_eq!(visible, vec!["bash".to_string(), "todo".to_string()]);
        assert_eq!(agent.visible_tool_names(), vec!["bash", "todo"]);
    }

    #[test]
    fn set_visible_tools_none_restores_all_tools() {
        let client: DynLlmClient = Arc::new(MockLlmClient::new(vec![]));
        let dispatcher = create_default_tools(std::env::temp_dir());
        let mut agent = Agent::new(client, "test".to_string(), dispatcher).unwrap();

        agent.set_visible_tools(Some(&[]));
        assert!(agent.visible_tool_names().is_empty());

        let restored = agent.set_visible_tools(None);
        assert_eq!(restored.len(), 12);
        assert_eq!(
            agent.visible_tool_names(),
            vec![
                "apply_patch",
                "bash",
                "batch",
                "create_file",
                "edit_file",
                "glob_search",
                "grep_search",
                "list_dir",
                "load_skill",
                "read_file",
                "todo",
                "write_file"
            ]
        );
    }

    #[tokio::test]
    async fn hidden_tool_calls_return_tool_result_error() {
        let mut agent = make_agent(vec![
            tool_use_response("t1", "bash", serde_json::json!({"command": "echo hidden"})),
            text_response("done"),
        ]);
        agent.set_visible_tools(Some(&["read_file"]));
        agent.add_user_message("inspect workspace");

        let result = agent.run_loop().await.unwrap();

        assert_eq!(result, "done");

        let MessageContent::Blocks(blocks) = &agent.messages()[2].content else {
            panic!("expected tool result blocks");
        };

        assert!(matches!(
            &blocks[0],
            ContentBlock::ToolResult { content, is_error, .. }
                if is_error == &Some(true)
                    && content.as_str() == Some("Error: Tool 'bash' is not available in this workflow step")
        ));
    }

    #[tokio::test]
    async fn llm_error_propagates() {
        struct FailingClient;

        #[async_trait]
        impl LlmClient for FailingClient {
            async fn chat(&self, _request: ChatRequest) -> Result<ChatResponse, ClientError> {
                Err(ClientError::Config("network down".into()))
            }
            fn provider_name(&self) -> &'static str {
                "failing"
            }
        }

        let client: DynLlmClient = Arc::new(FailingClient);
        let dispatcher = create_default_tools(std::env::temp_dir());
        let mut agent = Agent::new(client, "test".to_string(), dispatcher).unwrap();
        agent.add_user_message("go");
        let err = agent.run_loop().await.unwrap_err();
        assert!(err.to_string().contains("network down"));
    }

    #[tokio::test]
    async fn max_iterations_guard() {
        // Mock that always returns tool_use, never stops
        struct InfiniteToolClient;

        #[async_trait]
        impl LlmClient for InfiniteToolClient {
            async fn chat(&self, _request: ChatRequest) -> Result<ChatResponse, ClientError> {
                Ok(ChatResponse {
                    id: "msg".to_string(),
                    model: None,
                    content: vec![ContentBlock::tool_use(
                        "t1",
                        "bash",
                        serde_json::json!({"command": "echo loop"}),
                    )],
                    stop_reason: Some(STOP_REASON_TOOL_USE.to_string()),
                    usage: None,
                })
            }
            fn provider_name(&self) -> &'static str {
                "infinite"
            }
        }

        let client: DynLlmClient = Arc::new(InfiniteToolClient);
        let tmp = std::env::temp_dir().join("omega-core-max-iter");
        let _ = std::fs::create_dir_all(&tmp);
        let dispatcher = create_default_tools(tmp);
        let mut agent = Agent::new(client, "test".to_string(), dispatcher).unwrap();
        agent.set_max_iterations(3);
        agent.add_user_message("infinite");
        let err = agent.run_loop().await.unwrap_err();
        assert!(err.to_string().contains("exceeded 3 iterations"));
    }
}
