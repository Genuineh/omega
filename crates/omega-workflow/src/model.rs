use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use crate::constants::{
    CHAT_SCENE_ID, CHAT_WORKFLOW_ID, DEEP_RESEARCH_SCENE_ID, DEEP_RESEARCH_WORKFLOW_ID,
    FEATURE_NON_EXECUTE_BLOCKED_GROUP, FEATURE_SCENE_ID, FEATURE_WORKFLOW_ID,
    RESEARCH_SCENE_ID, RESEARCH_WORKFLOW_ID, ROOT_WORKFLOW_ID,
};
use crate::defaults::{default_scenes_toml, default_workflow_toml, BuiltinWorkflowStepId};
use crate::policy::ToolPolicyConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StepLoopMode {
    #[default]
    AgentLoop,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepLoopContract {
    TodoItems {
        source: String,
        child_step_prefix: String,
        max_item_repeats: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum StepToolRequest {
    #[default]
    Inherit,
    Extend(Vec<String>),
    Block(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum StepSkillRequest {
    #[default]
    MatchTask,
    Append(Vec<String>),
    Disable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DataFormat {
    #[default]
    Json,
}

impl DataFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Json => "json",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum StepInputContract {
    #[default]
    None,
    Required {
        sources: Vec<String>,
    },
    Optional {
        sources: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum StepOutputContract {
    #[default]
    None,
    Required {
        format: DataFormat,
        schema_path: Option<PathBuf>,
        max_retries: u32,
        recovery_mode: OutputRecoveryMode,
    },
    Optional {
        format: DataFormat,
        schema_path: Option<PathBuf>,
        max_retries: u32,
        recovery_mode: OutputRecoveryMode,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputRecoveryMode {
    RegenerateOnly,
    #[default]
    RepairThenRegenerate,
}

impl OutputRecoveryMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RegenerateOnly => "regenerate_only",
            Self::RepairThenRegenerate => "repair_then_regenerate",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneDefinition {
    pub id: String,
    pub label: String,
    pub workflow_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneCatalog {
    pub root_workflow_id: String,
    pub default_scene_id: String,
    pub scenes: Vec<SceneDefinition>,
}

impl SceneCatalog {
    pub fn default_builtin() -> Self {
        Self {
            root_workflow_id: ROOT_WORKFLOW_ID.to_string(),
            default_scene_id: FEATURE_SCENE_ID.to_string(),
            scenes: vec![
                SceneDefinition {
                    id: CHAT_SCENE_ID.to_string(),
                    label: "Chat".to_string(),
                    workflow_id: CHAT_WORKFLOW_ID.to_string(),
                },
                SceneDefinition {
                    id: RESEARCH_SCENE_ID.to_string(),
                    label: "Research".to_string(),
                    workflow_id: RESEARCH_WORKFLOW_ID.to_string(),
                },
                SceneDefinition {
                    id: DEEP_RESEARCH_SCENE_ID.to_string(),
                    label: "Deep Research".to_string(),
                    workflow_id: DEEP_RESEARCH_WORKFLOW_ID.to_string(),
                },
                SceneDefinition {
                    id: FEATURE_SCENE_ID.to_string(),
                    label: "Feature".to_string(),
                    workflow_id: FEATURE_WORKFLOW_ID.to_string(),
                },
            ],
        }
    }

    pub fn default_scenes_toml() -> &'static str {
        default_scenes_toml()
    }

    pub fn scene(&self, scene_id: &str) -> Option<&SceneDefinition> {
        self.scenes.iter().find(|scene| scene.id == scene_id)
    }

    pub fn referenced_workflow_ids(&self) -> BTreeSet<String> {
        let mut ids = BTreeSet::new();
        ids.insert(self.root_workflow_id.clone());
        for scene in &self.scenes {
            ids.insert(scene.workflow_id.clone());
        }
        ids
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowPrompts {
    pub(crate) prompts: BTreeMap<String, String>,
}

impl WorkflowPrompts {
    pub fn builtin_defaults() -> Self {
        let mut prompts = BTreeMap::new();
        for step in BuiltinWorkflowStepId::all() {
            prompts.insert(
                step.as_str().to_string(),
                step.default_prompt_content().to_string(),
            );
        }
        Self { prompts }
    }

    pub fn prompt_for(&self, step_id: &str) -> Option<&str> {
        self.prompts.get(step_id).map(String::as_str)
    }

    pub fn step_ids(&self) -> Vec<&str> {
        self.prompts.keys().map(String::as_str).collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowStep {
    pub id: String,
    pub label: String,
    pub prompt_path: PathBuf,
    pub loop_mode: StepLoopMode,
    pub loop_contract: Option<StepLoopContract>,
    pub max_iterations: u32,
    pub max_step_repeats: u32,
    pub hooks: Vec<String>,
    pub tool_request: StepToolRequest,
    pub skill_request: StepSkillRequest,
    pub input_contract: StepInputContract,
    pub output_contract: StepOutputContract,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowDefinition {
    pub name: String,
    pub steps: Vec<WorkflowStep>,
}

impl WorkflowDefinition {
    pub fn default_linear() -> Self {
        Self::default_feature()
    }

    pub fn default_root() -> Self {
        Self::default_root_with_tool_policy(&ToolPolicyConfig::builtin_default())
    }

    pub fn default_root_with_tool_policy(tool_policy: &ToolPolicyConfig) -> Self {
        Self {
            name: ROOT_WORKFLOW_ID.to_string(),
            steps: [
                BuiltinWorkflowStepId::SelectWorkflow,
                BuiltinWorkflowStepId::SelectSkills,
            ]
            .into_iter()
            .map(|step| WorkflowStep::from_builtin_with_tool_policy(step, tool_policy))
            .collect(),
        }
    }

    pub fn default_chat() -> Self {
        Self::default_chat_with_tool_policy(&ToolPolicyConfig::builtin_default())
    }

    pub fn default_chat_with_tool_policy(tool_policy: &ToolPolicyConfig) -> Self {
        Self {
            name: CHAT_WORKFLOW_ID.to_string(),
            steps: [BuiltinWorkflowStepId::Chat]
                .into_iter()
                .map(|step| WorkflowStep::from_builtin_with_tool_policy(step, tool_policy))
                .collect(),
        }
    }

    pub fn default_feature() -> Self {
        Self::default_feature_with_tool_policy(&ToolPolicyConfig::builtin_default())
    }

    pub fn default_research() -> Self {
        Self::default_research_with_tool_policy(&ToolPolicyConfig::builtin_default())
    }

    pub fn default_research_with_tool_policy(tool_policy: &ToolPolicyConfig) -> Self {
        let mut steps = [
            BuiltinWorkflowStepId::Explore,
            BuiltinWorkflowStepId::Report,
        ]
        .into_iter()
        .map(|step| WorkflowStep::from_builtin_with_tool_policy(step, tool_policy))
        .collect::<Vec<_>>();

        if let Some(report_step) = steps
            .iter_mut()
            .find(|step| step.id == crate::REPORT_STEP_ID)
        {
            report_step.input_contract = StepInputContract::Required {
                sources: vec![crate::EXPLORE_STEP_ID.to_string()],
            };
        }

        Self {
            name: RESEARCH_WORKFLOW_ID.to_string(),
            steps,
        }
    }

    pub fn default_deep_research() -> Self {
        Self::default_deep_research_with_tool_policy(&ToolPolicyConfig::builtin_default())
    }

    pub fn default_deep_research_with_tool_policy(tool_policy: &ToolPolicyConfig) -> Self {
        let mut steps = [
            BuiltinWorkflowStepId::Explore,
            BuiltinWorkflowStepId::Plan,
            BuiltinWorkflowStepId::Execute,
            BuiltinWorkflowStepId::Report,
        ]
        .into_iter()
        .map(|step| WorkflowStep::from_builtin_with_tool_policy(step, tool_policy))
        .collect::<Vec<_>>();

        if let Some(execute_step) = steps
            .iter_mut()
            .find(|step| step.id == crate::EXECUTE_STEP_ID)
        {
            execute_step.tool_request = StepToolRequest::Block(
                tool_policy
                    .group_items(FEATURE_NON_EXECUTE_BLOCKED_GROUP)
                    .unwrap_or(&[])
                    .to_vec(),
            );
        }

        Self {
            name: DEEP_RESEARCH_WORKFLOW_ID.to_string(),
            steps,
        }
    }

    pub fn default_feature_with_tool_policy(tool_policy: &ToolPolicyConfig) -> Self {
        Self {
            name: FEATURE_WORKFLOW_ID.to_string(),
            steps: [
                BuiltinWorkflowStepId::Explore,
                BuiltinWorkflowStepId::Plan,
                BuiltinWorkflowStepId::Execute,
                BuiltinWorkflowStepId::Report,
            ]
            .into_iter()
            .map(|step| WorkflowStep::from_builtin_with_tool_policy(step, tool_policy))
            .collect(),
        }
    }

    pub fn default_workflow_toml() -> &'static str {
        default_workflow_toml()
    }

    pub fn enabled_steps(&self) -> impl Iterator<Item = &WorkflowStep> {
        self.steps.iter().filter(|step| step.enabled)
    }

    pub fn enabled_step_count(&self) -> usize {
        self.enabled_steps().count()
    }

    pub fn start_run(&self) -> WorkflowRun {
        WorkflowRun::new(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowCatalog {
    pub(crate) workflows: BTreeMap<String, WorkflowDefinition>,
}

impl WorkflowCatalog {
    pub fn default_builtin() -> Self {
        Self::default_builtin_with_tool_policy(&ToolPolicyConfig::builtin_default())
    }

    pub fn default_builtin_with_tool_policy(tool_policy: &ToolPolicyConfig) -> Self {
        let mut workflows = BTreeMap::new();
        workflows.insert(
            ROOT_WORKFLOW_ID.to_string(),
            WorkflowDefinition::default_root_with_tool_policy(tool_policy),
        );
        workflows.insert(
            CHAT_WORKFLOW_ID.to_string(),
            WorkflowDefinition::default_chat_with_tool_policy(tool_policy),
        );
        workflows.insert(
            RESEARCH_WORKFLOW_ID.to_string(),
            WorkflowDefinition::default_research_with_tool_policy(tool_policy),
        );
        workflows.insert(
            DEEP_RESEARCH_WORKFLOW_ID.to_string(),
            WorkflowDefinition::default_deep_research_with_tool_policy(tool_policy),
        );
        workflows.insert(
            FEATURE_WORKFLOW_ID.to_string(),
            WorkflowDefinition::default_feature_with_tool_policy(tool_policy),
        );
        Self { workflows }
    }

    pub fn workflow(&self, workflow_id: &str) -> Option<&WorkflowDefinition> {
        self.workflows.get(workflow_id)
    }

    pub fn workflow_ids(&self) -> Vec<&str> {
        self.workflows.keys().map(String::as_str).collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowPromptCatalog {
    pub(crate) prompts: BTreeMap<String, WorkflowPrompts>,
}

impl WorkflowPromptCatalog {
    pub fn prompts_for_workflow(&self, workflow_id: &str) -> Option<&WorkflowPrompts> {
        self.prompts.get(workflow_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowStepState {
    pub id: String,
    pub label: String,
    pub index: usize,
    pub total: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowRun {
    enabled_steps: Vec<WorkflowStep>,
    current_index: Option<usize>,
}

impl WorkflowRun {
    pub fn new(definition: &WorkflowDefinition) -> Self {
        let enabled_steps = definition.enabled_steps().cloned().collect::<Vec<_>>();
        let current_index = if enabled_steps.is_empty() {
            None
        } else {
            Some(0)
        };
        Self {
            enabled_steps,
            current_index,
        }
    }

    pub fn current_step_definition(&self) -> Option<&WorkflowStep> {
        let index = self.current_index?;
        self.enabled_steps.get(index)
    }

    pub fn current_step(&self) -> Option<WorkflowStepState> {
        let index = self.current_index?;
        let step = self.enabled_steps.get(index)?;
        Some(WorkflowStepState {
            id: step.id.clone(),
            label: step.label.clone(),
            index: index + 1,
            total: self.enabled_steps.len(),
        })
    }

    pub fn advance(&mut self) -> Option<WorkflowStepState> {
        let current_index = self.current_index?;
        if current_index + 1 >= self.enabled_steps.len() {
            self.current_index = None;
            return None;
        }

        self.current_index = Some(current_index + 1);
        self.current_step()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedWorkflow {
    pub definition: WorkflowDefinition,
    pub prompts: WorkflowPrompts,
    pub source: WorkflowSource,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedWorkflowCatalog {
    pub scene_catalog: SceneCatalog,
    pub workflow_catalog: WorkflowCatalog,
    pub prompt_catalog: WorkflowPromptCatalog,
    pub tool_policy: ToolPolicyConfig,
    pub warnings: Vec<String>,
    pub(crate) workflow_sources: BTreeMap<String, WorkflowSource>,
}

impl LoadedWorkflowCatalog {
    pub fn workflow_source(&self, workflow_id: &str) -> Option<&WorkflowSource> {
        self.workflow_sources.get(workflow_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkflowSource {
    BuiltinDefault,
    File(PathBuf),
    FileWithFallback(PathBuf),
}

impl WorkflowSource {
    pub fn source_label(&self) -> String {
        match self {
            Self::BuiltinDefault => "builtin".to_string(),
            Self::File(path) | Self::FileWithFallback(path) => path.display().to_string(),
        }
    }
}

impl LoadedWorkflow {
    pub fn source_label(&self) -> String {
        self.source.source_label()
    }
}
