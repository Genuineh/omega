use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Deserialize;

pub const DEFAULT_WORKFLOW_PATH: &str = ".omega/workflow.toml";
pub const DEFAULT_SCENES_PATH: &str = ".omega/scenes.toml";
pub const DEFAULT_WORKFLOWS_DIR: &str = ".omega/workflows";
pub const DEFAULT_ROOT_WORKFLOW_PATH: &str = ".omega/workflows/root.toml";
pub const DEFAULT_CHAT_WORKFLOW_PATH: &str = ".omega/workflows/chat.toml";
pub const DEFAULT_FEATURE_WORKFLOW_PATH: &str = ".omega/workflows/feature.toml";
pub const DEFAULT_STEP_PROMPT_DIR: &str = ".omega/prompt/step";

pub const ROOT_WORKFLOW_ID: &str = "root";
pub const CHAT_WORKFLOW_ID: &str = "chat";
pub const FEATURE_WORKFLOW_ID: &str = "feature";

pub const CHAT_SCENE_ID: &str = "chat";
pub const FEATURE_SCENE_ID: &str = "feature";

pub const SCENE_RECOGNITION_STEP_ID: &str = "scene-recognition";
pub const SELECT_WORKFLOW_STEP_ID: &str = "select-workflow";
pub const CHAT_STEP_ID: &str = "chat";
pub const ANALYSIS_STEP_ID: &str = "analysis";
pub const PLAN_STEP_ID: &str = "plan";
pub const EXECUTE_STEP_ID: &str = "execute";
pub const REPORT_STEP_ID: &str = "report";

pub const DEFAULT_SCENE_RECOGNITION_PROMPT_PATH: &str = ".omega/prompt/step/scene-recognition.md";
pub const DEFAULT_SELECT_WORKFLOW_PROMPT_PATH: &str = ".omega/prompt/step/select-workflow.md";
pub const DEFAULT_CHAT_PROMPT_PATH: &str = ".omega/prompt/step/chat.md";
pub const DEFAULT_ANALYSIS_PROMPT_PATH: &str = ".omega/prompt/step/analysis.md";
pub const DEFAULT_PLAN_PROMPT_PATH: &str = ".omega/prompt/step/plan.md";
pub const DEFAULT_EXECUTE_PROMPT_PATH: &str = ".omega/prompt/step/execute.md";
pub const DEFAULT_REPORT_PROMPT_PATH: &str = ".omega/prompt/step/report.md";

const DEFAULT_SCENES_TOML: &str = r#"# Default omega scene routing
root_workflow = "root"
default_scene = "feature"

[[scenes]]
id = "chat"
label = "Chat"
workflow = "chat"

[[scenes]]
id = "feature"
label = "Feature"
workflow = "feature"
"#;

const DEFAULT_ROOT_WORKFLOW_TOML: &str = r#"# Default root workflow
name = "root"

[[steps]]
id = "scene-recognition"
label = "Scene Recognition"
prompt = ".omega/prompt/step/scene-recognition.md"
loop_mode = "single_response"
skill_request = { mode = "match_task" }
enabled = true

[[steps]]
id = "select-workflow"
label = "Select Workflow"
prompt = ".omega/prompt/step/select-workflow.md"
loop_mode = "single_response"
skill_request = { mode = "match_task" }
enabled = true
"#;

const DEFAULT_CHAT_WORKFLOW_TOML: &str = r#"# Default chat workflow
name = "chat"

[[steps]]
id = "chat"
label = "Chat"
prompt = ".omega/prompt/step/chat.md"
loop_mode = "single_response"
skill_request = { mode = "match_task" }
enabled = true
"#;

const DEFAULT_FEATURE_WORKFLOW_TOML: &str = r#"# Default feature workflow
name = "feature"

[[steps]]
id = "analysis"
label = "Analyze"
prompt = ".omega/prompt/step/analysis.md"
loop_mode = "single_response"
skill_request = { mode = "match_task" }
enabled = true

[[steps]]
id = "plan"
label = "Plan"
prompt = ".omega/prompt/step/plan.md"
loop_mode = "single_response"
skill_request = { mode = "match_task" }
enabled = true

[[steps]]
id = "execute"
label = "Execute"
prompt = ".omega/prompt/step/execute.md"
loop_mode = "tool_loop"
tool_request = { mode = "inherit" }
skill_request = { mode = "match_task" }
enabled = true

[[steps]]
id = "report"
label = "Report"
prompt = ".omega/prompt/step/report.md"
loop_mode = "single_response"
skill_request = { mode = "match_task" }
enabled = true
"#;

const DEFAULT_WORKFLOW_TOML: &str = r#"# Legacy compatibility workflow
# This file is kept for backward compatibility.
# The active scene-aware config lives under .omega/scenes.toml and .omega/workflows/*.toml.
name = "feature"

[[steps]]
id = "analysis"
label = "Analyze"
prompt = ".omega/prompt/step/analysis.md"
loop_mode = "single_response"
skill_request = { mode = "match_task" }
enabled = true

[[steps]]
id = "plan"
label = "Plan"
prompt = ".omega/prompt/step/plan.md"
loop_mode = "single_response"
skill_request = { mode = "match_task" }
enabled = true

[[steps]]
id = "execute"
label = "Execute"
prompt = ".omega/prompt/step/execute.md"
loop_mode = "tool_loop"
tool_request = { mode = "inherit" }
skill_request = { mode = "match_task" }
enabled = true

[[steps]]
id = "report"
label = "Report"
prompt = ".omega/prompt/step/report.md"
loop_mode = "single_response"
skill_request = { mode = "match_task" }
enabled = true
"#;

const DEFAULT_SCENE_RECOGNITION_PROMPT: &str = r#"You are in the scene recognition phase.

Classify the user's request into the most appropriate work scene.
Prefer `chat` for conversational, clarifying, explanatory, or lightweight requests.
Prefer `feature` for requests that likely require structured analysis, planning, execution, or reporting.
Produce only the internal scene judgment needed for the next phase.
Do not call tools.
Do not produce the final user-facing answer.
"#;

const DEFAULT_SELECT_WORKFLOW_PROMPT: &str = r#"You are in the workflow selection phase.

Based on the recognized scene, choose the workflow that should run next.
Prefer `chat` for the `chat` scene and `feature` for the `feature` scene unless explicit configuration says otherwise.
Produce only the internal routing decision needed for execution.
Do not call tools.
Do not produce the final user-facing answer.
"#;

const DEFAULT_CHAT_PROMPT: &str = r#"You are in the chat workflow.

Respond conversationally and directly to the user's request.
Use a lightweight interaction style unless the conversation clearly requires a more structured execution workflow.
Do not force an analysis/plan/execute/report structure when a direct answer is sufficient.
"#;

const DEFAULT_ANALYSIS_PROMPT: &str = r#"You are in the analysis phase.

Understand the user's request, constraints, affected files, and likely risks.
Do not call tools.
Do not produce the final user-facing answer.
Produce only the internal analysis needed for the next phase.
"#;

const DEFAULT_PLAN_PROMPT: &str = r#"You are in the planning phase.

Turn the analysis into a concrete execution plan with ordered steps and validation targets.
Do not call tools.
Do not produce the final user-facing answer.
Produce only the internal plan needed for execution.
"#;

const DEFAULT_EXECUTE_PROMPT: &str = r#"You are in the execute phase.

Carry out the plan. Use tools when needed. Make concrete changes and run validation when appropriate.
Do not produce the final user-facing wrap-up yet.
Leave the final summary for the report phase.
"#;

const DEFAULT_REPORT_PROMPT: &str = r#"You are in the report phase.

Based on the completed work and existing transcript, produce the final user-facing response.
Do not call tools in this phase.
Summarize what changed, what was verified, and any remaining risks or follow-up.
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StepLoopMode {
    #[default]
    SingleResponse,
    ToolLoop,
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
                    id: FEATURE_SCENE_ID.to_string(),
                    label: "Feature".to_string(),
                    workflow_id: FEATURE_WORKFLOW_ID.to_string(),
                },
            ],
        }
    }

    pub fn default_scenes_toml() -> &'static str {
        DEFAULT_SCENES_TOML
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

    pub fn load(root: &Path, warnings: &mut Vec<String>) -> Self {
        let path = root.join(DEFAULT_SCENES_PATH);
        if !path.exists() {
            return match Self::write_default_file(&path) {
                Ok(()) => match Self::load_from_file(&path) {
                    Ok(catalog) => catalog,
                    Err(error) => {
                        warnings.push(format!(
                            "Default scene config at {} was created but failed to load: {error}. Falling back to built-in scenes.",
                            path.display()
                        ));
                        Self::default_builtin()
                    }
                },
                Err(error) => {
                    warnings.push(format!(
                        "Failed to create default scene config at {}: {error}. Falling back to built-in scenes.",
                        path.display()
                    ));
                    Self::default_builtin()
                }
            };
        }

        match Self::load_from_file(&path) {
            Ok(catalog) => catalog,
            Err(error) => {
                warnings.push(format!(
                    "Scene config at {} is invalid: {error}. Falling back to built-in scenes.",
                    path.display()
                ));
                Self::default_builtin()
            }
        }
    }

    fn load_from_file(path: &Path) -> Result<Self> {
        let raw = fs::read_to_string(path)
            .with_context(|| format!("failed to read scene config {}", path.display()))?;
        let config = toml::from_str::<SceneCatalogConfig>(&raw)
            .with_context(|| format!("failed to parse scene config {}", path.display()))?;
        Self::from_config(config)
            .with_context(|| format!("failed to apply scene config {}", path.display()))
    }

    fn from_config(config: SceneCatalogConfig) -> Result<Self> {
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

            let label = scene
                .label
                .unwrap_or_else(|| id.clone())
                .trim()
                .to_string();
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
            bail!(
                "default_scene '{default_scene_id}' must refer to a declared scene"
            );
        }

        Ok(Self {
            root_workflow_id,
            default_scene_id,
            scenes,
        })
    }

    fn write_default_file(path: &Path) -> Result<()> {
        write_default_text_file(path, Self::default_scenes_toml())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowPrompts {
    prompts: BTreeMap<String, String>,
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

    fn load(root: &Path, definition: &WorkflowDefinition, warnings: &mut Vec<String>) -> Self {
        let mut prompts = BTreeMap::new();

        for step in &definition.steps {
            let default_content = builtin_step_for_id(&step.id)
                .map(BuiltinWorkflowStepId::default_prompt_content)
                .unwrap_or_default();
            let content = load_prompt_file(
                root,
                &step.prompt_path,
                default_content,
                warnings,
                step.id.as_str(),
            );
            prompts.insert(step.id.clone(), content);
        }

        Self { prompts }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowStep {
    pub id: String,
    pub label: String,
    pub prompt_path: PathBuf,
    pub loop_mode: StepLoopMode,
    pub tool_request: StepToolRequest,
    pub skill_request: StepSkillRequest,
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
        Self {
            name: ROOT_WORKFLOW_ID.to_string(),
            steps: [
                BuiltinWorkflowStepId::SceneRecognition,
                BuiltinWorkflowStepId::SelectWorkflow,
            ]
            .into_iter()
            .map(WorkflowStep::from_builtin)
            .collect(),
        }
    }

    pub fn default_chat() -> Self {
        Self {
            name: CHAT_WORKFLOW_ID.to_string(),
            steps: [BuiltinWorkflowStepId::Chat]
                .into_iter()
                .map(WorkflowStep::from_builtin)
                .collect(),
        }
    }

    pub fn default_feature() -> Self {
        Self {
            name: FEATURE_WORKFLOW_ID.to_string(),
            steps: [
                BuiltinWorkflowStepId::Analysis,
                BuiltinWorkflowStepId::Plan,
                BuiltinWorkflowStepId::Execute,
                BuiltinWorkflowStepId::Report,
            ]
            .into_iter()
            .map(WorkflowStep::from_builtin)
            .collect(),
        }
    }

    pub fn default_workflow_toml() -> &'static str {
        DEFAULT_WORKFLOW_TOML
    }

    pub fn load(root: &Path) -> LoadedWorkflow {
        let loaded_catalog = LoadedWorkflowCatalog::load(root);
        let definition = loaded_catalog
            .workflow_catalog
            .workflow(FEATURE_WORKFLOW_ID)
            .cloned()
            .unwrap_or_else(Self::default_feature);
        let prompts = loaded_catalog
            .prompt_catalog
            .prompts_for_workflow(FEATURE_WORKFLOW_ID)
            .cloned()
            .unwrap_or_else(WorkflowPrompts::builtin_defaults);
        let source = loaded_catalog
            .workflow_source(FEATURE_WORKFLOW_ID)
            .cloned()
            .unwrap_or(WorkflowSource::BuiltinDefault);

        LoadedWorkflow {
            definition,
            prompts,
            source,
            warnings: loaded_catalog.warnings,
        }
    }

    pub fn load_from_file(path: &Path) -> Result<Self> {
        let raw = fs::read_to_string(path)
            .with_context(|| format!("failed to read workflow file {}", path.display()))?;
        let config = toml::from_str::<WorkflowConfig>(&raw)
            .with_context(|| format!("failed to parse workflow file {}", path.display()))?;
        Self::from_config(config)
            .with_context(|| format!("failed to apply workflow file {}", path.display()))
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

    fn from_config(config: WorkflowConfig) -> Result<Self> {
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

            steps.push(WorkflowStep {
                id,
                label,
                prompt_path,
                loop_mode: step
                    .loop_mode
                    .map(Into::into)
                    .unwrap_or_else(|| step.id.default_loop_mode()),
                tool_request: step
                    .tool_request
                    .map(StepToolRequestConfig::into_request)
                    .transpose()?
                    .unwrap_or_else(|| step.id.default_tool_request()),
                skill_request: step
                    .skill_request
                    .map(StepSkillRequestConfig::into_request)
                    .transpose()?
                    .unwrap_or_else(|| step.id.default_skill_request()),
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowCatalog {
    workflows: BTreeMap<String, WorkflowDefinition>,
}

impl WorkflowCatalog {
    pub fn default_builtin() -> Self {
        let mut workflows = BTreeMap::new();
        workflows.insert(ROOT_WORKFLOW_ID.to_string(), WorkflowDefinition::default_root());
        workflows.insert(CHAT_WORKFLOW_ID.to_string(), WorkflowDefinition::default_chat());
        workflows.insert(
            FEATURE_WORKFLOW_ID.to_string(),
            WorkflowDefinition::default_feature(),
        );
        Self { workflows }
    }

    pub fn workflow(&self, workflow_id: &str) -> Option<&WorkflowDefinition> {
        self.workflows.get(workflow_id)
    }

    pub fn workflow_ids(&self) -> Vec<&str> {
        self.workflows.keys().map(String::as_str).collect()
    }

    fn load(root: &Path, scene_catalog: &SceneCatalog) -> Result<(Self, BTreeMap<String, WorkflowSource>)> {
        let mut workflows = BTreeMap::new();
        let mut sources = BTreeMap::new();

        for workflow_id in scene_catalog.referenced_workflow_ids() {
            let (definition, source) = Self::load_single(root, &workflow_id)?;
            workflows.insert(workflow_id.clone(), definition);
            sources.insert(workflow_id, source);
        }

        Ok((Self { workflows }, sources))
    }

    fn load_single(root: &Path, workflow_id: &str) -> Result<(WorkflowDefinition, WorkflowSource)> {
        let workflow_path = workflow_path_for_id(root, workflow_id);
        if workflow_path.exists() {
            let definition = WorkflowDefinition::load_from_file(&workflow_path)?;
            return Ok((definition, WorkflowSource::File(workflow_path)));
        }

        if workflow_id == FEATURE_WORKFLOW_ID {
            let legacy_path = root.join(DEFAULT_WORKFLOW_PATH);
            if legacy_path.exists() {
                let definition = WorkflowDefinition::load_from_file(&legacy_path)?;
                return Ok((definition, WorkflowSource::File(legacy_path)));
            }
        }

        if let Some(default_toml) = default_workflow_toml_for_id(workflow_id) {
            write_default_text_file(&workflow_path, default_toml)?;
            let definition = WorkflowDefinition::load_from_file(&workflow_path)?;
            return Ok((definition, WorkflowSource::File(workflow_path)));
        }

        bail!("workflow '{workflow_id}' is missing and has no built-in preset")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowPromptCatalog {
    prompts: BTreeMap<String, WorkflowPrompts>,
}

impl WorkflowPromptCatalog {
    pub fn prompts_for_workflow(&self, workflow_id: &str) -> Option<&WorkflowPrompts> {
        self.prompts.get(workflow_id)
    }

    fn load(root: &Path, workflow_catalog: &WorkflowCatalog, warnings: &mut Vec<String>) -> Self {
        let mut prompts = BTreeMap::new();
        for workflow_id in workflow_catalog.workflow_ids() {
            if let Some(definition) = workflow_catalog.workflow(workflow_id) {
                prompts.insert(
                    workflow_id.to_string(),
                    WorkflowPrompts::load(root, definition, warnings),
                );
            }
        }
        Self { prompts }
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
        let current_index = if enabled_steps.is_empty() { None } else { Some(0) };
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
    pub warnings: Vec<String>,
    workflow_sources: BTreeMap<String, WorkflowSource>,
}

impl LoadedWorkflowCatalog {
    pub fn load(root: &Path) -> Self {
        let mut warnings = Vec::new();
        let mut scene_catalog = SceneCatalog::load(root, &mut warnings);
        let (workflow_catalog, workflow_sources) = match WorkflowCatalog::load(root, &scene_catalog)
        {
            Ok(loaded) => loaded,
            Err(error) => {
                warnings.push(format!(
                    "Scene/workflow catalog is invalid: {error}. Falling back to built-in scene and workflow presets."
                ));
                scene_catalog = SceneCatalog::default_builtin();
                (
                    WorkflowCatalog::default_builtin(),
                    builtin_workflow_sources(),
                )
            }
        };
        let prompt_catalog = WorkflowPromptCatalog::load(root, &workflow_catalog, &mut warnings);

        Self {
            scene_catalog,
            workflow_catalog,
            prompt_catalog,
            warnings,
            workflow_sources,
        }
    }

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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SceneCatalogConfig {
    root_workflow: Option<String>,
    default_scene: Option<String>,
    scenes: Vec<SceneDefinitionConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SceneDefinitionConfig {
    id: String,
    label: Option<String>,
    workflow: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkflowConfig {
    name: Option<String>,
    steps: Vec<WorkflowStepConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkflowStepConfig {
    id: BuiltinWorkflowStepId,
    label: Option<String>,
    prompt: Option<PathBuf>,
    loop_mode: Option<StepLoopModeConfig>,
    tool_request: Option<StepToolRequestConfig>,
    skill_request: Option<StepSkillRequestConfig>,
    enabled: Option<bool>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum BuiltinWorkflowStepId {
    SceneRecognition,
    SelectWorkflow,
    Chat,
    Analysis,
    Plan,
    Execute,
    Report,
}

impl BuiltinWorkflowStepId {
    fn all() -> [Self; 7] {
        [
            Self::SceneRecognition,
            Self::SelectWorkflow,
            Self::Chat,
            Self::Analysis,
            Self::Plan,
            Self::Execute,
            Self::Report,
        ]
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::SceneRecognition => SCENE_RECOGNITION_STEP_ID,
            Self::SelectWorkflow => SELECT_WORKFLOW_STEP_ID,
            Self::Chat => CHAT_STEP_ID,
            Self::Analysis => ANALYSIS_STEP_ID,
            Self::Plan => PLAN_STEP_ID,
            Self::Execute => EXECUTE_STEP_ID,
            Self::Report => REPORT_STEP_ID,
        }
    }

    fn default_label(self) -> &'static str {
        match self {
            Self::SceneRecognition => "Scene Recognition",
            Self::SelectWorkflow => "Select Workflow",
            Self::Chat => "Chat",
            Self::Analysis => "Analyze",
            Self::Plan => "Plan",
            Self::Execute => "Execute",
            Self::Report => "Report",
        }
    }

    fn default_prompt_path(self) -> &'static str {
        match self {
            Self::SceneRecognition => DEFAULT_SCENE_RECOGNITION_PROMPT_PATH,
            Self::SelectWorkflow => DEFAULT_SELECT_WORKFLOW_PROMPT_PATH,
            Self::Chat => DEFAULT_CHAT_PROMPT_PATH,
            Self::Analysis => DEFAULT_ANALYSIS_PROMPT_PATH,
            Self::Plan => DEFAULT_PLAN_PROMPT_PATH,
            Self::Execute => DEFAULT_EXECUTE_PROMPT_PATH,
            Self::Report => DEFAULT_REPORT_PROMPT_PATH,
        }
    }

    fn default_prompt_content(self) -> &'static str {
        match self {
            Self::SceneRecognition => DEFAULT_SCENE_RECOGNITION_PROMPT,
            Self::SelectWorkflow => DEFAULT_SELECT_WORKFLOW_PROMPT,
            Self::Chat => DEFAULT_CHAT_PROMPT,
            Self::Analysis => DEFAULT_ANALYSIS_PROMPT,
            Self::Plan => DEFAULT_PLAN_PROMPT,
            Self::Execute => DEFAULT_EXECUTE_PROMPT,
            Self::Report => DEFAULT_REPORT_PROMPT,
        }
    }

    fn default_loop_mode(self) -> StepLoopMode {
        match self {
            Self::Execute => StepLoopMode::ToolLoop,
            Self::SceneRecognition
            | Self::SelectWorkflow
            | Self::Chat
            | Self::Analysis
            | Self::Plan
            | Self::Report => StepLoopMode::SingleResponse,
        }
    }

    fn default_tool_request(self) -> StepToolRequest {
        StepToolRequest::Inherit
    }

    fn default_skill_request(self) -> StepSkillRequest {
        StepSkillRequest::MatchTask
    }
}

impl WorkflowStep {
    fn from_builtin(step: BuiltinWorkflowStepId) -> Self {
        Self {
            id: step.as_str().to_string(),
            label: step.default_label().to_string(),
            prompt_path: PathBuf::from(step.default_prompt_path()),
            loop_mode: step.default_loop_mode(),
            tool_request: step.default_tool_request(),
            skill_request: step.default_skill_request(),
            enabled: true,
        }
    }
}

fn builtin_step_for_id(step_id: &str) -> Option<BuiltinWorkflowStepId> {
    match step_id {
        SCENE_RECOGNITION_STEP_ID => Some(BuiltinWorkflowStepId::SceneRecognition),
        SELECT_WORKFLOW_STEP_ID => Some(BuiltinWorkflowStepId::SelectWorkflow),
        CHAT_STEP_ID => Some(BuiltinWorkflowStepId::Chat),
        ANALYSIS_STEP_ID => Some(BuiltinWorkflowStepId::Analysis),
        PLAN_STEP_ID => Some(BuiltinWorkflowStepId::Plan),
        EXECUTE_STEP_ID => Some(BuiltinWorkflowStepId::Execute),
        REPORT_STEP_ID => Some(BuiltinWorkflowStepId::Report),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum StepLoopModeConfig {
    SingleResponse,
    ToolLoop,
}

impl From<StepLoopModeConfig> for StepLoopMode {
    fn from(value: StepLoopModeConfig) -> Self {
        match value {
            StepLoopModeConfig::SingleResponse => Self::SingleResponse,
            StepLoopModeConfig::ToolLoop => Self::ToolLoop,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StepToolRequestConfig {
    mode: StepToolRequestMode,
    #[serde(default)]
    items: Vec<String>,
}

impl StepToolRequestConfig {
    fn into_request(self) -> Result<StepToolRequest> {
        match self.mode {
            StepToolRequestMode::Inherit => {
                if !self.items.is_empty() {
                    bail!("tool_request mode 'inherit' does not accept items");
                }
                Ok(StepToolRequest::Inherit)
            }
            StepToolRequestMode::Extend => Ok(StepToolRequest::Extend(self.items)),
            StepToolRequestMode::Block => Ok(StepToolRequest::Block(self.items)),
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
struct StepSkillRequestConfig {
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

fn default_workflow_toml_for_id(workflow_id: &str) -> Option<&'static str> {
    match workflow_id {
        ROOT_WORKFLOW_ID => Some(DEFAULT_ROOT_WORKFLOW_TOML),
        CHAT_WORKFLOW_ID => Some(DEFAULT_CHAT_WORKFLOW_TOML),
        FEATURE_WORKFLOW_ID => Some(DEFAULT_FEATURE_WORKFLOW_TOML),
        _ => None,
    }
}

fn workflow_path_for_id(root: &Path, workflow_id: &str) -> PathBuf {
    root.join(DEFAULT_WORKFLOWS_DIR)
        .join(format!("{workflow_id}.toml"))
}

fn builtin_workflow_sources() -> BTreeMap<String, WorkflowSource> {
    let mut sources = BTreeMap::new();
    sources.insert(ROOT_WORKFLOW_ID.to_string(), WorkflowSource::BuiltinDefault);
    sources.insert(CHAT_WORKFLOW_ID.to_string(), WorkflowSource::BuiltinDefault);
    sources.insert(FEATURE_WORKFLOW_ID.to_string(), WorkflowSource::BuiltinDefault);
    sources
}

fn load_prompt_file(
    root: &Path,
    prompt_path: &Path,
    default_content: &str,
    warnings: &mut Vec<String>,
    prompt_name: &str,
) -> String {
    let path = resolve_prompt_path(root, prompt_path);
    if !path.exists() {
        if let Err(error) = write_default_text_file(&path, default_content) {
            warnings.push(format!(
                "Failed to create default {prompt_name} prompt file at {}: {error}. Falling back to built-in defaults.",
                path.display()
            ));
            return default_content.to_string();
        }
    }

    match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) => {
            warnings.push(format!(
                "Failed to read {prompt_name} prompt file at {}: {error}. Falling back to built-in defaults.",
                path.display()
            ));
            default_content.to_string()
        }
    }
}

fn resolve_prompt_path(root: &Path, prompt_path: &Path) -> PathBuf {
    if prompt_path.is_absolute() {
        prompt_path.to_path_buf()
    } else {
        root.join(prompt_path)
    }
}

fn write_default_text_file(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create parent dir {}", parent.display()))?;
    }
    fs::write(path, content)
        .with_context(|| format!("failed to write file {}", path.display()))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{
        LoadedWorkflow, LoadedWorkflowCatalog, SceneCatalog, StepLoopMode, StepSkillRequest,
        StepToolRequest, WorkflowDefinition, WorkflowPrompts, WorkflowSource,
        ANALYSIS_STEP_ID, CHAT_WORKFLOW_ID, DEFAULT_SCENES_PATH, DEFAULT_WORKFLOW_PATH,
        EXECUTE_STEP_ID, FEATURE_SCENE_ID, FEATURE_WORKFLOW_ID, REPORT_STEP_ID,
        ROOT_WORKFLOW_ID, SCENE_RECOGNITION_STEP_ID,
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
            vec![ANALYSIS_STEP_ID, "plan", EXECUTE_STEP_ID, REPORT_STEP_ID]
        );
    }

    #[test]
    fn missing_scene_and_workflow_catalog_is_created_and_loaded() {
        let root = unique_test_root("missing-scene-catalog");

        let loaded = LoadedWorkflowCatalog::load(&root);

        assert!(root.join(DEFAULT_SCENES_PATH).exists());
        assert!(root.join(".omega/workflows/root.toml").exists());
        assert!(root.join(".omega/workflows/chat.toml").exists());
        assert!(root.join(".omega/workflows/feature.toml").exists());
        assert!(root.join(".omega/prompt/step/scene-recognition.md").exists());
        assert!(root.join(".omega/prompt/step/select-workflow.md").exists());
        assert!(root.join(".omega/prompt/step/chat.md").exists());
        assert!(root.join(".omega/prompt/step/analysis.md").exists());
        assert!(loaded.warnings.is_empty());
        assert_eq!(loaded.scene_catalog.default_scene_id, FEATURE_SCENE_ID);
        assert_eq!(loaded.scene_catalog.root_workflow_id, ROOT_WORKFLOW_ID);
        assert!(loaded.workflow_catalog.workflow(ROOT_WORKFLOW_ID).is_some());
        assert!(loaded.workflow_catalog.workflow(CHAT_WORKFLOW_ID).is_some());
        assert!(loaded.workflow_catalog.workflow(FEATURE_WORKFLOW_ID).is_some());
        assert!(loaded
            .prompt_catalog
            .prompts_for_workflow(ROOT_WORKFLOW_ID)
            .and_then(|prompts| prompts.prompt_for(SCENE_RECOGNITION_STEP_ID))
            .is_some_and(|prompt| prompt.contains("scene recognition phase")));
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
            "name = \"feature\"\n\n[[steps]]\nid = \"analysis\"\nlabel = \"Scope\"\nenabled = true\n\n[[steps]]\nid = \"execute\"\nlabel = \"Ship\"\nenabled = true\n",
        )
        .unwrap();
        std::fs::write(
            root.join(DEFAULT_WORKFLOW_PATH),
            "name = \"legacy\"\n\n[[steps]]\nid = \"analysis\"\nlabel = \"Legacy\"\nenabled = true\n",
        )
        .unwrap();

        let loaded = WorkflowDefinition::load(&root);

        let steps = loaded.definition.enabled_steps().collect::<Vec<_>>();
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].label, "Scope");
        assert_eq!(steps[1].label, "Ship");
        assert!(matches!(loaded.source, WorkflowSource::File(path) if path.ends_with(".omega/workflows/feature.toml")));
    }

    #[test]
    fn legacy_workflow_file_is_used_for_feature_compatibility() {
        let root = unique_test_root("legacy-feature");
        let workflow_path = root.join(DEFAULT_WORKFLOW_PATH);
        std::fs::create_dir_all(workflow_path.parent().unwrap()).unwrap();
        std::fs::write(
            &workflow_path,
            "name = \"trimmed\"\n\n[[steps]]\nid = \"analysis\"\nlabel = \"Scope\"\nprompt = \".omega/prompt/step/analysis.md\"\nloop_mode = \"single_response\"\nskill_request = { mode = \"append\", items = [\"review\"] }\nenabled = true\n\n[[steps]]\nid = \"plan\"\nenabled = false\n\n[[steps]]\nid = \"execute\"\nlabel = \"Build\"\nprompt = \".omega/prompt/step/execute.md\"\nloop_mode = \"tool_loop\"\ntool_request = { mode = \"extend\", items = [\"todo\"] }\nenabled = true\n",
        )
        .unwrap();

        let loaded_catalog = LoadedWorkflowCatalog::load(&root);
        let feature = loaded_catalog
            .workflow_catalog
            .workflow(FEATURE_WORKFLOW_ID)
            .unwrap();

        assert_eq!(feature.name, "trimmed");
        assert_eq!(feature.enabled_step_count(), 2);
        assert!(matches!(loaded_catalog.workflow_source(FEATURE_WORKFLOW_ID), Some(WorkflowSource::File(path)) if path.ends_with(DEFAULT_WORKFLOW_PATH)));
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
            "name = \"trimmed\"\n\n[[steps]]\nid = \"analysis\"\nlabel = \"Scope\"\nprompt = \".omega/prompt/step/analysis.md\"\nloop_mode = \"single_response\"\nskill_request = { mode = \"append\", items = [\"review\"] }\nenabled = true\n\n[[steps]]\nid = \"plan\"\nenabled = false\n\n[[steps]]\nid = \"execute\"\nlabel = \"Build\"\nprompt = \".omega/prompt/step/execute.md\"\nloop_mode = \"tool_loop\"\ntool_request = { mode = \"extend\", items = [\"todo\"] }\nenabled = true\n",
        )
        .unwrap();

        let loaded = WorkflowDefinition::load(&root);

        assert!(loaded.warnings.is_empty());
        assert_eq!(loaded.definition.name, "trimmed");
        assert_eq!(loaded.definition.enabled_step_count(), 2);
        let steps = loaded.definition.enabled_steps().collect::<Vec<_>>();
        assert_eq!(steps[0].id, ANALYSIS_STEP_ID);
        assert_eq!(
            steps[0].skill_request,
            StepSkillRequest::Append(vec!["review".to_string()])
        );
        assert_eq!(steps[1].loop_mode, StepLoopMode::ToolLoop);
        assert_eq!(
            steps[1].tool_request,
            StepToolRequest::Extend(vec!["todo".to_string()])
        );
        let mut run = loaded.definition.start_run();
        let first = run.current_step().unwrap();
        assert_eq!(first.id, ANALYSIS_STEP_ID);
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

        let loaded = WorkflowDefinition::load(&root);

        assert!(loaded
            .prompts
            .prompt_for(EXECUTE_STEP_ID)
            .is_some_and(|prompt| prompt.contains("execute phase")));
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
}
