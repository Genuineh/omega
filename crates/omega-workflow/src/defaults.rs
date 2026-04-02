use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::constants::{
    CHAT_BLOCKED_GROUP, CHAT_STEP_ID, CHAT_WORKFLOW_ID, DEEP_RESEARCH_WORKFLOW_ID,
    DEFAULT_CHAT_PROMPT_PATH, DEFAULT_EXECUTE_PROMPT_PATH, DEFAULT_EXECUTE_SCHEMA_PATH,
    DEFAULT_EXPLORE_PROMPT_PATH, DEFAULT_EXPLORE_SCHEMA_PATH, DEFAULT_PLAN_PROMPT_PATH,
    DEFAULT_PLAN_SCHEMA_PATH, DEFAULT_REPORT_PROMPT_PATH,
    DEFAULT_SCENE_RECOGNITION_PROMPT_PATH, DEFAULT_SELECT_WORKFLOW_PROMPT_PATH,
    EXECUTE_STEP_ID, EXPLORE_STEP_ID, FEATURE_NON_EXECUTE_BLOCKED_GROUP,
    FEATURE_WORKFLOW_ID, PLAN_STEP_ID, REPORT_STEP_ID, RESEARCH_WORKFLOW_ID,
    ROOT_ROUTING_BLOCKED_GROUP, ROOT_WORKFLOW_ID, SCENE_RECOGNITION_STEP_ID,
    SELECT_WORKFLOW_STEP_ID,
};
use crate::model::{
    DataFormat, OutputRecoveryMode, StepInputContract, StepLoopContract, StepLoopMode,
    StepOutputContract, StepSkillRequest, StepToolRequest, WorkflowSource, WorkflowStep,
};
use crate::policy::ToolPolicyConfig;

const DEFAULT_SCENES_TOML: &str = r#"# Default omega scene routing
root_workflow = "root"
default_scene = "feature"

[[scenes]]
id = "chat"
label = "Chat"
workflow = "chat"

[[scenes]]
id = "research"
label = "Research"
workflow = "research"

[[scenes]]
id = "deep-research"
label = "Deep Research"
workflow = "deep-research"

[[scenes]]
id = "feature"
label = "Feature"
workflow = "feature"
"#;

const DEFAULT_ROOT_WORKFLOW_TOML: &str = r#"# Default root workflow
name = "root"

[[steps]]
id = "select-workflow"
label = "Select Workflow"
prompt = ".omega/prompt/step/select-workflow.md"
loop_mode = "agent_loop"
max_iterations = 4
tool_request = { mode = "block", groups = ["root_routing_blocked"] }
skill_request = { mode = "match_task" }
output_contract = { mode = "required", format = "json", max_retries = 1, recovery_mode = "repair_then_regenerate" }
enabled = true
"#;

const DEFAULT_CHAT_WORKFLOW_TOML: &str = r#"# Default chat workflow
name = "chat"

[[steps]]
id = "chat"
label = "Chat"
prompt = ".omega/prompt/step/chat.md"
loop_mode = "agent_loop"
max_iterations = 200
tool_request = { mode = "block", groups = ["chat_blocked"] }
skill_request = { mode = "match_task" }
enabled = true
"#;

const DEFAULT_RESEARCH_WORKFLOW_TOML: &str = r#"# Default research workflow
name = "research"

[[steps]]
id = "explore"
label = "Explore"
prompt = ".omega/prompt/step/explore.md"
loop_mode = "agent_loop"
max_iterations = 200
tool_request = { mode = "block", groups = ["feature_non_execute_blocked"] }
skill_request = { mode = "match_task" }
output_contract = { mode = "required", format = "json", schema_path = ".omega/schema/step/explore.json", max_retries = 2, recovery_mode = "repair_then_regenerate" }
enabled = true

[[steps]]
id = "report"
label = "Report"
prompt = ".omega/prompt/step/report.md"
loop_mode = "agent_loop"
max_iterations = 200
tool_request = { mode = "block", groups = ["feature_non_execute_blocked"] }
skill_request = { mode = "match_task" }
input_contract = { mode = "required", sources = ["explore"] }
enabled = true
"#;

const DEFAULT_DEEP_RESEARCH_WORKFLOW_TOML: &str = r#"# Default deep-research workflow
name = "deep-research"

[[steps]]
id = "explore"
label = "Explore"
prompt = ".omega/prompt/step/explore.md"
loop_mode = "agent_loop"
max_iterations = 200
tool_request = { mode = "block", groups = ["feature_non_execute_blocked"] }
skill_request = { mode = "match_task" }
output_contract = { mode = "required", format = "json", schema_path = ".omega/schema/step/explore.json", max_retries = 2, recovery_mode = "repair_then_regenerate" }
enabled = true

[[steps]]
id = "plan"
label = "Plan"
prompt = ".omega/prompt/step/plan.md"
loop_mode = "agent_loop"
max_iterations = 200
tool_request = { mode = "block", groups = ["feature_non_execute_blocked"] }
skill_request = { mode = "match_task" }
input_contract = { mode = "required", sources = ["explore"] }
output_contract = { mode = "required", format = "json", schema_path = ".omega/schema/step/plan.json", max_retries = 2, recovery_mode = "repair_then_regenerate" }
enabled = true

[[steps]]
id = "execute"
label = "Execute"
prompt = ".omega/prompt/step/execute.md"
loop_mode = "agent_loop"
loop_contract = { kind = "todo_items", source = "plan.tasks", child_step_prefix = "execute", max_item_repeats = 3 }
max_iterations = 200
# Hook manifests live under .omega/hooks/<hook-id>/Hook.toml and declare id, package, artifact, and api_version.
max_step_repeats = 8
hooks = ["todo_managed_execute"]
tool_request = { mode = "block", groups = ["feature_non_execute_blocked"] }
skill_request = { mode = "match_task" }
input_contract = { mode = "required", sources = ["plan"] }
output_contract = { mode = "required", format = "json", schema_path = ".omega/schema/step/execute.json", max_retries = 2, recovery_mode = "repair_then_regenerate" }
enabled = true

[[steps]]
id = "report"
label = "Report"
prompt = ".omega/prompt/step/report.md"
loop_mode = "agent_loop"
max_iterations = 200
tool_request = { mode = "block", groups = ["feature_non_execute_blocked"] }
skill_request = { mode = "match_task" }
input_contract = { mode = "optional", sources = ["explore", "plan", "execute"] }
enabled = true
"#;

const DEFAULT_FEATURE_WORKFLOW_TOML: &str = r#"# Default feature workflow
name = "feature"

[[steps]]
id = "explore"
label = "Explore"
prompt = ".omega/prompt/step/explore.md"
loop_mode = "agent_loop"
max_iterations = 200
tool_request = { mode = "block", groups = ["feature_non_execute_blocked"] }
skill_request = { mode = "match_task" }
output_contract = { mode = "required", format = "json", schema_path = ".omega/schema/step/explore.json", max_retries = 2, recovery_mode = "repair_then_regenerate" }
enabled = true

[[steps]]
id = "plan"
label = "Plan"
prompt = ".omega/prompt/step/plan.md"
loop_mode = "agent_loop"
max_iterations = 200
tool_request = { mode = "block", groups = ["feature_non_execute_blocked"] }
skill_request = { mode = "match_task" }
input_contract = { mode = "required", sources = ["explore"] }
output_contract = { mode = "required", format = "json", schema_path = ".omega/schema/step/plan.json", max_retries = 2, recovery_mode = "repair_then_regenerate" }
enabled = true

[[steps]]
id = "execute"
label = "Execute"
prompt = ".omega/prompt/step/execute.md"
loop_mode = "agent_loop"
loop_contract = { kind = "todo_items", source = "plan.tasks", child_step_prefix = "execute", max_item_repeats = 3 }
max_iterations = 200
# Hook manifests live under .omega/hooks/<hook-id>/Hook.toml and declare id, package, artifact, and api_version.
max_step_repeats = 8
hooks = ["todo_managed_execute"]
tool_request = { mode = "inherit" }
skill_request = { mode = "match_task" }
input_contract = { mode = "required", sources = ["plan"] }
output_contract = { mode = "required", format = "json", schema_path = ".omega/schema/step/execute.json", max_retries = 2, recovery_mode = "repair_then_regenerate" }
enabled = true

[[steps]]
id = "report"
label = "Report"
prompt = ".omega/prompt/step/report.md"
loop_mode = "agent_loop"
max_iterations = 200
tool_request = { mode = "block", groups = ["feature_non_execute_blocked"] }
skill_request = { mode = "match_task" }
input_contract = { mode = "optional", sources = ["explore", "plan", "execute"] }
enabled = true
"#;

const DEFAULT_WORKFLOW_TOML: &str = r#"# Legacy compatibility workflow
# This file is kept for backward compatibility.
# The active scene-aware config lives under .omega/scenes.toml and .omega/workflows/*.toml.
name = "feature"

[[steps]]
id = "explore"
label = "Explore"
prompt = ".omega/prompt/step/explore.md"
loop_mode = "agent_loop"
max_iterations = 200
tool_request = { mode = "block", groups = ["feature_non_execute_blocked"] }
skill_request = { mode = "match_task" }
output_contract = { mode = "required", format = "json", schema_path = ".omega/schema/step/explore.json", max_retries = 2, recovery_mode = "repair_then_regenerate" }
enabled = true

[[steps]]
id = "plan"
label = "Plan"
prompt = ".omega/prompt/step/plan.md"
loop_mode = "agent_loop"
max_iterations = 200
tool_request = { mode = "block", groups = ["feature_non_execute_blocked"] }
skill_request = { mode = "match_task" }
input_contract = { mode = "required", sources = ["explore"] }
output_contract = { mode = "required", format = "json", schema_path = ".omega/schema/step/plan.json", max_retries = 2, recovery_mode = "repair_then_regenerate" }
enabled = true

[[steps]]
id = "execute"
label = "Execute"
prompt = ".omega/prompt/step/execute.md"
loop_mode = "agent_loop"
loop_contract = { kind = "todo_items", source = "plan.tasks", child_step_prefix = "execute", max_item_repeats = 3 }
max_iterations = 200
# Hook manifests live under .omega/hooks/<hook-id>/Hook.toml and declare id, package, artifact, and api_version.
max_step_repeats = 8
hooks = ["todo_managed_execute"]
tool_request = { mode = "inherit" }
skill_request = { mode = "match_task" }
input_contract = { mode = "required", sources = ["plan"] }
output_contract = { mode = "required", format = "json", schema_path = ".omega/schema/step/execute.json", max_retries = 2, recovery_mode = "repair_then_regenerate" }
enabled = true

[[steps]]
id = "report"
label = "Report"
prompt = ".omega/prompt/step/report.md"
loop_mode = "agent_loop"
max_iterations = 200
tool_request = { mode = "block", groups = ["feature_non_execute_blocked"] }
skill_request = { mode = "match_task" }
input_contract = { mode = "optional", sources = ["explore", "plan", "execute"] }
enabled = true
"#;

const DEFAULT_SCENE_RECOGNITION_PROMPT: &str = r#"You are in the scene recognition phase.

Classify the user's request into the most appropriate work scene.
Choose `chat` only when the user is clearly asking for lightweight read-only conversation, clarification, explanation, or a simple direct answer with no requested file changes.
Choose `research` for deep, complex, or comprehensive read-only analysis, exploration, investigation, architecture study, tradeoff evaluation, or repository discovery that should stay read-only but benefits from structured analysis.
Choose `feature` for any request that asks you to implement, fix, update, edit, refactor, rename, add, remove, or otherwise change code, configs, docs, prompts, tests, or repository files.
When the request is ambiguous between `chat` and `research`, prefer `research` for substantial exploratory work.
When the request is ambiguous overall, default to `feature`, not `chat`.
Do not return `chat` or `research` for concrete delivery work, even if the request is short.
Classify from the user's request and existing routing context only.
Do not inspect repository files, list directories, or probe the workspace for this decision.
Produce only a JSON object for the next phase.
Return exactly this shape:
{"recognized_scene_id":"chat"}
or
{"recognized_scene_id":"research"}
or
{"recognized_scene_id":"feature"}
Do not wrap the JSON in markdown fences.
Do not add any extra prose.
Use tools only when they materially improve scene recognition.
Do not produce the final user-facing answer.
"#;

const DEFAULT_SELECT_WORKFLOW_PROMPT: &str = r#"You are in the workflow selection phase.

Identify the most appropriate scene and choose the workflow that should run next.
Choose `chat` only for lightweight read-only conversation, clarification, explanation, or simple direct answers with no requested repository changes.
Choose `research` for focused read-only investigation, targeted analysis, or narrower exploratory work that does not require a system-wide or deeply comprehensive study.
Choose `deep-research` for systematic, global, holistic, or deeply investigative analysis, such as comprehensive architecture studies, broad tradeoff evaluation, repo-wide discovery, or requests that explicitly ask for in-depth research.
Choose `feature` for any request that asks you to implement, fix, update, edit, refactor, rename, add, remove, or otherwise change repository files.
When the request is ambiguous between `research` and `deep-research`, prefer `deep-research` if it asks for system-level, comprehensive, global, or deeply detailed investigation; otherwise prefer `research`.
When the request is ambiguous overall, default to `feature`, not `chat`.
Choose from the user's request and existing routing context only.
Do not inspect repository files, list directories, or probe the workspace for this decision.
Produce only a JSON object for the execution handoff.
Return exactly this shape:
{"recognized_scene_id":"chat","selected_workflow_id":"chat"}
Replace the ids with the configured scene and workflow you want to start.
Do not wrap the JSON in markdown fences.
Do not add any extra prose.
Use tools only when they materially improve workflow selection.
Do not produce the final user-facing answer.
"#;

const DEFAULT_CHAT_PROMPT: &str = r#"You are in the chat workflow.

Respond conversationally and directly to the user's request.
Use a lightweight interaction style unless the conversation clearly requires a more structured execution workflow.
Do not force an explore/plan/execute/report structure when a direct answer is sufficient.
Use tools when they help you answer accurately, but avoid unnecessary tool churn.
For repository inspection questions, prefer structured read-only tools such as `list_dir`, `glob_search`, `grep_search`, `read_file`, and `batch` before reaching for `bash`.
Use `bash` only as a fallback when the structured tools cannot express the exact read-only query you need.
If you use `bash`, stick to simple single-line allowlisted commands with explicit workspace-relative paths.
Do not rely on shell expansion, redirection, or other shell-only shortcuts; those forms are blocked by the bash safety policy.
"#;

const DEFAULT_EXPLORE_PROMPT: &str = r#"You are in the explore phase.

Explore the project before planning so the next step has accurate, decision-useful context.
Inspect the relevant code, configs, prompts, docs, and tests to understand scope, constraints, affected areas, and likely risks.
Extract the key findings that should shape the plan instead of jumping straight into task decomposition.
Use tools when they materially improve the exploration.
Prefer structured read-only tools for repository inspection. Avoid `bash` unless the structured tool surface is insufficient for the exact read-only check you need.
Do not conclude that the workspace is empty from a single glob result, a partial truncated listing, or one failed read.
Before claiming the workspace or project is empty, verify the top-level workspace contents directly with `list_dir` on `.` or an equivalent direct root inspection.
Do not produce the final user-facing answer.
Produce only the internal exploration result needed for the next phase.
Capture the work in structured form: objective, key_findings, constraints, risks, and affected_paths.
Keep entries concrete and concise. Prefer repository-relative paths in affected_paths.
"#;

const DEFAULT_PLAN_PROMPT: &str = r#"You are in the planning phase.

Turn the exploration into a concrete execution plan with ordered steps and validation targets.
Use tools when they materially improve the plan.
Do not produce the final user-facing answer.
Produce only the internal plan needed for execution.
Return exactly one JSON object and nothing else.
Do not include markdown headings, narrative explanation, report prose, summaries, or code fences before or after the JSON.
Do not write a report, recommendation section, or final wrap-up.
The plan must be directly mappable to the todo system.
Use the explore context, especially key findings, risks, constraints, and affected paths, to decide what should happen next.
Each task should be actionable, ordered, and small enough to complete or validate in one execution slice.
If the active workflow is read-only or the visible tools do not include write/edit tools, every task must stay read-only: inspect, compare, validate, summarize, or gather evidence.
In read-only workflows, do not ask execute to edit files, create patches, update docs, modify code, or run other write-capable actions.
In read-only workflows, keep validation_targets satisfiable with the visible read-only tools.
Set `goal` to the overall outcome, `tasks` to the ordered worklist, and `validation_targets` to the checks execute/report should verify.
"#;

const DEFAULT_EXECUTE_PROMPT: &str = r#"You are in the execute phase.

Carry out the plan. Use tools when needed. Make concrete progress and run validation when appropriate.
Do not produce the final user-facing wrap-up yet.
Leave the final summary for the report phase.
Treat the todo list as the execution anchor.
Focus first on the current in-progress item, keep todo state aligned as work advances, and use validation_targets from the plan when verifying changes.
If no todo list is present, use the structured explore findings as the execution anchor and keep completed_tasks/open_tasks empty unless the workflow explicitly supplied task ids.
In itemized execute loops, only mark the current todo item as newly completed in completed_tasks; never mark future items complete before their own execute slice runs.
If this workflow is read-only, gather evidence instead of editing files and leave changed_paths empty when no workspace files changed.
In a read-only workflow, mark the current todo item as completed once you have gathered the requested evidence or finished the read-only validation for that item.
Do not keep a read-only todo item open merely because no files changed.
Prefer `apply_patch` and `create_file` for workspace edits, and use structured read-only tools for inspection. Use `bash` mainly for validation commands or gaps the structured tools do not cover.
When the step has a JSON execute contract, emit that JSON object and include completed_tasks, open_tasks, validation_results, and changed_paths for the current item.
"#;

const DEFAULT_REPORT_PROMPT: &str = r#"You are in the report phase.

Based on the completed work and existing transcript, produce the final user-facing response.
Use tools only when they materially improve the final report.
Summarize what changed, what was verified, and any remaining risks or follow-up.
Use the structured explore, plan, execute outputs, and current todo state when available.
"#;

const DEFAULT_EXPLORE_SCHEMA: &str = r#"{
    "type": "object",
    "required": ["objective", "key_findings", "constraints", "risks", "affected_paths"],
    "properties": {
        "objective": { "type": "string" },
        "key_findings": {
            "type": "array",
            "items": { "type": "string" }
        },
        "constraints": {
            "type": "array",
            "items": { "type": "string" }
        },
        "risks": {
            "type": "array",
            "items": { "type": "string" }
        },
        "affected_paths": {
            "type": "array",
            "items": { "type": "string" }
        }
    }
}"#;

const DEFAULT_PLAN_SCHEMA: &str = r#"{
    "type": "object",
    "required": ["goal", "tasks", "validation_targets"],
    "properties": {
        "goal": { "type": "string" },
        "tasks": {
            "type": "array",
            "items": {
                "type": "object",
                "required": ["id", "title", "description"],
                "properties": {
                    "id": { "type": "string" },
                    "title": { "type": "string" },
                    "description": { "type": "string" }
                }
            }
        },
        "validation_targets": {
            "type": "array",
            "items": { "type": "string" }
        }
    }
}"#;

const DEFAULT_EXECUTE_SCHEMA: &str = r#"{
    "type": "object",
    "required": ["completed_tasks", "open_tasks", "validation_results", "changed_paths"],
    "properties": {
        "completed_tasks": {
            "type": "array",
            "items": { "type": "string" }
        },
        "open_tasks": {
            "type": "array",
            "items": { "type": "string" }
        },
        "validation_results": {
            "type": "array",
            "items": {
                "type": "object",
                "required": ["target", "status"],
                "properties": {
                    "target": { "type": "string" },
                    "status": { "type": "string" },
                    "details": { "type": "string" }
                }
            }
        },
        "changed_paths": {
            "type": "array",
            "items": { "type": "string" }
        }
    }
}"#;

pub(crate) fn default_scenes_toml() -> &'static str {
    DEFAULT_SCENES_TOML
}

pub(crate) fn default_workflow_toml() -> &'static str {
    DEFAULT_WORKFLOW_TOML
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum BuiltinWorkflowStepId {
    SceneRecognition,
    SelectWorkflow,
    Chat,
    Explore,
    Plan,
    Execute,
    Report,
}

impl BuiltinWorkflowStepId {
    pub(crate) fn all() -> [Self; 7] {
        [
            Self::SceneRecognition,
            Self::SelectWorkflow,
            Self::Chat,
            Self::Explore,
            Self::Plan,
            Self::Execute,
            Self::Report,
        ]
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::SceneRecognition => SCENE_RECOGNITION_STEP_ID,
            Self::SelectWorkflow => SELECT_WORKFLOW_STEP_ID,
            Self::Chat => CHAT_STEP_ID,
            Self::Explore => EXPLORE_STEP_ID,
            Self::Plan => PLAN_STEP_ID,
            Self::Execute => EXECUTE_STEP_ID,
            Self::Report => REPORT_STEP_ID,
        }
    }

    pub(crate) fn default_label(self) -> &'static str {
        match self {
            Self::SceneRecognition => "Scene Recognition",
            Self::SelectWorkflow => "Select Workflow",
            Self::Chat => "Chat",
            Self::Explore => "Explore",
            Self::Plan => "Plan",
            Self::Execute => "Execute",
            Self::Report => "Report",
        }
    }

    pub(crate) fn default_prompt_path(self) -> &'static str {
        match self {
            Self::SceneRecognition => DEFAULT_SCENE_RECOGNITION_PROMPT_PATH,
            Self::SelectWorkflow => DEFAULT_SELECT_WORKFLOW_PROMPT_PATH,
            Self::Chat => DEFAULT_CHAT_PROMPT_PATH,
            Self::Explore => DEFAULT_EXPLORE_PROMPT_PATH,
            Self::Plan => DEFAULT_PLAN_PROMPT_PATH,
            Self::Execute => DEFAULT_EXECUTE_PROMPT_PATH,
            Self::Report => DEFAULT_REPORT_PROMPT_PATH,
        }
    }

    pub(crate) fn default_prompt_content(self) -> &'static str {
        match self {
            Self::SceneRecognition => DEFAULT_SCENE_RECOGNITION_PROMPT,
            Self::SelectWorkflow => DEFAULT_SELECT_WORKFLOW_PROMPT,
            Self::Chat => DEFAULT_CHAT_PROMPT,
            Self::Explore => DEFAULT_EXPLORE_PROMPT,
            Self::Plan => DEFAULT_PLAN_PROMPT,
            Self::Execute => DEFAULT_EXECUTE_PROMPT,
            Self::Report => DEFAULT_REPORT_PROMPT,
        }
    }

    pub(crate) fn default_loop_mode(self) -> StepLoopMode {
        StepLoopMode::AgentLoop
    }

    pub(crate) fn default_loop_contract(self) -> Option<StepLoopContract> {
        match self {
            Self::Execute => Some(StepLoopContract::TodoItems {
                source: "plan.tasks".to_string(),
                child_step_prefix: EXECUTE_STEP_ID.to_string(),
                max_item_repeats: 3,
            }),
            Self::SceneRecognition
            | Self::SelectWorkflow
            | Self::Chat
            | Self::Explore
            | Self::Plan
            | Self::Report => None,
        }
    }

    pub(crate) fn default_max_iterations(self) -> u32 {
        match self {
            Self::SceneRecognition | Self::SelectWorkflow => 4,
            Self::Chat | Self::Explore | Self::Plan | Self::Execute | Self::Report => 200,
        }
    }

    pub(crate) fn default_max_step_repeats(self) -> u32 {
        match self {
            Self::Execute => 8,
            Self::SceneRecognition
            | Self::SelectWorkflow
            | Self::Chat
            | Self::Explore
            | Self::Plan
            | Self::Report => 0,
        }
    }

    pub(crate) fn default_hooks(self) -> Vec<String> {
        match self {
            Self::Execute => vec!["todo_managed_execute".to_string()],
            Self::SceneRecognition
            | Self::SelectWorkflow
            | Self::Chat
            | Self::Explore
            | Self::Plan
            | Self::Report => Vec::new(),
        }
    }

    pub(crate) fn default_tool_request(self, tool_policy: &ToolPolicyConfig) -> StepToolRequest {
        match self {
            Self::Execute => StepToolRequest::Inherit,
            Self::SceneRecognition | Self::SelectWorkflow => StepToolRequest::Block(
                tool_policy
                    .group_items(ROOT_ROUTING_BLOCKED_GROUP)
                    .unwrap_or(&[])
                    .to_vec(),
            ),
            Self::Chat => StepToolRequest::Block(
                tool_policy
                    .group_items(CHAT_BLOCKED_GROUP)
                    .unwrap_or(&[])
                    .to_vec(),
            ),
            Self::Explore | Self::Plan | Self::Report => StepToolRequest::Block(
                tool_policy
                    .group_items(FEATURE_NON_EXECUTE_BLOCKED_GROUP)
                    .unwrap_or(&[])
                    .to_vec(),
            ),
        }
    }

    pub(crate) fn default_skill_request(self) -> StepSkillRequest {
        StepSkillRequest::MatchTask
    }

    pub(crate) fn default_input_contract(self) -> StepInputContract {
        match self {
            Self::Plan => StepInputContract::Required {
                sources: vec![EXPLORE_STEP_ID.to_string()],
            },
            Self::Execute => StepInputContract::Required {
                sources: vec![PLAN_STEP_ID.to_string()],
            },
            Self::Report => StepInputContract::Optional {
                sources: vec![
                    EXPLORE_STEP_ID.to_string(),
                    PLAN_STEP_ID.to_string(),
                    EXECUTE_STEP_ID.to_string(),
                ],
            },
            Self::SceneRecognition | Self::SelectWorkflow | Self::Chat | Self::Explore => {
                StepInputContract::None
            }
        }
    }

    pub(crate) fn default_output_contract(self) -> StepOutputContract {
        match self {
            Self::SceneRecognition | Self::SelectWorkflow => StepOutputContract::Required {
                format: DataFormat::Json,
                schema_path: None,
                max_retries: 1,
                recovery_mode: OutputRecoveryMode::RepairThenRegenerate,
            },
            Self::Explore => StepOutputContract::Required {
                format: DataFormat::Json,
                schema_path: Some(PathBuf::from(DEFAULT_EXPLORE_SCHEMA_PATH)),
                max_retries: 2,
                recovery_mode: OutputRecoveryMode::RepairThenRegenerate,
            },
            Self::Plan => StepOutputContract::Required {
                format: DataFormat::Json,
                schema_path: Some(PathBuf::from(DEFAULT_PLAN_SCHEMA_PATH)),
                max_retries: 2,
                recovery_mode: OutputRecoveryMode::RepairThenRegenerate,
            },
            Self::Execute => StepOutputContract::Optional {
                format: DataFormat::Json,
                schema_path: Some(PathBuf::from(DEFAULT_EXECUTE_SCHEMA_PATH)),
                max_retries: 2,
                recovery_mode: OutputRecoveryMode::RepairThenRegenerate,
            },
            Self::Chat | Self::Report => StepOutputContract::None,
        }
    }
}

impl WorkflowStep {
    pub(crate) fn from_builtin_with_tool_policy(
        step: BuiltinWorkflowStepId,
        tool_policy: &ToolPolicyConfig,
    ) -> Self {
        Self {
            id: step.as_str().to_string(),
            label: step.default_label().to_string(),
            prompt_path: PathBuf::from(step.default_prompt_path()),
            loop_mode: step.default_loop_mode(),
            loop_contract: step.default_loop_contract(),
            max_iterations: step.default_max_iterations(),
            max_step_repeats: step.default_max_step_repeats(),
            hooks: step.default_hooks(),
            tool_request: step.default_tool_request(tool_policy),
            skill_request: step.default_skill_request(),
            input_contract: step.default_input_contract(),
            output_contract: step.default_output_contract(),
            enabled: true,
        }
    }
}

pub(crate) fn builtin_step_for_id(step_id: &str) -> Option<BuiltinWorkflowStepId> {
    match step_id {
        SCENE_RECOGNITION_STEP_ID => Some(BuiltinWorkflowStepId::SceneRecognition),
        SELECT_WORKFLOW_STEP_ID => Some(BuiltinWorkflowStepId::SelectWorkflow),
        CHAT_STEP_ID => Some(BuiltinWorkflowStepId::Chat),
        EXPLORE_STEP_ID => Some(BuiltinWorkflowStepId::Explore),
        PLAN_STEP_ID => Some(BuiltinWorkflowStepId::Plan),
        EXECUTE_STEP_ID => Some(BuiltinWorkflowStepId::Execute),
        REPORT_STEP_ID => Some(BuiltinWorkflowStepId::Report),
        _ => None,
    }
}

pub(crate) fn default_workflow_toml_for_id(workflow_id: &str) -> Option<&'static str> {
    match workflow_id {
        ROOT_WORKFLOW_ID => Some(DEFAULT_ROOT_WORKFLOW_TOML),
        CHAT_WORKFLOW_ID => Some(DEFAULT_CHAT_WORKFLOW_TOML),
        RESEARCH_WORKFLOW_ID => Some(DEFAULT_RESEARCH_WORKFLOW_TOML),
        DEEP_RESEARCH_WORKFLOW_ID => Some(DEFAULT_DEEP_RESEARCH_WORKFLOW_TOML),
        FEATURE_WORKFLOW_ID => Some(DEFAULT_FEATURE_WORKFLOW_TOML),
        _ => None,
    }
}

pub(crate) fn builtin_schema_content_for_path(schema_path: &Path) -> Option<&'static str> {
    match schema_path.to_string_lossy().as_ref() {
        DEFAULT_EXPLORE_SCHEMA_PATH => Some(DEFAULT_EXPLORE_SCHEMA),
        DEFAULT_PLAN_SCHEMA_PATH => Some(DEFAULT_PLAN_SCHEMA),
        DEFAULT_EXECUTE_SCHEMA_PATH => Some(DEFAULT_EXECUTE_SCHEMA),
        _ => None,
    }
}

pub(crate) fn builtin_workflow_sources() -> BTreeMap<String, WorkflowSource> {
    let mut sources = BTreeMap::new();
    sources.insert(ROOT_WORKFLOW_ID.to_string(), WorkflowSource::BuiltinDefault);
    sources.insert(CHAT_WORKFLOW_ID.to_string(), WorkflowSource::BuiltinDefault);
    sources.insert(
        RESEARCH_WORKFLOW_ID.to_string(),
        WorkflowSource::BuiltinDefault,
    );
    sources.insert(
        DEEP_RESEARCH_WORKFLOW_ID.to_string(),
        WorkflowSource::BuiltinDefault,
    );
    sources.insert(
        FEATURE_WORKFLOW_ID.to_string(),
        WorkflowSource::BuiltinDefault,
    );
    sources
}
