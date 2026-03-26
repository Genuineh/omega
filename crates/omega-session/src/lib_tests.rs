use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use omega_client::{
    test_support::{IdleLlmClient, ScriptedLlmClient}, ChatResponse, ContentBlock,
    STOP_REASON_END_TURN, STOP_REASON_TOOL_USE,
};
use omega_core::DynLlmClient;
use omega_workflow::{
    DataFormat, LoadedWorkflowCatalog, OutputRecoveryMode, StepInputContract, StepLoopMode,
    StepOutputContract, CHAT_STEP_ID, CHAT_WORKFLOW_ID, DEFAULT_EXPLORE_SCHEMA_PATH,
    EXECUTE_STEP_ID, EXPLORE_STEP_ID, FEATURE_WORKFLOW_ID, PLAN_STEP_ID, REPORT_STEP_ID,
    RESEARCH_WORKFLOW_ID, ROOT_WORKFLOW_ID, SCENE_RECOGNITION_STEP_ID, SELECT_WORKFLOW_STEP_ID,
};

use super::{
    parse_json_values, preview_text, render_output_contract, resolve_structured_input,
    validate_schema_file, validate_structured_output, AgentSession, AgentSessionConfig,
    ConversationMessage, ProviderMarkupSanitizer, ResponseSectionDelta, ResponseSectionKind,
    ResponseSectionState, RuntimeContentKind, RuntimeMessage, RuntimeMessageEnvelope,
    RuntimeSource, RuntimeUiEffect, RuntimeUiEnvelope, SessionContext, SessionSkillCatalog,
    SessionToolCatalog, StateMessage, StatusSlot, StatusValue, StepContextWriteKind,
    StepOutputAttemptKind, StepOutputStatus, StepSkillRequest, StepToolRequest, ToolRunStatus,
    UiMessageKind, UiSource, UiTarget, WorkflowRunRole,
};

type SequencedClient = ScriptedLlmClient;

#[allow(non_upper_case_globals)]
const IdleClient: IdleLlmClient =
    IdleLlmClient::new("chat should not be called in AgentSession unit tests");

fn sequenced_client(responses: Vec<ChatResponse>) -> Arc<SequencedClient> {
    Arc::new(SequencedClient::from_responses(responses))
}

fn feature_explore_json() -> &'static str {
    r#"{"objective":"Implement the requested change","key_findings":["The workflow runtime resolves plan input from the first step's structured output","Session tests assert the first child step's stable id and label"],"constraints":["preserve existing behavior"],"risks":["regression risk"],"affected_paths":["crates/omega-session/src/lib.rs"]}"#
}

fn feature_plan_json() -> &'static str {
    r#"{"goal":"Implement the requested change safely","tasks":[{"id":"task-1","title":"Inspect code","description":"Review the relevant workflow and session logic"},{"id":"task-2","title":"Apply changes","description":"Implement the requested code and test updates"}],"validation_targets":["cargo test -p omega-workflow -p omega-session"]}"#
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

fn unique_session_test_root(name: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("omega-agent-session-{name}-{unique}"));
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::create_dir_all(&root);
    root
}

fn write_review_skill(root: &Path) {
    let skills_dir = root.join(".claude/skills/review");
    let _ = std::fs::create_dir_all(&skills_dir);
    let _ = std::fs::write(
        skills_dir.join("SKILL.md"),
        "---\nname: review\ndescription: Review code\n---\nFind regressions.",
    );
}

fn compile_hook_fixture(hook_dir: &Path, crate_name: &str) -> PathBuf {
    let _ = std::fs::create_dir_all(hook_dir);
    let source_path = hook_dir.join("fixture.rs");
    let artifact_path = hook_dir.join(format!("lib{crate_name}.so"));
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
output_contract = {{ mode = "optional", format = "json", schema_path = ".omega/schema/step/execute.json" }}
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

    assert!(system_logs.iter().any(|line| {
        line.contains("Hook todo_managed_execute [info] fixture before step")
    }));
    assert!(system_logs.iter().any(|line| {
        line.contains("Hook todo_managed_execute [info] fixture after step")
    }));
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
fn structured_contract_helpers_extract_embedded_json_value() {
    let step = omega_workflow::WorkflowStep {
        id: SCENE_RECOGNITION_STEP_ID.to_string(),
        label: "Scene Recognition".to_string(),
        prompt_path: PathBuf::from(".omega/prompt/step/scene-recognition.md"),
        loop_mode: StepLoopMode::AgentLoop,
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
fn render_output_contract_inlines_plan_schema_details() {
    let root = std::env::temp_dir().join("omega-agent-session-render-output-contract-test");
    let _ = std::fs::remove_dir_all(&root);
    let loaded = LoadedWorkflowCatalog::load(&root);
    assert!(loaded.warnings.is_empty());

    let workflow = loaded
        .workflow_catalog
        .workflow(RESEARCH_WORKFLOW_ID)
        .expect("research workflow should exist");
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
                    feature_plan_json()
                ))],
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
        .is_some_and(|preview| preview.contains("Implement the requested change safely")));
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
                content: vec![ContentBlock::text(feature_plan_json())],
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
        .spawn_turn_ui_compat("请你仔细帮我分析下此项目的好坏".to_string(), 43, tx)
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
        .any(|panel| panel.contains("[>] #task-1")));
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
        .spawn_turn_ui_compat("请你仔细帮我分析下此项目的好坏".to_string(), 44, tx)
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
        .any(|panel| panel.contains("[>] #task-1")));
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

    assert!(warnings.iter().any(|warning| {
        warning.contains("Step 'execute' advance denied; repeating (1/1)")
    }));
    assert!(errors.iter().any(|error| {
        error.contains("Hook-managed step failed: step 'execute' exhausted max_step_repeats=1")
    }));
    assert!(errors.iter().any(|error| {
        error.contains("Error: step 'execute' exhausted max_step_repeats=1")
    }));

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
                SCENE_RECOGNITION_STEP_ID.to_string(),
                "Scene Recognition".to_string(),
            ),
            (
                ROOT_WORKFLOW_ID.to_string(),
                WorkflowRunRole::Root,
                SELECT_WORKFLOW_STEP_ID.to_string(),
                "Select Workflow".to_string(),
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
                ROOT_WORKFLOW_ID.to_string(),
                WorkflowRunRole::Root,
                Some("feature".to_string()),
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
        .any(|line| line.contains("Selected workflow 'feature'")));
    assert!(todo_panels.iter().any(|panel| panel.contains("#task-1")));
    assert!(todo_panels.iter().any(|panel| panel.contains("#task-2")));
    let systems = client.recorded_systems();
    assert_eq!(systems.len(), 7);
    assert!(systems[0]
        .as_deref()
        .is_some_and(|system| system.contains("Workflow role: root")));
    assert!(systems[0]
        .as_deref()
        .is_some_and(|system| system.contains("Visible tools: none")));
    assert!(systems[1]
        .as_deref()
        .is_some_and(|system| system.contains("Recognized scene: feature")));
    assert!(systems[1]
        .as_deref()
        .is_some_and(|system| system.contains("Recognized scene: feature.")));
    assert!(systems[1]
        .as_deref()
        .is_some_and(|system| system.contains("Visible tools: none")));
    assert!(systems[2]
        .as_deref()
        .is_some_and(|system| system.contains("Workflow role: child")));
    assert!(systems[2]
        .as_deref()
        .is_some_and(|system| system.contains("Active workflow: feature")));
    assert!(systems[2]
        .as_deref()
        .is_some_and(|system| system.contains("Selected workflow: feature.")));
    assert!(systems[2]
        .as_deref()
        .is_some_and(|system| system.contains("hello")));
    assert!(systems
        .iter()
        .filter_map(|system| system.as_deref())
        .any(|system| system.contains("<todo_state step_id=\"execute\">")));
    assert!(systems
        .iter()
        .filter_map(|system| system.as_deref())
        .any(|system| system.contains("#task-1")));
    assert!(systems[6]
        .as_deref()
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
                SCENE_RECOGNITION_STEP_ID.to_string(),
            ),
            (
                ROOT_WORKFLOW_ID.to_string(),
                WorkflowRunRole::Root,
                SELECT_WORKFLOW_STEP_ID.to_string(),
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
    assert_eq!(systems.len(), 3);
    assert!(systems[0]
        .as_deref()
        .is_some_and(|system| system.contains("Visible tools: none")));
    assert!(systems[1]
        .as_deref()
        .is_some_and(|system| system.contains("Visible tools: none")));
    assert!(systems[2]
        .as_deref()
        .is_some_and(|system| system.contains("Active workflow: chat")));
    assert!(systems[2]
        .as_deref()
        .is_some_and(|system| system.contains("Selected workflow: chat.")));
    let max_tokens = client.recorded_max_tokens();
    assert_eq!(max_tokens, vec![24_000, 24_000, 24_000]);
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
                content: vec![ContentBlock::text(feature_plan_json())],
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
        .any(|warning| warning.contains("research-oriented")));
    assert!(routes.iter().any(|route| {
        route
            == &(
                Some("research".to_string()),
                Some(RESEARCH_WORKFLOW_ID.to_string()),
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
    assert_eq!(systems.len(), 6);
    assert!(systems[3]
        .as_deref()
        .is_some_and(|system| system.contains("second question")));
    assert!(systems[3]
        .as_deref()
        .is_some_and(|system| system.contains("first answer")));
    assert!(systems[4]
        .as_deref()
        .is_some_and(|system| system.contains("Selected workflow: chat.")));
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
                    section.metadata.workflow_id,
                    section.metadata.workflow_role,
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
        entry
            == &(
                "turn-11:root:root:scene-recognition".to_string(),
                None,
                ResponseSectionKind::Routing,
                "Scene Recognition".to_string(),
                ROOT_WORKFLOW_ID.to_string(),
                WorkflowRunRole::Root,
                None,
            )
    }));
    assert!(began.iter().any(|entry| {
        entry
            == &(
                "turn-11:root:root:select-workflow".to_string(),
                None,
                ResponseSectionKind::Routing,
                "Select Workflow".to_string(),
                ROOT_WORKFLOW_ID.to_string(),
                WorkflowRunRole::Root,
                Some("chat".to_string()),
            )
    }));
    assert!(began.iter().any(|entry| {
        entry
            == &(
                "turn-11:child:chat:chat".to_string(),
                None,
                ResponseSectionKind::FinalAnswer,
                "Final Answer".to_string(),
                CHAT_WORKFLOW_ID.to_string(),
                WorkflowRunRole::Child,
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
                CHAT_WORKFLOW_ID.to_string(),
                WorkflowRunRole::Child,
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
                id: "scene-1".to_string(),
                model: Some("test-model".to_string()),
                content: vec![ContentBlock::text("This request fits the chat scene.")],
                stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                usage: None,
            },
            ChatResponse {
                id: "scene-2".to_string(),
                model: Some("test-model".to_string()),
                content: vec![ContentBlock::text("I still think this belongs to chat.")],
                stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                usage: None,
            },
            ChatResponse {
                id: "select-1".to_string(),
                model: Some("test-model".to_string()),
                content: vec![ContentBlock::text("Use the chat workflow.")],
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
        .spawn_turn_ui_compat("分析下这个项目的优缺点".to_string(), 73, tx)
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
                began.push((section.id, section.metadata.workflow_id, section.kind));
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
        entry
            == &(
                "turn-73:child:chat:chat".to_string(),
                CHAT_WORKFLOW_ID.to_string(),
                ResponseSectionKind::FinalAnswer,
            )
    }));
    assert!(diagnostics.iter().any(|diagnostics| {
        diagnostics.step_id == SCENE_RECOGNITION_STEP_ID
            && diagnostics.output.status == StepOutputStatus::Invalid
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
                id: "scene-1".to_string(),
                model: Some("test-model".to_string()),
                content: vec![ContentBlock::text(
                    "Best match is feature.\n{\"recognized_scene_id\":\"feature\"}",
                )],
                stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                usage: None,
            },
            ChatResponse {
                id: "select-1".to_string(),
                model: Some("test-model".to_string()),
                content: vec![ContentBlock::text(
                    "Use the feature workflow.\n{\"selected_workflow_id\":\"feature\"}",
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
        diagnostics.step_id == SCENE_RECOGNITION_STEP_ID
            && diagnostics.output.status == StepOutputStatus::Valid
            && diagnostics.output.retry_count == 0
    }));
    assert!(diagnostics.iter().any(|diagnostics| {
        diagnostics.step_id == SELECT_WORKFLOW_STEP_ID
            && diagnostics.output.status == StepOutputStatus::Valid
            && diagnostics.output.retry_count == 0
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
        "turn-12:child:feature:execute"
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
                if id == "turn-12:child:feature:execute"
                    || id == "turn-12:child:feature:execute:thinking" =>
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
    let root = std::env::temp_dir().join("omega-agent-session-runtime-message-chat-test");
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::create_dir_all(&root);
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
        .spawn_turn("just chat".to_string(), 31, tx)
        .unwrap();

    let mut began = Vec::new();
    let mut appended = Vec::new();
    let finished = loop {
        match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
            RuntimeMessageEnvelope {
                turn_id,
                message: RuntimeMessage::Conversation(ConversationMessage::BeginSection { section }),
            } => {
                assert_eq!(turn_id, 31);
                began.push((section.id, section.kind));
            }
            RuntimeMessageEnvelope {
                turn_id,
                message:
                    RuntimeMessage::Conversation(ConversationMessage::AppendSection { id, delta }),
            } => {
                assert_eq!(turn_id, 31);
                appended.push((id, delta));
            }
            RuntimeMessageEnvelope {
                turn_id,
                message: RuntimeMessage::State(StateMessage::TurnFinished),
            } => {
                assert_eq!(turn_id, 31);
                break true;
            }
            _ => {}
        }
    };

    assert!(began.iter().any(|entry| {
        entry
            == &(
                "turn-31:root:root:scene-recognition".to_string(),
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
    assert!(finished);
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
    let root = std::env::temp_dir().join("omega-agent-session-runtime-message-tool-test");
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::create_dir_all(&root);
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

    session.spawn_turn("hello".to_string(), 32, tx).unwrap();

    let mut saw_tool_begin = false;
    let mut saw_tool_complete = false;
    let mut saw_tool_log = false;
    loop {
        match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
            RuntimeMessageEnvelope {
                turn_id,
                message:
                    RuntimeMessage::Conversation(ConversationMessage::BeginToolRun { tool_run }),
            } => {
                assert_eq!(turn_id, 32);
                assert_eq!(tool_run.id, "tool-1");
                saw_tool_begin = true;
            }
            RuntimeMessageEnvelope {
                turn_id,
                message:
                    RuntimeMessage::Conversation(ConversationMessage::CompleteToolRun { id, status }),
            } => {
                assert_eq!(turn_id, 32);
                assert_eq!(id, "tool-1");
                assert_eq!(status, ToolRunStatus::Complete);
                saw_tool_complete = true;
            }
            RuntimeMessageEnvelope {
                turn_id,
                message:
                    RuntimeMessage::State(StateMessage::Activity { source, kind, text, .. }),
            } => {
                assert_eq!(turn_id, 32);
                if let RuntimeSource::Tool { .. } = source {
                    if kind == RuntimeContentKind::Log && text == "$ echo hi" {
                        saw_tool_log = true;
                    }
                }
            }
            RuntimeMessageEnvelope {
                turn_id,
                message: RuntimeMessage::State(StateMessage::TurnFinished),
            } => {
                assert_eq!(turn_id, 32);
                break;
            }
            _ => {}
        }
    }

    assert!(saw_tool_begin);
    assert!(saw_tool_complete);
    assert!(saw_tool_log);
}

#[test]
fn session_tool_catalog_matches_current_default_tool_set() {
    let dispatcher = omega_core::create_default_tools(std::env::temp_dir());
    let catalog = SessionToolCatalog::new(
        dispatcher
            .tool_names()
            .into_iter()
            .map(ToOwned::to_owned)
            .collect(),
    );

    let inherit = catalog.resolve_for_step(&StepToolRequest::Inherit);
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
            "read_file",
            "todo",
            "write_file"
        ]
    );
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
            "todo",
            "write_file"
        ]
    );
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
        &StepSkillRequest::Append(vec!["docs-specs".to_string()]),
    );

    assert!(prompt.contains("Skills available:"));
    assert!(prompt.contains("review: Review code"));
    assert!(prompt.contains("Preloaded skills for this task:"));
    assert!(prompt.contains("<skill name=\"review\">"));
    assert!(prompt.contains("<skill name=\"docs-specs\">"));
}
