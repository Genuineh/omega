use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use super::{
    LoadedWorkflow, LoadedWorkflowCatalog, OutputRecoveryMode, SceneCatalog, StepInputContract,
    StepLoopContract, StepLoopMode, StepOutputContract, StepSkillRequest, StepToolRequest,
    WorkflowDefinition, WorkflowPrompts, WorkflowSource, CHAT_WORKFLOW_ID,
    DEEP_RESEARCH_SCENE_ID, DEEP_RESEARCH_WORKFLOW_ID,
    DEFAULT_EXECUTE_SCHEMA_PATH, DEFAULT_EXPLORE_SCHEMA_PATH, DEFAULT_HOOKS_DIR,
    DEFAULT_HOOK_MANIFEST_FILE, DEFAULT_PLAN_SCHEMA_PATH, DEFAULT_SCENES_PATH,
    DEFAULT_WORKFLOW_PATH, EXECUTE_STEP_ID, EXPLORE_STEP_ID, FEATURE_SCENE_ID, FEATURE_WORKFLOW_ID,
    PLAN_STEP_ID, REPORT_STEP_ID, RESEARCH_SCENE_ID, RESEARCH_WORKFLOW_ID, ROOT_WORKFLOW_ID,
    SELECT_SKILLS_STEP_ID,
};

#[test]
fn default_linear_workflow_has_four_enabled_steps() {
    let workflow = WorkflowDefinition::default_linear();

    assert_eq!(workflow.name, FEATURE_WORKFLOW_ID);
    assert_eq!(workflow.enabled_step_count(), 4);
    assert_eq!(
        workflow
            .enabled_steps()
            .map(|step| step.id.as_str())
            .collect::<Vec<_>>(),
        vec![EXPLORE_STEP_ID, "plan", EXECUTE_STEP_ID, REPORT_STEP_ID]
    );
}

#[test]
fn default_research_workflow_has_two_enabled_steps_and_reports_from_explore() {
    let workflow = WorkflowDefinition::default_research();

    assert_eq!(workflow.name, RESEARCH_WORKFLOW_ID);
    assert_eq!(workflow.enabled_step_count(), 2);
    assert_eq!(
        workflow
            .enabled_steps()
            .map(|step| step.id.as_str())
            .collect::<Vec<_>>(),
        vec![EXPLORE_STEP_ID, REPORT_STEP_ID]
    );

    let research_steps = workflow.enabled_steps().collect::<Vec<_>>();
    assert!(matches!(
        &research_steps[0].output_contract,
        StepOutputContract::Required {
            schema_path: Some(schema_path),
            recovery_mode: OutputRecoveryMode::RepairThenRegenerate,
            ..
        } if schema_path == &PathBuf::from(DEFAULT_EXPLORE_SCHEMA_PATH)
    ));
    assert!(matches!(
        &research_steps[1].input_contract,
        StepInputContract::Required { sources }
        if sources == &vec![EXPLORE_STEP_ID.to_string()]
    ));
    assert!(matches!(
        &research_steps[1].tool_request,
        StepToolRequest::Block(blocked)
        if blocked.iter().any(|item| item == "bash")
            && blocked.iter().any(|item| item == "apply_patch")
            && blocked.iter().any(|item| item == "write_file")
    ));
}

#[test]
fn default_deep_research_workflow_keeps_four_stage_read_only_flow() {
    let workflow = WorkflowDefinition::default_deep_research();

    assert_eq!(workflow.name, DEEP_RESEARCH_WORKFLOW_ID);
    assert_eq!(workflow.enabled_step_count(), 4);
    assert_eq!(
        workflow
            .enabled_steps()
            .map(|step| step.id.as_str())
            .collect::<Vec<_>>(),
        vec![EXPLORE_STEP_ID, PLAN_STEP_ID, EXECUTE_STEP_ID, REPORT_STEP_ID]
    );

    let steps = workflow.enabled_steps().collect::<Vec<_>>();
    assert!(matches!(
        &steps[2].input_contract,
        StepInputContract::Required { sources }
        if sources == &vec![PLAN_STEP_ID.to_string()]
    ));
    assert!(matches!(
        &steps[2].output_contract,
        StepOutputContract::Optional {
            schema_path: Some(schema_path),
            max_retries: 2,
            recovery_mode: OutputRecoveryMode::RepairThenRegenerate,
            ..
        } if schema_path == &PathBuf::from(DEFAULT_EXECUTE_SCHEMA_PATH)
    ));
    assert_eq!(steps[2].max_step_repeats, 8);
    assert_eq!(steps[2].hooks, vec!["todo_managed_execute".to_string()]);
    assert!(matches!(
        &steps[2].loop_contract,
        Some(StepLoopContract::TodoItems {
            source,
            child_step_prefix,
            max_item_repeats,
        }) if source == "plan.tasks" && child_step_prefix == EXECUTE_STEP_ID && *max_item_repeats == 3
    ));
}

#[test]
fn builtin_explore_and_plan_prompts_include_structured_field_guidance() {
    let prompts = WorkflowPrompts::builtin_defaults();

    assert!(prompts
        .prompt_for(EXPLORE_STEP_ID)
        .is_some_and(|prompt| prompt
            .contains("objective, key_findings, constraints, risks, and affected_paths")));
    assert!(prompts
        .prompt_for(EXPLORE_STEP_ID)
        .is_some_and(|prompt| prompt.contains("Do not conclude that the workspace is empty from a single glob result")));
    assert!(prompts
        .prompt_for(PLAN_STEP_ID)
        .is_some_and(|prompt| prompt.contains("Set `goal` to the overall outcome")));
    assert!(prompts.prompt_for(PLAN_STEP_ID).is_some_and(|prompt| prompt
        .contains("Return exactly one JSON object and nothing else.")));
    assert!(prompts
        .prompt_for(PLAN_STEP_ID)
        .is_some_and(|prompt| prompt.contains("Do not write a report")));
}

#[test]
fn missing_scene_and_workflow_catalog_is_created_and_loaded() {
    let root = unique_test_root("missing-scene-catalog");

    let loaded = LoadedWorkflowCatalog::load(&root);

    assert!(root.join(DEFAULT_SCENES_PATH).exists());
    assert!(root.join(".omega/workflows/root.toml").exists());
    assert!(root.join(".omega/workflows/chat.toml").exists());
    assert!(root.join(".omega/workflows/research.toml").exists());
    assert!(root.join(".omega/workflows/deep-research.toml").exists());
    assert!(root.join(".omega/workflows/feature.toml").exists());
    assert!(root.join(".omega/prompt/step/select-workflow.md").exists());
    assert!(root.join(".omega/prompt/step/chat.md").exists());
    assert!(root.join(".omega/prompt/step/explore.md").exists());
    assert!(root.join(DEFAULT_EXPLORE_SCHEMA_PATH).exists());
    assert!(root.join(DEFAULT_PLAN_SCHEMA_PATH).exists());
    assert!(root.join(DEFAULT_EXECUTE_SCHEMA_PATH).exists());
    assert!(loaded.warnings.is_empty());
    assert_eq!(loaded.scene_catalog.default_scene_id, FEATURE_SCENE_ID);
    assert_eq!(loaded.scene_catalog.root_workflow_id, ROOT_WORKFLOW_ID);
    assert!(loaded.scene_catalog.scene(RESEARCH_SCENE_ID).is_some());
    assert!(loaded.scene_catalog.scene(DEEP_RESEARCH_SCENE_ID).is_some());
    assert!(loaded.workflow_catalog.workflow(ROOT_WORKFLOW_ID).is_some());
    assert!(loaded.workflow_catalog.workflow(CHAT_WORKFLOW_ID).is_some());
    assert!(loaded
        .workflow_catalog
        .workflow(RESEARCH_WORKFLOW_ID)
        .is_some());
    assert!(loaded
        .workflow_catalog
        .workflow(DEEP_RESEARCH_WORKFLOW_ID)
        .is_some());
    assert!(loaded
        .workflow_catalog
        .workflow(FEATURE_WORKFLOW_ID)
        .is_some());
    assert!(loaded
        .prompt_catalog
        .prompts_for_workflow(ROOT_WORKFLOW_ID)
        .and_then(|prompts| prompts.prompt_for("select-workflow"))
        .is_some_and(|prompt| prompt.contains("recognized_scene_id")));
}

#[test]
fn workflow_definition_load_prefers_feature_workflow_catalog() {
    let root = unique_test_root("prefer-feature-catalog");
    let scenes_path = root.join(DEFAULT_SCENES_PATH);
    std::fs::create_dir_all(scenes_path.parent().unwrap()).unwrap();
    std::fs::write(&scenes_path, SceneCatalog::default_scenes_toml()).unwrap();
    std::fs::create_dir_all(root.join(".omega/workflows")).unwrap();
    std::fs::write(
        root.join(".omega/workflows/feature.toml"),
        "name = \"feature\"\n\n[[steps]]\nid = \"explore\"\nlabel = \"Scope\"\nenabled = true\n\n[[steps]]\nid = \"execute\"\nlabel = \"Ship\"\nenabled = true\n",
    )
    .unwrap();
    std::fs::write(
        root.join(DEFAULT_WORKFLOW_PATH),
        "name = \"legacy\"\n\n[[steps]]\nid = \"explore\"\nlabel = \"Legacy\"\nenabled = true\n",
    )
    .unwrap();

    let loaded = WorkflowDefinition::load(&root);

    let steps = loaded.definition.enabled_steps().collect::<Vec<_>>();
    assert_eq!(steps.len(), 2);
    assert_eq!(steps[0].label, "Scope");
    assert_eq!(steps[1].label, "Ship");
    assert!(
        matches!(loaded.source, WorkflowSource::File(path) if path.ends_with(".omega/workflows/feature.toml"))
    );
}

#[test]
fn legacy_workflow_file_is_used_for_feature_compatibility() {
    let root = unique_test_root("legacy-feature");
    let workflow_path = root.join(DEFAULT_WORKFLOW_PATH);
    std::fs::create_dir_all(workflow_path.parent().unwrap()).unwrap();
    std::fs::write(
        &workflow_path,
        "name = \"trimmed\"\n\n[[steps]]\nid = \"explore\"\nlabel = \"Scope\"\nprompt = \".omega/prompt/step/explore.md\"\nloop_mode = \"single_response\"\nmax_iterations = 5\nskill_request = { mode = \"append\", items = [\"review\"] }\nenabled = true\n\n[[steps]]\nid = \"plan\"\nenabled = false\n\n[[steps]]\nid = \"execute\"\nlabel = \"Build\"\nprompt = \".omega/prompt/step/execute.md\"\nloop_mode = \"tool_loop\"\nmax_iterations = 12\ntool_request = { mode = \"extend\", items = [\"todo\"] }\nenabled = true\n",
    )
    .unwrap();

    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    let feature = loaded_catalog
        .workflow_catalog
        .workflow(FEATURE_WORKFLOW_ID)
        .unwrap();

    assert_eq!(feature.name, "trimmed");
    assert_eq!(feature.enabled_step_count(), 2);
    assert!(
        matches!(loaded_catalog.workflow_source(FEATURE_WORKFLOW_ID), Some(WorkflowSource::File(path)) if path.ends_with(DEFAULT_WORKFLOW_PATH))
    );
}

#[test]
fn builtin_workflows_default_to_agent_loop_with_step_budgets() {
    let root = WorkflowDefinition::default_root();
    let feature = WorkflowDefinition::default_feature();

    assert!(root
        .enabled_steps()
        .all(|step| step.loop_mode == StepLoopMode::AgentLoop));
    assert!(feature
        .enabled_steps()
        .all(|step| step.loop_mode == StepLoopMode::AgentLoop));

    let root_steps = root.enabled_steps().collect::<Vec<_>>();
    assert_eq!(root_steps[0].max_iterations, 4);
    assert_eq!(
        root_steps[0].tool_request,
        StepToolRequest::Block(vec![
            "bash".to_string(),
            "batch".to_string(),
            "read_file".to_string(),
            "list_dir".to_string(),
            "glob_search".to_string(),
            "grep_search".to_string(),
            "web_search".to_string(),
            "web_fetch".to_string(),
            "apply_patch".to_string(),
            "create_file".to_string(),
            "edit_file".to_string(),
            "todo".to_string(),
            "todo_read".to_string(),
            "todo_write".to_string(),
            "ask_user_question".to_string(),
            "task".to_string(),
            "write_file".to_string(),
            "load_skill".to_string(),
            "manage_document".to_string(),
            "search_codebase".to_string(),
        ])
    );
    assert_eq!(root_steps.len(), 2);
    assert_eq!(root_steps[1].id, SELECT_SKILLS_STEP_ID);
    assert_eq!(root_steps[1].max_iterations, 4);
    assert_eq!(root_steps[1].skill_request, StepSkillRequest::Disable);

    let feature_steps = feature.enabled_steps().collect::<Vec<_>>();
    assert_eq!(
        feature_steps[0].tool_request,
        StepToolRequest::Block(vec![
            "bash".to_string(),
            "apply_patch".to_string(),
            "create_file".to_string(),
            "edit_file".to_string(),
            "todo".to_string(),
            "todo_read".to_string(),
            "todo_write".to_string(),
            "ask_user_question".to_string(),
            "write_file".to_string(),
            "manage_document".to_string(),
        ])
    );
    assert_eq!(feature_steps[2].id, EXECUTE_STEP_ID);
    assert_eq!(feature_steps[2].max_iterations, 200);
    assert_eq!(feature_steps[2].max_step_repeats, 8);
    assert_eq!(feature_steps[2].tool_request, StepToolRequest::Inherit);
    assert_eq!(
        feature_steps[2].hooks,
        vec!["todo_managed_execute".to_string()]
    );
    assert!(matches!(
        &feature_steps[2].loop_contract,
        Some(StepLoopContract::TodoItems {
            source,
            child_step_prefix,
            max_item_repeats,
        }) if source == "plan.tasks" && child_step_prefix == EXECUTE_STEP_ID && *max_item_repeats == 3
    ));
    assert!(matches!(
        &feature_steps[0].output_contract,
        StepOutputContract::Required {
            schema_path: Some(schema_path),
            recovery_mode: OutputRecoveryMode::RepairThenRegenerate,
            ..
        } if schema_path == &PathBuf::from(DEFAULT_EXPLORE_SCHEMA_PATH)
    ));
    assert!(matches!(
        &feature_steps[1].output_contract,
        StepOutputContract::Required {
            schema_path: Some(schema_path),
            recovery_mode: OutputRecoveryMode::RepairThenRegenerate,
            ..
        } if schema_path == &PathBuf::from(DEFAULT_PLAN_SCHEMA_PATH)
    ));
    assert!(matches!(
        &feature_steps[2].output_contract,
        StepOutputContract::Optional {
            schema_path: Some(schema_path),
            max_retries: 2,
            recovery_mode: OutputRecoveryMode::RepairThenRegenerate,
            ..
        } if schema_path == &PathBuf::from(DEFAULT_EXECUTE_SCHEMA_PATH)
    ));
}

#[test]
fn invalid_scene_file_falls_back_to_builtin_catalog() {
    let root = unique_test_root("invalid-scenes");
    let scenes_path = root.join(DEFAULT_SCENES_PATH);
    std::fs::create_dir_all(scenes_path.parent().unwrap()).unwrap();
    std::fs::write(
        &scenes_path,
        "root_workflow = \"root\"\ndefault_scene = \"missing\"\n\n[[scenes]]\nid = \"chat\"\nworkflow = \"chat\"\n",
    )
    .unwrap();

    let loaded = LoadedWorkflowCatalog::load(&root);

    assert_eq!(loaded.scene_catalog.default_scene_id, FEATURE_SCENE_ID);
    assert!(loaded.workflow_catalog.workflow(ROOT_WORKFLOW_ID).is_some());
    assert!(!loaded.warnings.is_empty());
}

#[test]
fn workflow_file_supports_labels_disabled_steps_and_requests() {
    let root = unique_test_root("custom-workflow");
    let workflow_path = root.join(DEFAULT_WORKFLOW_PATH);
    std::fs::create_dir_all(workflow_path.parent().unwrap()).unwrap();
    std::fs::write(
        &workflow_path,
        "name = \"trimmed\"\n\n[[steps]]\nid = \"explore\"\nlabel = \"Scope\"\nprompt = \".omega/prompt/step/explore.md\"\nloop_mode = \"agent_loop\"\nmax_iterations = 5\nskill_request = { mode = \"append\", items = [\"review\"] }\noutput_contract = { mode = \"required\", format = \"json\", max_retries = 3, recovery_mode = \"regenerate_only\" }\nenabled = true\n\n[[steps]]\nid = \"plan\"\nenabled = false\n\n[[steps]]\nid = \"execute\"\nlabel = \"Build\"\nprompt = \".omega/prompt/step/execute.md\"\nloop_mode = \"agent_loop\"\nloop_contract = { kind = \"todo_items\", source = \"plan.tasks\", child_step_prefix = \"execute\", max_item_repeats = 2 }\nmax_iterations = 12\nmax_step_repeats = 4\nhooks = [\"todo_managed_execute\", \"artifact_gate\"]\ntool_request = { mode = \"extend\", items = [\"todo\"] }\noutput_contract = { mode = \"optional\", format = \"json\", schema_path = \".omega/schema/step/execute.json\", max_retries = 4, recovery_mode = \"regenerate_only\" }\nenabled = true\n",
    )
    .unwrap();

    let loaded = WorkflowDefinition::load(&root);

    assert!(loaded.warnings.is_empty());
    assert_eq!(loaded.definition.name, "trimmed");
    assert_eq!(loaded.definition.enabled_step_count(), 2);
    let steps = loaded.definition.enabled_steps().collect::<Vec<_>>();
    assert_eq!(steps[0].id, EXPLORE_STEP_ID);
    assert_eq!(steps[0].loop_mode, StepLoopMode::AgentLoop);
    assert_eq!(steps[0].max_iterations, 5);
    assert_eq!(
        steps[0].skill_request,
        StepSkillRequest::Append(vec!["review".to_string()])
    );
    assert!(matches!(
        steps[0].output_contract,
        StepOutputContract::Required {
            max_retries: 3,
            recovery_mode: OutputRecoveryMode::RegenerateOnly,
            ..
        }
    ));
    assert_eq!(steps[1].loop_mode, StepLoopMode::AgentLoop);
    assert_eq!(steps[1].max_iterations, 12);
    assert_eq!(steps[1].max_step_repeats, 4);
    assert!(matches!(
        steps[1].output_contract,
        StepOutputContract::Optional {
            schema_path: Some(_),
            max_retries: 4,
            recovery_mode: OutputRecoveryMode::RegenerateOnly,
            ..
        }
    ));
    assert!(matches!(
        &steps[1].loop_contract,
        Some(StepLoopContract::TodoItems {
            source,
            child_step_prefix,
            max_item_repeats,
        }) if source == "plan.tasks" && child_step_prefix == EXECUTE_STEP_ID && *max_item_repeats == 2
    ));
    assert_eq!(
        steps[1].hooks,
        vec![
            "todo_managed_execute".to_string(),
            "artifact_gate".to_string(),
        ]
    );
    assert_eq!(
        steps[1].tool_request,
        StepToolRequest::Extend(vec!["todo".to_string()])
    );
    let mut run = loaded.definition.start_run();
    let first = run.current_step().unwrap();
    assert_eq!(first.id, EXPLORE_STEP_ID);
    assert_eq!(first.label, "Scope");
    assert_eq!(first.index, 1);
    assert_eq!(first.total, 2);
    let second = run.advance().unwrap();
    assert_eq!(second.id, EXECUTE_STEP_ID);
    assert_eq!(second.label, "Build");
    assert!(run.advance().is_none());
    assert!(run.current_step().is_none());
}

#[test]
fn missing_prompt_file_is_created_from_builtin_prompt() {
    let root = unique_test_root("prompt-fallback");
    let workflow_path = root.join(DEFAULT_WORKFLOW_PATH);
    std::fs::create_dir_all(workflow_path.parent().unwrap()).unwrap();
    std::fs::write(
        &workflow_path,
        "name = \"custom\"\n\n[[steps]]\nid = \"execute\"\nprompt = \".omega/prompt/step/execute.md\"\nenabled = true\n",
    )
    .unwrap();

    let loaded = WorkflowDefinition::load(&root);

    assert!(loaded
        .prompts
        .prompt_for(EXECUTE_STEP_ID)
        .is_some_and(|prompt| prompt.contains("execute phase")));
}

#[test]
fn default_feature_workflow_file_documents_hook_manifest_contract() {
    let root = unique_test_root("default-hook-contract");

    let _ = LoadedWorkflowCatalog::load(&root);

    let workflow_text =
        std::fs::read_to_string(root.join(".omega/workflows/feature.toml")).unwrap();
    assert!(workflow_text.contains("loop_contract = { kind = \"todo_items\""));
    assert!(workflow_text.contains("max_step_repeats = 8"));
    assert!(workflow_text.contains("hooks = [\"todo_managed_execute\"]"));
    assert!(workflow_text.contains(DEFAULT_HOOKS_DIR));
    assert!(workflow_text.contains(DEFAULT_HOOK_MANIFEST_FILE));
}

#[test]
fn workflow_file_rejects_invalid_hook_ids() {
    let root = unique_test_root("invalid-hook-ids");
    let workflow_path = root.join(DEFAULT_WORKFLOW_PATH);
    std::fs::create_dir_all(workflow_path.parent().unwrap()).unwrap();
    std::fs::write(
        &workflow_path,
        "name = \"bad\"\n\n[[steps]]\nid = \"execute\"\nmax_step_repeats = 2\nhooks = [\"todo_managed_execute\", \" \", \"todo_managed_execute\"]\nenabled = true\n",
    )
    .unwrap();

    let error = WorkflowDefinition::load_from_file(
        &workflow_path,
        &super::ToolPolicyConfig::builtin_default(),
    )
    .unwrap_err();
    let messages = error
        .chain()
        .map(|cause| cause.to_string())
        .collect::<Vec<_>>();
    assert!(
        messages
            .iter()
            .any(|message| message.contains("workflow step hooks cannot contain empty ids"))
            || messages.iter().any(|message| {
                message.contains("workflow step hook 'todo_managed_execute' is duplicated")
            })
    );
}

#[test]
fn legacy_loop_mode_values_map_to_agent_loop() {
    let root = unique_test_root("legacy-loop-mode-values");
    let workflow_path = root.join(DEFAULT_WORKFLOW_PATH);
    std::fs::create_dir_all(workflow_path.parent().unwrap()).unwrap();
    std::fs::write(
        &workflow_path,
        "name = \"compat\"\n\n[[steps]]\nid = \"explore\"\nloop_mode = \"single_response\"\nenabled = true\n\n[[steps]]\nid = \"execute\"\nloop_mode = \"tool_loop\"\nenabled = true\n",
    )
    .unwrap();

    let loaded = WorkflowDefinition::load(&root);
    let steps = loaded.definition.enabled_steps().collect::<Vec<_>>();

    assert_eq!(steps.len(), 2);
    assert!(steps
        .iter()
        .all(|step| step.loop_mode == StepLoopMode::AgentLoop));
    assert_eq!(steps[0].max_iterations, 200);
    assert_eq!(steps[1].max_iterations, 200);
}

#[test]
fn workflow_source_label_matches_path_or_builtin() {
    let file = LoadedWorkflow {
        definition: WorkflowDefinition::default_linear(),
        prompts: WorkflowPrompts::builtin_defaults(),
        source: WorkflowSource::File(PathBuf::from(".omega/workflow.toml")),
        warnings: Vec::new(),
    };

    assert_eq!(file.source_label(), ".omega/workflow.toml");
    assert_eq!(WorkflowSource::BuiltinDefault.source_label(), "builtin");
}

fn unique_test_root(name: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("omega-workflow-{name}-{unique}"));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}
