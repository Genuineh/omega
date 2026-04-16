use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use omega_client::{
    test_support::{IdleLlmClient, ScriptedLlmClient},
    ChatResponse, ContentBlock, STOP_REASON_END_TURN, STOP_REASON_TOOL_USE,
};
use omega_context::GovernanceEventSignal;
use omega_core::DynLlmClient;
use omega_project::{
    ProjectRegistry, ProjectResolutionInput, ProjectSessionStatus, ProjectSessionUpdate,
    SessionContextRecord, SessionContextRecordKind, SessionReplayEntry, SessionReplayEntryKind,
};
use omega_test_support::persistent_test_root;
use omega_workflow::{
    DataFormat, LoadedWorkflowCatalog, OutputRecoveryMode, StepInputContract, StepLoopMode,
    StepOutputContract, CHAT_STEP_ID, CHAT_WORKFLOW_ID, DEEP_RESEARCH_SCENE_ID,
    DEEP_RESEARCH_WORKFLOW_ID, DEFAULT_EXPLORE_SCHEMA_PATH, EXECUTE_STEP_ID, EXPLORE_STEP_ID,
    FEATURE_WORKFLOW_ID, PLAN_STEP_ID, REPORT_STEP_ID, RESEARCH_WORKFLOW_ID, ROOT_WORKFLOW_ID,
    SCENE_RECOGNITION_STEP_ID, SELECT_SKILLS_STEP_ID, SELECT_WORKFLOW_STEP_ID,
};
use serde_json::Value;

use super::output::validate_workflow_step_output;
use super::{
    build_turn_retention_signals, parse_json_values, preview_text, render_output_contract,
    resolve_structured_input,
    validate_schema_file, validate_structured_output, AgentSession, AgentSessionConfig,
    ConversationMessage, ProviderMarkupSanitizer, ResponseSectionDelta, ResponseSectionKind,
    ResponseSectionState, RuntimeContentKind, RuntimeEnvelopeRecorder, RuntimeMessage,
    RuntimeMessageEnvelope, RuntimeSource, RuntimeUiEffect, RuntimeUiEnvelope, SectionOrigin,
    SessionContext, OverlayTarget, OperatorPickerIntent, UiContent,
    SessionSkillCatalog, SessionToolCatalog, StateMessage, StatusSlot, StatusValue,
    StepContextWriteKind, StepOutputAttemptKind, StepOutputStatus, StepSkillRequest,
    StepToolRequest, ToolRunStatus, UiMessageKind, UiSource, UiTarget, WorkflowRunRole,
};

type SequencedClient = ScriptedLlmClient;

#[allow(non_upper_case_globals)]
const IdleClient: IdleLlmClient =
    IdleLlmClient::new("chat should not be called in AgentSession unit tests");

fn sequenced_client(responses: Vec<ChatResponse>) -> Arc<SequencedClient> {
    Arc::new(SequencedClient::from_responses(normalize_root_routing_responses(
        responses,
    )))
}

fn counting_sequenced_client(
    responses: Vec<ChatResponse>,
    token_count: u32,
) -> Arc<SequencedClient> {
    let mut builder = SequencedClient::builder();
    for _ in 0..16 {
        builder = builder.push_count_tokens(token_count);
    }
    for response in normalize_root_routing_responses(responses) {
        builder = builder.push_response(response);
    }
    Arc::new(builder.build())
}

fn failing_count_sequenced_client(responses: Vec<ChatResponse>) -> Arc<SequencedClient> {
    let mut builder = SequencedClient::builder();
    for _ in 0..16 {
        builder = builder.push_count_tokens_error(
            omega_client::ProviderCapabilityError {
                provider: "failing-scripted".to_string(),
                operation: "messages.count_tokens".to_string(),
                detail: "precise token counting is not supported by this client".to_string(),
            }
            .into(),
        );
    }
    for response in normalize_root_routing_responses(responses) {
        builder = builder.push_response(response);
    }
    Arc::new(builder.build())
}

fn normalize_root_routing_responses(mut responses: Vec<ChatResponse>) -> Vec<ChatResponse> {
    if responses.len() < 2 {
        return responses;
    }

    let Some(scene_value) = routing_json_value(&responses[0], "recognized_scene_id") else {
        return responses;
    };
    let Some(workflow_value) = routing_json_value(&responses[1], "selected_workflow_id") else {
        return responses;
    };

    let uses_legacy_full_research_flow = responses.iter().any(|response| {
        response.id.starts_with("plan-")
            || response.id.starts_with("execute-")
                && response.content.iter().any(|block| match block {
                    ContentBlock::Text { text } => text.contains("completed_tasks"),
                    _ => false,
                })
    });

    let normalized_scene = if scene_value == RESEARCH_WORKFLOW_ID && uses_legacy_full_research_flow {
        DEEP_RESEARCH_SCENE_ID
    } else {
        scene_value.as_str()
    };
    let normalized_workflow = if workflow_value == RESEARCH_WORKFLOW_ID
        && uses_legacy_full_research_flow
    {
        DEEP_RESEARCH_WORKFLOW_ID
    } else {
        workflow_value.as_str()
    };

    responses[0].content = vec![ContentBlock::text(format!(
        "{{\"recognized_scene_id\":\"{}\",\"selected_workflow_id\":\"{}\"}}",
        normalized_scene, normalized_workflow
    ))];
    responses.remove(1);

    let has_select_skills_response = responses
        .get(1)
        .and_then(|response| routing_json_value(response, "selected_skill_ids"))
        .is_some();
    if !has_select_skills_response {
        responses.insert(
            1,
            ChatResponse {
                id: "select-skills-compat".to_string(),
                model: Some("test-model".to_string()),
                content: vec![ContentBlock::text("{\"selected_skill_ids\":[]}")],
                stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                usage: None,
            },
        );
    }

    responses
}

fn routing_json_value(response: &ChatResponse, key: &str) -> Option<String> {
    let ContentBlock::Text { text } = response.content.first()? else {
        return None;
    };
    let value = serde_json::from_str::<Value>(text).ok()?;
    match value.get(key)? {
        Value::String(value) => Some(value.to_string()),
        Value::Array(values) => Some(
            values
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(","),
        ),
        _ => None,
    }
}

fn feature_explore_json() -> &'static str {
    r#"{"objective":"Implement the requested change","key_findings":["The workflow runtime resolves plan input from the first step's structured output","Session tests assert the first child step's stable id and label"],"constraints":["preserve existing behavior"],"risks":["regression risk"],"affected_paths":["crates/omega-session/src/lib.rs"]}"#
}

fn feature_plan_json() -> &'static str {
    r#"{"goal":"Implement the requested change safely","tasks":[{"id":"task-1","title":"Inspect code","description":"Review the relevant workflow and session logic"},{"id":"task-2","title":"Apply changes","description":"Implement the requested code and test updates"}],"validation_targets":["cargo test -p omega-workflow -p omega-session"]}"#
}

fn research_plan_json() -> &'static str {
    r#"{"goal":"Analyze the requested topic with read-only evidence","tasks":[{"id":"task-1","title":"Inspect relevant implementation","description":"Gather evidence from the relevant code, config, and tests using read-only tools"},{"id":"task-2","title":"Validate the key risks","description":"Confirm or reject the suspected risks with read-only checks and summarize the evidence"}],"validation_targets":["rg --files crates"]}"#
}

fn research_plan_report_prose_json() -> &'static str {
    r#"# Omega 项目分析报告

## 项目概述

{"goal":"Analyze the requested topic with read-only evidence","tasks":[{"id":"task-1","title":"Inspect relevant implementation","description":"Gather evidence from the relevant code, config, and tests using read-only tools"},{"id":"task-2","title":"Validate the key risks","description":"Confirm or reject the suspected risks with read-only checks and summarize the evidence"}],"validation_targets":["rg --files crates"]}"#
}

fn research_plan_with_explore_echo_json() -> String {
    format!(
        "根据探索阶段的发现：\n\n{}\n\n以下是执行计划：\n\n{}",
        feature_explore_json(),
        research_plan_json()
    )
}

fn feature_execute_partial_json() -> &'static str {
    r#"{"completed_tasks":["task-1"],"open_tasks":["task-2"],"validation_results":[{"target":"cargo test -p omega-workflow -p omega-session","status":"passed"}],"changed_paths":["crates/omega-session/src/lib.rs"]}"#
}

fn feature_execute_complete_json() -> &'static str {
    r#"{"completed_tasks":["task-1","task-2"],"open_tasks":[],"validation_results":[{"target":"cargo test -p omega-workflow -p omega-session","status":"passed"}],"changed_paths":["crates/omega-session/src/lib.rs"]}"#
}

fn research_execute_partial_json() -> &'static str {
    r#"{"completed_tasks":["task-1"],"open_tasks":["task-2"],"validation_results":[{"target":"rg --files crates","status":"passed"}],"changed_paths":[]}"#
}

fn research_execute_no_progress_json() -> &'static str {
    r#"{"completed_tasks":[],"open_tasks":["task-1","task-2"],"validation_results":[{"target":"rg --files crates","status":"passed"}],"changed_paths":[]}"#
}

fn research_execute_complete_json() -> &'static str {
    r#"{"completed_tasks":["task-1","task-2"],"open_tasks":[],"validation_results":[{"target":"rg --files crates","status":"passed"}],"changed_paths":[]}"#
}

fn research_execute_complete_with_display_text_json() -> &'static str {
    r#"{"completed_tasks":["Inspect relevant implementation: Gather evidence from the relevant code, config, and tests using read-only tools"],"open_tasks":["Validate the key risks: Confirm or reject the suspected risks with read-only checks and summarize the evidence"],"validation_results":[{"target":"rg --files crates","status":"passed"}],"changed_paths":[]}"#
}

fn research_execute_future_only_json() -> &'static str {
    r#"{"completed_tasks":["task-2"],"open_tasks":["task-1"],"validation_results":[{"target":"rg --files crates","status":"passed"}],"changed_paths":[]}"#
}

fn feature_execute_future_completion_json() -> &'static str {
    r#"{"completed_tasks":["task-1","task-2"],"open_tasks":[],"validation_results":[{"target":"cargo test -p omega-workflow -p omega-session","status":"passed"}],"changed_paths":["crates/omega-session/src/lib.rs"]}"#
}

fn unique_session_test_root(name: &str) -> PathBuf {
    persistent_test_root(&format!("agent-session-{name}"))
}

fn write_document_fixture(root: &Path) {
    let _ = std::fs::create_dir_all(root.join("docs/specs"));
    let _ = std::fs::write(root.join("README.md"), "# Omega Test Fixture\n");
    let _ = std::fs::write(root.join("docs/README.md"), "# Docs\n");
    let _ = std::fs::write(root.join("docs/TODO.md"), "# TODO\n");
}

struct DocumentEmbeddingBackendGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    previous: Option<OsString>,
}

impl Drop for DocumentEmbeddingBackendGuard {
    fn drop(&mut self) {
        if let Some(previous) = self.previous.as_ref() {
            std::env::set_var("OMEGA_DOCUMENT_EMBEDDING_BACKEND", previous);
        } else {
            std::env::remove_var("OMEGA_DOCUMENT_EMBEDDING_BACKEND");
        }
    }
}

fn force_mock_document_embedding_backend() -> DocumentEmbeddingBackendGuard {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let lock = LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
    let previous = std::env::var_os("OMEGA_DOCUMENT_EMBEDDING_BACKEND");
    std::env::set_var("OMEGA_DOCUMENT_EMBEDDING_BACKEND", "mock");
    DocumentEmbeddingBackendGuard {
        _lock: lock,
        previous,
    }
}

fn write_review_skill(root: &Path) {
    write_named_skill(root, "review", "Review code", "Find regressions.");
}

fn run_command(
    session: &AgentSession,
    input: impl Into<String>,
    turn_id: u64,
) -> Vec<RuntimeMessageEnvelope> {
    let recorder = RuntimeEnvelopeRecorder::new();
    session
        .spawn_command_with_test_bridge(input.into(), turn_id, recorder.runtime_bridge())
        .unwrap();
    recorder.wait_for_turn_finished_messages(turn_id, Duration::from_secs(30))
}

fn command_body(recorded: &[RuntimeMessageEnvelope]) -> String {
    recorded
        .iter()
        .filter_map(|envelope| match &envelope.message {
            RuntimeMessage::Conversation(ConversationMessage::AppendSection { delta, .. }) => {
                match delta {
                    ResponseSectionDelta::Text(text) => Some(text.as_str()),
                }
            }
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn show_overlay_content(recorded: &[RuntimeMessageEnvelope], target: OverlayTarget) -> Option<&UiContent> {
    recorded.iter().find_map(|envelope| match &envelope.message {
        RuntimeMessage::State(StateMessage::ShowOverlay { request }) if request.target == target => {
            Some(&request.content)
        }
        _ => None,
    })
}

fn write_builtin_hook_manifest(root: &Path, hook_id: &str) {
    let hook_dir = root.join(".omega/hooks").join(hook_id);
    let _ = std::fs::create_dir_all(&hook_dir);
    let _ = std::fs::write(
        hook_dir.join("Hook.toml"),
        format!(
            "id = \"{hook_id}\"\npackage = \"builtin\"\nartifact = \"builtin:{hook_id}\"\napi_version = 1\n"
        ),
    );
}

fn write_named_skill(root: &Path, name: &str, description: &str, body: &str) {
    let skills_dir = root.join(".claude/skills").join(name);
    let _ = std::fs::create_dir_all(&skills_dir);
    let _ = std::fs::write(
        skills_dir.join("SKILL.md"),
        format!(
            "---\nname: {name}\ndescription: {description}\n---\n{body}",
        ),
    );
}

fn compile_hook_fixture(hook_dir: &Path, crate_name: &str) -> PathBuf {
    let _ = std::fs::create_dir_all(hook_dir);
    let source_path = hook_dir.join("fixture.rs");
    let hook_id = hook_dir
        .file_name()
        .and_then(|value| value.to_str())
        .expect("hook source dir should end with hook id");
    let artifact_dir = hook_dir
        .parent()
        .and_then(|path| path.parent())
        .and_then(|path| path.parent())
        .map(|root| root.join(".omega-state/hooks").join(hook_id))
        .expect("hook source dir should be rooted under .omega/hooks/<hook-id>");
    let _ = std::fs::create_dir_all(&artifact_dir);
    let artifact_path = artifact_dir.join(format!("lib{crate_name}.so"));
    let _ = std::fs::write(&source_path, hook_fixture_source());

    let status = Command::new("rustc")
        .args([
            "--crate-type",
            "cdylib",
            "--edition",
            "2021",
            source_path.to_str().unwrap(),
            "-o",
            artifact_path.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status.success(), "failed to compile hook fixture");

    artifact_path
}

fn write_hook_manifest(root: &Path, hook_id: &str, artifact_path: &Path) {
    let hook_dir = root.join(".omega/hooks").join(hook_id);
    let _ = std::fs::create_dir_all(&hook_dir);
    let _ = std::fs::write(
        hook_dir.join("Hook.toml"),
        format!(
            "id = \"{hook_id}\"\npackage = \"{hook_id}\"\nartifact = \"{}\"\napi_version = 1\n",
            artifact_path.file_name().unwrap().to_string_lossy()
        ),
    );
}

fn write_feature_workflow_with_hook(root: &Path, hook_id: &str, max_step_repeats: u32) {
    let workflows_dir = root.join(".omega/workflows");
    let _ = std::fs::create_dir_all(&workflows_dir);
    let _ = std::fs::write(
        workflows_dir.join("feature.toml"),
        format!(
            r#"# Test feature workflow
name = "feature"

[[steps]]
id = "explore"
label = "Explore"
prompt = ".omega/prompt/step/explore.md"
loop_mode = "agent_loop"
max_iterations = 200
tool_request = {{ mode = "block", groups = ["feature_non_execute_blocked"] }}
skill_request = {{ mode = "match_task" }}
output_contract = {{ mode = "required", format = "json", schema_path = ".omega/schema/step/explore.json", max_retries = 2, recovery_mode = "repair_then_regenerate" }}
enabled = true

[[steps]]
id = "plan"
label = "Plan"
prompt = ".omega/prompt/step/plan.md"
loop_mode = "agent_loop"
max_iterations = 200
tool_request = {{ mode = "block", groups = ["feature_non_execute_blocked"] }}
skill_request = {{ mode = "match_task" }}
input_contract = {{ mode = "required", sources = ["explore"] }}
output_contract = {{ mode = "required", format = "json", schema_path = ".omega/schema/step/plan.json", max_retries = 2, recovery_mode = "repair_then_regenerate" }}
enabled = true

[[steps]]
id = "execute"
label = "Execute"
prompt = ".omega/prompt/step/execute.md"
loop_mode = "agent_loop"
max_iterations = 200
max_step_repeats = {max_step_repeats}
hooks = ["{hook_id}"]
tool_request = {{ mode = "inherit" }}
skill_request = {{ mode = "match_task" }}
input_contract = {{ mode = "required", sources = ["plan"] }}
output_contract = {{ mode = "required", format = "json", schema_path = ".omega/schema/step/execute.json", max_retries = 2, recovery_mode = "repair_then_regenerate" }}
enabled = true

[[steps]]
id = "report"
label = "Report"
prompt = ".omega/prompt/step/report.md"
loop_mode = "agent_loop"
max_iterations = 200
tool_request = {{ mode = "block", groups = ["feature_non_execute_blocked"] }}
skill_request = {{ mode = "match_task" }}
input_contract = {{ mode = "optional", sources = ["explore", "plan", "execute"] }}
enabled = true
"#
        ),
    );
}

fn write_feature_workflow_with_hook_and_item_repeats(
    root: &Path,
    hook_id: &str,
    max_step_repeats: u32,
    max_item_repeats: u32,
) {
    let workflows_dir = root.join(".omega/workflows");
    let _ = std::fs::create_dir_all(&workflows_dir);
    let _ = std::fs::write(
        workflows_dir.join("feature.toml"),
        format!(
            r#"# Test feature workflow
name = "feature"

[[steps]]
id = "explore"
label = "Explore"
prompt = ".omega/prompt/step/explore.md"
loop_mode = "agent_loop"
max_iterations = 200
tool_request = {{ mode = "block", groups = ["feature_non_execute_blocked"] }}
skill_request = {{ mode = "match_task" }}
output_contract = {{ mode = "required", format = "json", schema_path = ".omega/schema/step/explore.json", max_retries = 2, recovery_mode = "repair_then_regenerate" }}
enabled = true

[[steps]]
id = "plan"
label = "Plan"
prompt = ".omega/prompt/step/plan.md"
loop_mode = "agent_loop"
max_iterations = 200
tool_request = {{ mode = "block", groups = ["feature_non_execute_blocked"] }}
skill_request = {{ mode = "match_task" }}
input_contract = {{ mode = "required", sources = ["explore"] }}
output_contract = {{ mode = "required", format = "json", schema_path = ".omega/schema/step/plan.json", max_retries = 2, recovery_mode = "repair_then_regenerate" }}
enabled = true

[[steps]]
id = "execute"
label = "Execute"
prompt = ".omega/prompt/step/execute.md"
loop_mode = "agent_loop"
loop_contract = {{ kind = "todo_items", source = "plan.tasks", child_step_prefix = "execute", max_item_repeats = {max_item_repeats} }}
max_iterations = 200
max_step_repeats = {max_step_repeats}
hooks = ["{hook_id}"]
tool_request = {{ mode = "inherit" }}
skill_request = {{ mode = "match_task" }}
input_contract = {{ mode = "required", sources = ["plan"] }}
output_contract = {{ mode = "required", format = "json", schema_path = ".omega/schema/step/execute.json", max_retries = 2, recovery_mode = "repair_then_regenerate" }}
enabled = true

[[steps]]
id = "report"
label = "Report"
prompt = ".omega/prompt/step/report.md"
loop_mode = "agent_loop"
max_iterations = 200
tool_request = {{ mode = "block", groups = ["feature_non_execute_blocked"] }}
skill_request = {{ mode = "match_task" }}
input_contract = {{ mode = "optional", sources = ["explore", "plan", "execute"] }}
enabled = true
"#
        ),
    );
}

fn hook_fixture_source() -> &'static str {
    r#"
use std::ffi::{CStr, CString};
use std::os::raw::c_char;

#[no_mangle]
pub extern "C" fn omega_hook_api_version() -> u32 {
    1
}

#[no_mangle]
pub extern "C" fn omega_hook_invoke_json(input: *const c_char) -> *mut c_char {
    let input = unsafe { CStr::from_ptr(input) }.to_str().unwrap_or("");
    let response = if input.contains("\"event\":\"before_step\"") {
        "{\"diagnostics\":[{\"level\":\"info\",\"message\":\"fixture before step\"}],\"storage\":{\"seen\":1}}".to_string()
    } else if input.contains("\"event\":\"before_advance\"") && input.contains("\"seen\":1") {
        "{\"diagnostics\":[],\"storage\":{\"seen\":1}}".to_string()
    } else if input.contains("\"event\":\"after_step\"") && input.contains("\"seen\":1") {
        "{\"diagnostics\":[{\"level\":\"info\",\"message\":\"fixture after step\"}],\"storage\":{}}".to_string()
    } else if input.contains("\"event\":\"step_failed\"") {
        "{\"diagnostics\":[{\"level\":\"error\",\"message\":\"fixture saw failure\"}],\"storage\":{}}".to_string()
    } else {
        "{\"diagnostics\":[],\"storage\":{}}".to_string()
    };

    CString::new(response).unwrap().into_raw()
}

#[no_mangle]
pub extern "C" fn omega_hook_free_string(ptr: *mut c_char) {
    if ptr.is_null() {
        return;
    }
    unsafe {
        let _ = CString::from_raw(ptr);
    }
}
"#
}

#[test]
fn spawn_turn_emits_hook_diagnostics_for_execute_step() {
    let client: Arc<SequencedClient> = counting_sequenced_client(
        vec![
        ChatResponse {
            id: "scene-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"recognized_scene_id\":\"feature\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "select-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"selected_workflow_id\":\"feature\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "explore-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_explore_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "plan-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_plan_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_execute_partial_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-2".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_execute_complete_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "report-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("done")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ],
        321,
    );
    let client_dyn: DynLlmClient = client;
    let root = unique_session_test_root("hook-runtime");
    write_review_skill(&root);
    let hook_dir = root.join(".omega/hooks/todo_managed_execute");
    let artifact_path = compile_hook_fixture(&hook_dir, "session_fixture_hook");
    write_hook_manifest(&root, "todo_managed_execute", &artifact_path);
    write_feature_workflow_with_hook(&root, "todo_managed_execute", 8);

    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: client_dyn,
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();
    let (tx, rx) = mpsc::channel();

    session
        .spawn_turn_ui_compat("fix this bug".to_string(), 81, tx)
        .unwrap();

    let mut system_logs = Vec::new();
    loop {
        match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
            RuntimeUiEnvelope::Message { turn_id, message }
                if turn_id == 81
                    && matches!(message.source, UiSource::System)
                    && matches!(
                        message.kind,
                        UiMessageKind::Log | UiMessageKind::Warning | UiMessageKind::Error
                    ) =>
            {
                system_logs.push(message.content.as_text().to_string());
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::SetStatusSlot {
                        slot: StatusSlot::Agent,
                        value: StatusValue::Label(label),
                    },
            } => {
                assert_eq!(turn_id, 81);
                assert_eq!(label, "Idle");
                break;
            }
            _ => {}
        }
    }

    assert!(system_logs
        .iter()
        .any(|line| { line.contains("Hook todo_managed_execute [info] fixture before step") }));
    assert!(system_logs
        .iter()
        .any(|line| { line.contains("Hook todo_managed_execute [info] fixture after step") }));
}

#[test]
fn implementation_request_detector_prefers_feature_scene() {
    assert!(super::latest_user_turn_requires_feature_scene(
        "fix this bug"
    ));
    assert!(super::latest_user_turn_requires_feature_scene(
        "请你更新相关文档，并修复这个 bug"
    ));
    assert!(!super::latest_user_turn_requires_feature_scene(
        "分析下这个项目的优缺点"
    ));
}

#[test]
fn research_request_detector_prefers_research_scene() {
    assert!(super::latest_user_turn_prefers_research_scene(
        "请对这个仓库做一次深度复杂的综合分析和探索"
    ));
    assert!(super::latest_user_turn_prefers_research_scene(
        "Need a comprehensive architecture analysis and investigation"
    ));
    assert!(!super::latest_user_turn_prefers_research_scene(
        "Explain what this function does"
    ));
}

#[test]
fn preview_text_preserves_utf8_boundaries() {
    assert_eq!(preview_text("你好世界", 3), "你好世...");
}

#[test]
fn provider_markup_sanitizer_strips_known_tool_wrappers_across_chunks() {
    let mut sanitizer = ProviderMarkupSanitizer::default();

    assert_eq!(sanitizer.push("before<minimax:tool_"), "before");
    assert_eq!(
        sanitizer.push("call><invoke name=\"bash\">ignored</invoke></minimax:tool_call>after"),
        "after"
    );
    assert_eq!(sanitizer.finish(), "");
}

#[test]
fn structured_contract_helpers_resolve_inputs_and_validate_required_json() {
    let mut session_context = SessionContext::new(ROOT_WORKFLOW_ID);
    session_context.step_outputs.insert(
        EXPLORE_STEP_ID.to_string(),
        serde_json::json!({"summary": "explore"}),
    );

    let step = omega_workflow::WorkflowStep {
        id: "plan".to_string(),
        label: "Plan".to_string(),
        prompt_path: PathBuf::from(".omega/prompt/step/plan.md"),
        loop_mode: StepLoopMode::AgentLoop,
        loop_contract: None,
        max_iterations: 8,
        max_step_repeats: 0,
        hooks: Vec::new(),
        tool_request: StepToolRequest::Block(Vec::new()),
        skill_request: StepSkillRequest::MatchTask,
        input_contract: StepInputContract::Required {
            sources: vec![EXPLORE_STEP_ID.to_string()],
        },
        output_contract: StepOutputContract::Required {
            format: DataFormat::Json,
            schema_path: None,
            max_retries: 2,
            recovery_mode: OutputRecoveryMode::RepairThenRegenerate,
        },
        enabled: true,
    };

    let structured_input = resolve_structured_input(&session_context, &step)
        .unwrap()
        .unwrap();
    assert_eq!(
        structured_input,
        serde_json::json!({
            EXPLORE_STEP_ID: {"summary": "explore"}
        })
    );

    let structured_output =
            validate_structured_output(
                &step.output_contract,
                "{\"goal\":\"ship\",\"tasks\":[{\"id\":\"task-1\",\"title\":\"Inspect\",\"description\":\"Review code\"}],\"validation_targets\":[\"cargo test\"]}",
            )
                .unwrap()
                .unwrap();
    assert_eq!(
        structured_output,
        serde_json::json!({
            "goal": "ship",
            "tasks": [{"id": "task-1", "title": "Inspect", "description": "Review code"}],
            "validation_targets": ["cargo test"]
        })
    );
    assert!(validate_structured_output(&step.output_contract, "not json").is_err());
}

#[test]
fn retention_signals_collect_plan_and_execute_outputs() {
    let mut session_context = SessionContext::new(ROOT_WORKFLOW_ID);
    session_context.latest_user_turn =
        "Prefer narrow validation and keep patch sizes small".to_string();
    session_context.step_outputs.insert(
        PLAN_STEP_ID.to_string(),
        serde_json::json!({
            "goal": "Ship memory query",
            "tasks": [
                {"id": "task-1", "title": "Wire memory query", "description": "Connect planner"},
                {"id": "task-2", "title": "Render supervision", "description": "Show new stats"}
            ],
            "validation_targets": ["cargo test -p omega-context"]
        }),
    );
    session_context.step_outputs.insert(
        EXECUTE_STEP_ID.to_string(),
        serde_json::json!({
            "completed_tasks": ["task-1"],
            "open_tasks": ["task-2"],
            "validation_results": [
                {"target": "cargo test -p omega-session", "status": "passed"}
            ],
            "changed_paths": ["crates/omega-context/src/lib.rs"]
        }),
    );

    let signals = build_turn_retention_signals(&session_context);

    assert_eq!(signals.changed_paths, vec!["crates/omega-context/src/lib.rs"]);
    assert_eq!(signals.completed_tasks, vec!["task-1"]);
    assert_eq!(signals.open_tasks, vec!["task-2"]);
    assert_eq!(
        signals.validation_targets,
        vec![
            "cargo test -p omega-context".to_string(),
            "cargo test -p omega-session".to_string(),
        ]
    );
    assert_eq!(
        signals.developer_preferences,
        vec!["Prefer narrow validation and keep patch sizes small".to_string()]
    );
    assert!(signals.governance_events.is_empty());
}

#[test]
fn retention_signals_include_governance_events() {
    let mut session_context = SessionContext::new(ROOT_WORKFLOW_ID);
    session_context.governance_events.push(GovernanceEventSignal {
        label: "document.archive docs/specs/command-spec.md".to_string(),
        at: 99,
    });

    let signals = build_turn_retention_signals(&session_context);

    assert_eq!(signals.governance_events.len(), 1);
    assert_eq!(
        signals.governance_events[0].label,
        "document.archive docs/specs/command-spec.md"
    );
}

#[test]
fn structured_contract_helpers_extract_embedded_json_value() {
    let step = omega_workflow::WorkflowStep {
        id: SCENE_RECOGNITION_STEP_ID.to_string(),
        label: "Scene Recognition".to_string(),
        prompt_path: PathBuf::from(".omega/prompt/step/scene-recognition.md"),
        loop_mode: StepLoopMode::AgentLoop,
        loop_contract: None,
        max_iterations: 2,
        max_step_repeats: 0,
        hooks: Vec::new(),
        tool_request: StepToolRequest::Block(Vec::new()),
        skill_request: StepSkillRequest::MatchTask,
        input_contract: StepInputContract::None,
        output_contract: StepOutputContract::Required {
            format: DataFormat::Json,
            schema_path: None,
            max_retries: 1,
            recovery_mode: OutputRecoveryMode::RepairThenRegenerate,
        },
        enabled: true,
    };

    let structured_output = validate_structured_output(
        &step.output_contract,
        "Scene: feature\n{\"recognized_scene_id\":\"feature\"}",
    )
    .unwrap()
    .unwrap();

    assert_eq!(
        structured_output,
        serde_json::json!({"recognized_scene_id": "feature"})
    );
}

#[test]
fn schema_validator_rejects_missing_required_keys() {
    let root = std::env::temp_dir().join("omega-agent-session-schema-validation-test");
    let _ = std::fs::remove_dir_all(&root);
    let loaded = LoadedWorkflowCatalog::load(&root);
    assert!(loaded.warnings.is_empty());

    let error = validate_schema_file(
        &root,
        &PathBuf::from(DEFAULT_EXPLORE_SCHEMA_PATH),
        &serde_json::json!({"objective": "Ship feature"}),
    )
    .unwrap_err();

    assert!(error.to_string().contains("missing required key"));
}

#[test]
fn structured_contract_helpers_collect_multiple_json_candidates() {
    let response = format!(
        "Plan summary\n{}\n{}",
        feature_explore_json(),
        feature_plan_json()
    );

    let values = parse_json_values(&response);

    assert_eq!(values.len(), 2);
    assert_eq!(values[0]["objective"], "Implement the requested change");
    assert_eq!(values[1]["goal"], "Implement the requested change safely");
}

#[test]
fn parse_json_values_unwraps_array_candidates_to_individual_objects() {
    let response = r#"[{"completed_tasks":["task-1"],"open_tasks":["task-2"],"validation_results":[],"changed_paths":[]},{"completed_tasks":["task-1","task-2"],"open_tasks":[],"validation_results":[],"changed_paths":[]}]"#;
    let values = parse_json_values(response);

    // Array + 2 unwrapped objects = 3 candidates.
    assert_eq!(values.len(), 3);
    assert!(
        values[0].is_array(),
        "first candidate is the original array"
    );
    assert!(
        values[1].is_object(),
        "second candidate is first unwrapped object"
    );
    assert!(
        values[2].is_object(),
        "third candidate is second unwrapped object"
    );
    assert_eq!(values[1]["completed_tasks"], serde_json::json!(["task-1"]));
    assert_eq!(
        values[2]["completed_tasks"],
        serde_json::json!(["task-1", "task-2"])
    );
}

#[test]
fn parse_json_values_preserves_non_array_candidates() {
    // Single object should not trigger unwrapping.
    let response = r#"{"completed_tasks":["task-1"],"open_tasks":[],"validation_results":[],"changed_paths":[]}"#;
    let values = parse_json_values(response);
    assert_eq!(values.len(), 1);
    assert!(values[0].is_object());
}

#[test]
fn render_output_contract_inlines_plan_schema_details() {
    let root = std::env::temp_dir().join("omega-agent-session-render-output-contract-test");
    let _ = std::fs::remove_dir_all(&root);
    let loaded = LoadedWorkflowCatalog::load(&root);
    assert!(loaded.warnings.is_empty());

    let workflow = loaded
        .workflow_catalog
        .workflow(DEEP_RESEARCH_WORKFLOW_ID)
        .expect("deep-research workflow should exist");
    let plan_step = workflow
        .enabled_steps()
        .find(|step| step.id == PLAN_STEP_ID)
        .expect("plan step should exist");

    let rendered = render_output_contract(&root, &plan_step.output_contract);

    assert!(rendered.contains("schema_path: .omega/schema/step/plan.json"));
    assert!(rendered.contains("schema_json:"));
    assert!(rendered.contains("\"required\": ["));
    assert!(rendered.contains("\"goal\""));
    assert!(rendered.contains("\"tasks\""));
    assert!(rendered.contains("\"id\""));
    assert!(rendered.contains("\"title\""));
    assert!(rendered.contains("\"description\""));
}

#[test]
fn research_plan_validation_allows_read_only_upgrade_recommendations() {
    let root = unique_session_test_root("research-plan-upgrade-advice");
    write_review_skill(&root);
    let loaded = LoadedWorkflowCatalog::load(&root);
    let workflow = loaded
        .workflow_catalog
        .workflow(DEEP_RESEARCH_WORKFLOW_ID)
        .expect("deep-research workflow should exist");
    let plan_step = workflow
        .enabled_steps()
        .find(|step| step.id == PLAN_STEP_ID)
        .expect("plan step should exist")
        .clone();

    let value = serde_json::json!({
        "goal": "完成 Omega Rust 项目的全面风险评估报告",
        "tasks": [
            {
                "id": "2",
                "title": "依赖安全审计",
                "description": "检查 Cargo.lock 中依赖的已知 CVE，识别需要紧急更新的依赖版本，并生成依赖漏洞报告和升级建议"
            }
        ],
        "validation_targets": [
            "Cargo.lock 无已知高危 CVE",
            "报告包含升级建议和缓解措施"
        ]
    });

    validate_workflow_step_output(&root, DEEP_RESEARCH_WORKFLOW_ID, &plan_step, &value)
        .expect("read-only research planning should allow upgrade advice without requiring edits");
}

#[test]
fn research_plan_validation_allows_analytical_tasks_mentioning_optimization_concepts() {
    let root = unique_session_test_root("research-plan-optimization-analysis");
    write_review_skill(&root);
    let loaded = LoadedWorkflowCatalog::load(&root);
    let workflow = loaded
        .workflow_catalog
        .workflow(DEEP_RESEARCH_WORKFLOW_ID)
        .expect("deep-research workflow should exist");
    let plan_step = workflow
        .enabled_steps()
        .find(|step| step.id == PLAN_STEP_ID)
        .expect("plan step should exist")
        .clone();

    // These tasks describe optimization analysis but mention words like "update",
    // "config", "code", "module" in an analytical context — not as write actions.
    let value = serde_json::json!({
        "goal": "Identify optimization opportunities in the Omega Rust workspace",
        "tasks": [
            {
                "id": "opt-1",
                "title": "Analyze config update patterns",
                "description": "Review config loading code for unnecessary clone allocations and identify hot-path update patterns that could use references instead"
            },
            {
                "id": "opt-2",
                "title": "Evaluate module concurrency model",
                "description": "Examine Arc<Mutex<...>> usage across session code and assess whether RwLock or sharded locks would reduce contention"
            },
            {
                "id": "opt-3",
                "title": "Assess workflow config caching opportunity",
                "description": "Analyze how frequently workflow configs and prompt files are read from disk and evaluate whether an in-memory LRU cache would reduce file I/O"
            }
        ],
        "validation_targets": [
            "rg --files crates",
            "Cargo.toml dependency analysis completed"
        ]
    });

    validate_workflow_step_output(&root, DEEP_RESEARCH_WORKFLOW_ID, &plan_step, &value)
        .expect("analytical tasks mentioning optimization concepts should pass read-only validation");
}

#[test]
fn spawn_turn_clears_plan_validation_error_after_successful_regenerate() {
    let client: Arc<SequencedClient> = sequenced_client(vec![
        ChatResponse {
            id: "scene-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"recognized_scene_id\":\"research\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "select-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(
                "{\"selected_workflow_id\":\"research\"}",
            )],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "analysis-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_explore_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "plan-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_explore_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "plan-2".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_explore_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "plan-3".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(format!(
                "项目评估总结\n{}\n{}",
                feature_explore_json(),
                research_plan_json()
            ))],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_execute_partial_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-2".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_execute_complete_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "report-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("done")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
    ]);
    let client_dyn: DynLlmClient = client.clone();
    let root = std::env::temp_dir().join("omega-agent-session-plan-validation-clear-test");
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::create_dir_all(&root);
    let skills_dir = root.join(".claude/skills/review");
    let _ = std::fs::create_dir_all(&skills_dir);
    let _ = std::fs::write(
        skills_dir.join("SKILL.md"),
        "---\nname: review\ndescription: Review code\n---\nFind regressions.",
    );
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: client_dyn,
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();
    let (tx, rx) = mpsc::channel();

    session
        .spawn_turn_ui_compat("请你帮我仔细分析此项目的好坏".to_string(), 52, tx)
        .unwrap();

    let mut diagnostics = Vec::new();
    loop {
        match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::UpsertStepDiagnostics {
                        diagnostics: update,
                    },
            } => {
                assert_eq!(turn_id, 52);
                diagnostics.push(*update);
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::SetStatusSlot {
                        slot: StatusSlot::Agent,
                        value: StatusValue::Label(label),
                    },
            } => {
                assert_eq!(turn_id, 52);
                assert_eq!(label, "Idle");
                break;
            }
            _ => {}
        }
    }

    let plan_diagnostics = diagnostics
        .iter()
        .rev()
        .find(|diagnostics| diagnostics.step_id == PLAN_STEP_ID)
        .expect("plan diagnostics should be emitted");

    assert_eq!(plan_diagnostics.output.status, StepOutputStatus::Valid);
    assert_eq!(
        plan_diagnostics.output.attempt_kind,
        StepOutputAttemptKind::Regenerate
    );
    assert!(plan_diagnostics.output.validation_error.is_none());
    assert!(plan_diagnostics.output.previous_response_preview.is_none());
    assert!(plan_diagnostics
        .output
        .extracted_json_preview
        .as_deref()
        .is_some_and(
            |preview| preview.contains("Analyze the requested topic with read-only evidence")
        ));
}

#[test]
fn spawn_turn_rejects_research_plan_that_requires_write_access() {
    let client: Arc<SequencedClient> = sequenced_client(vec![
        ChatResponse {
            id: "scene-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"recognized_scene_id\":\"research\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "select-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(
                "{\"selected_workflow_id\":\"research\"}",
            )],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "explore-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_explore_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "plan-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_plan_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "plan-2".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_plan_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_execute_partial_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-2".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_execute_complete_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "report-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("done")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
    ]);
    let client_dyn: DynLlmClient = client.clone();
    let root = unique_session_test_root("research-plan-read-only-guard");
    write_review_skill(&root);

    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: client_dyn,
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();
    let (tx, rx) = mpsc::channel();

    session
        .spawn_turn_ui_compat(
            "请你对当前项目做一次深度系统性的潜在风险分析".to_string(),
            96,
            tx,
        )
        .unwrap();

    let mut warnings = Vec::new();
    let mut diagnostics = Vec::new();
    loop {
        match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
            RuntimeUiEnvelope::Message { turn_id, message }
                if turn_id == 96
                    && matches!(message.source, UiSource::System)
                    && message.kind == UiMessageKind::Warning =>
            {
                warnings.push(message.content.as_text().to_string());
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::UpsertStepDiagnostics {
                        diagnostics: update,
                    },
            } => {
                assert_eq!(turn_id, 96);
                diagnostics.push(*update);
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::SetStatusSlot {
                        slot: StatusSlot::Agent,
                        value: StatusValue::Label(label),
                    },
            } => {
                assert_eq!(turn_id, 96);
                assert_eq!(label, "Idle");
                break;
            }
            _ => {}
        }
    }

    assert!(diagnostics.iter().any(|diagnostics| {
        diagnostics.step_id == PLAN_STEP_ID
            && diagnostics.output.status == StepOutputStatus::Invalid
            && diagnostics
                .output
                .validation_error
                .as_deref()
                .is_some_and(|error| error.contains("must stay read-only"))
    }));
    assert!(diagnostics.iter().any(|diagnostics| {
        diagnostics.step_id == PLAN_STEP_ID
            && diagnostics.output.status == StepOutputStatus::Valid
            && diagnostics.output.attempt_kind != StepOutputAttemptKind::Primary
    }));

    let systems = client.recorded_systems();
    assert!(systems.iter().any(|system| {
        system
            .as_deref()
            .is_some_and(|system| system.contains("<output_repair step_id=\"plan\">"))
    }));
}

#[test]
fn spawn_turn_accepts_plan_when_response_wraps_single_json_object() {
    let client: Arc<SequencedClient> = sequenced_client(vec![
        ChatResponse {
            id: "scene-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"recognized_scene_id\":\"research\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "select-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(
                "{\"selected_workflow_id\":\"research\"}",
            )],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "explore-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_explore_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "plan-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_plan_report_prose_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "plan-2".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_plan_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_execute_partial_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-2".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_execute_complete_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "report-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("done")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
    ]);
    let client_dyn: DynLlmClient = client.clone();
    let root = unique_session_test_root("plan-accepts-wrapped-json");
    write_review_skill(&root);

    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: client_dyn,
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();
    let (tx, rx) = mpsc::channel();

    session
        .spawn_turn_ui_compat("请你分析当前项目并给出研究计划".to_string(), 97, tx)
        .unwrap();

    let mut diagnostics = Vec::new();
    let mut saw_result = false;
    loop {
        match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
            RuntimeUiEnvelope::Message { turn_id, message }
                if turn_id == 97
                    && matches!(message.source, UiSource::Assistant)
                    && message.kind == UiMessageKind::Result =>
            {
                assert_eq!(message.content.as_text(), "done");
                saw_result = true;
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::UpsertStepDiagnostics {
                        diagnostics: update,
                    },
            } => {
                assert_eq!(turn_id, 97);
                diagnostics.push(*update);
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::SetStatusSlot {
                        slot: StatusSlot::Agent,
                        value: StatusValue::Label(label),
                    },
            } => {
                assert_eq!(turn_id, 97);
                assert_eq!(label, "Idle");
                break;
            }
            _ => {}
        }
    }

    assert!(diagnostics.iter().any(|diagnostics| {
        diagnostics.step_id == PLAN_STEP_ID
            && diagnostics.output.status == StepOutputStatus::Valid
            && diagnostics.output.attempt_kind == StepOutputAttemptKind::Primary
    }));
    assert!(saw_result);
}

#[test]
fn spawn_turn_accepts_plan_when_response_contains_explore_echo_plus_plan_json() {
    let client: Arc<SequencedClient> = sequenced_client(vec![
        ChatResponse {
            id: "scene-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"recognized_scene_id\":\"research\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "select-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(
                "{\"selected_workflow_id\":\"research\"}",
            )],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "explore-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_explore_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "plan-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(
                research_plan_with_explore_echo_json(),
            )],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_execute_partial_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-2".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_execute_complete_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "report-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("done")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
    ]);
    let client_dyn: DynLlmClient = client.clone();
    let root = unique_session_test_root("plan-accepts-explore-echo-plus-plan");
    write_review_skill(&root);

    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: client_dyn,
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();
    let (tx, rx) = mpsc::channel();

    session
        .spawn_turn_ui_compat("请你分析项目优化方向".to_string(), 98, tx)
        .unwrap();

    let mut diagnostics = Vec::new();
    let mut saw_result = false;
    loop {
        match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
            RuntimeUiEnvelope::Message { turn_id, message }
                if turn_id == 98
                    && matches!(message.source, UiSource::Assistant)
                    && message.kind == UiMessageKind::Result =>
            {
                assert_eq!(message.content.as_text(), "done");
                saw_result = true;
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::UpsertStepDiagnostics {
                        diagnostics: update,
                    },
            } => {
                assert_eq!(turn_id, 98);
                diagnostics.push(*update);
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::SetStatusSlot {
                        slot: StatusSlot::Agent,
                        value: StatusValue::Label(label),
                    },
            } => {
                assert_eq!(turn_id, 98);
                assert_eq!(label, "Idle");
                break;
            }
            _ => {}
        }
    }

    // Plan passes on primary attempt with no retry needed — the plan-shaped
    // candidate is selected even though explore JSON is also present.
    assert!(diagnostics.iter().any(|diagnostics| {
        diagnostics.step_id == PLAN_STEP_ID
            && diagnostics.output.status == StepOutputStatus::Valid
            && diagnostics.output.attempt_kind == StepOutputAttemptKind::Primary
    }));
    assert!(saw_result);
}

#[test]
fn plan_section_hides_invalid_report_prose_and_only_shows_validated_plan_summary() {
    let client: Arc<SequencedClient> = sequenced_client(vec![
        ChatResponse {
            id: "scene-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"recognized_scene_id\":\"research\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "select-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(
                "{\"selected_workflow_id\":\"research\"}",
            )],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "explore-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_explore_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "plan-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_plan_report_prose_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "plan-2".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_plan_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_execute_partial_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-2".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_execute_complete_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "report-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("done")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
    ]);
    let client_dyn: DynLlmClient = client;
    let root = unique_session_test_root("plan-section-suppresses-invalid-prose");
    write_review_skill(&root);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: client_dyn,
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();
    let recorder = RuntimeEnvelopeRecorder::new();

    session
        .spawn_turn_with_test_bridge(
            "请你分析当前项目并给出研究计划".to_string(),
            98,
            recorder.runtime_bridge(),
        )
        .unwrap();

    let recorded = recorder.wait_for_turn_finished_messages(98, Duration::from_secs(2));
    let plan_text = recorded
        .iter()
        .filter_map(|envelope| match envelope {
            RuntimeMessageEnvelope {
                turn_id,
                message:
                    RuntimeMessage::Conversation(ConversationMessage::AppendSection { id, delta }),
            } if *turn_id == 98
                && id == &format!("turn-98:child:{}:plan", DEEP_RESEARCH_WORKFLOW_ID) =>
            {
                match delta {
                    ResponseSectionDelta::Text(text) => Some(text.as_str()),
                }
            }
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(!plan_text.contains("Omega 项目分析报告"));
    assert!(!plan_text.contains("## 项目概述"));
    assert!(plan_text.contains("Analyze the requested topic with read-only evidence"));
    assert!(plan_text.contains("\"tasks\""));
}

#[cfg(feature = "document-backend")]
#[test]
fn spawn_command_document_health_emits_command_section() {
    let _embedding_backend_guard = force_mock_document_embedding_backend();
    let root = unique_session_test_root("document-command-init");
    write_document_fixture(&root);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: Arc::new(IdleClient),
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();
    let recorder = RuntimeEnvelopeRecorder::new();

    session
        .spawn_command_with_test_bridge(
            "/document health".to_string(),
            501,
            recorder.runtime_bridge(),
        )
        .unwrap();

    let recorded = recorder.wait_for_turn_finished_messages(501, Duration::from_secs(30));
    assert!(recorded.iter().any(|envelope| {
        matches!(
            &envelope.message,
            RuntimeMessage::Conversation(ConversationMessage::BeginSection { section })
                if section.kind == ResponseSectionKind::Command
                    && matches!(
                        &section.metadata.origin,
                        SectionOrigin::Command { command_name, source }
                            if command_name == "/document health" && source == "builtin"
                    )
        )
    }));
    assert!(recorded.iter().any(|envelope| {
        matches!(
            &envelope.message,
            RuntimeMessage::Conversation(ConversationMessage::CompleteSection { state, .. })
                if *state == ResponseSectionState::Complete
        )
    }));

    let body = recorded
        .iter()
        .filter_map(|envelope| match &envelope.message {
            RuntimeMessage::Conversation(ConversationMessage::AppendSection { delta, .. }) => {
                match delta {
                    ResponseSectionDelta::Text(text) => Some(text.as_str()),
                }
            }
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(body.contains("Running /document health..."));
    assert!(body.contains("Overall health:"));
    assert!(body.contains("Total docs:"));
}

#[cfg(feature = "document-backend")]
#[test]
fn spawn_command_document_query_emits_step_knowledge_summary() {
    let _embedding_backend_guard = force_mock_document_embedding_backend();
    let root = unique_session_test_root("document-command-query-knowledge");
    write_document_fixture(&root);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: Arc::new(IdleClient),
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();
    let recorder = RuntimeEnvelopeRecorder::new();

    session
        .spawn_command_with_test_bridge(
            "/document query roadmap".to_string(),
            502,
            recorder.runtime_bridge(),
        )
        .unwrap();

    let recorded = recorder.wait_for_turn_finished_messages(502, Duration::from_secs(30));
    let summary = recorded.iter().find_map(|envelope| match &envelope.message {
        RuntimeMessage::State(StateMessage::StepKnowledgeSummary {
            section_id,
            summary,
        }) if section_id == "turn-502:command" => Some(summary.as_ref()),
        _ => None,
    });

    let summary = summary.expect("expected step knowledge summary for command section");
    let document = summary.document.as_ref().expect("expected document knowledge summary");
    assert_eq!(document.query, "roadmap");
    assert_eq!(document.mode, "hybrid");
}

#[test]
fn command_hint_renders_ready_state_for_document_query() {
    let root = unique_session_test_root("command-hint");
    write_document_fixture(&root);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: Arc::new(IdleClient),
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();

    let hint = session.command_hint("/document query roadmap").unwrap();
    assert!(hint.contains("/document"));
}

#[test]
fn spawn_command_project_info_emits_project_status_snapshot() {
    let root = unique_session_test_root("project-command-info");
    write_document_fixture(&root);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: Arc::new(IdleClient),
        system: "system".to_string(),
        cwd: root.clone(),
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();
    let recorder = RuntimeEnvelopeRecorder::new();

    session
        .spawn_command_with_test_bridge(
            "/project info".to_string(),
            560,
            recorder.runtime_bridge(),
        )
        .unwrap();

    let recorded = recorder.wait_for_turn_finished_messages(560, Duration::from_secs(30));
    let snapshot = recorded.iter().find_map(|envelope| match &envelope.message {
        RuntimeMessage::State(StateMessage::ProjectStatus { snapshot }) => Some(snapshot.as_ref()),
        _ => None,
    });

    let snapshot = snapshot.expect("expected project status snapshot");
    assert_eq!(snapshot.record.root, root.canonicalize().unwrap());
    assert!(matches!(
        snapshot.record.detection_kind,
        omega_project::ProjectDetectionKind::Cwd | omega_project::ProjectDetectionKind::LooseDirectory
    ));
    assert!(snapshot.sessions.is_empty());
    assert!(snapshot.record.active_session_id.is_none());

    let body = recorded
        .iter()
        .filter_map(|envelope| match &envelope.message {
            RuntimeMessage::Conversation(ConversationMessage::AppendSection { delta, .. }) => {
                match delta {
                    ResponseSectionDelta::Text(text) => Some(text.as_str()),
                }
            }
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(body.contains("Project ID:"));
    assert!(body.contains("Document readiness:"));
}

#[test]
fn spawn_command_project_switch_rebinds_active_project() {
    let root = unique_session_test_root("project-command-switch-a");
    let other_root = unique_session_test_root("project-command-switch-b");
    write_document_fixture(&root);
    write_document_fixture(&other_root);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: Arc::new(IdleClient),
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();
    let recorder = RuntimeEnvelopeRecorder::new();

    session
        .spawn_command_with_test_bridge(
            format!("/project switch {}", other_root.display()),
            561,
            recorder.runtime_bridge(),
        )
        .unwrap();

    let recorded = recorder.wait_for_turn_finished_messages(561, Duration::from_secs(30));
    let snapshot = recorded.iter().find_map(|envelope| match &envelope.message {
        RuntimeMessage::State(StateMessage::ProjectStatus { snapshot }) => Some(snapshot.as_ref()),
        _ => None,
    });

    let snapshot = snapshot.expect("expected project status snapshot after switch");
    assert_eq!(snapshot.record.root, other_root.canonicalize().unwrap());
    assert_eq!(
        session.project_detail_snapshot().unwrap().record.root,
        other_root.canonicalize().unwrap()
    );
}

#[test]
fn spawn_command_project_switch_rebinds_runtime_skill_hook_and_tool_surfaces() {
    let root = unique_session_test_root("project-command-switch-bindings-a");
    let other_root = unique_session_test_root("project-command-switch-bindings-b");
    write_document_fixture(&root);
    write_document_fixture(&other_root);
    write_named_skill(&root, "review", "Review code", "Find regressions.");
    write_named_skill(&other_root, "docs-specs", "Write specs", "Be precise.");
    write_builtin_hook_manifest(&root, "hook-a");
    write_builtin_hook_manifest(&other_root, "hook-b");

    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: Arc::new(IdleClient),
        system: "system".to_string(),
        cwd: root.clone(),
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();

    let before = session.debug_runtime_bindings_snapshot();
    assert!(before.skill_descriptions.iter().any(|line| line.contains("review")));
    assert!(!before.skill_descriptions.iter().any(|line| line.contains("docs-specs")));
    assert!(before.available_tool_ids.iter().any(|id| id == "load_skill"));
    assert_eq!(before.hook_ids, vec!["hook-a".to_string()]);

    let recorder = RuntimeEnvelopeRecorder::new();
    session
        .spawn_command_with_test_bridge(
            format!("/project switch {}", other_root.display()),
            562,
            recorder.runtime_bridge(),
        )
        .unwrap();

    let _recorded = recorder.wait_for_turn_finished_messages(562, Duration::from_secs(30));
    let after = session.debug_runtime_bindings_snapshot();
    assert_eq!(after.cwd, other_root.canonicalize().unwrap());
    assert!(after.skill_descriptions.iter().any(|line| line.contains("docs-specs")));
    assert!(!after.skill_descriptions.iter().any(|line| line.contains("review: Review code")));
    assert!(after.available_tool_ids.iter().any(|id| id == "load_skill"));
    assert_eq!(after.hook_ids, vec!["hook-b".to_string()]);
}

#[test]
fn spawn_command_session_new_list_info_and_delete_manage_project_sessions() {
    let root = unique_session_test_root("session-command-manage");
    write_document_fixture(&root);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: Arc::new(IdleClient),
        system: "system".to_string(),
        cwd: root.clone(),
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();

    let initial_snapshot = session.project_detail_snapshot().unwrap();
    assert!(initial_snapshot.record.active_session_id.is_none());
    assert!(initial_snapshot.sessions.is_empty());

    let new_events = run_command(&session, "/session new Follow up", 570);
    let restored = new_events.iter().find_map(|envelope| match &envelope.message {
        RuntimeMessage::State(StateMessage::SessionRestored { snapshot }) => Some(snapshot.as_ref()),
        _ => None,
    });
    let restored = restored.expect("expected session restored state after /session new");
    let new_session_id = restored.session_id.clone();
    assert_eq!(restored.title, "Follow up");
    assert_eq!(restored.root_workflow_id, ROOT_WORKFLOW_ID);
    assert_eq!(restored.active_workflow_id, ROOT_WORKFLOW_ID);
    assert!(command_body(&new_events).contains("Started new session"));

    let after_new = session.project_detail_snapshot().unwrap();
    assert_eq!(after_new.record.active_session_id.as_deref(), Some(new_session_id.as_str()));
    assert_eq!(after_new.sessions.len(), 1);

    let session_events = run_command(&session, "/session", 5701);
    let picker = match show_overlay_content(&session_events, OverlayTarget::Picker) {
        Some(UiContent::OperatorPicker(request)) => request,
        other => panic!("expected session picker overlay, got {other:?}"),
    };
    assert_eq!(picker.items.len(), 1);
    assert_eq!(picker.primary_action.label, "Detail");
    assert!(matches!(
        &picker.primary_action.intent,
        OperatorPickerIntent::SubmitSlashCommand { command_template }
            if command_template == "/session info {id} --picker"
    ));

    let list_events = run_command(&session, "/session list", 571);
    assert!(command_body(&list_events).trim().is_empty());
    let list_picker = match show_overlay_content(&list_events, OverlayTarget::Picker) {
        Some(UiContent::OperatorPicker(request)) => request,
        other => panic!("expected session list picker overlay, got {other:?}"),
    };
    assert!(list_picker.items.iter().any(|item| item.id == new_session_id));

    let info_events = run_command(&session, format!("/session info {new_session_id}"), 572);
    assert!(command_body(&info_events).trim().is_empty());
    let info_overlay = match show_overlay_content(&info_events, OverlayTarget::Detail) {
        Some(UiContent::Text(text)) => text,
        other => panic!("expected session detail overlay, got {other:?}"),
    };
    assert!(info_overlay.contains(&format!("Session ID: {new_session_id}")));
    assert!(info_overlay.contains("Resume ready: true"));

    let delete_events = run_command(&session, "/session delete", 573);
    assert!(command_body(&delete_events)
        .contains("Error: refusing to delete the active session; create or resume another session first"));
}

#[test]
fn spawn_command_plan_create_list_show_and_select_manage_project_tasks() {
    let root = unique_session_test_root("plan-command-manage");
    write_document_fixture(&root);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: Arc::new(IdleClient),
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();

    let empty_events = run_command(&session, "/plan list", 5740);
    assert!(command_body(&empty_events).contains("No tasks."));

    let create_events = run_command(&session, "/plan create --priority p1 Build plan store", 5741);
    let create_body = command_body(&create_events);
    assert!(create_body.contains("Task: TASK-0001"));
    assert!(create_body.contains("Priority: p1"));
    assert!(create_body.contains("Build plan store"));

    let list_events = run_command(&session, "/plan list all", 5742);
    assert!(command_body(&list_events).contains("TASK-0001"));

    let show_events = run_command(&session, "/plan show TASK-0001", 5743);
    assert!(command_body(&show_events).contains("Task: TASK-0001"));
    assert!(command_body(&show_events).contains("Build plan store"));

    let select_events = run_command(&session, "/plan select TASK-0001", 5744);
    assert!(command_body(&select_events).contains("Selected task TASK-0001"));
    assert_eq!(session.current_selected_task_id().as_deref(), Some("TASK-0001"));

    let clear_events = run_command(&session, "/plan select none", 5745);
    assert!(command_body(&clear_events).contains("Cleared selected project task"));
    assert_eq!(session.current_selected_task_id(), None);
}

#[test]
fn spawn_command_plan_list_enter_uses_links_navigator() {
    let root = unique_session_test_root("plan-command-project-task-store");
    write_document_fixture(&root);
    std::fs::create_dir_all(root.join("docs-data/tasks")).unwrap();
    std::fs::write(
        root.join("docs-data/tasks/project-tasks.jsonl"),
        r#"{"id":"TASK-0001","title":"Bootstrap plan tasks","kind":"chore","status":"ready","priority":"p1","order_key":1000,"summary":"Create project tasks from project task store","requirement":"Read project tasks from docs-data/tasks/project-tasks.jsonl","acceptance":["project task store exists"],"parent_id":null,"depends_on":[],"tags":["structured-docs"],"design_links":[{"kind":"spec","path":"docs/specs/omega-project-plan-system.md","label":null}],"implementation_links":[],"doc_scope":["spec"]}
{"id":"TASK-0002","title":"Validate bootstrap order","kind":"chore","status":"blocked","priority":"p2","order_key":2000,"summary":"Keep dependency mapping stable","requirement":"Preserve task dependencies in omega-plan","acceptance":["dependency chain is preserved"],"parent_id":null,"depends_on":["TASK-0001"],"tags":["structured-docs"],"design_links":[{"kind":"spec","path":"docs/specs/omega-structured-document-system.md","label":null}],"implementation_links":[],"doc_scope":["spec"]}
"#,
    )
    .unwrap();

    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: Arc::new(IdleClient),
        system: "system".to_string(),
        cwd: root.clone(),
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();

    let snapshot = session.project_detail_snapshot().unwrap();
    assert_eq!(snapshot.plan.current_task_count, 2);

    let list_events = run_command(&session, "/plan list", 5746);
    let list_body = command_body(&list_events);
    assert!(list_body.contains("Plan view: current"));
    assert!(list_body.contains("Bootstrap plan tasks"));
    assert!(!list_body.contains("No tasks."));

    let picker = match show_overlay_content(&list_events, OverlayTarget::Picker) {
        Some(UiContent::OperatorPicker(request)) => request,
        other => panic!("expected /plan list picker overlay, got {other:?}"),
    };
    assert_eq!(picker.items.len(), 2);
    assert_eq!(picker.primary_action.label, "Links");
    assert!(matches!(
        &picker.primary_action.intent,
        OperatorPickerIntent::SubmitSlashCommand { command_template }
            if command_template == "/plan links {id}"
    ));
    assert!(picker.items.iter().any(|item| item.title == "Bootstrap plan tasks"));
    assert!(picker.secondary_actions.iter().any(|action| {
        action.label == "Select"
            && matches!(
                &action.intent,
                OperatorPickerIntent::SubmitSlashCommand { command_template }
                    if command_template == "/plan select {id}"
            )
    }));
}

#[test]
fn spawn_command_plan_links_unknown_task_returns_error() {
    let root = unique_session_test_root("plan-command-links-unknown-task");
    write_document_fixture(&root);

    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: Arc::new(IdleClient),
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();

    let events = run_command(&session, "/plan links TASK-9999", 58439);
    assert!(command_body(&events).contains("Error: unknown task 'TASK-9999'"));
    assert!(show_overlay_content(&events, OverlayTarget::Picker).is_none());
}

#[test]
fn spawn_command_plan_links_emits_picker_overlay() {
    let root = unique_session_test_root("plan-command-links-picker");
    write_document_fixture(&root);
    std::fs::create_dir_all(root.join("docs/specs")).unwrap();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("docs/specs/navigator.md"),
        "# Navigator\n\nDesign link preview.\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/navigator.rs"),
        "pub fn navigator() -> &'static str {\n    \"ok\"\n}\n",
    )
    .unwrap();

    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: Arc::new(IdleClient),
        system: "system".to_string(),
        cwd: root.clone(),
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();

    let _create = run_command(&session, "/plan create Build navigator", 5840);
    let _design = run_command(
        &session,
        "/plan link TASK-0001 design docs/specs/navigator.md",
        5841,
    );
    let _implementation = run_command(
        &session,
        "/plan link TASK-0001 implementation src/navigator.rs",
        5842,
    );
    let _log = run_command(&session, "/plan log TASK-0001 Investigate navigator flow", 5843);

    let links_events = run_command(&session, "/plan links TASK-0001", 5844);
    let links_body = command_body(&links_events);
    assert!(
        !links_body.contains("Error:"),
        "unexpected links command body: {links_body}"
    );
    let picker = match show_overlay_content(&links_events, OverlayTarget::Picker) {
        Some(UiContent::OperatorPicker(request)) => request,
        other => panic!("expected /plan links picker overlay, got {other:?}"),
    };
    assert!(picker.title.contains("TASK-0001"));
    assert!(matches!(
        &picker.primary_action.intent,
        OperatorPickerIntent::SubmitSlashCommand { command_template }
            if command_template == "/plan open-link TASK-0001 {id}"
    ));
    assert!(picker.items.iter().any(|item| {
        item.id == "docs/specs/navigator.md" && item.badges.iter().any(|badge| badge == "doc")
    }));
    assert!(picker.items.iter().any(|item| item.id == "src/navigator.rs"));
    assert!(picker
        .items
        .iter()
        .any(|item| item.id.starts_with("log-entry:")));
    assert!(picker.secondary_actions.iter().any(|action| {
        action.label == "Back"
            && matches!(
                &action.intent,
                OperatorPickerIntent::SubmitSlashCommand { command_template }
                    if command_template == "/plan list"
            )
    }));
}

#[test]
fn spawn_command_plan_view_file_emits_detail_overlay() {
    let root = unique_session_test_root("plan-command-view-file");
    write_document_fixture(&root);
    std::fs::create_dir_all(root.join("docs/specs")).unwrap();
    std::fs::write(
        root.join("docs/specs/navigator.md"),
        "---\nstatus: draft\n---\n\n# Navigator\n\nUseful content.\n",
    )
    .unwrap();

    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: Arc::new(IdleClient),
        system: "system".to_string(),
        cwd: root.clone(),
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();

    let view_events = run_command(&session, "/plan view-file docs/specs/navigator.md", 5845);
    let view_body = command_body(&view_events);
    assert!(
        !view_body.contains("Error:"),
        "unexpected view-file command body: {view_body}"
    );
    let overlay = match show_overlay_content(&view_events, OverlayTarget::Detail) {
        Some(UiContent::Text(text)) => text,
        other => panic!("expected /plan view-file detail overlay, got {other:?}"),
    };
    assert!(overlay.contains("Navigator"));
    assert!(overlay.contains("Useful content."));
}

#[test]
fn spawn_command_plan_view_file_rejects_path_traversal() {
    let root = unique_session_test_root("plan-command-view-file-traversal");
    write_document_fixture(&root);

    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: Arc::new(IdleClient),
        system: "system".to_string(),
        cwd: root.clone(),
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();

    let view_events = run_command(&session, "/plan view-file ../Cargo.toml", 5846);
    assert!(command_body(&view_events).contains("Error:"));
    assert!(show_overlay_content(&view_events, OverlayTarget::Detail).is_none());
}

#[test]
fn spawn_command_plan_mutations_and_sync_todo_update_task_projection() {
    let root = unique_session_test_root("plan-command-mutations");
    write_document_fixture(&root);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: Arc::new(IdleClient),
        system: "system".to_string(),
        cwd: root.clone(),
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();

    let _first_create = run_command(&session, "/plan create First task", 5746);
    let _second_create = run_command(&session, "/plan create Second task", 5747);
    let update_events = run_command(
        &session,
        "/plan update TASK-0001 --status ready --summary Ready summary --accept passes tests --tag core",
        5748,
    );
    assert!(command_body(&update_events).contains("Status: ready"));
    assert!(command_body(&update_events).contains("passes tests"));

    let _reprioritize_first = run_command(&session, "/plan prioritize TASK-0001 p0", 5749);
    let reprioritize_second = run_command(
        &session,
        "/plan prioritize TASK-0002 p0 --before TASK-0001",
        5750,
    );
    assert!(command_body(&reprioritize_second).contains("Priority: p0"));

    let list_events = run_command(&session, "/plan list all --priority p0", 5751);
    let list_body = command_body(&list_events);
    assert!(list_body.contains("TASK-0002 [p0 backlog]"));
    assert!(list_body.contains("TASK-0001 [p0 ready]"));

    let depends_events = run_command(&session, "/plan depends add TASK-0001 TASK-0002", 5752);
    assert!(command_body(&depends_events).contains("Dependencies: TASK-0002"));

    let log_events = run_command(&session, "/plan log TASK-0001 Investigate edge cases", 5753);
    assert!(command_body(&log_events).contains("Investigate edge cases"));

    let link_events = run_command(
        &session,
        "/plan link TASK-0001 implementation crates/omega-plan/src/lib.rs",
        5754,
    );
    assert!(command_body(&link_events).contains("Implementation links:"));
    assert!(command_body(&link_events).contains("crates/omega-plan/src/lib.rs"));

    let sync_events = run_command(&session, "/plan sync-todo", 5755);
    assert!(command_body(&sync_events).contains("Synced 2 open project-plan tasks"));
    let todo = std::fs::read_to_string(root.join("docs/TODO.md")).unwrap();
    assert!(todo.contains("<!-- omega-plan-sync:start -->"));
    assert!(todo.contains("TASK-0001 [p0 ready] First task"));
    assert!(todo.contains("TASK-0002 [p0 backlog] Second task"));
}

#[test]
fn spawn_command_plan_migrate_todo_imports_open_tracks_idempotently() {
    let root = unique_session_test_root("plan-command-migrate-todo");
    write_document_fixture(&root);
    std::fs::write(
        root.join("docs/TODO.md"),
        r#"---
status: active
owner: omega-team
last_verified_commit: N/A
updated: 2026-04-14
---

# TODO

## Active Tasks

### Task 10: omega-subagent — SubAgent
- **Status**: Pending
- **Priority**: High
- **Description**: Wire the parent task tool to real child execution.
- **Related**: `docs/specs/omega-agent-impl-plan.md`

### Task 4: omega-plan / omega-project — Project Plan Management
- **Status**: In Progress
- **Priority**: Medium
- **Description**: Finish project plan migration and runtime integration.
- **Blocked by**: `Task 10`
- **Related**: `docs/specs/omega-project-plan-system.md`, `docs/specs/omega-command-system.md`

## Notes

- test fixture
"#,
    )
    .unwrap();

    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: Arc::new(IdleClient),
        system: "system".to_string(),
        cwd: root.clone(),
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();

    let migrate_events = run_command(&session, "/plan migrate-todo", 5756);
    let migrate_body = command_body(&migrate_events);
    assert!(migrate_body.contains("Imported 2 docs/TODO.md open task(s)."));
    assert!(migrate_body.contains("TASK-0001"));
    assert!(migrate_body.contains("TASK-0002"));

    let show_first = run_command(&session, "/plan show TASK-0001", 5757);
    let show_first_body = command_body(&show_first);
    assert!(show_first_body.contains("omega-subagent — SubAgent"));
    assert!(show_first_body.contains("docs/specs/omega-agent-impl-plan.md"));

    let show_second = run_command(&session, "/plan show TASK-0002", 5758);
    let show_second_body = command_body(&show_second);
    assert!(show_second_body.contains("Dependencies: TASK-0001"));
    assert!(show_second_body.contains("docs/specs/omega-project-plan-system.md"));
    assert!(show_second_body.contains("docs/specs/omega-command-system.md"));

    let rerun_events = run_command(&session, "/plan migrate-todo", 5759);
    let rerun_body = command_body(&rerun_events);
    assert!(
        rerun_body.contains("Imported 0 docs/TODO.md open task(s)."),
        "rerun_body: {rerun_body}"
    );
    assert!(
        rerun_body.contains("Skipped existing imports: Task 10, Task 4"),
        "rerun_body: {rerun_body}"
    );

    let list_events = run_command(&session, "/plan list all", 5760);
    let list_body = command_body(&list_events);
    assert_eq!(list_body.matches("TASK-").count(), 2);
}

#[test]
fn plan_load_previews_and_imports_task_heading_docs_idempotently() {
    let root = unique_session_test_root("plan-load-task-headings");
    write_document_fixture(&root);
    std::fs::create_dir_all(root.join("docs/prds")).unwrap();
    std::fs::write(
        root.join("docs/prds/roadmap.md"),
        r#"---
status: draft
owner: omega-team
last_verified_commit: N/A
updated: 2026-04-14
---

# Roadmap

### Task 40: Build loader
- **Status**: Pending
- **Priority**: High
- **Description**: Build document-backed plan loading.
- **Related**: `docs/specs/omega-project-plan-system.md`

#### Task 41: Validate preview
- **Status**: Blocked
- **Priority**: Medium
- **Description**: Validate preview and apply behavior.
- **Blocked by**: `Task 40`
"#,
    )
    .unwrap();

    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: Arc::new(IdleClient),
        system: "system".to_string(),
        cwd: root.clone(),
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();

    let preview_events = run_command(&session, "/plan load docs/prds", 5761);
    let preview_body = command_body(&preview_events);
    assert!(preview_body.contains("Mode: preview"));
    assert!(preview_body.contains("Matched files: docs/prds/roadmap.md"));
    assert!(preview_body.contains("Candidates: 2"));
    assert!(preview_body.contains("Would create: 2"));
    let picker = match show_overlay_content(&preview_events, OverlayTarget::Picker) {
        Some(UiContent::OperatorPicker(request)) => request,
        other => panic!("expected /plan load picker overlay, got {other:?}"),
    };
    assert_eq!(picker.items.len(), 2);
    assert_eq!(picker.primary_action.label, "Detail");
    assert!(picker.secondary_actions.iter().any(|action| {
        action.label == "Load"
            && matches!(
                &action.intent,
                OperatorPickerIntent::RequestConfirmSlashCommand { command_template, .. }
                    if command_template == "/plan load docs/prds --apply"
            )
    }));

    let empty_list = run_command(&session, "/plan list", 5762);
    assert!(command_body(&empty_list).contains("No tasks."));

    let apply_events = run_command(&session, "/plan load docs/prds --apply", 5763);
    let apply_body = command_body(&apply_events);
    assert!(apply_body.contains("Mode: apply"));
    assert!(apply_body.contains("Created task ids: TASK-0001, TASK-0002"));

    let show_first = run_command(&session, "/plan show TASK-0001", 5764);
    let show_first_body = command_body(&show_first);
    assert!(show_first_body.contains("Build loader"));
    assert!(show_first_body.contains("docs/prds/roadmap.md"));

    let show_second = run_command(&session, "/plan show TASK-0002", 5765);
    let show_second_body = command_body(&show_second);
    assert!(show_second_body.contains("Dependencies: TASK-0001"));

    std::fs::write(
        root.join("docs/prds/roadmap.md"),
        r#"---
status: draft
owner: omega-team
last_verified_commit: N/A
updated: 2026-04-14
---

# Roadmap

### Task 40: Build loader
- **Status**: In Progress
- **Priority**: High
- **Description**: Build document-backed plan loading and rerun updates.
- **Related**: `docs/specs/omega-project-plan-system.md`

#### Task 41: Validate preview
- **Status**: Blocked
- **Priority**: Medium
- **Description**: Validate preview and apply behavior.
- **Blocked by**: `Task 40`
"#,
    )
    .unwrap();

    let reapply_events = run_command(&session, "/plan load docs/prds --apply", 5766);
    let reapply_body = command_body(&reapply_events);
    assert!(reapply_body.contains("Created task ids: none"));
    assert!(reapply_body.contains("Updated task ids: TASK-0001, TASK-0002"));

    let updated_show = run_command(&session, "/plan show TASK-0001", 5767);
    let updated_body = command_body(&updated_show);
    assert!(updated_body.contains("Status: in_progress") || updated_body.contains("Status: in progress") || updated_body.contains("Status: in_progress"));
    assert!(updated_body.contains("Build document-backed plan loading and rerun updates."));

    let list_events = run_command(&session, "/plan list all", 5768);
    assert_eq!(command_body(&list_events).matches("TASK-").count(), 2);
}

#[test]
fn plan_load_todo_kind_reuses_existing_todo_import_identity() {
    let root = unique_session_test_root("plan-load-todo-compat");
    write_document_fixture(&root);
    std::fs::write(
        root.join("docs/TODO.md"),
        r#"---
status: active
owner: omega-team
last_verified_commit: N/A
updated: 2026-04-14
---

# TODO

## Active Tasks

### Task 10: omega-subagent — SubAgent
- **Status**: Pending
- **Priority**: High
- **Description**: Wire the parent task tool to real child execution.
- **Related**: `docs/specs/omega-agent-impl-plan.md`

### Task 4: omega-plan / omega-project — Project Plan Management
- **Status**: In Progress
- **Priority**: Medium
- **Description**: Finish project plan migration and runtime integration.
- **Blocked by**: `Task 10`
- **Related**: `docs/specs/omega-project-plan-system.md`, `docs/specs/omega-command-system.md`
"#,
    )
    .unwrap();

    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: Arc::new(IdleClient),
        system: "system".to_string(),
        cwd: root.clone(),
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();

    let migrate_events = run_command(&session, "/plan migrate-todo", 5769);
    assert!(command_body(&migrate_events).contains("Imported 2 docs/TODO.md open task(s)."));

    let load_events = run_command(
        &session,
        "/plan load docs/TODO.md --kind todo --apply",
        5770,
    );
    let load_body = command_body(&load_events);
    assert!(load_body.contains("Created task ids: none"));
    assert!(load_body.contains("Updated task ids: TASK-0001, TASK-0002"));

    let list_events = run_command(&session, "/plan list all", 5771);
    assert_eq!(command_body(&list_events).matches("TASK-").count(), 2);
}

#[test]
fn plan_load_docs_skips_invalid_task_heading_status_with_warning() {
    let root = unique_session_test_root("plan-load-invalid-status-warning");
    write_document_fixture(&root);
    std::fs::create_dir_all(root.join("docs/specs")).unwrap();
    std::fs::write(
        root.join("docs/specs/valid-loader.md"),
        r#"---
status: draft
owner: omega-team
last_verified_commit: N/A
updated: 2026-04-14
---

# Loader

### Task 70: Import valid tasks
- **Status**: Pending
- **Priority**: Medium
- **Description**: Import valid tasks from docs.
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("docs/specs/invalid-status.md"),
        r#"---
status: draft
owner: omega-team
last_verified_commit: N/A
updated: 2026-04-14
---

# Invalid

### Task 71: Old completed slice
- **Status**: Implemented on 2026-04-08.
- **Priority**: Medium
- **Description**: Historical visual cleanup slice.
"#,
    )
    .unwrap();

    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: Arc::new(IdleClient),
        system: "system".to_string(),
        cwd: root.clone(),
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();

    let preview_events = run_command(&session, "/plan load docs", 5772);
    let preview_body = command_body(&preview_events);
    assert!(preview_body.contains("Mode: preview"));
    assert!(preview_body.contains("docs/specs/valid-loader.md"));
    assert!(preview_body.contains("Would create: 1"));
    assert!(preview_body.contains("Warnings:"));
    assert!(preview_body.contains("docs/specs/invalid-status.md"));
    assert!(preview_body.contains("unsupported task import status 'implemented on 2026-04-08.'"));

    let apply_events = run_command(&session, "/plan load docs --apply", 5773);
    let apply_body = command_body(&apply_events);
    assert!(apply_body.contains("Created task ids: TASK-0001"));

    let show_events = run_command(&session, "/plan show TASK-0001", 5774);
    let show_body = command_body(&show_events);
    assert!(show_body.contains("Import valid tasks"));
}

#[test]
fn selected_plan_task_is_restored_after_session_resume() {
    let root = unique_session_test_root("plan-command-resume");
    write_document_fixture(&root);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: Arc::new(IdleClient),
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();

    let new_events = run_command(&session, "/session new Planning", 5750);
    let original_session_id = new_events
        .iter()
        .find_map(|envelope| match &envelope.message {
            RuntimeMessage::State(StateMessage::SessionRestored { snapshot }) => {
                Some(snapshot.session_id.clone())
            }
            _ => None,
        })
        .expect("expected planning session id");

    let _create_events = run_command(&session, "/plan create Implement selected task restore", 5751);
    let _select_events = run_command(&session, "/plan select TASK-0001", 5752);
    assert_eq!(session.current_selected_task_id().as_deref(), Some("TASK-0001"));

    let _scratch_events = run_command(&session, "/session new Scratch", 5753);
    assert_eq!(session.current_selected_task_id(), None);

    let _resume_events = run_command(&session, format!("/session resume {original_session_id}"), 5754);
    assert_eq!(session.current_selected_task_id().as_deref(), Some("TASK-0001"));
}

#[test]
fn missing_selected_plan_task_is_cleared_with_warning_after_session_resume() {
    let root = unique_session_test_root("plan-command-resume-missing-task");
    write_document_fixture(&root);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: Arc::new(IdleClient),
        system: "system".to_string(),
        cwd: root.clone(),
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();

    let new_events = run_command(&session, "/session new Planning", 57540);
    let original_session_id = new_events
        .iter()
        .find_map(|envelope| match &envelope.message {
            RuntimeMessage::State(StateMessage::SessionRestored { snapshot }) => {
                Some(snapshot.session_id.clone())
            }
            _ => None,
        })
        .expect("expected planning session id");

    let _create_events = run_command(&session, "/plan create Implement selected task restore", 57541);
    let _select_events = run_command(&session, "/plan select TASK-0001", 57542);
    assert_eq!(session.current_selected_task_id().as_deref(), Some("TASK-0001"));

    let tasks_path = root.join("docs-data/tasks/project-tasks.jsonl");
    let retained = std::fs::read_to_string(&tasks_path)
        .unwrap()
        .lines()
        .filter(|line| !line.contains("\"id\":\"TASK-0001\""))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(
        &tasks_path,
        if retained.is_empty() {
            String::new()
        } else {
            format!("{retained}\n")
        },
    )
    .unwrap();

    let resume_events = run_command(&session, format!("/session resume {original_session_id}"), 57543);
    assert_eq!(session.current_selected_task_id(), None);
    assert!(resume_events.iter().any(|envelope| {
        matches!(
            &envelope.message,
            RuntimeMessage::State(StateMessage::Activity { kind, text, .. })
                if *kind == RuntimeContentKind::Warning
                    && text.contains("Selected project task TASK-0001 no longer exists")
        )
    }));
}

#[test]
fn selected_plan_task_is_injected_into_system_prompt_for_turns() {
    let client: Arc<SequencedClient> = sequenced_client(vec![
        ChatResponse {
            id: "scene-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"recognized_scene_id\":\"feature\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "select-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"selected_workflow_id\":\"feature\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "explore-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_explore_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "plan-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_plan_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_execute_partial_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-2".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_execute_complete_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "report-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("done")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
    ]);
    let root = unique_session_test_root("plan-selected-task-prompt");
    write_document_fixture(&root);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: client.clone(),
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();

    let _session_events = run_command(&session, "/session new Planning", 5755);
    let _create_events = run_command(&session, "/plan create Implement selected task injection", 5756);
    let _select_events = run_command(&session, "/plan select TASK-0001", 5757);

    let recorder = RuntimeEnvelopeRecorder::new();
    session
        .spawn_turn_with_test_bridge(
            "implement the requested change".to_string(),
            5758,
            recorder.runtime_bridge(),
        )
        .unwrap();

    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if client.recorded_systems().iter().flatten().any(|system| {
            system.contains("<selected_project_task>")
                && system.contains("task_id: TASK-0001")
                && system.contains("Implement selected task injection")
        }) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    let recorded_systems = client.recorded_systems();
    assert!(
        recorded_systems.iter().flatten().any(|system| {
            system.contains("<selected_project_task>")
                && system.contains("task_id: TASK-0001")
                && system.contains("Implement selected task injection")
        }),
        "recorded systems: {recorded_systems:#?}"
    );
}

#[test]
fn plan_send_dispatches_normal_turn_and_appends_task_turn_log() {
    let client: Arc<SequencedClient> = counting_sequenced_client(
        vec![
            ChatResponse {
                id: "scene-1".to_string(),
                model: Some("test-model".to_string()),
                content: vec![ContentBlock::text("{\"recognized_scene_id\":\"feature\"}")],
                stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                usage: None,
            },
            ChatResponse {
                id: "select-1".to_string(),
                model: Some("test-model".to_string()),
                content: vec![ContentBlock::text("{\"selected_workflow_id\":\"feature\"}")],
                stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                usage: None,
            },
            ChatResponse {
                id: "explore-1".to_string(),
                model: Some("test-model".to_string()),
                content: vec![ContentBlock::text(feature_explore_json())],
                stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                usage: None,
            },
            ChatResponse {
                id: "plan-1".to_string(),
                model: Some("test-model".to_string()),
                content: vec![ContentBlock::text(feature_plan_json())],
                stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                usage: None,
            },
            ChatResponse {
                id: "execute-1".to_string(),
                model: Some("test-model".to_string()),
                content: vec![ContentBlock::text(feature_execute_partial_json())],
                stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                usage: None,
            },
            ChatResponse {
                id: "execute-2".to_string(),
                model: Some("test-model".to_string()),
                content: vec![ContentBlock::text(feature_execute_complete_json())],
                stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                usage: None,
            },
            ChatResponse {
                id: "report-1".to_string(),
                model: Some("test-model".to_string()),
                content: vec![ContentBlock::text("done")],
                stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                usage: None,
            },
        ],
        321,
    );
    let root = unique_session_test_root("plan-send-turn-log");
    write_document_fixture(&root);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: client.clone(),
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();

    let _session_events = run_command(&session, "/session new Planning", 5756);
    let _create_events = run_command(&session, "/plan create Implement plan send", 5757);
    let send_events = run_command(
        &session,
        "/plan send TASK-0001 implement the requested change",
        5758,
    );
    assert!(send_events.iter().any(|envelope| {
        matches!(
            &envelope.message,
            RuntimeMessage::State(StateMessage::TurnFinished)
        )
    }));
    assert_eq!(session.current_selected_task_id().as_deref(), Some("TASK-0001"));
    assert!(client.recorded_systems().iter().flatten().any(|system| {
        system.contains("<selected_project_task>")
            && system.contains("task_id: TASK-0001")
            && system.contains("Implement plan send")
    }));

    let show_events = run_command(&session, "/plan show TASK-0001", 5759);
    let show_body = command_body(&show_events);
    assert!(show_body.contains("DeliveryAttached"));
    assert!(show_body.contains("Prompt: implement the requested change"));
    assert!(show_body.contains("Response: done"));
    assert!(show_body.contains("delivery: model=test-model"));
    assert!(show_body.contains("llm_requests=7"));
}

#[test]
fn plan_send_failure_appends_partial_delivery_log() {
    let mut builder = SequencedClient::builder();
    for _ in 0..16 {
        builder = builder.push_count_tokens(321);
    }
    builder = builder.push_failure(omega_client::ClientError::Stream(
        "forced task send failure".to_string(),
    ));
    let client = Arc::new(builder.build());

    let root = unique_session_test_root("plan-send-failure-log");
    write_document_fixture(&root);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client,
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();

    let _session_events = run_command(&session, "/session new Planning", 5760);
    let _create_events = run_command(&session, "/plan create Implement failure logging", 5761);
    let send_events = run_command(
        &session,
        "/plan send TASK-0001 trigger failure path",
        5762,
    );
    assert!(send_events.iter().any(|envelope| {
        matches!(
            &envelope.message,
            RuntimeMessage::Conversation(ConversationMessage::Text { kind, text, .. })
                if *kind == RuntimeContentKind::Error
                    && text.contains("forced task send failure")
        )
    }));
    assert!(send_events.iter().any(|envelope| {
        matches!(&envelope.message, RuntimeMessage::State(StateMessage::TurnFinished))
    }));

    let show_events = run_command(&session, "/plan show TASK-0001", 5763);
    let show_body = command_body(&show_events);
    assert!(show_body.contains("PartialDelivery"));
    assert!(show_body.contains("forced task send failure"));
}

#[test]
fn spawn_command_session_resume_restores_saved_routing_context() {
    let client: Arc<SequencedClient> = sequenced_client(vec![
        ChatResponse {
            id: "scene-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"recognized_scene_id\":\"feature\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "select-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"selected_workflow_id\":\"feature\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "explore-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_explore_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "plan-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_plan_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_execute_partial_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-2".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_execute_complete_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "report-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("done")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
    ]);
    let root = unique_session_test_root("session-command-resume-routing");
    write_document_fixture(&root);
    write_review_skill(&root);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: client,
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();

    let turn_recorder = RuntimeEnvelopeRecorder::new();
    session
        .spawn_turn_with_test_bridge("fix this bug".to_string(), 580, turn_recorder.runtime_bridge())
        .unwrap();
    let _turn_events = turn_recorder.wait_for_turn_finished_messages(580, Duration::from_secs(30));
    let original_session_id = session
        .current_session_id()
        .expect("first real turn should lazily bind a session");

    let _new_events = run_command(&session, "/session new Scratch", 581);
    let picker_events = run_command(&session, "/session resume", 5811);
    assert!(command_body(&picker_events).trim().is_empty());
    let resume_picker = match show_overlay_content(&picker_events, OverlayTarget::Picker) {
        Some(UiContent::OperatorPicker(request)) => request,
        other => panic!("expected resume picker overlay, got {other:?}"),
    };
    assert!(resume_picker.items.iter().any(|item| item.id == original_session_id));
    assert_eq!(resume_picker.primary_action.label, "Resume");
    assert!(matches!(
        &resume_picker.primary_action.intent,
        OperatorPickerIntent::SubmitSlashCommand { command_template }
            if command_template == "/session resume {id} --picker"
    ));
    assert!(matches!(
        &resume_picker.secondary_actions[0].intent,
        OperatorPickerIntent::SubmitSlashCommand { command_template }
            if command_template == "/session info {id} --picker"
    ));

    let resume_events = run_command(&session, format!("/session resume {original_session_id}"), 582);

    let restored = resume_events.iter().find_map(|envelope| match &envelope.message {
        RuntimeMessage::State(StateMessage::SessionRestored { snapshot }) => Some(snapshot.as_ref()),
        _ => None,
    });
    let restored = restored.expect("expected session restored state after /session resume");
    assert_eq!(restored.session_id, original_session_id);
    assert_eq!(restored.root_workflow_id, ROOT_WORKFLOW_ID);
    assert_eq!(restored.active_workflow_id, FEATURE_WORKFLOW_ID);
    assert_eq!(restored.recognized_scene_id.as_deref(), Some("feature"));
    assert_eq!(restored.selected_workflow_id.as_deref(), Some(FEATURE_WORKFLOW_ID));
    assert!(command_body(&resume_events).contains("Resumed session"));
    assert_eq!(
        session.project_detail_snapshot().unwrap().record.active_session_id.as_deref(),
        Some(original_session_id.as_str())
    );
}

#[test]
fn unbound_session_resume_picker_does_not_create_a_new_session() {
    let root = unique_session_test_root("session-command-resume-unbound-picker");
    write_document_fixture(&root);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);

    let first = AgentSession::new(AgentSessionConfig {
        client: Arc::new(IdleClient),
        system: "system".to_string(),
        cwd: root.clone(),
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog.clone(),
        workflow_catalog: loaded_catalog.workflow_catalog.clone(),
        prompt_catalog: loaded_catalog.prompt_catalog.clone(),
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();

    let original_events = run_command(&first, "/session new Original", 5825);
    let original_session_id = original_events
        .iter()
        .find_map(|envelope| match &envelope.message {
            RuntimeMessage::State(StateMessage::SessionRestored { snapshot }) => {
                Some(snapshot.session_id.clone())
            }
            _ => None,
        })
        .expect("expected original session id from /session new");

    let restored = AgentSession::new(AgentSessionConfig {
        client: Arc::new(IdleClient),
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();

    assert_eq!(restored.current_session_id(), None);
    assert_eq!(restored.project_detail_snapshot().unwrap().sessions.len(), 1);

    let picker_events = run_command(&restored, "/session resume", 5826);
    assert!(command_body(&picker_events).trim().is_empty());
    let resume_picker = match show_overlay_content(&picker_events, OverlayTarget::Picker) {
        Some(UiContent::OperatorPicker(request)) => request,
        other => panic!("expected resume picker overlay, got {other:?}"),
    };
    assert_eq!(resume_picker.primary_action.label, "Resume");
    assert_eq!(restored.current_session_id(), None);
    let project_snapshot = restored.project_detail_snapshot().unwrap();
    assert_eq!(project_snapshot.sessions.len(), 1);
    assert_eq!(
        project_snapshot.record.active_session_id.as_deref(),
        Some(original_session_id.as_str())
    );

    let resume_events = run_command(&restored, format!("/session resume {original_session_id}"), 5827);
    assert!(command_body(&resume_events).contains("Resumed session"));
    assert_eq!(
        restored.current_session_id().as_deref(),
        Some(original_session_id.as_str())
    );
}

#[test]
fn spawn_command_session_resume_reports_non_resume_ready_sessions() {
    let root = unique_session_test_root("session-command-resume-not-ready");
    write_document_fixture(&root);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: Arc::new(IdleClient),
        system: "system".to_string(),
        cwd: root.clone(),
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();

    let original_events = run_command(&session, "/session new Original", 5830);
    let original_session_id = original_events
        .iter()
        .find_map(|envelope| match &envelope.message {
            RuntimeMessage::State(StateMessage::SessionRestored { snapshot }) => {
                Some(snapshot.session_id.clone())
            }
            _ => None,
        })
        .expect("expected initial session id from /session new");

    let _new_events = run_command(&session, "/session new Scratch", 58301);
    let ledger_path = root
        .join(".omega-state")
        .join("sessions")
        .join(&original_session_id)
        .join("session.context.jsonl");
    std::fs::remove_file(&ledger_path).expect("remove ledger to force not resume-ready");

    let resume_events = run_command(&session, format!("/session resume {original_session_id}"), 5831);

    assert!(command_body(&resume_events).contains("Error: session exists but is not resume-ready"));
    assert!(resume_events.iter().any(|envelope| {
        matches!(
            &envelope.message,
            RuntimeMessage::Conversation(ConversationMessage::CompleteSection { state, .. })
                if *state == ResponseSectionState::Failed
        )
    }));
}

#[test]
fn new_starts_unbound_even_with_existing_saved_session() {
    let client: Arc<SequencedClient> = sequenced_client(vec![
        ChatResponse {
            id: "scene-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"recognized_scene_id\":\"chat\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "select-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"selected_workflow_id\":\"chat\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "chat-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("chat answer")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
    ]);
    let root = unique_session_test_root("startup-session-restore");
    write_document_fixture(&root);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let first = AgentSession::new(AgentSessionConfig {
        client: client.clone(),
        system: "system".to_string(),
        cwd: root.clone(),
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog.clone(),
        workflow_catalog: loaded_catalog.workflow_catalog.clone(),
        prompt_catalog: loaded_catalog.prompt_catalog.clone(),
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();

    let turn_recorder = RuntimeEnvelopeRecorder::new();
    first
        .spawn_turn_with_test_bridge("just chat".to_string(), 590, turn_recorder.runtime_bridge())
        .unwrap();
    let _turn_events = turn_recorder.wait_for_turn_finished_messages(590, Duration::from_secs(30));

    let original_id = first
        .current_session_id()
        .expect("first real turn should lazily bind a session");
    assert_eq!(first.project_detail_snapshot().unwrap().sessions.len(), 1);

    let restored = AgentSession::new(AgentSessionConfig {
        client: Arc::new(IdleClient),
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();

    assert_eq!(restored.current_session_id(), None);
    assert_eq!(restored.project_detail_snapshot().unwrap().sessions.len(), 1);
    assert_eq!(
        restored.project_detail_snapshot().unwrap().record.active_session_id.as_deref(),
        Some(original_id.as_str())
    );
    assert!(restored.startup_restore_snapshot().is_none());
}

#[cfg(feature = "document-backend")]
#[test]
fn spawn_command_document_create_list_and_archive_emit_complete_sections() {
    let _embedding_backend_guard = force_mock_document_embedding_backend();
    let root = unique_session_test_root("document-command-governance");
    write_document_fixture(&root);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: Arc::new(IdleClient),
        system: "system".to_string(),
        cwd: root.clone(),
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();

    let create_recorder = RuntimeEnvelopeRecorder::new();
    session
        .spawn_command_with_test_bridge(
            "/document create docs/specs/command-spec.md spec Command System".to_string(),
            601,
            create_recorder.runtime_bridge(),
        )
        .unwrap();
    let create_events = create_recorder.wait_for_turn_finished_messages(601, Duration::from_secs(30));
    assert!(create_events.iter().any(|envelope| {
        matches!(
            &envelope.message,
            RuntimeMessage::Conversation(ConversationMessage::CompleteSection { state, .. })
                if *state == ResponseSectionState::Complete
        )
    }));

    let list_recorder = RuntimeEnvelopeRecorder::new();
    session
        .spawn_command_with_test_bridge(
            "/document list spec active".to_string(),
            602,
            list_recorder.runtime_bridge(),
        )
        .unwrap();
    let list_events = list_recorder.wait_for_turn_finished_messages(602, Duration::from_secs(30));
    let list_body = list_events
        .iter()
        .filter_map(|envelope| match &envelope.message {
            RuntimeMessage::Conversation(ConversationMessage::AppendSection { delta, .. }) => {
                match delta {
                    ResponseSectionDelta::Text(text) => Some(text.as_str()),
                }
            }
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(list_body.contains("command-spec.md"));

    let archive_recorder = RuntimeEnvelopeRecorder::new();
    session
        .spawn_command_with_test_bridge(
            "/document archive docs/specs/command-spec.md superseded".to_string(),
            603,
            archive_recorder.runtime_bridge(),
        )
        .unwrap();
    let archive_events =
        archive_recorder.wait_for_turn_finished_messages(603, Duration::from_secs(30));
    assert!(archive_events.iter().any(|envelope| {
        matches!(
            &envelope.message,
            RuntimeMessage::Conversation(ConversationMessage::CompleteSection { state, .. })
                if *state == ResponseSectionState::Complete
        )
    }));
}

#[cfg(feature = "document-backend")]
#[test]
fn spawn_command_document_init_streams_scan_phases_and_samples() {
    let _embedding_backend_guard = force_mock_document_embedding_backend();
    let root = unique_session_test_root("document-command-init-rich");
    write_document_fixture(&root);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: Arc::new(IdleClient),
        system: "system".to_string(),
        cwd: root.clone(),
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();
    let recorder = RuntimeEnvelopeRecorder::new();

    session
        .spawn_command_with_test_bridge(
            "/document init".to_string(),
            701,
            recorder.runtime_bridge(),
        )
        .unwrap();

    let recorded = recorder.wait_for_turn_finished_messages(701, Duration::from_secs(30));
    let body = recorded
        .iter()
        .filter_map(|envelope| match &envelope.message {
            RuntimeMessage::Conversation(ConversationMessage::AppendSection { delta, .. }) => {
                match delta {
                    ResponseSectionDelta::Text(text) => Some(text.as_str()),
                }
            }
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(body.contains("Running /document init..."));
    assert!(body.contains("Phase: load storeignore rules and scan workspace"));
    assert!(body.contains("Phase: scan complete, preparing command summary"));
    assert!(body.contains("Indexed files:"));
    assert!(body.contains("Embedded files:"));
    assert!(body.contains("Manifest: .omega-state/store/files.jsonl"));
}

#[test]
fn spawn_turn_retries_invalid_required_structured_output() {
    let client: Arc<SequencedClient> = sequenced_client(vec![
        ChatResponse {
            id: "scene-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"recognized_scene_id\":\"feature\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "select-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"selected_workflow_id\":\"feature\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "analysis-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("explore")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "analysis-2".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_explore_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "plan-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_plan_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("execution complete")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "report-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("done")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
    ]);
    let client_dyn: DynLlmClient = client.clone();
    let root = std::env::temp_dir().join("omega-agent-session-structured-retry-test");
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::create_dir_all(&root);
    let skills_dir = root.join(".claude/skills/review");
    let _ = std::fs::create_dir_all(&skills_dir);
    let _ = std::fs::write(
        skills_dir.join("SKILL.md"),
        "---\nname: review\ndescription: Review code\n---\nFind regressions.",
    );
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: client_dyn,
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();
    let (tx, rx) = mpsc::channel();

    session
        .spawn_turn_ui_compat("hello".to_string(), 21, tx)
        .unwrap();

    let mut warnings = Vec::new();
    let mut diagnostics = Vec::new();
    loop {
        match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
            RuntimeUiEnvelope::Message { turn_id, message }
                if turn_id == 21
                    && matches!(message.source, UiSource::System)
                    && message.kind == UiMessageKind::Warning =>
            {
                warnings.push(message.content.as_text().to_string());
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::UpsertStepDiagnostics {
                        diagnostics: update,
                    },
            } => {
                assert_eq!(turn_id, 21);
                diagnostics.push(*update);
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::SetStatusSlot {
                        slot: StatusSlot::Agent,
                        value: StatusValue::Label(label),
                    },
            } => {
                assert_eq!(turn_id, 21);
                assert_eq!(label, "Idle");
                break;
            }
            _ => {}
        }
    }

    let systems = client.recorded_systems();

    assert!(warnings.iter().any(|warning| {
        warning.contains("Step 'explore' produced invalid structured output")
            && warning.contains("repair pass")
    }));
    assert!(diagnostics.iter().any(|diagnostics| {
        diagnostics.step_id == EXPLORE_STEP_ID
            && diagnostics.output.status == StepOutputStatus::Invalid
    }));
    assert!(systems.iter().any(|system| {
        system.as_ref().is_some_and(|system| {
            system.contains("<output_repair step_id=\"explore\">")
                && system.contains("Visible tools: none")
                && system.contains("error_kind: extract_failed")
        })
    }));
}

#[test]
fn spawn_turn_syncs_execute_output_back_into_todo_state_for_report() {
    let client: Arc<SequencedClient> = sequenced_client(vec![
        ChatResponse {
            id: "scene-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"recognized_scene_id\":\"feature\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "select-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"selected_workflow_id\":\"feature\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "analysis-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_explore_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "plan-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_plan_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_execute_partial_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-2".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_execute_complete_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "report-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("done")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
    ]);
    let client_dyn: DynLlmClient = client.clone();
    let root = std::env::temp_dir().join("omega-agent-session-execute-todo-sync-test");
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::create_dir_all(&root);
    let skills_dir = root.join(".claude/skills/review");
    let _ = std::fs::create_dir_all(&skills_dir);
    let _ = std::fs::write(
        skills_dir.join("SKILL.md"),
        "---\nname: review\ndescription: Review code\n---\nFind regressions.",
    );
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: client_dyn,
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();
    let (tx, rx) = mpsc::channel();

    session
        .spawn_turn_ui_compat("hello".to_string(), 41, tx)
        .unwrap();

    let mut todo_panels = Vec::new();
    let mut diagnostics = Vec::new();
    loop {
        match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::ReplacePanel {
                        target: UiTarget::Todo,
                        content,
                    },
            } => {
                assert_eq!(turn_id, 41);
                todo_panels.push(content.as_text().to_string());
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::UpsertStepDiagnostics {
                        diagnostics: update,
                    },
            } => {
                assert_eq!(turn_id, 41);
                diagnostics.push(*update);
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::SetStatusSlot {
                        slot: StatusSlot::Agent,
                        value: StatusValue::Label(label),
                    },
            } => {
                assert_eq!(turn_id, 41);
                assert_eq!(label, "Idle");
                break;
            }
            _ => {}
        }
    }

    assert!(todo_panels
        .iter()
        .any(|panel| { panel.contains("[x] #task-1") && panel.contains("[>] #task-2") }));
    assert!(todo_panels
        .iter()
        .any(|panel| { panel.contains("[x] #task-1") && panel.contains("[x] #task-2") }));
    assert!(diagnostics.iter().any(|diagnostics| {
        diagnostics.step_id == PLAN_STEP_ID
            && diagnostics.output.status == StepOutputStatus::Valid
            && diagnostics.session_writes.iter().any(|write| {
                write.path == "step_outputs.plan"
                    && write.kind == StepContextWriteKind::Added
                    && write.before_preview.is_none()
                    && write.after_preview.is_some()
            })
    }));
    assert!(diagnostics.iter().any(|diagnostics| {
        diagnostics.step_id == PLAN_STEP_ID
            && diagnostics.session_writes.iter().any(|write| {
                write.path == "todo.rendered"
                    && write.kind == StepContextWriteKind::Added
                    && write.before_preview.is_none()
                    && write
                        .after_preview
                        .as_deref()
                        .is_some_and(|preview| preview.contains("#task-1"))
            })
    }));
    assert!(diagnostics.iter().any(|diagnostics| {
        diagnostics.step_id == EXECUTE_STEP_ID
            && diagnostics.session_writes.iter().any(|write| {
                write.path == "todo.rendered"
                    && write.kind == StepContextWriteKind::Updated
                    && write
                        .before_preview
                        .as_deref()
                        .is_some_and(|preview| preview.contains("[>] #task-1"))
                    && write
                        .after_preview
                        .as_deref()
                        .is_some_and(|preview| preview.contains("[x] #task-1"))
            })
    }));
    assert!(diagnostics.iter().any(|diagnostics| {
        diagnostics.step_id == EXECUTE_STEP_ID
            && diagnostics.session_writes.iter().any(|write| {
                write.path == "todo.rendered"
                    && write.kind == StepContextWriteKind::Updated
                    && write
                        .before_preview
                        .as_deref()
                        .is_some_and(|preview| preview.contains("[>] #task-2"))
                    && write
                        .after_preview
                        .as_deref()
                        .is_some_and(|preview| preview.contains("[x] #task-2"))
            })
    }));
    assert!(diagnostics.iter().any(|diagnostics| {
        diagnostics.step_id == REPORT_STEP_ID && diagnostics.input.todo_state_preview.is_some()
    }));

    let systems = client.recorded_systems();
    assert!(systems
        .iter()
        .filter_map(|system| system.as_deref())
        .any(|system| system.contains("<todo_state step_id=\"report\">")));
    assert!(systems
        .iter()
        .filter_map(|system| system.as_deref())
        .any(|system| system.contains("(2/2 completed)")));
    assert!(
        systems
            .iter()
            .filter_map(|system| system.as_deref())
            .filter(|system| system.contains("<todo_state step_id=\"execute\">"))
            .count()
            >= 2
    );
}

#[test]
fn itemized_execute_auto_repairs_future_todo_completion() {
    let client: Arc<SequencedClient> = sequenced_client(vec![
        ChatResponse {
            id: "scene-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"recognized_scene_id\":\"feature\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "select-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"selected_workflow_id\":\"feature\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "analysis-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_explore_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "plan-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_plan_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_execute_future_completion_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-2".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_execute_partial_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-3".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_execute_complete_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "report-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("done")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
    ]);
    let client_dyn: DynLlmClient = client.clone();
    let root = unique_session_test_root("execute-future-completion-auto-repair");
    write_review_skill(&root);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: client_dyn,
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();
    let (tx, rx) = mpsc::channel();

    session
        .spawn_turn_ui_compat("hello".to_string(), 141, tx)
        .unwrap();

    let mut warnings = Vec::new();
    loop {
        match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
            RuntimeUiEnvelope::Message { turn_id, message }
                if turn_id == 141
                    && matches!(message.source, UiSource::System)
                    && message.kind == UiMessageKind::Warning =>
            {
                warnings.push(message.content.as_text().to_string());
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::SetStatusSlot {
                        slot: StatusSlot::Agent,
                        value: StatusValue::Label(label),
                    },
            } => {
                assert_eq!(turn_id, 141);
                assert_eq!(label, "Idle");
                break;
            }
            _ => {}
        }
    }

    // Auto-repair strips future items silently — no invalid-output warnings.
    assert!(
        !warnings
            .iter()
            .any(|w| w.contains("cannot complete future todo item")),
        "auto-repair should prevent future-item validation warnings"
    );
}

#[test]
fn research_itemized_execute_auto_repairs_future_todo_completion() {
    let client: Arc<SequencedClient> = sequenced_client(vec![
        ChatResponse {
            id: "scene-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"recognized_scene_id\":\"research\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "select-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(
                "{\"selected_workflow_id\":\"research\"}",
            )],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "analysis-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_explore_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "plan-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_plan_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_execute_complete_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-2".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_execute_partial_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-3".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_execute_complete_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "report-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("done")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
    ]);
    let client_dyn: DynLlmClient = client.clone();
    let root = unique_session_test_root("research-execute-future-completion-auto-repair");
    write_review_skill(&root);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: client_dyn,
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();
    let (tx, rx) = mpsc::channel();

    session
        .spawn_turn_ui_compat("hello".to_string(), 142, tx)
        .unwrap();

    let mut warnings = Vec::new();
    loop {
        match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
            RuntimeUiEnvelope::Message { turn_id, message }
                if turn_id == 142
                    && matches!(message.source, UiSource::System)
                    && message.kind == UiMessageKind::Warning =>
            {
                warnings.push(message.content.as_text().to_string());
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::SetStatusSlot {
                        slot: StatusSlot::Agent,
                        value: StatusValue::Label(label),
                    },
            } => {
                assert_eq!(turn_id, 142);
                assert_eq!(label, "Idle");
                break;
            }
            _ => {}
        }
    }

    // Auto-repair strips future items silently — no invalid-output warnings.
    assert!(
        !warnings
            .iter()
            .any(|w| w.contains("cannot complete future todo item")),
        "auto-repair should prevent future-item validation warnings"
    );
}

#[test]
fn research_itemized_execute_auto_repairs_multiple_future_completions() {
    let client: Arc<SequencedClient> = sequenced_client(vec![
        ChatResponse {
            id: "scene-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"recognized_scene_id\":\"research\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "select-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(
                "{\"selected_workflow_id\":\"research\"}",
            )],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "analysis-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_explore_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "plan-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_plan_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        // execute-1 for task-1: completes both tasks → auto-repair keeps only task-1
        ChatResponse {
            id: "execute-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_execute_complete_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        // execute-2 for task-2: completes both tasks → task-1 already done, task-2 current → passes
        ChatResponse {
            id: "execute-2".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_execute_complete_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "report-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("done")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
    ]);
    let client_dyn: DynLlmClient = client.clone();
    let root = unique_session_test_root("research-execute-auto-repair-multiple");
    write_review_skill(&root);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: client_dyn,
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();
    let (tx, rx) = mpsc::channel();

    session
        .spawn_turn_ui_compat("hello".to_string(), 143, tx)
        .unwrap();

    let mut warnings = Vec::new();
    let mut saw_result = false;
    loop {
        match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
            RuntimeUiEnvelope::Message { turn_id, message }
                if turn_id == 143
                    && matches!(message.source, UiSource::System)
                    && message.kind == UiMessageKind::Warning =>
            {
                warnings.push(message.content.as_text().to_string());
            }
            RuntimeUiEnvelope::Message { turn_id, message }
                if turn_id == 143
                    && matches!(message.source, UiSource::Assistant)
                    && message.kind == UiMessageKind::Result =>
            {
                assert_eq!(message.content.as_text(), "done");
                saw_result = true;
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::SetStatusSlot {
                        slot: StatusSlot::Agent,
                        value: StatusValue::Label(label),
                    },
            } => {
                assert_eq!(turn_id, 143);
                assert_eq!(label, "Idle");
                break;
            }
            _ => {}
        }
    }

    // Auto-repair handles future-item completions without retry warnings.
    assert_eq!(
        warnings
            .iter()
            .filter(|w| w.contains("invalid structured output"))
            .count(),
        0
    );
    let session_context = session.session_context.lock().unwrap();
    let first_execute_summary = session_context
        .step_summaries
        .iter()
        .find(|summary| summary.step_id == EXECUTE_STEP_ID)
        .expect("first execute summary should be stored");
    let summary_value: serde_json::Value = serde_json::from_str(&first_execute_summary.summary)
        .expect("summary should stay canonical JSON");
    assert_eq!(
        summary_value["completed_tasks"],
        serde_json::json!(["task-1"])
    );
    assert_eq!(summary_value["open_tasks"], serde_json::json!(["task-2"]));
    assert!(saw_result);
}

#[test]
fn research_itemized_execute_retries_when_current_item_response_omits_json() {
    let client: Arc<SequencedClient> = sequenced_client(vec![
        ChatResponse {
            id: "scene-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"recognized_scene_id\":\"research\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "select-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(
                "{\"selected_workflow_id\":\"research\"}",
            )],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "analysis-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_explore_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "plan-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_plan_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(
                "Inspected the relevant code and validated the first research task.",
            )],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-2".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_execute_partial_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-3".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_execute_complete_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "report-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("done")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
    ]);
    let client_dyn: DynLlmClient = client.clone();
    let root = unique_session_test_root("research-execute-missing-json-retry");
    write_review_skill(&root);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: client_dyn,
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();
    let (tx, rx) = mpsc::channel();

    session
        .spawn_turn_ui_compat("hello".to_string(), 144, tx)
        .unwrap();

    let mut warnings = Vec::new();
    let mut diagnostics = Vec::new();
    let mut saw_result = false;
    loop {
        match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
            RuntimeUiEnvelope::Message { turn_id, message }
                if turn_id == 144
                    && matches!(message.source, UiSource::System)
                    && message.kind == UiMessageKind::Warning =>
            {
                warnings.push(message.content.as_text().to_string());
            }
            RuntimeUiEnvelope::Message { turn_id, message }
                if turn_id == 144
                    && matches!(message.source, UiSource::Assistant)
                    && message.kind == UiMessageKind::Result =>
            {
                assert_eq!(message.content.as_text(), "done");
                saw_result = true;
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::UpsertStepDiagnostics {
                        diagnostics: update,
                    },
            } => {
                assert_eq!(turn_id, 144);
                diagnostics.push(*update);
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::SetStatusSlot {
                        slot: StatusSlot::Agent,
                        value: StatusValue::Label(label),
                    },
            } => {
                assert_eq!(turn_id, 144);
                assert_eq!(label, "Idle");
                break;
            }
            _ => {}
        }
    }

    assert!(!warnings
        .iter()
        .any(|warning| warning.contains("advance denied")));
    assert!(diagnostics.iter().any(|diagnostics| {
        diagnostics.step_id == EXECUTE_STEP_ID
            && diagnostics.output.status == StepOutputStatus::Invalid
            && diagnostics.output.validation_error.is_some()
    }));
    assert!(diagnostics.iter().any(|diagnostics| {
        diagnostics.step_id == EXECUTE_STEP_ID
            && diagnostics.output.status == StepOutputStatus::Valid
            && diagnostics.output.attempt_kind != StepOutputAttemptKind::Primary
    }));
    assert!(saw_result);
}

#[test]
fn research_itemized_execute_retries_when_current_item_output_repeats_previous_completion() {
    let client: Arc<SequencedClient> = sequenced_client(vec![
        ChatResponse {
            id: "scene-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"recognized_scene_id\":\"research\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "select-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(
                "{\"selected_workflow_id\":\"research\"}",
            )],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "analysis-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_explore_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "plan-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_plan_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_execute_partial_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-2".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(
                r#"{"completed_tasks":["task-1"],"open_tasks":["task-2"],"validation_results":[{"target":"rg --files crates","status":"passed"}],"changed_paths":[]}"#,
            )],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-3".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_execute_complete_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "report-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("done")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
    ]);
    let client_dyn: DynLlmClient = client.clone();
    let root = unique_session_test_root("research-itemized-execute-stale-completion");
    write_review_skill(&root);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: client_dyn,
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();
    let (tx, rx) = mpsc::channel();

    session
        .spawn_turn_ui_compat("hello".to_string(), 196, tx)
        .unwrap();

    let mut warnings = Vec::new();
    let mut diagnostics = Vec::new();
    loop {
        match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
            RuntimeUiEnvelope::Message { turn_id, message }
                if turn_id == 196
                    && matches!(message.source, UiSource::System)
                    && message.kind == UiMessageKind::Warning =>
            {
                warnings.push(message.content.as_text().to_string());
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::UpsertStepDiagnostics {
                        diagnostics: update,
                    },
            } => {
                assert_eq!(turn_id, 196);
                diagnostics.push(*update);
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::SetStatusSlot {
                        slot: StatusSlot::Agent,
                        value: StatusValue::Label(label),
                    },
            } => {
                assert_eq!(turn_id, 196);
                assert_eq!(label, "Idle");
                break;
            }
            _ => {}
        }
    }

    assert!(warnings.iter().any(|warning| {
        warning.contains("Step 'execute' produced invalid structured output")
            && warning.contains("current todo item")
    }));
    assert!(diagnostics.iter().any(|diagnostics| {
        diagnostics.step_id == EXECUTE_STEP_ID
            && diagnostics.output.status == StepOutputStatus::Invalid
            && diagnostics.output.validation_error.as_deref().is_some_and(|error| {
                error.contains("current todo item") && error.contains("task-2")
            })
    }));
    assert!(diagnostics.iter().any(|diagnostics| {
        diagnostics.step_id == EXECUTE_STEP_ID
            && diagnostics.output.status == StepOutputStatus::Valid
            && diagnostics
                .execute_progress
                .as_ref()
                .is_some_and(|progress| {
                    progress.current_item_id.as_deref() == Some("task-2")
                        && progress.completion_source.as_deref() == Some("structured_output")
                        && progress.todo_completed == 2
                        && progress.todo_open == 0
                })
    }));
    assert!(!warnings
        .iter()
        .any(|warning| warning.contains("advance denied")));
}

#[test]
fn research_itemized_execute_retries_when_current_item_completes_only_future_todo() {
    let client: Arc<SequencedClient> = sequenced_client(vec![
        ChatResponse {
            id: "scene-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"recognized_scene_id\":\"research\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "select-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(
                "{\"selected_workflow_id\":\"research\"}",
            )],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "analysis-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_explore_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "plan-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_plan_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_execute_future_only_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-2".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_execute_partial_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-3".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_execute_complete_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "report-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("done")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
    ]);
    let client_dyn: DynLlmClient = client.clone();
    let root = unique_session_test_root("research-execute-future-only-retry");
    write_review_skill(&root);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: client_dyn,
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();
    let (tx, rx) = mpsc::channel();

    session
        .spawn_turn_ui_compat("hello".to_string(), 198, tx)
        .unwrap();

    let mut warnings = Vec::new();
    let mut diagnostics = Vec::new();
    let mut saw_result = false;
    loop {
        match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
            RuntimeUiEnvelope::Message { turn_id, message }
                if turn_id == 198
                    && matches!(message.source, UiSource::System)
                    && message.kind == UiMessageKind::Warning =>
            {
                warnings.push(message.content.as_text().to_string());
            }
            RuntimeUiEnvelope::Message { turn_id, message }
                if turn_id == 198
                    && matches!(message.source, UiSource::Assistant)
                    && message.kind == UiMessageKind::Result =>
            {
                assert_eq!(message.content.as_text(), "done");
                saw_result = true;
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::UpsertStepDiagnostics {
                        diagnostics: update,
                    },
            } => {
                assert_eq!(turn_id, 198);
                diagnostics.push(*update);
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::SetStatusSlot {
                        slot: StatusSlot::Agent,
                        value: StatusValue::Label(label),
                    },
            } => {
                assert_eq!(turn_id, 198);
                assert_eq!(label, "Idle");
                break;
            }
            _ => {}
        }
    }

    assert!(warnings.iter().any(|warning| {
        warning.contains("Step 'execute' produced invalid structured output")
            && warning.contains("cannot complete future todo item")
    }));
    assert!(diagnostics.iter().any(|diagnostics| {
        diagnostics.step_id == EXECUTE_STEP_ID
            && diagnostics.output.status == StepOutputStatus::Invalid
            && diagnostics.output.validation_error.as_deref().is_some_and(|error| {
                error.contains("cannot complete future todo item 'task-2'")
                    && error.contains("current item is 'task-1'")
            })
    }));
    assert!(diagnostics.iter().any(|diagnostics| {
        diagnostics.step_id == EXECUTE_STEP_ID
            && diagnostics.output.status == StepOutputStatus::Valid
            && diagnostics.output.attempt_kind != StepOutputAttemptKind::Primary
    }));
    assert!(!warnings
        .iter()
        .any(|warning| warning.contains("advance denied")));
    assert!(saw_result);
}

#[test]
fn research_itemized_execute_retries_when_current_item_is_missing_from_open_tasks() {
    let client: Arc<SequencedClient> = sequenced_client(vec![
        ChatResponse {
            id: "scene-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"recognized_scene_id\":\"research\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "select-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(
                "{\"selected_workflow_id\":\"research\"}",
            )],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "analysis-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_explore_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "plan-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_plan_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(
                r#"{"completed_tasks":[],"open_tasks":["task-2"],"validation_results":[{"target":"rg --files crates","status":"passed"}],"changed_paths":[]}"#,
            )],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-2".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_execute_partial_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-3".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_execute_complete_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "report-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("done")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
    ]);
    let client_dyn: DynLlmClient = client.clone();
    let root = unique_session_test_root("research-execute-missing-current-open");
    write_review_skill(&root);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: client_dyn,
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();
    let (tx, rx) = mpsc::channel();

    session
        .spawn_turn_ui_compat("hello".to_string(), 199, tx)
        .unwrap();

    let mut warnings = Vec::new();
    let mut diagnostics = Vec::new();
    let mut saw_result = false;
    loop {
        match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
            RuntimeUiEnvelope::Message { turn_id, message }
                if turn_id == 199
                    && matches!(message.source, UiSource::System)
                    && message.kind == UiMessageKind::Warning =>
            {
                warnings.push(message.content.as_text().to_string());
            }
            RuntimeUiEnvelope::Message { turn_id, message }
                if turn_id == 199
                    && matches!(message.source, UiSource::Assistant)
                    && message.kind == UiMessageKind::Result =>
            {
                assert_eq!(message.content.as_text(), "done");
                saw_result = true;
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::UpsertStepDiagnostics {
                        diagnostics: update,
                    },
            } => {
                assert_eq!(turn_id, 199);
                diagnostics.push(*update);
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::SetStatusSlot {
                        slot: StatusSlot::Agent,
                        value: StatusValue::Label(label),
                    },
            } => {
                assert_eq!(turn_id, 199);
                assert_eq!(label, "Idle");
                break;
            }
            _ => {}
        }
    }

    assert!(warnings.iter().any(|warning| {
        warning.contains("Step 'execute' produced invalid structured output")
            && warning.contains("must keep current todo item 'task-1' in open_tasks")
    }));
    assert!(diagnostics.iter().any(|diagnostics| {
        diagnostics.step_id == EXECUTE_STEP_ID
            && diagnostics.output.status == StepOutputStatus::Invalid
            && diagnostics.output.validation_error.as_deref().is_some_and(|error| {
                error.contains("must keep current todo item 'task-1' in open_tasks")
            })
    }));
    assert!(!warnings
        .iter()
        .any(|warning| warning.contains("Only one task can be in_progress at a time")));
    assert!(saw_result);
}

#[test]
fn deep_research_execute_accepts_todo_display_text_aliases() {
    let client: Arc<SequencedClient> = sequenced_client(vec![
        ChatResponse {
            id: "scene-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"recognized_scene_id\":\"deep-research\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "select-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(
                "{\"selected_workflow_id\":\"deep-research\"}",
            )],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "analysis-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_explore_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "plan-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_plan_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_execute_complete_with_display_text_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-2".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_execute_complete_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "report-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("done")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
    ]);
    let client_dyn: DynLlmClient = client.clone();
    let root = unique_session_test_root("deep-research-execute-display-text-aliases");
    write_review_skill(&root);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: client_dyn,
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();
    let (tx, rx) = mpsc::channel();

    session
        .spawn_turn_ui_compat("请你对项目做一次深入分析".to_string(), 220, tx)
        .unwrap();

    let mut warnings = Vec::new();
    let mut todo_panels = Vec::new();
    loop {
        match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
            RuntimeUiEnvelope::Message { turn_id, message }
                if turn_id == 220
                    && matches!(message.source, UiSource::System)
                    && message.kind == UiMessageKind::Warning =>
            {
                warnings.push(message.content.as_text().to_string());
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect: RuntimeUiEffect::ReplacePanel { target: UiTarget::Todo, content },
            } if turn_id == 220 => {
                todo_panels.push(content.as_text().to_string());
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::SetStatusSlot {
                        slot: StatusSlot::Agent,
                        value: StatusValue::Label(label),
                    },
            } => {
                assert_eq!(turn_id, 220);
                assert_eq!(label, "Idle");
                break;
            }
            _ => {}
        }
    }

    assert!(!warnings.iter().any(|warning| warning.contains("unknown todo item")));
    assert!(todo_panels.iter().any(|panel| panel.contains("[x] #task-1")));
    assert!(todo_panels.iter().any(|panel| panel.contains("[>] #task-2")));
    assert_eq!(client.remaining_steps(), 0);
}

#[test]
fn research_report_retries_when_final_answer_is_raw_json() {
    let client: Arc<SequencedClient> = sequenced_client(vec![
        ChatResponse {
            id: "scene-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"recognized_scene_id\":\"research\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "select-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(
                "{\"selected_workflow_id\":\"research\"}",
            )],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "analysis-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_explore_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "plan-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_plan_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_execute_partial_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-2".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_execute_complete_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "report-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(
                r#"{"completed_tasks":["analysis-1","analysis-2","analysis-3","analysis-4","analysis-5","analysis-6","analysis-7"],"open_tasks":[],"validation_results":[{"target":"架构设计分析","status":"verified"}],"changed_paths":[]}"#,
            )],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "report-2".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(
                "项目分析报告：架构分层清晰，context/tool/test 基础设施扎实，但 runner.rs 仍然过重，跨 crate 学习成本偏高。",
            )],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
    ]);
    let client_dyn: DynLlmClient = client.clone();
    let root = unique_session_test_root("research-report-json-retry");
    write_review_skill(&root);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: client_dyn,
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();
    let (tx, rx) = mpsc::channel();

    session
        .spawn_turn_ui_compat(
            "请你对这个仓库做一次深度复杂的综合分析，并给出结构化的项目优劣报告"
                .to_string(),
            197,
            tx,
        )
        .unwrap();

    let mut warnings = Vec::new();
    let mut diagnostics = Vec::new();
    let mut assistant_results = Vec::new();
    loop {
        match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
            RuntimeUiEnvelope::Message { turn_id, message }
                if turn_id == 197
                    && matches!(message.source, UiSource::System)
                    && message.kind == UiMessageKind::Warning =>
            {
                warnings.push(message.content.as_text().to_string());
            }
            RuntimeUiEnvelope::Message { turn_id, message }
                if turn_id == 197
                    && matches!(message.source, UiSource::Assistant)
                    && message.kind == UiMessageKind::Result =>
            {
                assistant_results.push(message.content.as_text().to_string());
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::UpsertStepDiagnostics {
                        diagnostics: update,
                    },
            } => {
                assert_eq!(turn_id, 197);
                diagnostics.push(*update);
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::SetStatusSlot {
                        slot: StatusSlot::Agent,
                        value: StatusValue::Label(label),
                    },
            } => {
                assert_eq!(turn_id, 197);
                assert_eq!(label, "Idle");
                break;
            }
            _ => {}
        }
    }

    assert!(warnings.iter().any(|warning| {
        warning.contains("Step 'report' produced invalid structured output")
            && warning.contains("user-facing prose")
    }));
    assert!(diagnostics.iter().any(|diagnostics| {
        diagnostics.step_id == REPORT_STEP_ID
            && diagnostics.output.status == StepOutputStatus::Invalid
            && diagnostics.output.validation_error.as_deref().is_some_and(|error| {
                error.contains("user-facing prose")
            })
    }));
    if let Some(last_result) = assistant_results.last() {
        assert_eq!(
            last_result,
            "项目分析报告：架构分层清晰，context/tool/test 基础设施扎实，但 runner.rs 仍然过重，跨 crate 学习成本偏高。"
        );
    }
    assert_eq!(client.remaining_steps(), 0);
}

#[test]
fn spawn_turn_syncs_research_execute_output_back_into_todo_state_for_report() {
    let client: Arc<SequencedClient> = sequenced_client(vec![
        ChatResponse {
            id: "scene-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"recognized_scene_id\":\"research\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "select-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(
                "{\"selected_workflow_id\":\"research\"}",
            )],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "analysis-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_explore_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "plan-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_plan_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_execute_partial_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-2".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_execute_complete_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "report-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("done")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
    ]);
    let client_dyn: DynLlmClient = client.clone();
    let root = std::env::temp_dir().join("omega-agent-session-research-execute-todo-sync-test");
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::create_dir_all(&root);
    let skills_dir = root.join(".claude/skills/review");
    let _ = std::fs::create_dir_all(&skills_dir);
    let _ = std::fs::write(
        skills_dir.join("SKILL.md"),
        "---\nname: review\ndescription: Review code\n---\nFind regressions.",
    );
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: client_dyn,
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();
    let (tx, rx) = mpsc::channel();

    session
        .spawn_turn_ui_compat(
            "请你对这个项目做一次深入、系统、全局的优劣分析".to_string(),
            43,
            tx,
        )
        .unwrap();

    let mut todo_panels = Vec::new();
    let mut diagnostics = Vec::new();
    loop {
        match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::ReplacePanel {
                        target: UiTarget::Todo,
                        content,
                    },
            } => {
                assert_eq!(turn_id, 43);
                todo_panels.push(content.as_text().to_string());
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::UpsertStepDiagnostics {
                        diagnostics: update,
                    },
            } => {
                assert_eq!(turn_id, 43);
                diagnostics.push(*update);
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::SetStatusSlot {
                        slot: StatusSlot::Agent,
                        value: StatusValue::Label(label),
                    },
            } => {
                assert_eq!(turn_id, 43);
                assert_eq!(label, "Idle");
                break;
            }
            _ => {}
        }
    }

    assert!(todo_panels
        .iter()
        .any(|panel| panel.contains("#task-1") && panel.contains("#task-2")));
    assert!(todo_panels
        .iter()
        .any(|panel| panel.contains("[ ] #task-2")));
    assert!(todo_panels
        .iter()
        .any(|panel| panel.contains("[x] #task-1")));
    assert!(todo_panels
        .iter()
        .any(|panel| panel.contains("[>] #task-2")));
    assert!(todo_panels
        .iter()
        .any(|panel| panel.contains("[x] #task-2")));
    assert!(diagnostics.iter().any(|diagnostics| {
        diagnostics.step_id == PLAN_STEP_ID
            && diagnostics.output.status == StepOutputStatus::Valid
            && diagnostics.session_writes.iter().any(|write| {
                write.path == "todo.rendered"
                    && write.kind == StepContextWriteKind::Added
                    && write.before_preview.is_none()
                    && write
                        .after_preview
                        .as_deref()
                        .is_some_and(|preview| preview.contains("#task-1"))
            })
    }));
    assert!(diagnostics.iter().any(|diagnostics| {
        diagnostics.step_id == EXECUTE_STEP_ID
            && diagnostics.output.status == StepOutputStatus::Valid
            && diagnostics.session_writes.iter().any(|write| {
                write.path == "todo.rendered"
                    && write.kind == StepContextWriteKind::Updated
                    && write
                        .before_preview
                        .as_deref()
                        .is_some_and(|preview| preview.contains("[>] #task-1"))
                    && write.after_preview.as_deref().is_some_and(|preview| {
                        preview.contains("[x] #task-1") && preview.contains("[>] #task-2")
                    })
            })
    }));
    assert!(diagnostics.iter().any(|diagnostics| {
        diagnostics.step_id == EXECUTE_STEP_ID
            && diagnostics.output.status == StepOutputStatus::Valid
            && diagnostics.session_writes.iter().any(|write| {
                write.path == "todo.rendered"
                    && write.kind == StepContextWriteKind::Updated
                    && write
                        .before_preview
                        .as_deref()
                        .is_some_and(|preview| preview.contains("[>] #task-2"))
                    && write
                        .after_preview
                        .as_deref()
                        .is_some_and(|preview| preview.contains("[x] #task-2"))
            })
    }));
    assert!(diagnostics.iter().any(|diagnostics| {
        diagnostics.step_id == REPORT_STEP_ID && diagnostics.input.todo_state_preview.is_some()
    }));

    let systems = client.recorded_systems();
    assert!(systems
        .iter()
        .filter_map(|system| system.as_deref())
        .any(|system| system.contains("<todo_state step_id=\"report\">")));
    assert!(systems
        .iter()
        .filter_map(|system| system.as_deref())
        .any(|system| system.contains("(2/2 completed)")));
    assert!(
        systems
            .iter()
            .filter_map(|system| system.as_deref())
            .filter(|system| system.contains("<todo_state step_id=\"execute\">"))
            .count()
            >= 2
    );
}

#[test]
fn spawn_turn_repeats_research_execute_without_initial_todo_diff() {
    let client: Arc<SequencedClient> = sequenced_client(vec![
        ChatResponse {
            id: "scene-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"recognized_scene_id\":\"research\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "select-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(
                "{\"selected_workflow_id\":\"research\"}",
            )],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "explore-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_explore_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "plan-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_plan_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_execute_no_progress_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-2".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_execute_partial_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-3".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_execute_complete_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "report-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("done")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
    ]);
    let client_dyn: DynLlmClient = client.clone();
    let root = std::env::temp_dir().join("omega-agent-session-research-execute-repeat-test");
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::create_dir_all(&root);
    let skills_dir = root.join(".claude/skills/review");
    let _ = std::fs::create_dir_all(&skills_dir);
    let _ = std::fs::write(
        skills_dir.join("SKILL.md"),
        "---\nname: review\ndescription: Review code\n---\nFind regressions.",
    );
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: client_dyn,
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();
    let (tx, rx) = mpsc::channel();

    session
        .spawn_turn_ui_compat(
            "请你对这个项目做一次深入、系统、全局的优劣分析".to_string(),
            44,
            tx,
        )
        .unwrap();

    let mut todo_panels = Vec::new();
    loop {
        match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::ReplacePanel {
                        target: UiTarget::Todo,
                        content,
                    },
            } => {
                assert_eq!(turn_id, 44);
                todo_panels.push(content.as_text().to_string());
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::SetStatusSlot {
                        slot: StatusSlot::Agent,
                        value: StatusValue::Label(label),
                    },
            } => {
                assert_eq!(turn_id, 44);
                assert_eq!(label, "Idle");
                break;
            }
            _ => {}
        }
    }

    assert!(todo_panels
        .iter()
        .any(|panel| panel.contains("#task-1") && panel.contains("#task-2")));
    assert!(todo_panels
        .iter()
        .any(|panel| panel.contains("[x] #task-1")));
    assert!(todo_panels
        .iter()
        .any(|panel| panel.contains("[x] #task-2")));

    let systems = client.recorded_systems();
    assert!(
        systems
            .iter()
            .filter_map(|system| system.as_deref())
            .filter(|system| system.contains("<todo_state step_id=\"execute\">"))
            .count()
            >= 2
    );
}

#[test]
fn spawn_turn_emits_execute_progress_diagnostics_for_item_loop() {
    let client: Arc<SequencedClient> = sequenced_client(vec![
        ChatResponse {
            id: "scene-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"recognized_scene_id\":\"research\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "select-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(
                "{\"selected_workflow_id\":\"research\"}",
            )],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "explore-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_explore_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "plan-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_plan_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_execute_no_progress_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-2".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_execute_partial_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-3".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_execute_complete_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "report-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("done")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
    ]);
    let client_dyn: DynLlmClient = client;
    let root = unique_session_test_root("execute-progress-diagnostics");
    write_review_skill(&root);

    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: client_dyn,
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();
    let (tx, rx) = mpsc::channel();

    session
        .spawn_turn_ui_compat("hello".to_string(), 91, tx)
        .unwrap();

    let mut diagnostics = Vec::new();
    loop {
        match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::UpsertStepDiagnostics {
                        diagnostics: update,
                    },
            } => {
                assert_eq!(turn_id, 91);
                diagnostics.push(*update);
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::SetStatusSlot {
                        slot: StatusSlot::Agent,
                        value: StatusValue::Label(label),
                    },
            } => {
                assert_eq!(turn_id, 91);
                assert_eq!(label, "Idle");
                break;
            }
            _ => {}
        }
    }

    assert!(diagnostics.iter().any(|diagnostics| {
        diagnostics.step_id == EXECUTE_STEP_ID
            && diagnostics
                .execute_progress
                .as_ref()
                .is_some_and(|progress| {
                    progress.current_item_id.as_deref() == Some("task-1")
                        && progress.repeat_count == 1
                        && progress.todo_open == 2
                })
    }));
    assert!(diagnostics.iter().any(|diagnostics| {
        diagnostics.step_id == EXECUTE_STEP_ID
            && diagnostics
                .execute_progress
                .as_ref()
                .is_some_and(|progress| {
                    progress.current_item_id.as_deref() == Some("task-2")
                        && progress.completion_source.as_deref() == Some("structured_output")
                        && progress.todo_completed == 2
                        && progress.todo_open == 0
                })
    }));
}

#[test]
fn spawn_turn_fails_when_before_advance_denial_exhausts_repeat_budget() {
    let client: Arc<SequencedClient> = sequenced_client(vec![
        ChatResponse {
            id: "scene-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"recognized_scene_id\":\"feature\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "select-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"selected_workflow_id\":\"feature\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "explore-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_explore_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "plan-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_plan_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_execute_no_progress_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-2".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_execute_no_progress_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "report-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("unused report")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
    ]);
    let client_dyn: DynLlmClient = client.clone();
    let root = unique_session_test_root("before-advance-repeat-exhaustion");
    write_review_skill(&root);
    write_feature_workflow_with_hook(&root, "todo_managed_execute", 1);

    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: client_dyn,
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();
    let (tx, rx) = mpsc::channel();

    session
        .spawn_turn_ui_compat("hello".to_string(), 45, tx)
        .unwrap();

    let mut warnings = Vec::new();
    let mut errors = Vec::new();
    loop {
        match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
            RuntimeUiEnvelope::Message { turn_id, message }
                if turn_id == 45
                    && matches!(message.source, UiSource::System)
                    && message.kind == UiMessageKind::Warning =>
            {
                warnings.push(message.content.as_text().to_string());
            }
            RuntimeUiEnvelope::Message { turn_id, message }
                if turn_id == 45
                    && matches!(message.source, UiSource::System)
                    && message.kind == UiMessageKind::Error =>
            {
                errors.push(message.content.as_text().to_string());
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::SetStatusSlot {
                        slot: StatusSlot::Agent,
                        value: StatusValue::Label(label),
                    },
            } => {
                assert_eq!(turn_id, 45);
                assert_eq!(label, "Idle");
                break;
            }
            _ => {}
        }
    }

    assert!(warnings
        .iter()
        .any(|warning| { warning.contains("Step 'execute' advance denied; repeating (1/1)") }));
    assert!(errors.iter().any(|error| {
        error.contains("Hook-managed step failed: step 'execute' exhausted max_step_repeats=1")
    }));
    assert!(errors
        .iter()
        .any(|error| { error.contains("Error: step 'execute' exhausted max_step_repeats=1") }));

    let systems = client.recorded_systems();
    assert!(
        systems
            .iter()
            .filter_map(|system| system.as_deref())
            .filter(|system| system.contains("<todo_state step_id=\"execute\">"))
            .count()
            >= 2
    );
    assert_eq!(client.remaining_steps(), 1);
}

#[test]
fn spawn_turn_fails_when_item_repeat_budget_exhausts_before_step_budget() {
    let client: Arc<SequencedClient> = sequenced_client(vec![
        ChatResponse {
            id: "scene-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"recognized_scene_id\":\"feature\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "select-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"selected_workflow_id\":\"feature\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "explore-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_explore_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "plan-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_plan_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_execute_no_progress_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-2".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_execute_no_progress_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
    ]);
    let client_dyn: DynLlmClient = client.clone();
    let root = unique_session_test_root("item-repeat-exhaustion");
    write_review_skill(&root);
    write_feature_workflow_with_hook_and_item_repeats(&root, "todo_managed_execute", 5, 1);

    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: client_dyn,
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();
    let (tx, rx) = mpsc::channel();

    session
        .spawn_turn_ui_compat("hello".to_string(), 92, tx)
        .unwrap();

    let mut diagnostics = Vec::new();
    let mut errors = Vec::new();
    loop {
        match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::UpsertStepDiagnostics {
                        diagnostics: update,
                    },
            } => {
                assert_eq!(turn_id, 92);
                diagnostics.push(*update);
            }
            RuntimeUiEnvelope::Message { turn_id, message }
                if turn_id == 92
                    && matches!(message.source, UiSource::System)
                    && message.kind == UiMessageKind::Error =>
            {
                errors.push(message.content.as_text().to_string());
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::SetStatusSlot {
                        slot: StatusSlot::Agent,
                        value: StatusValue::Label(label),
                    },
            } => {
                assert_eq!(turn_id, 92);
                assert_eq!(label, "Idle");
                break;
            }
            _ => {}
        }
    }

    assert!(errors.iter().any(|error| {
        error.contains("exhausted max_item_repeats=1") && error.contains("todo item 'task-1'")
    }));
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostics| {
                diagnostics.step_id == EXECUTE_STEP_ID
                    && diagnostics.output.status == StepOutputStatus::Valid
            })
            .count(),
        2
    );
    assert!(diagnostics.iter().any(|diagnostics| {
        diagnostics.step_id == EXECUTE_STEP_ID
            && diagnostics.output.status == StepOutputStatus::Valid
            && diagnostics
                .execute_progress
                .as_ref()
                .is_some_and(|progress| {
                    progress.current_item_id.as_deref() == Some("task-1")
                        && progress.repeat_count == 1
                })
    }));
}

#[test]
fn interrupt_restores_checkpoint_messages() {
    let client: DynLlmClient = Arc::new(IdleClient);
    let root = std::env::temp_dir().join("omega-agent-session-test");
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::create_dir_all(&root);
    let skills_dir = root.join(".claude/skills/review");
    let _ = std::fs::create_dir_all(&skills_dir);
    let _ = std::fs::write(
        skills_dir.join("SKILL.md"),
        "---\nname: review\ndescription: Review code\n---\nFind regressions.",
    );
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client,
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();

    {
        let mut slot = session.agent_slot.lock().unwrap();
        let agent = slot.agent.as_mut().unwrap();
        agent.add_user_message("checkpoint me");
    }
    session.checkpoint_current_messages();
    session.interrupt(42).unwrap();

    let slot = session.agent_slot.lock().unwrap();
    let restored = slot.agent.as_ref().unwrap().messages();
    assert_eq!(slot.turn_id, 42);
    assert_eq!(restored.len(), 1);
}

#[test]
fn spawn_turn_emits_root_then_child_workflow_steps_and_uses_phase_prompts() {
    let client: Arc<SequencedClient> = sequenced_client(vec![
        ChatResponse {
            id: "scene-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"recognized_scene_id\":\"feature\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "select-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"selected_workflow_id\":\"feature\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "analysis-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_explore_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "plan-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_plan_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::tool_use(
                "tool-1",
                "bash",
                serde_json::json!({"command": "echo hi"}),
            )],
            stop_reason: Some(STOP_REASON_TOOL_USE.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-2".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_execute_partial_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-3".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_execute_complete_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "report-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("done")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
    ]);
    let client_dyn: DynLlmClient = client.clone();
    let root = std::env::temp_dir().join("omega-agent-session-workflow-test");
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::create_dir_all(&root);
    let skills_dir = root.join(".claude/skills/review");
    let _ = std::fs::create_dir_all(&skills_dir);
    let _ = std::fs::write(
        skills_dir.join("SKILL.md"),
        "---\nname: review\ndescription: Review code\n---\nFind regressions.",
    );
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: client_dyn,
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();
    let (tx, rx) = mpsc::channel();

    session
        .spawn_turn_ui_compat("hello".to_string(), 7, tx)
        .unwrap();

    let mut steps = Vec::new();
    let mut step_texts = Vec::new();
    let mut session_routes = Vec::new();
    let mut todo_panels = Vec::new();
    let mut logs = Vec::new();
    let mut saw_text = false;
    loop {
        match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::SetStatusSlot {
                        slot: StatusSlot::Workflow,
                        value:
                            StatusValue::WorkflowStep {
                                workflow_id,
                                workflow_role,
                                step_id,
                                step_label,
                                ..
                            },
                    },
            } => {
                assert_eq!(turn_id, 7);
                steps.push((workflow_id, workflow_role, step_id, step_label));
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::SetStatusSlot {
                        slot: StatusSlot::Session,
                        value:
                            StatusValue::SessionRouting {
                                root_workflow_id,
                                active_workflow_id,
                                active_workflow_role,
                                recognized_scene_id,
                                selected_workflow_id,
                            },
                    },
            } => {
                assert_eq!(turn_id, 7);
                session_routes.push((
                    root_workflow_id,
                    active_workflow_id,
                    active_workflow_role,
                    recognized_scene_id,
                    selected_workflow_id,
                ));
            }
            RuntimeUiEnvelope::Message { turn_id, message } => {
                assert_eq!(turn_id, 7);
                match (message.source, message.kind) {
                    (
                        UiSource::WorkflowStep {
                            workflow_id,
                            workflow_role,
                            step_id,
                            step_label,
                            ..
                        },
                        UiMessageKind::Narrative,
                    ) => step_texts.push((
                        workflow_id,
                        workflow_role,
                        step_id,
                        step_label,
                        message.content.as_text().to_string(),
                    )),
                    (UiSource::Assistant, UiMessageKind::Result) => {
                        assert_eq!(message.content.as_text(), "done");
                        saw_text = true;
                    }
                    (UiSource::SessionRouting, UiMessageKind::Summary | UiMessageKind::Warning) => {
                        logs.push(message.content.as_text().to_string())
                    }
                    (UiSource::System, UiMessageKind::Summary | UiMessageKind::Warning) => {
                        logs.push(message.content.as_text().to_string())
                    }
                    _ => {}
                }
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::ReplacePanel {
                        target: UiTarget::Todo,
                        content,
                    },
            } => {
                assert_eq!(turn_id, 7);
                todo_panels.push(content.as_text().to_string());
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::SetStatusSlot {
                        slot: StatusSlot::Agent,
                        value: StatusValue::Label(label),
                    },
            } => {
                assert_eq!(turn_id, 7);
                assert_eq!(label, "Idle");
                break;
            }
            _ => {}
        }
    }

    assert_eq!(
        steps,
        vec![
            (
                ROOT_WORKFLOW_ID.to_string(),
                WorkflowRunRole::Root,
                SELECT_WORKFLOW_STEP_ID.to_string(),
                "Select Workflow".to_string(),
            ),
            (
                ROOT_WORKFLOW_ID.to_string(),
                WorkflowRunRole::Root,
                SELECT_SKILLS_STEP_ID.to_string(),
                "Select Skills".to_string(),
            ),
            (
                FEATURE_WORKFLOW_ID.to_string(),
                WorkflowRunRole::Child,
                EXPLORE_STEP_ID.to_string(),
                "Explore".to_string(),
            ),
            (
                FEATURE_WORKFLOW_ID.to_string(),
                WorkflowRunRole::Child,
                "plan".to_string(),
                "Plan".to_string(),
            ),
            (
                FEATURE_WORKFLOW_ID.to_string(),
                WorkflowRunRole::Child,
                EXECUTE_STEP_ID.to_string(),
                "Execute".to_string(),
            ),
            (
                FEATURE_WORKFLOW_ID.to_string(),
                WorkflowRunRole::Child,
                EXECUTE_STEP_ID.to_string(),
                "Execute".to_string(),
            ),
            (
                FEATURE_WORKFLOW_ID.to_string(),
                WorkflowRunRole::Child,
                "report".to_string(),
                "Report".to_string(),
            ),
        ]
    );
    assert_eq!(
        step_texts,
        vec![
            (
                FEATURE_WORKFLOW_ID.to_string(),
                WorkflowRunRole::Child,
                EXPLORE_STEP_ID.to_string(),
                "Explore".to_string(),
                feature_explore_json().to_string(),
            ),
            (
                FEATURE_WORKFLOW_ID.to_string(),
                WorkflowRunRole::Child,
                "plan".to_string(),
                "Plan".to_string(),
                feature_plan_json().to_string(),
            ),
            (
                FEATURE_WORKFLOW_ID.to_string(),
                WorkflowRunRole::Child,
                EXECUTE_STEP_ID.to_string(),
                "Execute".to_string(),
                feature_execute_partial_json().to_string(),
            ),
            (
                FEATURE_WORKFLOW_ID.to_string(),
                WorkflowRunRole::Child,
                EXECUTE_STEP_ID.to_string(),
                "Execute".to_string(),
                feature_execute_complete_json().to_string(),
            ),
        ]
    );
    assert!(saw_text);
    assert!(session_routes.iter().any(|route| {
        route
            == &(
                ROOT_WORKFLOW_ID.to_string(),
                ROOT_WORKFLOW_ID.to_string(),
                WorkflowRunRole::Root,
                None,
                None,
            )
    }));
    assert!(session_routes.iter().any(|route| {
        route
            == &(
                ROOT_WORKFLOW_ID.to_string(),
                FEATURE_WORKFLOW_ID.to_string(),
                WorkflowRunRole::Child,
                Some("feature".to_string()),
                Some(FEATURE_WORKFLOW_ID.to_string()),
            )
    }));
    assert!(logs
        .iter()
        .any(|line| line.contains("Recognized scene 'feature'")));
    assert!(logs
        .iter()
        .any(|line| line.contains("selected workflow 'feature'")));
    assert!(todo_panels.iter().any(|panel| panel.contains("#task-1")));
    assert!(todo_panels.iter().any(|panel| panel.contains("#task-2")));
    let systems = client.recorded_systems();
    assert!(systems.iter().filter_map(|system| system.as_deref()).any(|system| {
        system.contains("Workflow role: root") && system.contains("Visible tools: none")
    }));
    assert!(systems.iter().filter_map(|system| system.as_deref()).any(|system| {
        system.contains("feature") && system.contains("Visible tools: none")
    }));
    assert!(systems.iter().filter_map(|system| system.as_deref()).any(|system| {
        system.contains("Workflow role: child")
            && system.contains("Active workflow: feature")
            && system.contains("Selected workflow: feature.")
            && system.contains("hello")
    }));
    assert!(systems
        .iter()
        .filter_map(|system| system.as_deref())
        .any(|system| system.contains("<todo_state step_id=\"execute\">")));
    assert!(systems
        .iter()
        .filter_map(|system| system.as_deref())
        .any(|system| system.contains("#task-1")));
    assert!(systems
        .last()
        .and_then(|system| system.as_deref())
        .is_some_and(|system| system.contains("Workflow phase: Report")));
}

#[test]
fn chat_scene_routes_to_chat_workflow_without_showing_root_text() {
    let client: Arc<SequencedClient> = sequenced_client(vec![
        ChatResponse {
            id: "scene-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"recognized_scene_id\":\"chat\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "select-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"selected_workflow_id\":\"chat\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "chat-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("chat answer")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
    ]);
    let client_dyn: DynLlmClient = client.clone();
    let root = std::env::temp_dir().join("omega-agent-session-chat-test");
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::create_dir_all(&root);
    let skills_dir = root.join(".claude/skills/review");
    let _ = std::fs::create_dir_all(&skills_dir);
    let _ = std::fs::write(
        skills_dir.join("SKILL.md"),
        "---\nname: review\ndescription: Review code\n---\nFind regressions.",
    );
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: client_dyn,
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 24_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();
    let (tx, rx) = mpsc::channel();

    session
        .spawn_turn_ui_compat("just chat".to_string(), 9, tx)
        .unwrap();

    let mut steps = Vec::new();
    let mut root_narratives = Vec::new();
    let mut assistant_results = Vec::new();
    let mut session_routes = Vec::new();
    loop {
        match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::SetStatusSlot {
                        slot: StatusSlot::Workflow,
                        value:
                            StatusValue::WorkflowStep {
                                workflow_id,
                                workflow_role,
                                step_id,
                                ..
                            },
                    },
            } => {
                assert_eq!(turn_id, 9);
                steps.push((workflow_id, workflow_role, step_id));
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::SetStatusSlot {
                        slot: StatusSlot::Session,
                        value:
                            StatusValue::SessionRouting {
                                root_workflow_id,
                                active_workflow_id,
                                active_workflow_role,
                                recognized_scene_id,
                                selected_workflow_id,
                            },
                    },
            } => {
                assert_eq!(turn_id, 9);
                session_routes.push((
                    root_workflow_id,
                    active_workflow_id,
                    active_workflow_role,
                    recognized_scene_id,
                    selected_workflow_id,
                ));
            }
            RuntimeUiEnvelope::Message { turn_id, message } => {
                assert_eq!(turn_id, 9);
                match (message.source, message.kind) {
                    (UiSource::WorkflowStep { step_id, .. }, UiMessageKind::Narrative) => {
                        root_narratives.push(step_id)
                    }
                    (UiSource::Assistant, UiMessageKind::Result) => {
                        assistant_results.push(message.content.as_text().to_string())
                    }
                    _ => {}
                }
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::SetStatusSlot {
                        slot: StatusSlot::Agent,
                        value: StatusValue::Label(label),
                    },
            } => {
                assert_eq!(turn_id, 9);
                assert_eq!(label, "Idle");
                break;
            }
            _ => {}
        }
    }

    assert_eq!(
        steps,
        vec![
            (
                ROOT_WORKFLOW_ID.to_string(),
                WorkflowRunRole::Root,
                SELECT_WORKFLOW_STEP_ID.to_string(),
            ),
            (
                ROOT_WORKFLOW_ID.to_string(),
                WorkflowRunRole::Root,
                SELECT_SKILLS_STEP_ID.to_string(),
            ),
            (
                CHAT_WORKFLOW_ID.to_string(),
                WorkflowRunRole::Child,
                CHAT_STEP_ID.to_string(),
            ),
        ]
    );
    assert!(root_narratives.is_empty());
    assert_eq!(assistant_results, vec!["chat answer".to_string()]);
    assert!(session_routes.iter().any(|route| {
        route
            == &(
                ROOT_WORKFLOW_ID.to_string(),
                CHAT_WORKFLOW_ID.to_string(),
                WorkflowRunRole::Child,
                Some("chat".to_string()),
                Some(CHAT_WORKFLOW_ID.to_string()),
            )
    }));
    let systems = client.recorded_systems();
    assert!(systems.iter().filter_map(|system| system.as_deref()).any(|system| {
        system.contains("Visible tools: none")
    }));
    assert!(systems.iter().filter_map(|system| system.as_deref()).any(|system| {
        system.contains("Active workflow: chat") && system.contains("Selected workflow: chat.")
    }));
    let max_tokens = client.recorded_max_tokens();
    assert_eq!(max_tokens, vec![24_000, 24_000, 24_000]);
}

#[test]
fn text_routing_fallback_still_loads_routed_skills_before_child_workflow() {
    let client: Arc<SequencedClient> = sequenced_client(vec![
        ChatResponse {
            id: "select-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("This request fits the chat scene.")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "select-2".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("chat is the right workflow here.")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "select-skills-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(
                r#"{"selected_skill_ids":["review","missing-skill","review"]}"#,
            )],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "chat-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("chat answer")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
    ]);
    let client_dyn: DynLlmClient = client.clone();
    let root = unique_session_test_root("text-fallback-routed-skill-load");
    let _ = std::fs::create_dir_all(&root);
    write_named_skill(&root, "review", "Review code", "Find regressions.");
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: client_dyn,
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 24_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();
    let (tx, rx) = mpsc::channel();

    session
        .spawn_turn_ui_compat("just chat".to_string(), 10, tx)
        .unwrap();

    let mut assistant_results = Vec::new();
    let mut logs = Vec::new();
    loop {
        match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
            RuntimeUiEnvelope::Message { turn_id, message } => {
                assert_eq!(turn_id, 10);
                match (message.source, message.kind) {
                    (UiSource::Assistant, UiMessageKind::Result) => {
                        assistant_results.push(message.content.as_text().to_string());
                    }
                    (UiSource::SessionRouting, UiMessageKind::Summary | UiMessageKind::Warning)
                    | (UiSource::System, UiMessageKind::Summary | UiMessageKind::Warning) => {
                        logs.push(message.content.as_text().to_string());
                    }
                    _ => {}
                }
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::SetStatusSlot {
                        slot: StatusSlot::Agent,
                        value: StatusValue::Label(label),
                    },
            } => {
                assert_eq!(turn_id, 10);
                assert_eq!(label, "Idle");
                break;
            }
            _ => {}
        }
    }

    assert_eq!(assistant_results, vec!["chat answer".to_string()]);
    assert!(logs.iter().any(|line| {
        line.contains("Loaded routed skills [review] before child workflow start; ignored [missing-skill].")
    }));
    let child_system = client
        .recorded_systems()
        .into_iter()
        .flatten()
        .find(|system| {
            system.contains("Workflow role: child") && system.contains("Active workflow: chat")
        })
        .expect("expected child workflow system prompt");
    assert!(child_system.contains("Recognized routed skills: review, missing-skill"));
    assert!(child_system.contains("<skill name=\"review\">"));
    assert!(!child_system.contains("<skill name=\"missing-skill\">"));
}

#[test]
fn select_skills_invalid_json_falls_back_without_aborting_turn() {
    let client: Arc<SequencedClient> = sequenced_client(vec![
        ChatResponse {
            id: "select-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("This request fits the chat scene.")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "select-2".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("chat is the right workflow here.")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "select-skills-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("No extra skills are needed for this turn.")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "chat-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("chat answer")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
    ]);
    let client_dyn: DynLlmClient = client;
    let root = unique_session_test_root("select-skills-text-fallback");
    let _ = std::fs::create_dir_all(&root);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: client_dyn,
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 24_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();
    let (tx, rx) = mpsc::channel();

    session
        .spawn_turn_ui_compat("just chat".to_string(), 10, tx)
        .unwrap();

    let mut diagnostics = Vec::new();
    let mut began = Vec::new();
    let mut warnings = Vec::new();
    loop {
        match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::UpsertStepDiagnostics {
                        diagnostics: update,
                    },
            } => {
                assert_eq!(turn_id, 10);
                diagnostics.push(*update);
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect: RuntimeUiEffect::BeginResponseSection { section },
            } => {
                assert_eq!(turn_id, 10);
                began.push((section.id, section.kind));
            }
            RuntimeUiEnvelope::Message { turn_id, message } => {
                assert_eq!(turn_id, 10);
                match (message.source, message.kind) {
                    (UiSource::System, UiMessageKind::Warning) => {
                        warnings.push(message.content.as_text().to_string());
                    }
                    _ => {}
                }
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::SetStatusSlot {
                        slot: StatusSlot::Agent,
                        value: StatusValue::Label(label),
                    },
            } => {
                assert_eq!(turn_id, 10);
                assert_eq!(label, "Idle");
                break;
            }
            _ => {}
        }
    }

    assert_eq!(
        warnings
            .iter()
            .filter(|text| {
                text.contains("select-skills") && text.contains("falling back to text routing")
            })
            .count(),
        1
    );
    assert!(diagnostics.iter().any(|diagnostics| {
        diagnostics.step_id == SELECT_SKILLS_STEP_ID
            && diagnostics.output.status == StepOutputStatus::Invalid
    }));
    assert!(began.iter().any(|entry| {
        entry.0.starts_with("turn-10:child:") && entry.1 == ResponseSectionKind::FinalAnswer
    }));
}

#[test]
fn unresolved_scene_and_workflow_fallback_to_feature_not_chat() {
    let client: Arc<SequencedClient> = sequenced_client(vec![
        ChatResponse {
            id: "scene-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"recognized_scene_id\":\"unknown\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "select-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"selected_workflow_id\":\"unknown\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "analysis-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_explore_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "plan-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_plan_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_execute_partial_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-2".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_execute_complete_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "report-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("done")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
    ]);
    let client_dyn: DynLlmClient = client;
    let root = std::env::temp_dir().join("omega-agent-session-default-feature-fallback-test");
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::create_dir_all(&root);
    let skills_dir = root.join(".claude/skills/review");
    let _ = std::fs::create_dir_all(&skills_dir);
    let _ = std::fs::write(
        skills_dir.join("SKILL.md"),
        "---\nname: review\ndescription: Review code\n---\nFind regressions.",
    );
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: client_dyn,
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();
    let (tx, rx) = mpsc::channel();

    session
        .spawn_turn_ui_compat("fix this bug".to_string(), 74, tx)
        .unwrap();

    let mut warnings = Vec::new();
    let mut routes = Vec::new();
    loop {
        match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
            RuntimeUiEnvelope::Message { turn_id, message }
                if turn_id == 74
                    && matches!(message.source, UiSource::System)
                    && message.kind == UiMessageKind::Warning =>
            {
                warnings.push(message.content.as_text().to_string());
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::SetStatusSlot {
                        slot: StatusSlot::Session,
                        value:
                            StatusValue::SessionRouting {
                                recognized_scene_id,
                                selected_workflow_id,
                                ..
                            },
                    },
            } => {
                assert_eq!(turn_id, 74);
                routes.push((recognized_scene_id, selected_workflow_id));
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::SetStatusSlot {
                        slot: StatusSlot::Agent,
                        value: StatusValue::Label(label),
                    },
            } => {
                assert_eq!(turn_id, 74);
                assert_eq!(label, "Idle");
                break;
            }
            _ => {}
        }
    }

    assert!(warnings
        .iter()
        .any(|warning| { warning.contains("defaulting to 'feature'") }));
    assert!(routes
        .iter()
        .any(|route| { route == &(Some("feature".to_string()), Some("feature".to_string())) }));
}

#[test]
fn recognized_research_scene_with_unknown_workflow_falls_back_to_scene_workflow() {
    let client: Arc<SequencedClient> = sequenced_client(vec![
        ChatResponse {
            id: "scene-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"recognized_scene_id\":\"research\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "select-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"selected_workflow_id\":\"unknown\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "analysis-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_explore_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "plan-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_plan_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_execute_partial_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-2".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_execute_complete_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "report-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("done")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
    ]);
    let client_dyn: DynLlmClient = client.clone();
    let root = unique_session_test_root("research-unknown-workflow-fallback");
    write_review_skill(&root);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: client_dyn,
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();
    let (tx, rx) = mpsc::channel();

    session
        .spawn_turn_ui_compat(
            "请对这个仓库做一次深度复杂的综合分析和探索".to_string(),
            79,
            tx,
        )
        .unwrap();

    let mut routes = Vec::new();
    loop {
        match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::SetStatusSlot {
                        slot: StatusSlot::Session,
                        value:
                            StatusValue::SessionRouting {
                                recognized_scene_id,
                                selected_workflow_id,
                                ..
                            },
                    },
            } => {
                assert_eq!(turn_id, 79);
                routes.push((recognized_scene_id, selected_workflow_id));
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::SetStatusSlot {
                        slot: StatusSlot::Agent,
                        value: StatusValue::Label(label),
                    },
            } => {
                assert_eq!(turn_id, 79);
                assert_eq!(label, "Idle");
                break;
            }
            _ => {}
        }
    }

    assert!(routes.iter().any(|route| {
        route
            == &(
                Some(DEEP_RESEARCH_SCENE_ID.to_string()),
                Some(DEEP_RESEARCH_WORKFLOW_ID.to_string()),
            )
    }));
    assert!(client.recorded_systems().iter().any(|system| {
        system
            .as_deref()
            .is_some_and(|system| system.contains("Selected workflow: deep-research."))
    }));
}

#[test]
fn recognized_research_scene_with_root_workflow_selection_falls_back_to_scene_workflow() {
    let client: Arc<SequencedClient> = sequenced_client(vec![
        ChatResponse {
            id: "scene-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"recognized_scene_id\":\"research\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "select-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"selected_workflow_id\":\"root\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "analysis-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_explore_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "plan-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_plan_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_execute_partial_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-2".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_execute_complete_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "report-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("done")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
    ]);
    let client_dyn: DynLlmClient = client.clone();
    let root = unique_session_test_root("research-root-workflow-fallback");
    write_review_skill(&root);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: client_dyn,
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();
    let (tx, rx) = mpsc::channel();

    session
        .spawn_turn_ui_compat(
            "请对这个仓库做一次深度复杂的综合分析和探索".to_string(),
            80,
            tx,
        )
        .unwrap();

    let mut routes = Vec::new();
    loop {
        match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::SetStatusSlot {
                        slot: StatusSlot::Session,
                        value:
                            StatusValue::SessionRouting {
                                recognized_scene_id,
                                selected_workflow_id,
                                ..
                            },
                    },
            } => {
                assert_eq!(turn_id, 80);
                routes.push((recognized_scene_id, selected_workflow_id));
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::SetStatusSlot {
                        slot: StatusSlot::Agent,
                        value: StatusValue::Label(label),
                    },
            } => {
                assert_eq!(turn_id, 80);
                assert_eq!(label, "Idle");
                break;
            }
            _ => {}
        }
    }

    assert!(routes.iter().any(|route| {
        route
            == &(
                Some(DEEP_RESEARCH_SCENE_ID.to_string()),
                Some(DEEP_RESEARCH_WORKFLOW_ID.to_string()),
            )
    }));
    assert!(routes
        .iter()
        .all(|route| route.1.as_deref() != Some(ROOT_WORKFLOW_ID)));
    assert!(client.recorded_systems().iter().any(|system| {
        system
            .as_deref()
            .is_some_and(|system| system.contains("Selected workflow: deep-research."))
    }));
}

#[test]
fn implementation_requests_are_promoted_from_chat_to_feature() {
    let client: Arc<SequencedClient> = sequenced_client(vec![
        ChatResponse {
            id: "scene-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"recognized_scene_id\":\"chat\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "select-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"selected_workflow_id\":\"chat\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "analysis-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_explore_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "plan-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_plan_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_execute_partial_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-2".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_execute_complete_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "report-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("done")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
    ]);
    let client_dyn: DynLlmClient = client;
    let root = std::env::temp_dir().join("omega-agent-session-scene-promotion-test");
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::create_dir_all(&root);
    let skills_dir = root.join(".claude/skills/review");
    let _ = std::fs::create_dir_all(&skills_dir);
    let _ = std::fs::write(
        skills_dir.join("SKILL.md"),
        "---\nname: review\ndescription: Review code\n---\nFind regressions.",
    );
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: client_dyn,
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();
    let (tx, rx) = mpsc::channel();

    session
        .spawn_turn_ui_compat("请你更新相关文档，并修复这个 bug".to_string(), 75, tx)
        .unwrap();

    let mut warnings = Vec::new();
    let mut routes = Vec::new();
    loop {
        match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
            RuntimeUiEnvelope::Message { turn_id, message }
                if turn_id == 75
                    && matches!(message.source, UiSource::System)
                    && message.kind == UiMessageKind::Warning =>
            {
                warnings.push(message.content.as_text().to_string());
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::SetStatusSlot {
                        slot: StatusSlot::Session,
                        value:
                            StatusValue::SessionRouting {
                                recognized_scene_id,
                                selected_workflow_id,
                                ..
                            },
                    },
            } => {
                assert_eq!(turn_id, 75);
                routes.push((recognized_scene_id, selected_workflow_id));
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::SetStatusSlot {
                        slot: StatusSlot::Agent,
                        value: StatusValue::Label(label),
                    },
            } => {
                assert_eq!(turn_id, 75);
                assert_eq!(label, "Idle");
                break;
            }
            _ => {}
        }
    }

    assert!(warnings
        .iter()
        .any(|warning| { warning.contains("promoting to 'feature'") }));
    assert!(routes
        .iter()
        .any(|route| { route == &(Some("feature".to_string()), Some("feature".to_string())) }));
}

#[test]
fn research_requests_are_promoted_to_research_scene_and_workflow() {
    let client: Arc<SequencedClient> = sequenced_client(vec![
        ChatResponse {
            id: "scene-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"recognized_scene_id\":\"chat\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "select-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"selected_workflow_id\":\"feature\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "analysis-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_explore_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "plan-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_plan_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_execute_partial_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-2".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(research_execute_complete_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "report-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("done")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
    ]);
    let client_dyn: DynLlmClient = client;
    let root = std::env::temp_dir().join("omega-agent-session-research-promotion-test");
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::create_dir_all(&root);
    let skills_dir = root.join(".claude/skills/review");
    let _ = std::fs::create_dir_all(&skills_dir);
    let _ = std::fs::write(
        skills_dir.join("SKILL.md"),
        "---\nname: review\ndescription: Review code\n---\nFind regressions.",
    );
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: client_dyn,
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();
    let (tx, rx) = mpsc::channel();

    session
        .spawn_turn_ui_compat(
            "请对这个仓库做一次深度复杂的综合分析和探索".to_string(),
            76,
            tx,
        )
        .unwrap();

    let mut warnings = Vec::new();
    let mut routes = Vec::new();
    loop {
        match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
            RuntimeUiEnvelope::Message { turn_id, message }
                if turn_id == 76
                    && matches!(message.source, UiSource::System)
                    && message.kind == UiMessageKind::Warning =>
            {
                warnings.push(message.content.as_text().to_string());
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::SetStatusSlot {
                        slot: StatusSlot::Session,
                        value:
                            StatusValue::SessionRouting {
                                recognized_scene_id,
                                selected_workflow_id,
                                ..
                            },
                    },
            } => {
                assert_eq!(turn_id, 76);
                routes.push((recognized_scene_id, selected_workflow_id));
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::SetStatusSlot {
                        slot: StatusSlot::Agent,
                        value: StatusValue::Label(label),
                    },
            } => {
                assert_eq!(turn_id, 76);
                assert_eq!(label, "Idle");
                break;
            }
            _ => {}
        }
    }

    assert!(warnings
        .iter()
        .any(|warning| warning.contains("deep-research-oriented")));
    assert!(routes.iter().any(|route| {
        route
            == &(
                Some(DEEP_RESEARCH_SCENE_ID.to_string()),
                Some(DEEP_RESEARCH_WORKFLOW_ID.to_string()),
            )
    }));
}

#[test]
fn session_context_persists_step_summaries_across_turns() {
    let client: Arc<SequencedClient> = sequenced_client(vec![
        ChatResponse {
            id: "scene-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"recognized_scene_id\":\"chat\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "select-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"selected_workflow_id\":\"chat\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "chat-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("first answer")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "scene-2".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"recognized_scene_id\":\"chat\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "select-2".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"selected_workflow_id\":\"chat\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "chat-2".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("second answer")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
    ]);
    let client_dyn: DynLlmClient = client.clone();
    let root = std::env::temp_dir().join("omega-agent-session-context-persistence-test");
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::create_dir_all(&root);
    let skills_dir = root.join(".claude/skills/review");
    let _ = std::fs::create_dir_all(&skills_dir);
    let _ = std::fs::write(
        skills_dir.join("SKILL.md"),
        "---\nname: review\ndescription: Review code\n---\nFind regressions.",
    );
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: client_dyn,
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 24_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();

    for (turn_id, input) in [(31, "first question"), (32, "second question")] {
        let (tx, rx) = mpsc::channel();
        session
            .spawn_turn_ui_compat(input.to_string(), turn_id, tx)
            .unwrap();

        loop {
            if let RuntimeUiEnvelope::Effect {
                turn_id: observed_turn_id,
                effect:
                    RuntimeUiEffect::SetStatusSlot {
                        slot: StatusSlot::Agent,
                        value: StatusValue::Label(label),
                    },
            } = rx.recv_timeout(Duration::from_secs(2)).unwrap()
            {
                assert_eq!(observed_turn_id, turn_id);
                assert_eq!(label, "Idle");
                break;
            }
        }
    }

    let systems = client.recorded_systems();
    assert!(systems.iter().filter_map(|system| system.as_deref()).any(|system| {
        system.contains("second question") && system.contains("first answer")
    }));
    assert!(systems.iter().filter_map(|system| system.as_deref()).any(|system| {
        system.contains("Selected workflow: chat.")
    }));
}

#[test]
fn prompt_assembly_includes_checkpoint_backed_ledger_context() {
    let client: Arc<SequencedClient> = sequenced_client(vec![
        ChatResponse {
            id: "scene-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"recognized_scene_id\":\"chat\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "select-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"selected_workflow_id\":\"chat\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "chat-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("x".repeat(1_700_000))],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "scene-2".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"recognized_scene_id\":\"chat\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "select-2".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"selected_workflow_id\":\"chat\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "chat-2".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("second answer")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
    ]);
    let client_dyn: DynLlmClient = client.clone();
    let root = unique_session_test_root("session-ledger-prompt-assembly");
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: client_dyn,
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 24_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();

    for (turn_id, input) in [(41, "first question"), (42, "second question")] {
        let (tx, rx) = mpsc::channel();
        session
            .spawn_turn_ui_compat(input.to_string(), turn_id, tx)
            .unwrap();

        loop {
            if let RuntimeUiEnvelope::Effect {
                turn_id: observed_turn_id,
                effect:
                    RuntimeUiEffect::SetStatusSlot {
                        slot: StatusSlot::Agent,
                        value: StatusValue::Label(label),
                    },
            } = rx.recv_timeout(Duration::from_secs(2)).unwrap()
            {
                assert_eq!(observed_turn_id, turn_id);
                assert_eq!(label, "Idle");
                break;
            }
        }
    }

    let second_turn_system = client
        .recorded_systems()
        .into_iter()
        .flatten()
        .find(|system| {
            system.contains("second question") && system.contains("Workflow role: child")
        })
        .expect("expected second-turn child workflow system prompt");
    assert!(second_turn_system.contains("<session_ledger_context>"));
    assert!(second_turn_system.contains("Compacted"));
}

#[test]
fn prompt_assembly_includes_session_history_hits_from_ledger_search() {
    let client: Arc<SequencedClient> = sequenced_client(vec![
        ChatResponse {
            id: "scene-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"recognized_scene_id\":\"chat\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "select-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"selected_workflow_id\":\"chat\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "chat-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("first answer")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "scene-2".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"recognized_scene_id\":\"chat\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "select-2".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"selected_workflow_id\":\"chat\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "chat-2".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("second answer")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
    ]);
    let client_dyn: DynLlmClient = client.clone();
    let root = unique_session_test_root("session-ledger-history-search");
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: client_dyn,
        system: "system".to_string(),
        cwd: root.clone(),
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 24_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();

    let (tx, rx) = mpsc::channel();
    session
        .spawn_turn_ui_compat("start a chat session".to_string(), 51, tx)
        .unwrap();
    loop {
        if let RuntimeUiEnvelope::Effect {
            turn_id: observed_turn_id,
            effect:
                RuntimeUiEffect::SetStatusSlot {
                    slot: StatusSlot::Agent,
                    value: StatusValue::Label(label),
                },
        } = rx.recv_timeout(Duration::from_secs(2)).unwrap()
        {
            assert_eq!(observed_turn_id, 51);
            assert_eq!(label, "Idle");
            break;
        }
    }

    let handle = ProjectRegistry::new()
        .resolve(ProjectResolutionInput {
            current_file_path: None,
            cwd: root.clone(),
            explicit_root: Some(root.clone()),
        })
        .unwrap();
    let session_id = handle
        .list_sessions()
        .unwrap()
        .into_iter()
        .next()
        .expect("expected first turn to create a session")
        .session_id;
    handle
        .append_context_records(
            &session_id,
            &[SessionContextRecord {
                schema_version: 1,
                session_id: session_id.clone(),
                sequence: 0,
                recorded_at: 2,
                token_estimate: Some(24),
                record: SessionContextRecordKind::CompressionCheckpoint {
                    checkpoint_id: "checkpoint:widget-cache".to_string(),
                    source_sequence_start: 1,
                    source_sequence_end: 4,
                    summary: "Widget cache invalidation history".to_string(),
                    keywords: vec!["widget".to_string(), "cache".to_string(), "invalidation".to_string()],
                    retained_facts: vec![
                        "The old widget pipeline required clearing stale cache keys before retries."
                            .to_string(),
                    ],
                    token_count: 24,
                },
            }],
        )
        .unwrap();

    let (tx, rx) = mpsc::channel();
    session
        .spawn_turn_ui_compat("Explain widget cache invalidation".to_string(), 52, tx)
        .unwrap();
    loop {
        if let RuntimeUiEnvelope::Effect {
            turn_id: observed_turn_id,
            effect:
                RuntimeUiEffect::SetStatusSlot {
                    slot: StatusSlot::Agent,
                    value: StatusValue::Label(label),
                },
        } = rx.recv_timeout(Duration::from_secs(2)).unwrap()
        {
            assert_eq!(observed_turn_id, 52);
            assert_eq!(label, "Idle");
            break;
        }
    }

    let second_turn_system = client
        .recorded_systems()
        .into_iter()
        .flatten()
        .find(|system| {
            system.contains("Explain widget cache invalidation")
                && system.contains("Workflow role: child")
        })
        .expect("expected second-turn child workflow system prompt");
    assert!(second_turn_system.contains("<session_history_hits>"));
    assert!(second_turn_system.contains("Widget cache invalidation history"));
}

#[test]
fn persist_session_artifacts_appends_compaction_checkpoint_records() {
    let root = unique_session_test_root("session-checkpoint-writeback");
    let handle = ProjectRegistry::new()
        .resolve(ProjectResolutionInput {
            current_file_path: None,
            cwd: root.clone(),
            explicit_root: Some(root.clone()),
        })
        .unwrap();
    handle
        .upsert_session(ProjectSessionUpdate {
            session_id: "session-a".to_string(),
            title: Some("Session A".to_string()),
            status: ProjectSessionStatus::Active,
            turn_count: 1,
            last_user_turn_preview: Some("oversized history".to_string()),
            archived_turn_count: Some(0),
        })
        .unwrap();

    let mut session_context = SessionContext::new(ROOT_WORKFLOW_ID.to_string());
    session_context.latest_user_turn = "oversized history".to_string();
    let todo_manager = Arc::new(Mutex::new(super::TodoManager::new()));
    let replay_entries = vec![SessionReplayEntry {
        session_id: "session-a".to_string(),
        recorded_at: 1,
        kind: SessionReplayEntryKind::AssistantResponse,
        title: Some("Assistant".to_string()),
        body: "x".repeat(1_700_000),
        state: Some("complete".to_string()),
    }];

    super::persist_session_artifacts(
        &handle,
        "session-a",
        &root,
        &session_context,
        &todo_manager,
        None,
        Some(1),
        &replay_entries,
    )
    .unwrap();

    let records = handle.load_context_records("session-a").unwrap();
    assert!(records.iter().any(|record| {
        matches!(
            record.record,
            SessionContextRecordKind::CompressionCheckpoint { .. }
        )
    }));
}

#[test]
fn spawn_turn_emits_response_sections_for_routing_and_thinking() {
    let client: Arc<SequencedClient> = sequenced_client(vec![
        ChatResponse {
            id: "scene-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"recognized_scene_id\":\"chat\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "select-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"selected_workflow_id\":\"chat\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "chat-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![
                ContentBlock::Thinking {
                    thinking: "outline answer".to_string(),
                    signature: None,
                },
                ContentBlock::text("chat answer"),
            ],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
    ]);
    let client_dyn: DynLlmClient = client;
    let root = std::env::temp_dir().join("omega-agent-session-response-section-test");
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::create_dir_all(&root);
    let skills_dir = root.join(".claude/skills/review");
    let _ = std::fs::create_dir_all(&skills_dir);
    let _ = std::fs::write(
        skills_dir.join("SKILL.md"),
        "---\nname: review\ndescription: Review code\n---\nFind regressions.",
    );
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: client_dyn,
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();
    let (tx, rx) = mpsc::channel();
    session
        .spawn_turn_ui_compat("just chat".to_string(), 11, tx)
        .unwrap();

    let mut began = Vec::new();
    let mut appended = Vec::new();
    let mut completed = Vec::new();
    loop {
        match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect: RuntimeUiEffect::BeginResponseSection { section },
            } => {
                assert_eq!(turn_id, 11);
                began.push((
                    section.id,
                    section.parent_id,
                    section.kind,
                    section.title,
                    section.metadata.origin,
                    section.metadata.scene_id,
                ));
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect: RuntimeUiEffect::AppendResponseSection { id, delta },
            } => {
                assert_eq!(turn_id, 11);
                appended.push((id, delta));
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect: RuntimeUiEffect::CompleteResponseSection { id, state },
            } => {
                assert_eq!(turn_id, 11);
                completed.push((id, state));
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::SetStatusSlot {
                        slot: StatusSlot::Agent,
                        value: StatusValue::Label(label),
                    },
            } => {
                assert_eq!(turn_id, 11);
                assert_eq!(label, "Idle");
                break;
            }
            _ => {}
        }
    }

    assert!(began.iter().any(|entry| {
        entry.0 == "turn-11:root:root:select-workflow"
            && entry.2 == ResponseSectionKind::Routing
            && entry.3 == "Select Workflow"
            && entry.4
                == SectionOrigin::Workflow {
                    workflow_id: ROOT_WORKFLOW_ID.to_string(),
                    workflow_role: WorkflowRunRole::Root,
                }
    }));
    assert!(began.iter().any(|entry| {
        entry
            == &(
                "turn-11:child:chat:chat".to_string(),
                None,
                ResponseSectionKind::FinalAnswer,
                "Final Answer".to_string(),
                SectionOrigin::Workflow {
                    workflow_id: CHAT_WORKFLOW_ID.to_string(),
                    workflow_role: WorkflowRunRole::Child,
                },
                Some("chat".to_string()),
            )
    }));
    assert!(began.iter().any(|entry| {
        entry
            == &(
                "turn-11:child:chat:chat:thinking".to_string(),
                Some("turn-11:child:chat:chat".to_string()),
                ResponseSectionKind::Thinking,
                "Thinking".to_string(),
                SectionOrigin::Workflow {
                    workflow_id: CHAT_WORKFLOW_ID.to_string(),
                    workflow_role: WorkflowRunRole::Child,
                },
                Some("chat".to_string()),
            )
    }));
    assert!(appended.iter().any(|entry| {
        entry
            == &(
                "turn-11:child:chat:chat:thinking".to_string(),
                ResponseSectionDelta::Text("outline answer".to_string()),
            )
    }));
    assert!(appended.iter().any(|entry| {
        entry
            == &(
                "turn-11:child:chat:chat".to_string(),
                ResponseSectionDelta::Text("chat answer".to_string()),
            )
    }));
    assert!(completed.iter().any(|entry| {
        entry
            == &(
                "turn-11:child:chat:chat:thinking".to_string(),
                ResponseSectionState::Complete,
            )
    }));
    assert!(completed.iter().any(|entry| {
        entry
            == &(
                "turn-11:child:chat:chat".to_string(),
                ResponseSectionState::Complete,
            )
    }));
}

#[test]
fn spawn_turn_falls_back_to_text_routing_when_root_json_validation_fails() {
    let client: Arc<SequencedClient> = sequenced_client(vec![
        ChatResponse {
            id: "select-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("This request fits the chat scene.")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "select-2".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("chat is the right workflow here.")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "select-skills-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"selected_skill_ids\":[]}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "chat-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("chat answer")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
    ]);
    let client_dyn: DynLlmClient = client;
    let root = std::env::temp_dir().join("omega-agent-session-root-routing-fallback-test");
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::create_dir_all(&root);
    let skills_dir = root.join(".claude/skills/review");
    let _ = std::fs::create_dir_all(&skills_dir);
    let _ = std::fs::write(
        skills_dir.join("SKILL.md"),
        "---\nname: review\ndescription: Review code\n---\nFind regressions.",
    );
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: client_dyn,
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();
    let (tx, rx) = mpsc::channel();

    session
        .spawn_turn_ui_compat("just chat".to_string(), 73, tx)
        .unwrap();

    let mut diagnostics = Vec::new();
    let mut began = Vec::new();
    loop {
        match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::UpsertStepDiagnostics {
                        diagnostics: update,
                    },
            } => {
                assert_eq!(turn_id, 73);
                diagnostics.push(*update);
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect: RuntimeUiEffect::BeginResponseSection { section },
            } => {
                assert_eq!(turn_id, 73);
                began.push((section.id, section.metadata.origin, section.kind));
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::SetStatusSlot {
                        slot: StatusSlot::Agent,
                        value: StatusValue::Label(label),
                    },
            } => {
                assert_eq!(turn_id, 73);
                assert_eq!(label, "Idle");
                break;
            }
            _ => {}
        }
    }

    assert!(began.iter().any(|entry| {
        entry.0.starts_with("turn-73:child:") && entry.2 == ResponseSectionKind::FinalAnswer
    }));
    assert!(diagnostics.iter().any(|diagnostics| {
        diagnostics.step_id == SELECT_WORKFLOW_STEP_ID
            && diagnostics.output.status == StepOutputStatus::Invalid
    }));
}

#[test]
fn spawn_turn_accepts_root_json_when_model_adds_short_preface() {
    let client: Arc<SequencedClient> = sequenced_client(vec![
        ChatResponse {
            id: "select-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(
                "Best match is feature. Use the feature workflow.\n{\"recognized_scene_id\":\"feature\",\"selected_workflow_id\":\"feature\"}",
            )],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "analysis-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_explore_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "plan-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_plan_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("execution complete")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "report-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("done")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
    ]);
    let client_dyn: DynLlmClient = client;
    let root = std::env::temp_dir().join("omega-agent-session-root-embedded-json-test");
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::create_dir_all(&root);
    let skills_dir = root.join(".claude/skills/review");
    let _ = std::fs::create_dir_all(&skills_dir);
    let _ = std::fs::write(
        skills_dir.join("SKILL.md"),
        "---\nname: review\ndescription: Review code\n---\nFind regressions.",
    );
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: client_dyn,
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();
    let (tx, rx) = mpsc::channel();

    session
        .spawn_turn_ui_compat("fix this bug".to_string(), 71, tx)
        .unwrap();

    let mut warnings = Vec::new();
    let mut diagnostics = Vec::new();
    loop {
        match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
            RuntimeUiEnvelope::Message { turn_id, message }
                if turn_id == 71
                    && matches!(message.source, UiSource::System)
                    && message.kind == UiMessageKind::Warning =>
            {
                warnings.push(message.content.as_text().to_string());
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::UpsertStepDiagnostics {
                        diagnostics: update,
                    },
            } => {
                assert_eq!(turn_id, 71);
                diagnostics.push(*update);
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::SetStatusSlot {
                        slot: StatusSlot::Agent,
                        value: StatusValue::Label(label),
                    },
            } => {
                assert_eq!(turn_id, 71);
                assert_eq!(label, "Idle");
                break;
            }
            _ => {}
        }
    }

    assert!(!warnings.iter().any(|warning| {
        warning.contains("scene-recognition") || warning.contains("select-workflow")
    }));
    assert!(diagnostics.iter().any(|diagnostics| {
        diagnostics.step_id == SELECT_WORKFLOW_STEP_ID
            && diagnostics.output.status == StepOutputStatus::Valid
            && diagnostics.output.retry_count == 0
    }));
}

#[test]
fn spawn_turn_uses_precise_token_count_and_emits_cache_diagnostics() {
    let client = counting_sequenced_client(
        vec![
            ChatResponse {
                id: "scene-1".to_string(),
                model: Some("test-model".to_string()),
                content: vec![ContentBlock::text("{\"recognized_scene_id\":\"feature\"}")],
                stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                usage: None,
            },
            ChatResponse {
                id: "select-1".to_string(),
                model: Some("test-model".to_string()),
                content: vec![ContentBlock::text("{\"selected_workflow_id\":\"feature\"}")],
                stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                usage: None,
            },
            ChatResponse {
                id: "explore-1".to_string(),
                model: Some("test-model".to_string()),
                content: vec![ContentBlock::text(feature_explore_json())],
                stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                usage: None,
            },
            ChatResponse {
                id: "plan-1".to_string(),
                model: Some("test-model".to_string()),
                content: vec![ContentBlock::text(feature_plan_json())],
                stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                usage: None,
            },
            ChatResponse {
                id: "execute-1".to_string(),
                model: Some("test-model".to_string()),
                content: vec![ContentBlock::text(feature_execute_complete_json())],
                stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                usage: Some(omega_client::Usage {
                    input_tokens: 120,
                    output_tokens: 24,
                    cache_creation_input_tokens: Some(40),
                    cache_read_input_tokens: Some(60),
                }),
            },
            ChatResponse {
                id: "report-1".to_string(),
                model: Some("test-model".to_string()),
                content: vec![ContentBlock::text("done")],
                stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                usage: None,
            },
        ],
        321,
    );
    let client_dyn: DynLlmClient = client.clone();
    let root = unique_session_test_root("cache-diagnostics");
    write_review_skill(&root);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: client_dyn,
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();
    let recorder = RuntimeEnvelopeRecorder::new();

    session
        .spawn_turn_with_test_bridge("fix this bug".to_string(), 77, recorder.runtime_bridge())
        .unwrap();

    let diagnostics: Vec<_> = recorder
        .wait_for_idle_ui(77, Duration::from_secs(2))
        .into_iter()
        .filter_map(|envelope| match envelope {
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect: RuntimeUiEffect::UpsertStepDiagnostics { diagnostics },
            } if turn_id == 77 => Some(*diagnostics),
            _ => None,
        })
        .collect();

    assert!(!client.recorded_count_token_requests().is_empty());
    assert!(client.recorded_requests().iter().any(|request| {
        request.cache_last_assistant_turn
            && !request.system_blocks.is_empty()
            && request
                .system_blocks
                .iter()
                .any(|block| block.cache_control.is_some())
    }));
    assert!(diagnostics.iter().any(|update| {
        update.cache.as_ref().is_some_and(|cache| {
            cache.token_count_source == super::runtime_ui::TokenCountSource::ProviderCountTokens
                && cache.request_input_tokens == 321
                && cache
                    .cache_breakpoints
                    .iter()
                    .any(|anchor| anchor == "tools")
        })
    }));
    assert!(diagnostics.iter().any(|update| {
        update.step_id == EXECUTE_STEP_ID
            && update.cache.as_ref().is_some_and(|cache| {
                cache.cache_read_input_tokens == Some(60)
                    && cache.cache_creation_input_tokens == Some(40)
                    && cache
                        .cache_breakpoints
                        .iter()
                        .any(|anchor| anchor == "tools")
            })
    }));
}

#[test]
fn spawn_turn_falls_back_to_estimated_token_count_and_records_all_cache_breakpoints() {
    let client = failing_count_sequenced_client(vec![
        ChatResponse {
            id: "scene-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"recognized_scene_id\":\"feature\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "select-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"selected_workflow_id\":\"feature\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "explore-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_explore_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "plan-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_plan_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_execute_complete_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: Some(omega_client::Usage {
                input_tokens: 120,
                output_tokens: 24,
                cache_creation_input_tokens: Some(40),
                cache_read_input_tokens: Some(60),
            }),
        },
        ChatResponse {
            id: "report-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("done")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
    ]);
    let client_dyn: DynLlmClient = client.clone();
    let root = unique_session_test_root("cache-diagnostics-fallback");
    write_review_skill(&root);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: client_dyn,
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();
    let recorder = RuntimeEnvelopeRecorder::new();

    session
        .spawn_turn_with_test_bridge("fix this bug".to_string(), 88, recorder.runtime_bridge())
        .unwrap();

    let diagnostics: Vec<_> = recorder
        .wait_for_idle_ui(88, Duration::from_secs(2))
        .into_iter()
        .filter_map(|envelope| match envelope {
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect: RuntimeUiEffect::UpsertStepDiagnostics { diagnostics },
            } if turn_id == 88 => Some(*diagnostics),
            _ => None,
        })
        .collect();

    assert!(!client.recorded_count_token_requests().is_empty());
    assert!(diagnostics.iter().any(|update| {
        update.step_id == EXECUTE_STEP_ID
            && update.cache.as_ref().is_some_and(|cache| {
                cache.token_count_source == super::runtime_ui::TokenCountSource::Estimated
                    && cache.request_input_tokens > 0
                    && cache.cache_breakpoints
                        == vec![
                            "tools".to_string(),
                            "system:stable".to_string(),
                            "system:summaries".to_string(),
                            "messages:last_assistant_turn".to_string(),
                        ]
            })
    }));
}

#[test]
fn spawn_turn_emits_tool_runs_and_sanitizes_provider_markup() {
    let client: Arc<SequencedClient> = sequenced_client(vec![
                ChatResponse {
                    id: "scene-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text("{\"recognized_scene_id\":\"feature\"}")],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "select-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text("{\"selected_workflow_id\":\"feature\"}")],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "analysis-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text(feature_explore_json())],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "plan-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text(feature_plan_json())],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "execute-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![
                        ContentBlock::Thinking {
                            thinking: "thinking <minimax:tool_call><invoke name=\"bash\">ignored</invoke></minimax:tool_call> done".to_string(),
                            signature: None,
                        },
                        ContentBlock::text(
                            "before <invoke name=\"bash\">ignored</invoke> after",
                        ),
                        ContentBlock::tool_use(
                            "tool-1",
                            "bash",
                            serde_json::json!({"command": "echo hi"}),
                        ),
                    ],
                    stop_reason: Some(STOP_REASON_TOOL_USE.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "execute-2".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text("execution complete")],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "report-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text("done")],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
            ]);
    let client_dyn: DynLlmClient = client;
    let root = std::env::temp_dir().join("omega-agent-session-tool-run-test");
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::create_dir_all(&root);
    let skills_dir = root.join(".claude/skills/review");
    let _ = std::fs::create_dir_all(&skills_dir);
    let _ = std::fs::write(
        skills_dir.join("SKILL.md"),
        "---\nname: review\ndescription: Review code\n---\nFind regressions.",
    );
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: client_dyn,
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();
    let (tx, rx) = mpsc::channel();

    session
        .spawn_turn_ui_compat("hello".to_string(), 12, tx)
        .unwrap();

    let mut began_runs = Vec::new();
    let mut updated_runs = Vec::new();
    let mut completed_runs = Vec::new();
    let mut append_deltas = Vec::new();
    let mut tool_logs = Vec::new();
    loop {
        match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect: RuntimeUiEffect::BeginToolRun { tool_run },
            } => {
                assert_eq!(turn_id, 12);
                began_runs.push(tool_run);
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect: RuntimeUiEffect::UpdateToolRun { tool_run },
            } => {
                assert_eq!(turn_id, 12);
                updated_runs.push(tool_run);
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect: RuntimeUiEffect::CompleteToolRun { id, status },
            } => {
                assert_eq!(turn_id, 12);
                completed_runs.push((id, status));
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect: RuntimeUiEffect::AppendResponseSection { id, delta },
            } => {
                assert_eq!(turn_id, 12);
                append_deltas.push((id, delta));
            }
            RuntimeUiEnvelope::Message { turn_id, message }
                if matches!(message.source, UiSource::Tool { .. })
                    && message.kind == UiMessageKind::Log =>
            {
                assert_eq!(turn_id, 12);
                tool_logs.push(message.content.as_text().to_string());
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::SetStatusSlot {
                        slot: StatusSlot::Agent,
                        value: StatusValue::Label(label),
                    },
            } => {
                assert_eq!(turn_id, 12);
                assert_eq!(label, "Idle");
                break;
            }
            _ => {}
        }
    }

    assert_eq!(began_runs.len(), 1);
    assert_eq!(began_runs[0].id, "tool-1");
    assert_eq!(
        began_runs[0].parent_section_id,
        "turn-12:child:feature:execute-1"
    );
    assert_eq!(began_runs[0].tool_name, "bash");
    assert_eq!(began_runs[0].status, ToolRunStatus::Running);
    assert_eq!(began_runs[0].invocation_preview, "$ echo hi");
    assert!(began_runs[0].result_preview.is_none());

    assert_eq!(updated_runs.len(), 1);
    assert_eq!(updated_runs[0].id, "tool-1");
    assert_eq!(updated_runs[0].status, ToolRunStatus::Complete);
    assert!(updated_runs[0]
        .result_preview
        .as_deref()
        .is_some_and(|preview| preview.contains("hi")));
    assert!(updated_runs[0]
        .detail
        .lines
        .iter()
        .any(|line| line == "metadata:"));
    assert!(updated_runs[0]
        .detail
        .lines
        .iter()
        .any(|line| line.contains("\"command\": \"echo hi\"")));

    assert_eq!(
        completed_runs,
        vec![("tool-1".to_string(), ToolRunStatus::Complete)]
    );

    assert!(tool_logs.iter().any(|line| line == "$ echo hi"));
    assert!(tool_logs.iter().any(|line| line.contains("hi")));

    let sanitized_text = append_deltas
        .iter()
        .filter_map(|(id, delta)| match delta {
            ResponseSectionDelta::Text(text)
                if id == "turn-12:child:feature:execute-1"
                    || id == "turn-12:child:feature:execute-1:thinking" =>
            {
                Some(text.as_str())
            }
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(sanitized_text.contains("before "));
    assert!(sanitized_text.contains(" after"));
    assert!(sanitized_text.contains("thinking "));
    assert!(sanitized_text.contains(" done"));
    assert!(!sanitized_text.contains("<minimax:tool_call"));
    assert!(!sanitized_text.contains("<invoke"));
}

#[test]
fn preview_tool_invocation_formats_bash_description_and_workdir() {
    let preview = super::preview_tool_invocation(
        "bash",
        &serde_json::json!({
            "command": "rg --files src",
            "description": "List source files",
            "workdir": "crates/omega-tools"
        }),
    );

    assert_eq!(
        preview,
        "List source files @ crates/omega-tools: $ rg --files src"
    );
}

#[test]
fn spawn_turn_emits_batch_tool_run_metadata() {
    let client: Arc<SequencedClient> = sequenced_client(vec![
        ChatResponse {
            id: "scene-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"recognized_scene_id\":\"feature\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "select-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"selected_workflow_id\":\"feature\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "analysis-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_explore_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "plan-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_plan_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::tool_use(
                "tool-1",
                "batch",
                serde_json::json!({
                    "requests": [
                        {"tool": "list_dir", "input": {"path": "."}},
                        {"tool": "read_file", "input": {"path": "notes.txt", "start_line": 1, "end_line": 1}}
                    ]
                }),
            )],
            stop_reason: Some(STOP_REASON_TOOL_USE.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-2".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("execution complete")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "report-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("done")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
    ]);
    let client_dyn: DynLlmClient = client;
    let root = std::env::temp_dir().join("omega-agent-session-batch-tool-run-test");
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::create_dir_all(&root);
    let skills_dir = root.join(".claude/skills/review");
    let _ = std::fs::create_dir_all(&skills_dir);
    let _ = std::fs::write(
        skills_dir.join("SKILL.md"),
        "---\nname: review\ndescription: Review code\n---\nFind regressions.",
    );
    let _ = std::fs::write(root.join("notes.txt"), "hello\nworld\n");

    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: client_dyn,
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();
    let (tx, rx) = mpsc::channel();

    session
        .spawn_turn_ui_compat("hello".to_string(), 13, tx)
        .unwrap();

    let mut began_runs = Vec::new();
    let mut updated_runs = Vec::new();
    let mut tool_logs = Vec::new();
    loop {
        match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect: RuntimeUiEffect::BeginToolRun { tool_run },
            } => {
                assert_eq!(turn_id, 13);
                began_runs.push(tool_run);
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect: RuntimeUiEffect::UpdateToolRun { tool_run },
            } => {
                assert_eq!(turn_id, 13);
                updated_runs.push(tool_run);
            }
            RuntimeUiEnvelope::Message { turn_id, message }
                if matches!(message.source, UiSource::Tool { .. })
                    && message.kind == UiMessageKind::Log =>
            {
                assert_eq!(turn_id, 13);
                tool_logs.push(message.content.as_text().to_string());
            }
            RuntimeUiEnvelope::Effect {
                turn_id,
                effect:
                    RuntimeUiEffect::SetStatusSlot {
                        slot: StatusSlot::Agent,
                        value: StatusValue::Label(label),
                    },
            } => {
                assert_eq!(turn_id, 13);
                assert_eq!(label, "Idle");
                break;
            }
            _ => {}
        }
    }

    assert_eq!(began_runs.len(), 1);
    assert_eq!(began_runs[0].tool_name, "batch");
    assert!(began_runs[0].invocation_preview.contains("requests"));

    assert_eq!(updated_runs.len(), 1);
    assert_eq!(updated_runs[0].tool_name, "batch");
    assert_eq!(updated_runs[0].status, ToolRunStatus::Complete);
    assert!(updated_runs[0]
        .result_preview
        .as_deref()
        .is_some_and(|preview| preview.contains("Batch completed 2 requests")));
    assert!(updated_runs[0]
        .detail
        .lines
        .iter()
        .any(|line| line == "metadata:"));
    assert!(updated_runs[0]
        .detail
        .lines
        .iter()
        .any(|line| line.contains("\"request_count\": 2")));
    assert!(updated_runs[0]
        .detail
        .lines
        .iter()
        .any(|line| line.contains("=== [1] list_dir ===")));
    assert!(updated_runs[0]
        .detail
        .lines
        .iter()
        .any(|line| line.contains("=== [2] read_file ===")));
    assert!(tool_logs
        .iter()
        .any(|line| line.contains("Batch completed 2 requests")));
}

#[test]
fn spawn_turn_emits_runtime_message_envelopes_for_streaming_text_and_turn_finish() {
    let client: Arc<SequencedClient> = sequenced_client(vec![
        ChatResponse {
            id: "scene-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"recognized_scene_id\":\"chat\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "select-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"selected_workflow_id\":\"chat\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "chat-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![
                ContentBlock::Thinking {
                    thinking: "outline answer".to_string(),
                    signature: None,
                },
                ContentBlock::text("chat answer"),
            ],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
    ]);
    let client_dyn: DynLlmClient = client;
    let root = unique_session_test_root("runtime-message-chat");
    write_review_skill(&root);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: client_dyn,
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();
    let recorder = RuntimeEnvelopeRecorder::new();

    session
        .spawn_turn_with_test_bridge("just chat".to_string(), 31, recorder.runtime_bridge())
        .unwrap();

    let recorded = recorder.wait_for_turn_finished_messages(31, Duration::from_secs(2));
    let began: Vec<_> =
        recorded
            .iter()
            .filter_map(|envelope| match envelope {
                RuntimeMessageEnvelope {
                    turn_id,
                    message:
                        RuntimeMessage::Conversation(ConversationMessage::BeginSection { section }),
                } if *turn_id == 31 => Some((section.id.clone(), section.kind)),
                _ => None,
            })
            .collect();
    let appended: Vec<_> = recorded
        .iter()
        .filter_map(|envelope| match envelope {
            RuntimeMessageEnvelope {
                turn_id,
                message:
                    RuntimeMessage::Conversation(ConversationMessage::AppendSection { id, delta }),
            } if *turn_id == 31 => Some((id.clone(), delta.clone())),
            _ => None,
        })
        .collect();

    assert!(began.iter().any(|entry| {
        entry
            == &(
                "turn-31:root:root:select-workflow".to_string(),
                ResponseSectionKind::Routing,
            )
    }));
    assert!(began.iter().any(|entry| {
        entry
            == &(
                "turn-31:child:chat:chat".to_string(),
                ResponseSectionKind::FinalAnswer,
            )
    }));
    assert!(appended.iter().any(|entry| {
        entry
            == &(
                "turn-31:child:chat:chat:thinking".to_string(),
                ResponseSectionDelta::Text("outline answer".to_string()),
            )
    }));
    assert!(appended.iter().any(|entry| {
        entry
            == &(
                "turn-31:child:chat:chat".to_string(),
                ResponseSectionDelta::Text("chat answer".to_string()),
            )
    }));
    assert!(recorded.iter().any(|envelope| {
        envelope.turn_id == 31
            && matches!(
                envelope.message,
                RuntimeMessage::State(StateMessage::TurnFinished)
            )
    }));
}

#[test]
fn spawn_turn_emits_runtime_message_tool_activity_and_completion() {
    let client: Arc<SequencedClient> = sequenced_client(vec![
        ChatResponse {
            id: "scene-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"recognized_scene_id\":\"feature\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "select-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("{\"selected_workflow_id\":\"feature\"}")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "analysis-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_explore_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "plan-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_plan_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![
                ContentBlock::text("before <invoke name=\"bash\">ignored</invoke> after"),
                ContentBlock::tool_use("tool-1", "bash", serde_json::json!({"command": "echo hi"})),
            ],
            stop_reason: Some(STOP_REASON_TOOL_USE.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-2".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_execute_partial_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "execute-3".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text(feature_execute_complete_json())],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
        ChatResponse {
            id: "report-1".to_string(),
            model: Some("test-model".to_string()),
            content: vec![ContentBlock::text("done")],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: None,
        },
    ]);
    let client_dyn: DynLlmClient = client;
    let root = unique_session_test_root("runtime-message-tool");
    write_review_skill(&root);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let session = AgentSession::new(AgentSessionConfig {
        client: client_dyn,
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap();
    let recorder = RuntimeEnvelopeRecorder::new();

    session
        .spawn_turn_with_test_bridge("hello".to_string(), 32, recorder.runtime_bridge())
        .unwrap();

    let recorded = recorder.wait_for_turn_finished_messages(32, Duration::from_secs(2));

    assert!(recorded.iter().any(|envelope| {
        matches!(
            envelope,
            RuntimeMessageEnvelope {
                turn_id,
                message: RuntimeMessage::Conversation(ConversationMessage::BeginToolRun { tool_run }),
            } if *turn_id == 32 && tool_run.id == "tool-1"
        )
    }));
    assert!(recorded.iter().any(|envelope| {
        matches!(
            envelope,
            RuntimeMessageEnvelope {
                turn_id,
                message: RuntimeMessage::Conversation(ConversationMessage::CompleteToolRun { id, status }),
            } if *turn_id == 32 && id == "tool-1" && *status == ToolRunStatus::Complete
        )
    }));
    assert!(recorded.iter().any(|envelope| {
        matches!(
            envelope,
            RuntimeMessageEnvelope {
                turn_id,
                message: RuntimeMessage::State(StateMessage::Activity { source, kind, text, .. }),
            } if *turn_id == 32
                && matches!(source, RuntimeSource::Tool { .. })
                && *kind == RuntimeContentKind::Log
                && text == "$ echo hi"
        )
    }));
}

#[test]
fn session_tool_catalog_matches_current_default_tool_set() {
    let dispatcher = omega_core::create_default_tools(std::env::temp_dir());
    let available_manifests = dispatcher.manifest_metadata();
    let default_manifests = available_manifests
        .iter()
        .filter(|manifest| manifest.id != "ask_user_question")
        .cloned()
        .collect::<Vec<_>>();
    let catalog = SessionToolCatalog::with_available_manifests(default_manifests, available_manifests);

    let inherit = catalog.resolve_for_step(&StepToolRequest::Inherit);
    let extended = catalog.resolve_for_step(&StepToolRequest::Extend(vec![
        "ask_user_question".to_string(),
    ]));
    let blocked = catalog.resolve_for_step(&StepToolRequest::Block(vec![
        "bash".to_string(),
        "read_file".to_string(),
    ]));

    assert_eq!(
        inherit.tool_names(),
        [
            "apply_patch",
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
    assert!(extended.tool_names().contains(&"ask_user_question".to_string()));
    assert_eq!(
        blocked.tool_names(),
        [
            "apply_patch",
            "batch",
            "create_file",
            "edit_file",
            "glob_search",
            "grep_search",
            "list_dir",
            "load_skill",
            "manage_document",
            "search_codebase",
            "task",
            "todo_read",
            "todo_write",
            "web_fetch",
            "web_search",
            "write_file"
        ]
    );
    assert!(inherit
        .tool_manifests()
        .iter()
        .any(|manifest| manifest.id == "bash"
            && manifest.family == omega_core::CoreToolFamily::EscapeHatch));
}

#[test]
fn session_skill_catalog_preserves_existing_prompt_shape() {
    let root = std::env::temp_dir().join("omega-agent-session-skill-catalog-test");
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::create_dir_all(&root);
    let review = root.join(".claude/skills/review");
    let docs = root.join(".claude/skills/docs");
    let _ = std::fs::create_dir_all(&review);
    let _ = std::fs::create_dir_all(&docs);
    let _ = std::fs::write(
        review.join("SKILL.md"),
        "---\nname: review\ndescription: Review code\n---\nFind regressions.",
    );
    let _ = std::fs::write(
        docs.join("SKILL.md"),
        "---\nname: docs-specs\ndescription: Technical specs\n---\nBe precise.",
    );

    let loader = omega_skills::SkillLoader::from_repo_root(&root).unwrap();
    let catalog = SessionSkillCatalog::new(loader);
    let prompt = catalog.build_system_prompt(
        "Base prompt",
        "Please review this patch",
        &[],
        &StepSkillRequest::Append(vec!["docs-specs".to_string()]),
    );

    assert!(prompt.contains("Skills available:"));
    assert!(prompt.contains("review: Review code"));
    assert!(prompt.contains("Preloaded skills for this task:"));
    assert!(prompt.contains("<skill name=\"review\">"));
    assert!(prompt.contains("<skill name=\"docs-specs\">"));
}
