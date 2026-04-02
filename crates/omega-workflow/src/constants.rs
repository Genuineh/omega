pub const DEFAULT_WORKFLOW_PATH: &str = ".omega/workflow.toml";
pub const DEFAULT_SCENES_PATH: &str = ".omega/scenes.toml";
pub const DEFAULT_WORKFLOWS_DIR: &str = ".omega/workflows";
pub const DEFAULT_HOOKS_DIR: &str = ".omega/hooks";
pub const DEFAULT_HOOK_MANIFEST_FILE: &str = "Hook.toml";
pub const DEFAULT_ROOT_WORKFLOW_PATH: &str = ".omega/workflows/root.toml";
pub const DEFAULT_CHAT_WORKFLOW_PATH: &str = ".omega/workflows/chat.toml";
pub const DEFAULT_RESEARCH_WORKFLOW_PATH: &str = ".omega/workflows/research.toml";
pub const DEFAULT_DEEP_RESEARCH_WORKFLOW_PATH: &str = ".omega/workflows/deep-research.toml";
pub const DEFAULT_FEATURE_WORKFLOW_PATH: &str = ".omega/workflows/feature.toml";
pub const DEFAULT_STEP_PROMPT_DIR: &str = ".omega/prompt/step";
pub const DEFAULT_STEP_SCHEMA_DIR: &str = ".omega/schema/step";
pub const DEFAULT_MODEL_CONFIG_PATH: &str = ".omega/model.toml";

pub const ROOT_WORKFLOW_ID: &str = "root";
pub const CHAT_WORKFLOW_ID: &str = "chat";
pub const RESEARCH_WORKFLOW_ID: &str = "research";
pub const DEEP_RESEARCH_WORKFLOW_ID: &str = "deep-research";
pub const FEATURE_WORKFLOW_ID: &str = "feature";

pub const CHAT_SCENE_ID: &str = "chat";
pub const RESEARCH_SCENE_ID: &str = "research";
pub const DEEP_RESEARCH_SCENE_ID: &str = "deep-research";
pub const FEATURE_SCENE_ID: &str = "feature";

pub const SCENE_RECOGNITION_STEP_ID: &str = "scene-recognition";
pub const SELECT_WORKFLOW_STEP_ID: &str = "select-workflow";
pub const CHAT_STEP_ID: &str = "chat";
pub const EXPLORE_STEP_ID: &str = "explore";
pub const PLAN_STEP_ID: &str = "plan";
pub const EXECUTE_STEP_ID: &str = "execute";
pub const REPORT_STEP_ID: &str = "report";

pub const DEFAULT_SCENE_RECOGNITION_PROMPT_PATH: &str = ".omega/prompt/step/scene-recognition.md";
pub const DEFAULT_SELECT_WORKFLOW_PROMPT_PATH: &str = ".omega/prompt/step/select-workflow.md";
pub const DEFAULT_CHAT_PROMPT_PATH: &str = ".omega/prompt/step/chat.md";
pub const DEFAULT_EXPLORE_PROMPT_PATH: &str = ".omega/prompt/step/explore.md";
pub const DEFAULT_PLAN_PROMPT_PATH: &str = ".omega/prompt/step/plan.md";
pub const DEFAULT_EXECUTE_PROMPT_PATH: &str = ".omega/prompt/step/execute.md";
pub const DEFAULT_REPORT_PROMPT_PATH: &str = ".omega/prompt/step/report.md";
pub const DEFAULT_EXPLORE_SCHEMA_PATH: &str = ".omega/schema/step/explore.json";
pub const DEFAULT_PLAN_SCHEMA_PATH: &str = ".omega/schema/step/plan.json";
pub const DEFAULT_EXECUTE_SCHEMA_PATH: &str = ".omega/schema/step/execute.json";

pub const ROOT_ROUTING_BLOCKED_GROUP: &str = "root_routing_blocked";
pub const CHAT_BLOCKED_GROUP: &str = "chat_blocked";
pub const FEATURE_NON_EXECUTE_BLOCKED_GROUP: &str = "feature_non_execute_blocked";
