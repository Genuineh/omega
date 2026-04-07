use super::*;
use async_trait::async_trait;
use futures_util::Stream;
use omega_client::{
    test_support::{IdleLlmClient, ScriptedLlmClient},
    ChatEvent, ChatEventStream, ChatResponse, ClientError, MessageContent, Usage,
    STOP_REASON_END_TURN, STOP_REASON_TOOL_USE,
};
use omega_test_support::{test_root, TestRoot};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use tokio::sync::watch;

type RecordedToolCall = (String, String, String, Option<String>, String);

struct HangingStreamClient {
    started_tx: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    dropped: Arc<AtomicBool>,
}

struct AgentHarness {
    _root: TestRoot,
    agent: Agent,
}

struct HangingEventStream {
    dropped: Arc<AtomicBool>,
}

impl Stream for HangingEventStream {
    type Item = Result<ChatEvent, ClientError>;

    fn poll_next(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Pending
    }
}

impl Drop for HangingEventStream {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::SeqCst);
    }
}

#[async_trait]
impl LlmClient for HangingStreamClient {
    async fn chat(&self, _request: ChatRequest) -> Result<ChatResponse, ClientError> {
        panic!("chat should not be called in HangingStreamClient");
    }

    async fn chat_stream(&self, _request: ChatRequest) -> Result<ChatEventStream, ClientError> {
        if let Some(tx) = self.started_tx.lock().unwrap().take() {
            let _ = tx.send(());
        }

        Ok(Box::pin(HangingEventStream {
            dropped: self.dropped.clone(),
        }))
    }

    fn provider_name(&self) -> &'static str {
        "hanging"
    }
}

fn text_response(text: &str) -> ChatResponse {
    ChatResponse {
        id: "msg_test".to_string(),
        model: Some("mock".to_string()),
        content: vec![ContentBlock::text(text)],
        stop_reason: Some(STOP_REASON_END_TURN.to_string()),
        usage: Some(Usage {
            input_tokens: 10,
            output_tokens: 5,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
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
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        }),
    }
}

impl std::ops::Deref for AgentHarness {
    type Target = Agent;

    fn deref(&self) -> &Self::Target {
        &self.agent
    }
}

impl std::ops::DerefMut for AgentHarness {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.agent
    }
}

fn make_agent(responses: Vec<ChatResponse>) -> AgentHarness {
    let root = test_root("core");
    let client: DynLlmClient = Arc::new(ScriptedLlmClient::from_responses(responses));
    let dispatcher = create_default_tools(root.path_buf());
    let agent = Agent::new(client, "Test system prompt.".to_string(), dispatcher).unwrap();
    AgentHarness { _root: root, agent }
}

#[tokio::test]
async fn simple_text_response_terminates() {
    let mut agent = make_agent(vec![text_response("Hello!")]);
    agent.add_user_message("hi");
    let result = agent.run_loop().await.unwrap();
    assert_eq!(result, "Hello!");
}

#[tokio::test]
async fn single_response_uses_system_without_tools() {
    let client = Arc::new(ScriptedLlmClient::from_responses(vec![text_response(
        "planned",
    )]));
    let root = test_root("core-single-response");
    let dispatcher = create_default_tools(root.path_buf());
    let mut agent = Agent::new(client.clone(), "phase prompt".to_string(), dispatcher).unwrap();
    agent.add_user_message("go");

    let result = agent.run_single_response().await.unwrap();

    assert_eq!(result, "planned");
    assert_eq!(
        client.recorded_systems(),
        vec![Some("phase prompt".to_string())]
    );
    assert!(client.recorded_requests()[0].tools.is_empty());
}

#[tokio::test]
async fn messages_recorded() {
    let mut agent = make_agent(vec![text_response("done")]);
    agent.add_user_message("query");
    agent.run_loop().await.unwrap();
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

    let client: DynLlmClient = Arc::new(ScriptedLlmClient::from_responses(vec![
        tool_use_response(
            "t1",
            "bash",
            serde_json::json!({"command": "echo callback_test"}),
        ),
        text_response("ok"),
    ]));
    let root = test_root("core-callback");
    let dispatcher = create_default_tools(root.path_buf());
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

    let client: DynLlmClient = Arc::new(ScriptedLlmClient::from_responses(vec![ChatResponse {
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
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        }),
    }]));
    let root = test_root("core-event-callback");
    let dispatcher = create_default_tools(root.path_buf());
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
                    cache_creation_input_tokens: None,
                    cache_read_input_tokens: None,
                }),
            },
        ]
    );
}

#[tokio::test]
async fn interrupt_cancels_in_flight_streaming_turn() {
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let dropped = Arc::new(AtomicBool::new(false));
    let client: DynLlmClient = Arc::new(HangingStreamClient {
        started_tx: Mutex::new(Some(started_tx)),
        dropped: dropped.clone(),
    });
    let root = test_root("core-cancel");
    let dispatcher = create_default_tools(root.path_buf());
    let mut agent = Agent::new(client, "Test".to_string(), dispatcher).unwrap();
    agent.add_user_message("go");

    let (turn_tx, mut turn_rx) = watch::channel(91u64);
    let join = tokio::spawn(async move {
        agent
            .run_loop_with_events_until_turn_change(
                |_, _, _, _| {},
                |_| {},
                Some(&mut turn_rx),
                Some(91),
            )
            .await
    });

    tokio::time::timeout(std::time::Duration::from_secs(2), started_rx)
        .await
        .expect("streaming request should start promptly")
        .expect("streaming start signal should arrive");

    turn_tx.send(92).unwrap();

    let result = tokio::time::timeout(std::time::Duration::from_secs(2), join)
        .await
        .expect("canceled agent task should finish promptly")
        .expect("join should succeed");

    assert!(
        result.is_err(),
        "canceled streaming turn should exit with an error"
    );
    assert!(
        dropped.load(Ordering::SeqCst),
        "canceling the turn should drop the in-flight chat stream"
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
    assert!(dispatcher.has_tool("manage_document"));
    assert!(dispatcher.has_tool("search_codebase"));
    assert!(dispatcher.has_tool("todo"));
}

#[test]
fn search_and_document_tools_surface_backend_disabled_error_by_default() {
    let dispatcher = create_default_tools(std::env::temp_dir());

    let search = dispatcher
        .dispatch("search_codebase", serde_json::json!({"query": "omega"}))
        .unwrap();
    assert_eq!(search.error_kind, Some(omega_tools::ToolErrorKind::Execution));
    assert!(search.output.contains("document backend"));

    let manage = dispatcher
        .dispatch("manage_document", serde_json::json!({"action": "health_check"}))
        .unwrap();
    assert_eq!(manage.error_kind, Some(omega_tools::ToolErrorKind::Execution));
    assert!(manage.output.contains("document backend"));
}

#[test]
fn successful_tool_results_are_reinjected_as_plain_text_for_model_consumption() {
    let block = crate::helpers::tool_result_block(
        "tool-1",
        &omega_tools::ToolResult::success("Cargo.toml\ncrates/\ndocs/")
            .with_preview("Cargo.toml")
            .with_metadata(serde_json::json!({"entry_count": 3})),
    );

    assert!(matches!(
        block,
        ContentBlock::ToolResult {
            tool_use_id,
            content: serde_json::Value::String(content),
            is_error: None,
        } if tool_use_id == "tool-1" && content == "Cargo.toml\ncrates/\ndocs/"
    ));
}

#[test]
fn successful_tool_results_fall_back_to_preview_when_output_is_empty() {
    let block = crate::helpers::tool_result_block(
        "tool-1",
        &omega_tools::ToolResult::success("")
            .with_preview("3 entries")
            .with_metadata(serde_json::json!({"entry_count": 3})),
    );

    assert!(matches!(
        block,
        ContentBlock::ToolResult {
            tool_use_id,
            content: serde_json::Value::String(content),
            is_error: None,
        } if tool_use_id == "tool-1" && content == "3 entries"
    ));
}

#[test]
fn successful_tool_results_fall_back_to_metadata_when_output_and_preview_are_empty() {
    let block = crate::helpers::tool_result_block(
        "tool-1",
        &omega_tools::ToolResult::success("")
            .with_metadata(serde_json::json!({"path": ".", "entry_count": 3})),
    );

    assert!(matches!(
        block,
        ContentBlock::ToolResult {
            tool_use_id,
            content: serde_json::Value::String(content),
            is_error: None,
        } if tool_use_id == "tool-1"
            && content.contains("entry_count")
            && content.contains("path")
    ));
}

#[test]
fn error_tool_results_remain_structured_for_recovery() {
    let block = crate::helpers::tool_result_block(
        "tool-1",
        &omega_tools::ToolResult::error("Error: blocked", omega_tools::ToolErrorKind::Policy)
            .with_metadata(serde_json::json!({"error_kind": "policy"})),
    );

    assert!(matches!(
        block,
        ContentBlock::ToolResult {
            tool_use_id,
            content: serde_json::Value::Object(_),
            is_error: Some(true),
        } if tool_use_id == "tool-1"
    ));
}

#[test]
fn tool_definitions_deserialize() {
    let dispatcher = create_default_tools(std::env::temp_dir());
    let schemas = dispatcher.to_schemas();
    let defs: Vec<ToolDefinition> = schemas
        .into_iter()
        .map(|v| serde_json::from_value(v).unwrap())
        .collect();
    assert_eq!(defs.len(), 19);
    let names: Vec<&str> = defs.iter().map(|def| def.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "apply_patch",
            "ask_user_question",
            "bash",
            "batch",
            "create_file",
            "edit_file",
            "glob_search",
            "grep_search",
            "list_dir",
            "load_skill",
            "manage_document",
            "read_file",
            "search_codebase",
            "task",
            "todo_read",
            "todo_write",
            "web_fetch",
            "web_search",
            "write_file"
        ]
    );
}

#[test]
fn default_tools_expose_manifest_metadata() {
    let dispatcher = create_default_tools(std::env::temp_dir());
    let manifests = dispatcher.manifest_metadata();

    assert_eq!(manifests.len(), 19);
    let bash = manifests
        .iter()
        .find(|manifest| manifest.id == "bash")
        .expect("bash manifest should exist");
    assert_eq!(bash.display_name, "Bash");
    assert_eq!(bash.family, omega_tools::ToolFamily::EscapeHatch);
    assert!(bash.prompt.summary.contains("allowlisted shell command"));

    let patch = manifests
        .iter()
        .find(|manifest| manifest.id == "apply_patch")
        .expect("apply_patch manifest should exist");
    assert_eq!(patch.family, omega_tools::ToolFamily::Editing);
    assert!(patch.prompt.prefer_over.iter().any(|tool| tool == "edit_file"));
    assert_eq!(
        patch
            .permissions
            .as_ref()
            .expect("file edit permission profile")
            .permission_class,
        "workspace_write"
    );
    assert!(patch
        .permissions
        .as_ref()
        .expect("file edit permission profile")
        .requires_approval);
    assert!(patch
        .storage
        .as_ref()
        .expect("file edit storage profile")
        .produces_artifact);
    assert!(patch
        .ui
        .as_ref()
        .expect("file edit ui profile")
        .action_affordances
        .iter()
        .any(|action| action == "open_diff_preview"));

    let todo = manifests
        .iter()
        .find(|manifest| manifest.id == "todo_write")
        .expect("todo_write manifest should exist");
    assert!(todo
        .storage
        .as_ref()
        .expect("todo storage profile")
        .writes_todo);

    let search = manifests
        .iter()
        .find(|manifest| manifest.id == "search_codebase")
        .expect("search_codebase manifest should exist");
    assert!(search.observability.is_some());
    assert!(search
        .prompt
        .summary
        .contains("ranked keyword, semantic, or hybrid retrieval"));
    assert!(search
        .prompt
        .when_not_to_use
        .iter()
        .any(|line| line.contains("exact line matches")));

    let grep = manifests
        .iter()
        .find(|manifest| manifest.id == "grep_search")
        .expect("grep_search manifest should exist");
    assert!(grep
        .prompt
        .summary
        .contains("exact or regex content matching"));
    assert!(grep
        .prompt
        .when_to_use
        .iter()
        .any(|line| line.contains("exact string or regex matches")));
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
async fn hidden_todo_tool_does_not_inject_reminder() {
    let mut agent = make_agent(vec![
        tool_use_response(
            "t1",
            "todo",
            serde_json::json!({
                "items": [
                    {"id": "1", "text": "Investigate routing", "status": "pending"}
                ]
            }),
        ),
        tool_use_response("t2", "read_file", serde_json::json!({"path": "Cargo.toml"})),
        tool_use_response("t3", "read_file", serde_json::json!({"path": "Cargo.toml"})),
        tool_use_response("t4", "read_file", serde_json::json!({"path": "Cargo.toml"})),
        text_response("done"),
    ]);
    agent.set_visible_tools(Some(&["read_file"]));
    agent.add_user_message("analyze project");
    let _ = agent.run_loop().await.unwrap();

    let reminder_in_hidden_phase = agent.messages().iter().any(|message| {
        matches!(
            &message.content,
            MessageContent::Blocks(blocks)
                if blocks.iter().any(|block| matches!(
                    block,
                    ContentBlock::Text { text }
                        if text == "<reminder>Update your todos.</reminder>"
                ))
        )
    });

    assert!(!reminder_in_hidden_phase);
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
    assert_eq!(agent.messages().len(), 4);
}

#[tokio::test]
async fn multi_turn_tool_loop() {
    let mut agent = make_agent(vec![
        tool_use_response("t1", "bash", serde_json::json!({"command": "echo step1"})),
        tool_use_response("t2", "bash", serde_json::json!({"command": "echo step2"})),
        text_response("All steps complete."),
    ]);
    agent.add_user_message("multi step");
    let result = agent.run_loop().await.unwrap();
    assert_eq!(result, "All steps complete.");
    assert_eq!(agent.messages().len(), 6);
}

#[test]
fn set_max_tokens() {
    let client: DynLlmClient = Arc::new(IdleLlmClient::new("chat should not be called"));
    let dispatcher = create_default_tools(std::env::temp_dir());
    let mut agent = Agent::new(client, "test".to_string(), dispatcher).unwrap();
    agent.set_max_tokens(4096);
}

#[test]
fn set_visible_tools_filters_model_visible_tool_subset() {
    let client: DynLlmClient = Arc::new(IdleLlmClient::new("chat should not be called"));
    let dispatcher = create_default_tools(std::env::temp_dir());
    let mut agent = Agent::new(client, "test".to_string(), dispatcher).unwrap();

    let visible = agent.set_visible_tools(Some(&["todo", "bash", "missing"]));

    assert_eq!(visible, vec!["bash".to_string(), "todo_write".to_string()]);
    assert_eq!(agent.visible_tool_names(), vec!["bash", "todo_write"]);
}

#[test]
fn set_visible_tools_none_restores_all_tools() {
    let client: DynLlmClient = Arc::new(IdleLlmClient::new("chat should not be called"));
    let dispatcher = create_default_tools(std::env::temp_dir());
    let mut agent = Agent::new(client, "test".to_string(), dispatcher).unwrap();

    agent.set_visible_tools(Some(&[]));
    assert!(agent.visible_tool_names().is_empty());

    let restored = agent.set_visible_tools(None);
    assert_eq!(restored.len(), 19);
    assert_eq!(
        agent.visible_tool_names(),
        vec![
            "apply_patch",
            "ask_user_question",
            "bash",
            "batch",
            "create_file",
            "edit_file",
            "glob_search",
            "grep_search",
            "list_dir",
            "load_skill",
            "manage_document",
            "read_file",
            "search_codebase",
            "task",
            "todo_read",
            "todo_write",
            "web_fetch",
            "web_search",
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
                && content["output"] == "Error: Tool 'bash' is not available in this workflow step"
                && content["error_kind"] == "policy"
                && content["remediation"]["kind"] == "use_allowed_alternative"
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
    let root = test_root("core-max-iter");
    let dispatcher = create_default_tools(root.path_buf());
    let mut agent = Agent::new(client, "test".to_string(), dispatcher).unwrap();
    agent.set_max_iterations(3);
    agent.add_user_message("infinite");
    let err = agent.run_loop().await.unwrap_err();
    assert!(err.to_string().contains("exceeded 3 iterations"));
}
