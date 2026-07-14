use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use omega_hpc_paths::OmegaProjectLayout;

use crate::config::{SceneCatalogConfig, ToolPolicyModelConfig, WorkflowConfig};
use crate::constants::{
    DEFAULT_MODEL_CONFIG_PATH, DEFAULT_SCENES_PATH, DEFAULT_WORKFLOWS_DIR, DEFAULT_WORKFLOW_PATH,
    FEATURE_WORKFLOW_ID,
};
use crate::defaults::{
    builtin_schema_content_for_path, builtin_step_for_id, builtin_workflow_sources,
    default_workflow_toml_for_id,
};
use crate::model::{
    LoadedWorkflow, LoadedWorkflowCatalog, SceneCatalog, WorkflowCatalog, WorkflowDefinition,
    WorkflowPromptCatalog, WorkflowPrompts, WorkflowSource,
};
use crate::policy::ToolPolicyConfig;
use crate::StepOutputContract;

impl ToolPolicyConfig {
    fn load(root: &Path, warnings: &mut Vec<String>) -> Self {
        let path = OmegaProjectLayout::new(root.to_path_buf()).model_config_path();
        if !path.exists() {
            return Self::builtin_default();
        }

        match Self::load_from_file(&path) {
            Ok(policy) => policy,
            Err(error) => {
                warnings.push(format!(
                    "Tool policy config in {} is invalid: {error}. Falling back to built-in tool policy defaults.",
                    path.display()
                ));
                Self::builtin_default()
            }
        }
    }

    fn load_from_file(path: &Path) -> Result<Self> {
        let raw = fs::read_to_string(path)
            .with_context(|| format!("failed to read model config {}", path.display()))?;
        let config = toml::from_str::<ToolPolicyModelConfig>(&raw)
            .with_context(|| format!("failed to parse model config {}", path.display()))?;
        Self::from_model_config(config)
            .with_context(|| format!("failed to apply tool policy from {}", path.display()))
    }
}

impl SceneCatalog {
    pub fn load(root: &Path, warnings: &mut Vec<String>) -> Self {
        let path = OmegaProjectLayout::new(root.to_path_buf()).scenes_path();
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

    fn write_default_file(path: &Path) -> Result<()> {
        write_default_text_file(path, Self::default_scenes_toml())
    }
}

impl WorkflowPrompts {
    fn load(root: &Path, definition: &WorkflowDefinition, warnings: &mut Vec<String>) -> Self {
        let mut prompts = std::collections::BTreeMap::new();

        for step in &definition.steps {
            let default_content = builtin_step_for_id(&step.id)
                .map(|step| step.default_prompt_content())
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

impl WorkflowDefinition {
    pub fn load(root: &Path) -> LoadedWorkflow {
        let loaded_catalog = LoadedWorkflowCatalog::load(root);
        let definition = loaded_catalog
            .workflow_catalog
            .workflow(FEATURE_WORKFLOW_ID)
            .cloned()
            .unwrap_or_else(|| Self::default_feature_with_tool_policy(&loaded_catalog.tool_policy));
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

    pub fn load_from_file(path: &Path, tool_policy: &ToolPolicyConfig) -> Result<Self> {
        let raw = fs::read_to_string(path)
            .with_context(|| format!("failed to read workflow file {}", path.display()))?;
        let config = toml::from_str::<WorkflowConfig>(&raw)
            .with_context(|| format!("failed to parse workflow file {}", path.display()))?;
        Self::from_config(config, tool_policy)
            .with_context(|| format!("failed to apply workflow file {}", path.display()))
    }
}

impl WorkflowCatalog {
    fn load(
        root: &Path,
        scene_catalog: &SceneCatalog,
        tool_policy: &ToolPolicyConfig,
    ) -> Result<(Self, std::collections::BTreeMap<String, WorkflowSource>)> {
        let mut workflows = std::collections::BTreeMap::new();
        let mut sources = std::collections::BTreeMap::new();

        for workflow_id in scene_catalog.referenced_workflow_ids() {
            let (definition, source) = Self::load_single(root, &workflow_id, tool_policy)?;
            workflows.insert(workflow_id.clone(), definition);
            sources.insert(workflow_id, source);
        }

        Ok((Self { workflows }, sources))
    }

    fn load_single(
        root: &Path,
        workflow_id: &str,
        tool_policy: &ToolPolicyConfig,
    ) -> Result<(WorkflowDefinition, WorkflowSource)> {
        let workflow_path = workflow_path_for_id(root, workflow_id);
        if workflow_path.exists() {
            let definition = WorkflowDefinition::load_from_file(&workflow_path, tool_policy)?;
            return Ok((definition, WorkflowSource::File(workflow_path)));
        }

        if workflow_id == FEATURE_WORKFLOW_ID {
            let legacy_path = OmegaProjectLayout::new(root.to_path_buf()).legacy_workflow_path();
            if legacy_path.exists() {
                let definition = WorkflowDefinition::load_from_file(&legacy_path, tool_policy)?;
                return Ok((definition, WorkflowSource::File(legacy_path)));
            }
        }

        if let Some(default_toml) = default_workflow_toml_for_id(workflow_id) {
            write_default_text_file(&workflow_path, default_toml)?;
            let definition = WorkflowDefinition::load_from_file(&workflow_path, tool_policy)?;
            return Ok((definition, WorkflowSource::File(workflow_path)));
        }

        bail!("workflow '{workflow_id}' is missing and has no built-in preset")
    }
}

impl WorkflowPromptCatalog {
    fn load(root: &Path, workflow_catalog: &WorkflowCatalog, warnings: &mut Vec<String>) -> Self {
        let mut prompts = std::collections::BTreeMap::new();
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

impl LoadedWorkflowCatalog {
    pub fn load(root: &Path) -> Self {
        let mut warnings = Vec::new();
        let tool_policy = ToolPolicyConfig::load(root, &mut warnings);
        let mut scene_catalog = SceneCatalog::load(root, &mut warnings);
        let (workflow_catalog, workflow_sources) = match WorkflowCatalog::load(
            root,
            &scene_catalog,
            &tool_policy,
        ) {
            Ok(loaded) => loaded,
            Err(error) => {
                warnings.push(format!(
                        "Scene/workflow catalog is invalid: {error}. Falling back to built-in scene and workflow presets."
                    ));
                scene_catalog = SceneCatalog::default_builtin();
                (
                    WorkflowCatalog::default_builtin_with_tool_policy(&tool_policy),
                    builtin_workflow_sources(),
                )
            }
        };
        let prompt_catalog = WorkflowPromptCatalog::load(root, &workflow_catalog, &mut warnings);
        ensure_builtin_step_schema_files(root, &workflow_catalog, &mut warnings);

        Self {
            scene_catalog,
            workflow_catalog,
            prompt_catalog,
            tool_policy,
            warnings,
            workflow_sources,
        }
    }
}

fn workflow_path_for_id(root: &Path, workflow_id: &str) -> PathBuf {
    OmegaProjectLayout::new(root.to_path_buf())
        .workflows_dir()
        .join(format!("{workflow_id}.toml"))
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

fn ensure_builtin_step_schema_files(
    root: &Path,
    workflow_catalog: &WorkflowCatalog,
    warnings: &mut Vec<String>,
) {
    for workflow_id in workflow_catalog.workflow_ids() {
        let Some(workflow) = workflow_catalog.workflow(workflow_id) else {
            continue;
        };

        for step in &workflow.steps {
            let schema_path = match &step.output_contract {
                StepOutputContract::Required {
                    schema_path: Some(schema_path),
                    ..
                }
                | StepOutputContract::Optional {
                    schema_path: Some(schema_path),
                    ..
                } => schema_path,
                StepOutputContract::None
                | StepOutputContract::Required {
                    schema_path: None, ..
                }
                | StepOutputContract::Optional {
                    schema_path: None, ..
                } => continue,
            };

            let Some(default_content) = builtin_schema_content_for_path(schema_path) else {
                continue;
            };

            let path = resolve_prompt_path(root, schema_path);
            if path.exists() {
                continue;
            }

            if let Err(error) = write_default_text_file(&path, default_content) {
                warnings.push(format!(
                    "Failed to create default schema file for step '{}' at {}: {error}.",
                    step.id,
                    path.display()
                ));
            }
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
    fs::write(path, content).with_context(|| format!("failed to write file {}", path.display()))
}
