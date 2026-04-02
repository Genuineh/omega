use std::path::Path;
use std::sync::Arc;

mod env_config;
mod model_config;
mod runtime_message_policy;

use omega_core::{DynLlmClient, MinimaxClient, MinimaxConfig};
use omega_keymap::KeymapManager;
use omega_observability::init_tracing_channel;
use omega_session::{AgentSession, AgentSessionConfig};
use omega_theme::OmegaTheme;
use omega_tui::{run as run_tui, TuiBehaviorConfig, TuiLaunchConfig};
use omega_workflow::LoadedWorkflowCatalog;
use tracing::{info, warn};

use crate::env_config::AppEnvConfig;
use crate::model_config::{AgentModelConfig, ProviderPacingConfig};
use crate::runtime_message_policy::DefaultRuntimeMessagePolicy;

pub fn default_system_prompt(cwd: &Path) -> String {
    format!(
        "You are a coding agent at {}. Prefer structured workspace tools for inspection and editing, and use bash only as a fallback for simple allowlisted commands. Act, don't explain.",
        cwd.display()
    )
}

fn apply_provider_pacing_overrides(
    mut config: MinimaxConfig,
    provider: &ProviderPacingConfig,
) -> MinimaxConfig {
    if let Some(request_throttle_interval) = provider.request_throttle_interval {
        config = config.with_request_throttle_interval(request_throttle_interval);
    }
    if let Some(max_concurrent_requests) = provider.max_concurrent_requests {
        config = config.with_max_concurrent_requests(max_concurrent_requests);
    }
    if let Some(rate_limit_retry_delay) = provider.rate_limit_retry_delay {
        config = config.with_rate_limit_retry_delay(rate_limit_retry_delay);
    }

    config
}

pub async fn run() -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    let loaded_env_config = AppEnvConfig::load_and_apply(&cwd);
    let trace_rx = init_tracing_channel()?;
    for warning in &loaded_env_config.warnings {
        warn!(%warning, source = %loaded_env_config.source_label(), "env config fallback activated");
    }
    if !loaded_env_config.applied_keys.is_empty() {
        info!(
            source = %loaded_env_config.source_label(),
            keys = %loaded_env_config.applied_keys.join(", "),
            "startup env config applied"
        );
    }
    if !loaded_env_config.skipped_existing_keys.is_empty() {
        info!(
            source = %loaded_env_config.source_label(),
            keys = %loaded_env_config.skipped_existing_keys.join(", "),
            "startup env config skipped already-set variables"
        );
    }

    let loaded_model_config = AgentModelConfig::load(&cwd);
    for warning in &loaded_model_config.warnings {
        warn!(%warning, source = %loaded_model_config.source_label(), "model config fallback activated");
    }

    let config = apply_provider_pacing_overrides(
        MinimaxConfig::from_env().map_err(|e| anyhow::anyhow!("{e}"))?,
        &loaded_model_config.config.provider,
    );
    let provider_request_throttle_ms = config.request_throttle_interval.as_millis() as u64;
    let provider_max_concurrent_requests = config.max_concurrent_requests;
    let provider_rate_limit_retry_delay_ms = config.rate_limit_retry_delay.as_millis() as u64;
    let model_name = config.model.clone();
    let client: DynLlmClient =
        Arc::new(MinimaxClient::new(config).map_err(|e| anyhow::anyhow!("{e}"))?);

    let system = default_system_prompt(&cwd);
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
    info!(
        context_window = loaded_model_config.config.context_window,
        max_output_tokens = loaded_model_config.config.max_output_tokens,
        provider_request_throttle_ms,
        provider_max_concurrent_requests,
        provider_rate_limit_retry_delay_ms,
        bash_allowed_commands = loaded_workflow_catalog
            .tool_policy
            .bash_allowed_commands
            .len(),
        batch_max_requests = loaded_workflow_catalog.tool_policy.batch_max_requests,
        "model budget loaded"
    );

    let keymap_source = loaded_keymap.manager.source_label();
    let mut startup_warnings = Vec::new();
    if let Some(warning) = loaded_keymap.warning {
        startup_warnings.push(warning);
    }
    startup_warnings.extend(loaded_env_config.warnings.iter().cloned());
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
        bash_allowed_commands: loaded_workflow_catalog
            .tool_policy
            .bash_allowed_commands
            .clone(),
        batch_max_requests: loaded_workflow_catalog.tool_policy.batch_max_requests,
    })?;

    run_tui(TuiLaunchConfig {
        model_name,
        session,
        runtime_message_policy: Arc::new(DefaultRuntimeMessagePolicy),
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
    use std::time::Duration;

    use omega_core::MinimaxConfig;

    use super::{apply_provider_pacing_overrides, default_system_prompt};
    use crate::model_config::{AgentModelConfig, ProviderPacingConfig};

    #[test]
    fn system_prompt_includes_workspace_path() {
        let prompt = default_system_prompt(Path::new("/tmp/omega"));

        assert!(prompt.contains("/tmp/omega"));
        assert!(prompt.contains("Prefer structured workspace tools"));
    }

    #[test]
    fn model_config_defaults_to_expected_budgets() {
        let config = AgentModelConfig::default();
        assert_eq!(config.max_output_tokens, 32_000);
        assert_eq!(config.context_window, 200_000);
        assert!(config
            .bash_allowed_commands
            .iter()
            .any(|command| command == "find"));
        assert!(config
            .bash_allowed_commands
            .iter()
            .any(|command| command == "grep"));
        assert_eq!(config.provider, ProviderPacingConfig::default());
    }

    #[test]
    fn provider_pacing_overrides_apply_only_when_present() {
        let config = MinimaxConfig::international("key", "model-a");
        let overrides = ProviderPacingConfig {
            request_throttle_interval: Some(Duration::from_millis(250)),
            max_concurrent_requests: Some(3),
            rate_limit_retry_delay: Some(Duration::from_secs(12)),
        };

        let updated = apply_provider_pacing_overrides(config, &overrides);

        assert_eq!(updated.request_throttle_interval, Duration::from_millis(250));
        assert_eq!(updated.max_concurrent_requests, 3);
        assert_eq!(updated.rate_limit_retry_delay, Duration::from_secs(12));
    }

    #[test]
    fn provider_pacing_overrides_preserve_env_bootstrap_when_absent() {
        let config = MinimaxConfig::international("key", "model-a")
            .with_request_throttle_interval(Duration::from_millis(175))
            .with_max_concurrent_requests(2)
            .with_rate_limit_retry_delay(Duration::from_secs(9));

        let updated = apply_provider_pacing_overrides(config.clone(), &ProviderPacingConfig::default());

        assert_eq!(updated, config);
    }
}
