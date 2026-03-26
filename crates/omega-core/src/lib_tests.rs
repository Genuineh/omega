use super::*;
use async_trait::async_trait;
use futures_util::Stream;
use omega_client::ChatEvent;
use omega_client::{
    ChatEventStream, ChatResponse, ClientError, MessageContent, Usage, STOP_REASON_END_TURN,
    STOP_REASON_TOOL_USE,
};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};
use tokio::sync::watch;

type RecordedToolCall = (String, String, String, Option<String>, String);

struct MockLlmClient {
    responses: Mutex<Vec<ChatResponse>>,
}

struct HangingStreamClient {
    started_tx: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    dropped: Arc<AtomicBool>,
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

#[tokio::test]
async fn interrupt_cancels_in_flight_streaming_turn() {
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let dropped = Arc::new(AtomicBool::new(false));
    let client: DynLlmClient = Arc::new(HangingStreamClient {
        started_tx: Mutex::new(Some(started_tx)),
        dropped: dropped.clone(),
    });
    let tmp = std::env::temp_dir().join("omega-core-cancel-test");
    let _ = std::fs::create_dir_all(&tmp);
    let dispatcher = create_default_tools(tmp);
    let mut agent = Agent::new(client, "Test".to_string(), dispatcher).unwrap();
    agent.add_user_message("go");

    let (turn_tx, mut turn_rx) = watch::channel(91u64);
    let join = tokio::spawn(async move {
        agent
            .run_loop_with_events_until_turn_change(|_, _, _, _| {}, |_| {}, Some(&mut turn_rx), Some(91))
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
    let client: DynLlmClient = Arc::new(MockLlmClient::new(vec![]));
    let dispatcher = create_default_tools(std::env::temp_dir());
    let mut agent = Agent::new(client, "test".to_string(), dispatcher).unwrap();
    agent.set_max_tokens(4096);
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
