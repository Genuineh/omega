use std::path::Path;
use std::sync::Arc;

mod model_config;

use omega_core::{DynLlmClient, MinimaxClient, MinimaxConfig};
use omega_keymap::KeymapManager;
use omega_observability::init_tracing_channel;
use omega_session::{AgentSession, AgentSessionConfig};
use omega_theme::OmegaTheme;
use omega_tui::{run as run_tui, TuiBehaviorConfig, TuiLaunchConfig};
use omega_workflow::LoadedWorkflowCatalog;
use tracing::{info, warn};

use crate::model_config::AgentModelConfig;

pub fn default_system_prompt(cwd: &Path) -> String {
    format!(
        "You are a coding agent at {}. Use bash to solve tasks. Act, don't explain.",
        cwd.display()
    )
}

pub async fn run() -> anyhow::Result<()> {
    let config = MinimaxConfig::from_env().map_err(|e| anyhow::anyhow!("{e}"))?;
    let model_name = config.model.clone();
    let client: DynLlmClient =
        Arc::new(MinimaxClient::new(config).map_err(|e| anyhow::anyhow!("{e}"))?);

    let cwd = std::env::current_dir()?;
    let system = default_system_prompt(&cwd);
    let trace_rx = init_tracing_channel()?;
    info!(model = %model_name, cwd = %cwd.display(), "app config loaded");

    let loaded_keymap = KeymapManager::load(&cwd);
    if let Some(warning) = loaded_keymap.warning.as_deref() {
        warn!(%warning, "keymap config fallback activated");
    }

    let loaded_theme = OmegaTheme::load(&cwd);
    for warning in &loaded_theme.warnings {
        warn!(%warning, source = %loaded_theme.source_label(), "theme config fallback activated");
    }

    let loaded_tui_config = TuiBehaviorConfig::load(&cwd);
    for warning in &loaded_tui_config.warnings {
        warn!(%warning, source = %loaded_tui_config.source_label(), "tui config fallback activated");
    }

    let loaded_workflow_catalog = LoadedWorkflowCatalog::load(&cwd);
    for warning in &loaded_workflow_catalog.warnings {
        warn!(%warning, source = "workflow-catalog", "workflow config fallback activated");
    }

    let loaded_model_config = AgentModelConfig::load(&cwd);
    for warning in &loaded_model_config.warnings {
        warn!(%warning, source = %loaded_model_config.source_label(), "model config fallback activated");
    }
    info!(
        context_window = loaded_model_config.config.context_window,
        max_output_tokens = loaded_model_config.config.max_output_tokens,
        "model budget loaded"
    );

    let keymap_source = loaded_keymap.manager.source_label();
    let mut startup_warnings = Vec::new();
    if let Some(warning) = loaded_keymap.warning {
        startup_warnings.push(warning);
    }
    startup_warnings.extend(loaded_theme.warnings.iter().cloned());
    startup_warnings.extend(loaded_tui_config.warnings.iter().cloned());
    startup_warnings.extend(loaded_workflow_catalog.warnings.iter().cloned());
    startup_warnings.extend(loaded_model_config.warnings.iter().cloned());

    let session = AgentSession::new(AgentSessionConfig {
        client,
        system,
        cwd,
        runtime_handle: tokio::runtime::Handle::current(),
        scene_catalog: loaded_workflow_catalog.scene_catalog,
        workflow_catalog: loaded_workflow_catalog.workflow_catalog,
        prompt_catalog: loaded_workflow_catalog.prompt_catalog,
        context_window: loaded_model_config.config.context_window,
        max_output_tokens: loaded_model_config.config.max_output_tokens,
    })?;

    run_tui(TuiLaunchConfig {
        model_name,
        session,
        keymap: loaded_keymap.manager,
        theme: loaded_theme.theme,
        show_thinking: loaded_tui_config.config.show_thinking,
        keymap_source,
        startup_warnings,
        trace_rx,
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::default_system_prompt;
    use crate::model_config::AgentModelConfig;

    #[test]
    fn system_prompt_includes_workspace_path() {
        let prompt = default_system_prompt(Path::new("/tmp/omega"));

        assert!(prompt.contains("/tmp/omega"));
        assert!(prompt.contains("Use bash to solve tasks"));
    }

    #[test]
    fn model_config_defaults_to_expected_budgets() {
        let config = AgentModelConfig::default();
        assert_eq!(config.max_output_tokens, 32_000);
        assert_eq!(config.context_window, 200_000);
    }
}
