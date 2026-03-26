use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

use anyhow::{bail, Result};
use serde::Deserialize;

use crate::constants::{EXECUTE_STEP_ID, FEATURE_SCENE_ID, REPORT_STEP_ID, ROOT_WORKFLOW_ID};
use crate::defaults::BuiltinWorkflowStepId;
use crate::model::{
    DataFormat, OutputRecoveryMode, SceneCatalog, SceneDefinition, StepInputContract, StepLoopMode,
    StepOutputContract, StepSkillRequest, StepToolRequest, WorkflowDefinition, WorkflowStep,
};
use crate::policy::{
    dedupe_preserve_order, normalize_allowed_commands, normalize_tool_group, ToolPolicyConfig,
};

#[derive(Debug, Deserialize)]
pub(crate) struct ToolPolicyModelConfig {
    #[serde(default)]
    tools: Option<ToolPolicyToolsConfig>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ToolPolicyToolsConfig {
    #[serde(default)]
    bash: Option<ToolPolicyBashConfig>,
    #[serde(default)]
    batch: Option<ToolPolicyBatchConfig>,
    #[serde(default)]
    groups: Option<BTreeMap<String, Vec<String>>>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ToolPolicyBashConfig {
    #[serde(default)]
    allowed_commands: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ToolPolicyBatchConfig {
    #[serde(default)]
    max_requests: Option<usize>,
}

impl ToolPolicyConfig {
    pub(crate) fn from_model_config(config: ToolPolicyModelConfig) -> Result<Self> {
        let mut policy = Self::builtin_default();

        if let Some(tools) = config.tools {
            if let Some(bash) = tools.bash {
                if let Some(allowed_commands) = bash.allowed_commands {
                    policy.bash_allowed_commands = normalize_allowed_commands(allowed_commands)?;
                }
            }

            if let Some(batch) = tools.batch {
                if let Some(max_requests) = batch.max_requests {
                    if max_requests == 0 {
                        bail!("tools.batch.max_requests must be >= 1");
                    }
                    policy.batch_max_requests = max_requests;
                }
            }

            if let Some(groups) = tools.groups {
                for (name, items) in groups {
                    let normalized_items = normalize_tool_group(&name, items)?;
                    policy
                        .groups
                        .insert(name.trim().to_string(), normalized_items);
                }
            }
        }

        Ok(policy)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SceneCatalogConfig {
    root_workflow: Option<String>,
    default_scene: Option<String>,
    scenes: Vec<SceneDefinitionConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SceneDefinitionConfig {
    id: String,
    label: Option<String>,
    workflow: String,
}

impl SceneCatalog {
    pub(crate) fn from_config(config: SceneCatalogConfig) -> Result<Self> {
        if config.scenes.is_empty() {
            bail!("scene catalog must declare at least one scene");
        }

        let root_workflow_id = config
            .root_workflow
            .unwrap_or_else(|| ROOT_WORKFLOW_ID.to_string())
            .trim()
            .to_string();
        if root_workflow_id.is_empty() {
            bail!("scene catalog must declare a non-empty root_workflow");
        }

        let default_scene_id = config
            .default_scene
            .unwrap_or_else(|| FEATURE_SCENE_ID.to_string())
            .trim()
            .to_string();
        if default_scene_id.is_empty() {
            bail!("scene catalog must declare a non-empty default_scene");
        }

        let mut seen = HashSet::new();
        let mut scenes = Vec::with_capacity(config.scenes.len());
        for scene in config.scenes {
            let id = scene.id.trim().to_string();
            if id.is_empty() {
                bail!("scene id must be non-empty");
            }
            if !seen.insert(id.clone()) {
                bail!("scene '{id}' is duplicated");
            }

            let label = scene.label.unwrap_or_else(|| id.clone()).trim().to_string();
            if label.is_empty() {
                bail!("scene '{id}' must have a non-empty label");
            }

            let workflow_id = scene.workflow.trim().to_string();
            if workflow_id.is_empty() {
                bail!("scene '{id}' must bind a non-empty workflow id");
            }

            scenes.push(SceneDefinition {
                id,
                label,
                workflow_id,
            });
        }

        if !scenes.iter().any(|scene| scene.id == default_scene_id) {
            bail!("default_scene '{default_scene_id}' must refer to a declared scene");
        }

        Ok(Self {
            root_workflow_id,
            default_scene_id,
            scenes,
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WorkflowConfig {
    name: Option<String>,
    steps: Vec<WorkflowStepConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WorkflowStepConfig {
    id: BuiltinWorkflowStepId,
    label: Option<String>,
    prompt: Option<PathBuf>,
    loop_mode: Option<StepLoopModeConfig>,
    max_iterations: Option<u32>,
    max_step_repeats: Option<u32>,
    #[serde(default)]
    hooks: Vec<String>,
    tool_request: Option<StepToolRequestConfig>,
    skill_request: Option<StepSkillRequestConfig>,
    input_contract: Option<StepInputContractConfig>,
    output_contract: Option<StepOutputContractConfig>,
    enabled: Option<bool>,
}

impl WorkflowDefinition {
    pub(crate) fn from_config(
        config: WorkflowConfig,
        tool_policy: &ToolPolicyConfig,
    ) -> Result<Self> {
        if config.steps.is_empty() {
            bail!("workflow must declare at least one step");
        }

        let mut seen = HashSet::new();
        let mut steps = Vec::with_capacity(config.steps.len());
        for step in config.steps {
            let id = step.id.as_str().to_string();
            if !seen.insert(id.clone()) {
                bail!("workflow step '{id}' is duplicated");
            }

            let label = step
                .label
                .unwrap_or_else(|| step.id.default_label().to_string())
                .trim()
                .to_string();
            if label.is_empty() {
                bail!("workflow step '{id}' must have a non-empty label");
            }

            let prompt_path = step
                .prompt
                .unwrap_or_else(|| PathBuf::from(step.id.default_prompt_path()));
            if prompt_path.as_os_str().is_empty() {
                bail!("workflow step '{id}' must have a non-empty prompt path");
            }

            if step.max_iterations == Some(0) {
                bail!("workflow step '{id}' must have max_iterations >= 1");
            }

            steps.push(WorkflowStep {
                id,
                label,
                prompt_path,
                loop_mode: step
                    .loop_mode
                    .map(Into::into)
                    .unwrap_or_else(|| step.id.default_loop_mode()),
                max_iterations: step
                    .max_iterations
                    .unwrap_or_else(|| step.id.default_max_iterations())
                    .max(1),
                max_step_repeats: step
                    .max_step_repeats
                    .unwrap_or_else(|| step.id.default_max_step_repeats()),
                hooks: normalize_hook_ids(step.hooks)?,
                tool_request: step
                    .tool_request
                    .map(|request| request.into_request(tool_policy))
                    .transpose()?
                    .unwrap_or_else(|| step.id.default_tool_request(tool_policy)),
                skill_request: step
                    .skill_request
                    .map(StepSkillRequestConfig::into_request)
                    .transpose()?
                    .unwrap_or_else(|| step.id.default_skill_request()),
                input_contract: step
                    .input_contract
                    .map(StepInputContractConfig::into_contract)
                    .transpose()?
                    .unwrap_or_else(|| step.id.default_input_contract()),
                output_contract: step
                    .output_contract
                    .map(StepOutputContractConfig::into_contract)
                    .transpose()?
                    .unwrap_or_else(|| step.id.default_output_contract()),
                enabled: step.enabled.unwrap_or(true),
            });
        }

        if !steps.iter().any(|step| step.enabled) {
            bail!("workflow must keep at least one enabled step");
        }

        let execute_position = steps
            .iter()
            .position(|step| step.enabled && step.id == EXECUTE_STEP_ID);
        let report_position = steps
            .iter()
            .position(|step| step.enabled && step.id == REPORT_STEP_ID);
        if let (Some(execute_position), Some(report_position)) = (execute_position, report_position)
        {
            if report_position < execute_position {
                bail!(
                    "workflow step 'report' cannot appear before 'execute' when both are enabled"
                );
            }
        }

        Ok(Self {
            name: config.name.unwrap_or_else(|| "default".to_string()),
            steps,
        })
    }
}

fn normalize_hook_ids(hooks: Vec<String>) -> Result<Vec<String>> {
    let mut normalized = Vec::with_capacity(hooks.len());
    let mut seen = HashSet::new();

    for hook in hooks {
        let hook = hook.trim().to_string();
        if hook.is_empty() {
            bail!("workflow step hooks cannot contain empty ids");
        }
        if !seen.insert(hook.clone()) {
            bail!("workflow step hook '{hook}' is duplicated");
        }
        normalized.push(hook);
    }

    Ok(normalized)
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub(crate) enum StepLoopModeConfig {
    #[serde(rename = "agent_loop", alias = "single_response", alias = "tool_loop")]
    AgentLoop,
}

impl From<StepLoopModeConfig> for StepLoopMode {
    fn from(value: StepLoopModeConfig) -> Self {
        match value {
            StepLoopModeConfig::AgentLoop => Self::AgentLoop,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StepToolRequestConfig {
    mode: StepToolRequestMode,
    #[serde(default)]
    items: Vec<String>,
    #[serde(default)]
    groups: Vec<String>,
}

impl StepToolRequestConfig {
    fn into_request(self, tool_policy: &ToolPolicyConfig) -> Result<StepToolRequest> {
        match self.mode {
            StepToolRequestMode::Inherit => {
                if !self.items.is_empty() || !self.groups.is_empty() {
                    bail!("tool_request mode 'inherit' does not accept items or groups");
                }
                Ok(StepToolRequest::Inherit)
            }
            StepToolRequestMode::Extend => Ok(StepToolRequest::Extend(dedupe_preserve_order(
                tool_policy
                    .resolve_groups(&self.groups)?
                    .into_iter()
                    .chain(self.items),
            ))),
            StepToolRequestMode::Block => Ok(StepToolRequest::Block(dedupe_preserve_order(
                tool_policy
                    .resolve_groups(&self.groups)?
                    .into_iter()
                    .chain(self.items),
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum StepToolRequestMode {
    Inherit,
    Extend,
    Block,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StepSkillRequestConfig {
    mode: StepSkillRequestMode,
    #[serde(default)]
    items: Vec<String>,
}

impl StepSkillRequestConfig {
    fn into_request(self) -> Result<StepSkillRequest> {
        match self.mode {
            StepSkillRequestMode::MatchTask => {
                if !self.items.is_empty() {
                    bail!("skill_request mode 'match_task' does not accept items");
                }
                Ok(StepSkillRequest::MatchTask)
            }
            StepSkillRequestMode::Append => Ok(StepSkillRequest::Append(self.items)),
            StepSkillRequestMode::Disable => {
                if !self.items.is_empty() {
                    bail!("skill_request mode 'disable' does not accept items");
                }
                Ok(StepSkillRequest::Disable)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum StepSkillRequestMode {
    MatchTask,
    Append,
    Disable,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StepInputContractConfig {
    mode: StepInputContractMode,
    #[serde(default)]
    sources: Vec<String>,
}

impl StepInputContractConfig {
    fn into_contract(self) -> Result<StepInputContract> {
        match self.mode {
            StepInputContractMode::None => {
                if !self.sources.is_empty() {
                    bail!("input_contract mode 'none' does not accept sources");
                }
                Ok(StepInputContract::None)
            }
            StepInputContractMode::Required => {
                if self.sources.is_empty() {
                    bail!("input_contract mode 'required' needs at least one source");
                }
                Ok(StepInputContract::Required {
                    sources: self.sources,
                })
            }
            StepInputContractMode::Optional => {
                if self.sources.is_empty() {
                    bail!("input_contract mode 'optional' needs at least one source");
                }
                Ok(StepInputContract::Optional {
                    sources: self.sources,
                })
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum StepInputContractMode {
    None,
    Required,
    Optional,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StepOutputContractConfig {
    mode: StepOutputContractMode,
    #[serde(default)]
    format: Option<DataFormatConfig>,
    #[serde(default)]
    schema_path: Option<PathBuf>,
    #[serde(default)]
    max_retries: Option<u32>,
    #[serde(default)]
    recovery_mode: Option<OutputRecoveryModeConfig>,
}

impl StepOutputContractConfig {
    fn into_contract(self) -> Result<StepOutputContract> {
        let format = self.format.unwrap_or(DataFormatConfig::Json).into_format();
        match self.mode {
            StepOutputContractMode::None => {
                if self.schema_path.is_some()
                    || self.max_retries.is_some()
                    || self.format.is_some()
                    || self.recovery_mode.is_some()
                {
                    bail!("output_contract mode 'none' does not accept format, schema_path, max_retries, or recovery_mode");
                }
                Ok(StepOutputContract::None)
            }
            StepOutputContractMode::Required => Ok(StepOutputContract::Required {
                format,
                schema_path: self.schema_path,
                max_retries: self.max_retries.unwrap_or(1).max(1),
                recovery_mode: self
                    .recovery_mode
                    .unwrap_or(OutputRecoveryModeConfig::RepairThenRegenerate)
                    .into_mode(),
            }),
            StepOutputContractMode::Optional => {
                if self.max_retries.is_some() || self.recovery_mode.is_some() {
                    bail!("output_contract mode 'optional' does not accept max_retries or recovery_mode");
                }
                Ok(StepOutputContract::Optional {
                    format,
                    schema_path: self.schema_path,
                })
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum StepOutputContractMode {
    None,
    Required,
    Optional,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum OutputRecoveryModeConfig {
    RegenerateOnly,
    RepairThenRegenerate,
}

impl OutputRecoveryModeConfig {
    fn into_mode(self) -> OutputRecoveryMode {
        match self {
            Self::RegenerateOnly => OutputRecoveryMode::RegenerateOnly,
            Self::RepairThenRegenerate => OutputRecoveryMode::RepairThenRegenerate,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DataFormatConfig {
    Json,
}

impl DataFormatConfig {
    fn into_format(self) -> DataFormat {
        match self {
            Self::Json => DataFormat::Json,
        }
    }
}
