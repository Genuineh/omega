use std::path::{Path, PathBuf};
use std::process::Command;

use omega_hooks::{
    HookAdvanceOutcome, HookDispatchInput, HookEventKind, HookHost, HookSessionContextSnapshot,
    HookStepKey, HookTodoSnapshot, HookWorkflowRole, DEFAULT_HOOKS_DIR, DEFAULT_HOOK_MANIFEST_FILE,
};
use omega_hpc_paths::OmegaProjectLayout;
use omega_test_support::persistent_test_root;

#[test]
fn hook_host_loads_dynamic_fixture_and_preserves_storage_until_after_step() {
    let root = unique_test_root("hook-host");
    let hook_dir = root.join(DEFAULT_HOOKS_DIR).join("todo_managed_execute");
    let artifact_path = compile_fixture_hook(&root, &hook_dir, "todo_managed_execute");
    std::fs::write(
        hook_dir.join(DEFAULT_HOOK_MANIFEST_FILE),
        format!(
            "id = \"todo_managed_execute\"\npackage = \"todo_managed_execute\"\nartifact = \"{}\"\napi_version = 1\n",
            artifact_path.file_name().unwrap().to_string_lossy()
        ),
    )
    .unwrap();

    let host = HookHost::load(&root).unwrap();
    let mut session = host.start_session();
    let step_key = HookStepKey {
        workflow_id: "feature".to_string(),
        workflow_role: HookWorkflowRole::Child,
        step_id: "execute".to_string(),
    };
    assert!(session.activate_step(step_key.clone()));

    let before = host
        .dispatch(
            &mut session,
            &step_key,
            &["todo_managed_execute".to_string()],
            base_input(HookEventKind::BeforeStep),
        )
        .unwrap();
    assert!(before
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message.contains("fixture before step")));
    assert_eq!(
        session
            .hook_storage(&step_key, "todo_managed_execute")
            .and_then(|storage| storage.get("seen"))
            .and_then(|value| value.as_i64()),
        Some(1)
    );

    let advance = host
        .dispatch(
            &mut session,
            &step_key,
            &["todo_managed_execute".to_string()],
            HookDispatchInput {
                event: HookEventKind::BeforeAdvance,
                todo: HookTodoSnapshot {
                    rendered: Some("[ ] #task-1".to_string()),
                    has_open_items: true,
                    rounds_without_update: 0,
                },
                ..base_input(HookEventKind::BeforeAdvance)
            },
        )
        .unwrap();
    assert!(matches!(advance.advance, HookAdvanceOutcome::Deny { .. }));
    assert_eq!(
        session
            .hook_storage(&step_key, "todo_managed_execute")
            .and_then(|storage| storage.get("seen"))
            .and_then(|value| value.as_i64()),
        Some(3)
    );

    let after = host
        .dispatch(
            &mut session,
            &step_key,
            &["todo_managed_execute".to_string()],
            HookDispatchInput {
                event: HookEventKind::AfterStep,
                final_text: Some("done".to_string()),
                ..base_input(HookEventKind::AfterStep)
            },
        )
        .unwrap();
    assert!(after
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message.contains("fixture after step")));
    assert!(!session.is_step_active(&step_key));
    assert!(session
        .hook_storage(&step_key, "todo_managed_execute")
        .is_none());
}

#[test]
fn hook_host_errors_when_manifest_is_missing() {
    let root = unique_test_root("missing-hook");
    let host = HookHost::load(&root).unwrap();
    let mut session = host.start_session();
    let step_key = HookStepKey {
        workflow_id: "feature".to_string(),
        workflow_role: HookWorkflowRole::Child,
        step_id: "execute".to_string(),
    };

    let error = host
        .dispatch(
            &mut session,
            &step_key,
            &["missing_hook".to_string()],
            base_input(HookEventKind::BeforeStep),
        )
        .unwrap_err()
        .to_string();
    assert!(error.contains("missing hook manifest"));
}

#[test]
fn hook_host_dispatches_builtin_todo_managed_execute_without_manifest() {
    let root = unique_test_root("builtin-hook");
    let host = HookHost::load(&root).unwrap();
    let mut session = host.start_session();
    let step_key = HookStepKey {
        workflow_id: "feature".to_string(),
        workflow_role: HookWorkflowRole::Child,
        step_id: "execute".to_string(),
    };
    assert!(session.activate_step(step_key.clone()));

    let summary = host
        .dispatch(
            &mut session,
            &step_key,
            &["todo_managed_execute".to_string()],
            HookDispatchInput {
                event: HookEventKind::BeforeAdvance,
                todo: HookTodoSnapshot {
                    rendered: Some("[ ] #task-1".to_string()),
                    has_open_items: true,
                    rounds_without_update: 2,
                },
                ..base_input(HookEventKind::BeforeAdvance)
            },
        )
        .unwrap();

    assert!(matches!(summary.advance, HookAdvanceOutcome::Deny { .. }));
}

fn base_input(event: HookEventKind) -> HookDispatchInput {
    HookDispatchInput {
        event,
        workflow_id: "feature".to_string(),
        workflow_role: HookWorkflowRole::Child,
        step_id: "execute".to_string(),
        step_label: "Execute".to_string(),
        step_index: 3,
        step_total: 4,
        current_item_id: None,
        item_index: None,
        item_total: None,
        visible_tools: vec!["bash".to_string(), "todo".to_string()],
        structured_input: None,
        structured_output: None,
        final_text: None,
        error: None,
        tool_call: None,
        todo: HookTodoSnapshot {
            rendered: None,
            has_open_items: false,
            rounds_without_update: 0,
        },
        session_context: HookSessionContextSnapshot {
            latest_user_turn: "fix this bug".to_string(),
            recognized_scene_id: Some("feature".to_string()),
            selected_workflow_id: Some("feature".to_string()),
            active_workflow_id: "feature".to_string(),
            active_workflow_role: HookWorkflowRole::Child,
            step_summaries: Vec::new(),
            step_outputs: Default::default(),
        },
        storage: Default::default(),
    }
}

fn compile_fixture_hook(root: &Path, hook_dir: &Path, crate_name: &str) -> PathBuf {
    std::fs::create_dir_all(hook_dir).unwrap();
    let source_path = hook_dir.join("fixture.rs");
    let artifact_dir = OmegaProjectLayout::new(root.to_path_buf())
        .hook_artifacts_dir()
        .join(crate_name);
    std::fs::create_dir_all(&artifact_dir).unwrap();
    let artifact_path = artifact_dir.join(format!("lib{crate_name}.so"));
    std::fs::write(&source_path, fixture_source()).unwrap();

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
    assert!(status.success(), "failed to compile fixture hook");

    artifact_path
}

fn fixture_source() -> &'static str {
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
    } else if input.contains("\"event\":\"before_advance\"") && input.contains("\"has_open_items\":true") {
        "{\"diagnostics\":[{\"level\":\"warning\",\"message\":\"todo still open\"}],\"storage\":{\"seen\":3},\"advance\":{\"kind\":\"deny\",\"reason\":\"todo remains open\"}}".to_string()
    } else if input.contains("\"event\":\"after_step\"") && input.contains("\"seen\":3") {
        "{\"diagnostics\":[{\"level\":\"info\",\"message\":\"fixture after step\"}],\"storage\":{}}".to_string()
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

fn unique_test_root(name: &str) -> PathBuf {
    persistent_test_root(&format!("hooks-{name}"))
}
