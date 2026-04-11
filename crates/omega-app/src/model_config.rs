use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use omega_core::default_bash_allowed_commands;
use serde::Deserialize;

pub const DEFAULT_MODEL_CONFIG_PATH: &str = ".omega/model.toml";
const DEFAULT_MAX_OUTPUT_TOKENS: u32 = 32_000;
const DEFAULT_CONTEXT_WINDOW: u32 = 200_000;
const DEFAULT_SESSION_CONTEXT_BUDGET_TOKENS: usize = 400_000;

const DEFAULT_MODEL_CONFIG_TOML: &str = r#"# Model budget configuration
#
# `context_window` is the model's full context window size (input + output).
# This controls how much total content (system prompt, history, summaries,
# step context, and response) fits in a single request.
[context]
context_window = 200000

# `session_context_budget_tokens` controls how much session ledger history the
# resume and recall pipeline may reconstruct by default.
session_context_budget_tokens = 400000

# `max_tokens` controls the maximum assistant response length per request.
# It does NOT cap the full context window — only the output portion.
[request]
max_tokens = 32000

# `allowed_commands` replaces the built-in bash allowlist for the `bash` tool.
# Shell expansion, redirection, workspace escape, and dangerous sub-actions are
# still blocked even when a command name appears in this list.
[tools.bash]
allowed_commands = ["cat", "echo", "false", "find", "grep", "head", "ls", "printf", "pwd", "rg", "sleep", "tail", "touch", "tr", "true", "wait", "wc", "yes"]

# Provider pacing overrides apply after env bootstrap. Leave these commented to
# keep the transport defaults or env-derived values.
[provider]
# request_throttle_ms = 100
# max_concurrent_requests = 1
# rate_limit_retry_delay_ms = 10000

[tools.batch]
max_requests = 8

[tools.groups]
root_routing_blocked = ["bash", "batch", "read_file", "list_dir", "glob_search", "grep_search", "apply_patch", "create_file", "edit_file", "todo", "write_file", "load_skill"]
chat_blocked = ["apply_patch", "create_file", "edit_file", "todo", "write_file"]
feature_non_execute_blocked = ["bash", "apply_patch", "create_file", "edit_file", "todo", "write_file"]
"#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentModelConfig {
    pub context_window: u32,
    pub max_output_tokens: u32,
    pub session_context_budget_tokens: usize,
    pub bash_allowed_commands: Vec<String>,
    pub provider: ProviderPacingConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProviderPacingConfig {
    pub request_throttle_interval: Option<Duration>,
    pub max_concurrent_requests: Option<usize>,
    pub rate_limit_retry_delay: Option<Duration>,
}

impl Default for AgentModelConfig {
    fn default() -> Self {
        Self {
            context_window: DEFAULT_CONTEXT_WINDOW,
            max_output_tokens: DEFAULT_MAX_OUTPUT_TOKENS,
            session_context_budget_tokens: DEFAULT_SESSION_CONTEXT_BUDGET_TOKENS,
            bash_allowed_commands: default_bash_allowed_commands(),
            provider: ProviderPacingConfig::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedAgentModelConfig {
    pub config: AgentModelConfig,
    pub source: AgentModelConfigSource,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentModelConfigSource {
    BuiltinDefault,
    File(PathBuf),
    FileWithFallback(PathBuf),
}

impl AgentModelConfigSource {
    pub fn source_label(&self) -> String {
        match self {
            Self::BuiltinDefault => "builtin".to_string(),
            Self::File(path) | Self::FileWithFallback(path) => path.display().to_string(),
        }
    }
}

impl LoadedAgentModelConfig {
    pub fn source_label(&self) -> String {
        self.source.source_label()
    }
}

impl AgentModelConfig {
    pub fn load(root: &Path) -> LoadedAgentModelConfig {
        let path = root.join(DEFAULT_MODEL_CONFIG_PATH);
        if !path.exists() {
            return match Self::write_default_file(&path) {
                Ok(()) => match Self::load_from_file(&path) {
                    Ok((config, warnings)) => LoadedAgentModelConfig {
                        config,
                        source: AgentModelConfigSource::File(path),
                        warnings,
                    },
                    Err(error) => LoadedAgentModelConfig {
                        config: Self::default(),
                        source: AgentModelConfigSource::BuiltinDefault,
                        warnings: vec![format!(
                            "Default model config at {} was created but failed to load: {error}. Falling back to built-in defaults.",
                            path.display()
                        )],
                    },
                },
                Err(error) => LoadedAgentModelConfig {
                    config: Self::default(),
                    source: AgentModelConfigSource::BuiltinDefault,
                    warnings: vec![format!(
                        "Failed to create default model config at {}: {error}. Falling back to built-in defaults.",
                        path.display()
                    )],
                },
            };
        }

        match Self::load_from_file(&path) {
            Ok((config, warnings)) => LoadedAgentModelConfig {
                config,
                source: AgentModelConfigSource::File(path),
                warnings,
            },
            Err(error) => LoadedAgentModelConfig {
                config: Self::default(),
                source: AgentModelConfigSource::FileWithFallback(path.clone()),
                warnings: vec![format!(
                    "Invalid model config at {}: {error}. Falling back to built-in defaults.",
                    path.display()
                )],
            },
        }
    }

    fn load_from_file(path: &Path) -> Result<(Self, Vec<String>)> {
        let contents = fs::read_to_string(path)
            .with_context(|| format!("failed to read model config {}", path.display()))?;
        let file: AgentModelConfigFile = toml::from_str(&contents)
            .with_context(|| format!("failed to parse model config {}", path.display()))?;

        let mut config = Self::default();
        let mut warnings = Vec::new();
        if let Some(context) = file.context {
            if let Some(context_window) = context.context_window {
                if context_window == 0 {
                    anyhow::bail!("context.context_window must be >= 1");
                }
                config.context_window = context_window;
            }
            if let Some(session_context_budget_tokens) = context.session_context_budget_tokens {
                if session_context_budget_tokens == 0 {
                    anyhow::bail!("context.session_context_budget_tokens must be >= 1");
                }
                config.session_context_budget_tokens = session_context_budget_tokens;
            }
        }
        if let Some(request) = file.request {
            if let Some(max_tokens) = request.max_tokens {
                if max_tokens == 0 {
                    anyhow::bail!("request.max_tokens must be >= 1");
                }
                config.max_output_tokens = max_tokens;
            }
        }
        if let Some(tools) = file.tools {
            if let Some(bash) = tools.bash {
                if let Some(allowed_commands) = bash.allowed_commands {
                    config.bash_allowed_commands = normalize_allowed_commands(allowed_commands)?;
                }
            }
        }
        if let Some(provider) = file.provider {
            if let Some(request_throttle_ms) = provider.request_throttle_ms {
                config.provider.request_throttle_interval =
                    Some(Duration::from_millis(request_throttle_ms));
            }
            if let Some(max_concurrent_requests) = provider.max_concurrent_requests {
                if max_concurrent_requests == 0 {
                    anyhow::bail!("provider.max_concurrent_requests must be >= 1");
                }
                config.provider.max_concurrent_requests = Some(max_concurrent_requests);
            }
            if let Some(rate_limit_retry_delay_ms) = provider.rate_limit_retry_delay_ms {
                config.provider.rate_limit_retry_delay =
                    Some(Duration::from_millis(rate_limit_retry_delay_ms));
            }
        }

        if config.max_output_tokens >= config.context_window {
            warnings.push(format!(
                "request.max_tokens={} is too large for context.context_window={}. Resetting output budget to {}. If you want a larger transcript budget, set [context].context_window instead of [request].max_tokens.",
                config.max_output_tokens,
                config.context_window,
                DEFAULT_MAX_OUTPUT_TOKENS,
            ));
            config.max_output_tokens = DEFAULT_MAX_OUTPUT_TOKENS.min(config.context_window);
        }

        Ok((config, warnings))
    }

    fn write_default_file(path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create model config dir {}", parent.display())
            })?;
        }
        fs::write(path, DEFAULT_MODEL_CONFIG_TOML)
            .with_context(|| format!("failed to write model config {}", path.display()))?;
        Ok(())
    }
}

fn normalize_allowed_commands(allowed_commands: Vec<String>) -> Result<Vec<String>> {
    let mut normalized = Vec::new();
    let mut seen = std::collections::BTreeSet::new();

    for command in allowed_commands {
        let command = command.trim().to_ascii_lowercase();
        if command.is_empty() {
            anyhow::bail!("tools.bash.allowed_commands must not contain empty entries");
        }
        if seen.insert(command.clone()) {
            normalized.push(command);
        }
    }

    Ok(normalized)
}

#[derive(Debug, Deserialize)]
struct AgentModelConfigFile {
    #[serde(default)]
    context: Option<ContextConfigFile>,
    #[serde(default)]
    request: Option<RequestConfigFile>,
    #[serde(default)]
    provider: Option<ProviderConfigFile>,
    #[serde(default)]
    tools: Option<ToolsConfigFile>,
}

#[derive(Debug, Deserialize)]
struct ContextConfigFile {
    #[serde(default)]
    context_window: Option<u32>,
    #[serde(default)]
    session_context_budget_tokens: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct RequestConfigFile {
    #[serde(default)]
    max_tokens: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct ProviderConfigFile {
    #[serde(default)]
    request_throttle_ms: Option<u64>,
    #[serde(default)]
    max_concurrent_requests: Option<usize>,
    #[serde(default)]
    rate_limit_retry_delay_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct ToolsConfigFile {
    #[serde(default)]
    bash: Option<BashToolConfigFile>,
}

#[derive(Debug, Deserialize)]
struct BashToolConfigFile {
    #[serde(default)]
    allowed_commands: Option<Vec<String>>,
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{AgentModelConfig, DEFAULT_MODEL_CONFIG_PATH};

    fn temp_root(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "omega-model-config-{}-{}",
            name,
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    use std::path::PathBuf;

    #[test]
    fn missing_file_writes_default_config() {
        let root = temp_root("missing");

        let loaded = AgentModelConfig::load(&root);
        let written = std::fs::read_to_string(root.join(DEFAULT_MODEL_CONFIG_PATH)).unwrap();

        assert!(loaded.warnings.is_empty());
        assert_eq!(loaded.config.max_output_tokens, 32_000);
        assert_eq!(loaded.config.context_window, 200_000);
        assert_eq!(loaded.config.session_context_budget_tokens, 400_000);
        assert!(loaded
            .config
            .bash_allowed_commands
            .iter()
            .any(|command| command == "find"));
        assert!(loaded
            .config
            .bash_allowed_commands
            .iter()
            .any(|command| command == "grep"));
        assert!(written.contains("max_tokens = 32000"));
        assert!(written.contains("context_window = 200000"));
        assert!(written.contains("session_context_budget_tokens = 400000"));
        assert!(written.contains("allowed_commands"));
        assert!(written.contains("request_throttle_ms = 100"));
        assert!(loaded.config.provider.request_throttle_interval.is_none());
        assert!(loaded.config.provider.max_concurrent_requests.is_none());
        assert!(loaded.config.provider.rate_limit_retry_delay.is_none());
    }

    #[test]
    fn file_override_updates_request_budget() {
        let root = temp_root("override");
        let omega_dir = root.join(".omega");
        std::fs::create_dir_all(&omega_dir).unwrap();
        std::fs::write(
            omega_dir.join("model.toml"),
            "[context]\ncontext_window = 128000\nsession_context_budget_tokens = 512000\n\n[request]\nmax_tokens = 64000\n",
        )
        .unwrap();

        let loaded = AgentModelConfig::load(&root);

        assert!(loaded.warnings.is_empty());
        assert_eq!(loaded.config.max_output_tokens, 64_000);
        assert_eq!(loaded.config.context_window, 128_000);
        assert_eq!(loaded.config.session_context_budget_tokens, 512_000);
    }

    #[test]
    fn file_override_updates_provider_pacing() {
        let root = temp_root("provider-pacing");
        let omega_dir = root.join(".omega");
        std::fs::create_dir_all(&omega_dir).unwrap();
        std::fs::write(
            omega_dir.join("model.toml"),
            "[provider]\nrequest_throttle_ms = 250\nmax_concurrent_requests = 3\nrate_limit_retry_delay_ms = 12000\n",
        )
        .unwrap();

        let loaded = AgentModelConfig::load(&root);

        assert!(loaded.warnings.is_empty());
        assert_eq!(
            loaded.config.provider.request_throttle_interval,
            Some(Duration::from_millis(250))
        );
        assert_eq!(loaded.config.provider.max_concurrent_requests, Some(3));
        assert_eq!(
            loaded.config.provider.rate_limit_retry_delay,
            Some(Duration::from_millis(12_000))
        );
    }

    #[test]
    fn file_override_updates_bash_allowlist() {
        let root = temp_root("bash-allowlist");
        let omega_dir = root.join(".omega");
        std::fs::create_dir_all(&omega_dir).unwrap();
        std::fs::write(
            omega_dir.join("model.toml"),
            "[context]\ncontext_window = 128000\n\n[request]\nmax_tokens = 64000\n\n[tools.bash]\nallowed_commands = [\"ls\", \"find\", \"grep\"]\n",
        )
        .unwrap();

        let loaded = AgentModelConfig::load(&root);

        assert!(loaded.warnings.is_empty());
        assert_eq!(
            loaded.config.bash_allowed_commands,
            vec!["ls".to_string(), "find".to_string(), "grep".to_string()]
        );
    }

    #[test]
    fn file_override_rejects_empty_bash_allowlist_entries() {
        let root = temp_root("bash-allowlist-invalid");
        let omega_dir = root.join(".omega");
        std::fs::create_dir_all(&omega_dir).unwrap();
        std::fs::write(
            omega_dir.join("model.toml"),
            "[tools.bash]\nallowed_commands = [\"ls\", \"\"]\n",
        )
        .unwrap();

        let loaded = AgentModelConfig::load(&root);

        assert!(matches!(
            loaded.source,
            super::AgentModelConfigSource::FileWithFallback(_)
        ));
        assert!(loaded.warnings[0].contains("empty entries"));
    }

    #[test]
    fn file_override_rejects_zero_provider_concurrency() {
        let root = temp_root("provider-pacing-invalid");
        let omega_dir = root.join(".omega");
        std::fs::create_dir_all(&omega_dir).unwrap();
        std::fs::write(
            omega_dir.join("model.toml"),
            "[provider]\nmax_concurrent_requests = 0\n",
        )
        .unwrap();

        let loaded = AgentModelConfig::load(&root);

        assert!(matches!(
            loaded.source,
            super::AgentModelConfigSource::FileWithFallback(_)
        ));
        assert!(loaded.warnings[0].contains("provider.max_concurrent_requests must be >= 1"));
    }

    #[test]
    fn oversized_request_budget_falls_back_to_default_output_tokens() {
        let root = temp_root("oversized-request-budget");
        let omega_dir = root.join(".omega");
        std::fs::create_dir_all(&omega_dir).unwrap();
        std::fs::write(
            omega_dir.join("model.toml"),
            "[context]\ncontext_window = 204800\n\n[request]\nmax_tokens = 204800\n",
        )
        .unwrap();

        let loaded = AgentModelConfig::load(&root);

        assert_eq!(loaded.config.context_window, 204_800);
        assert_eq!(loaded.config.max_output_tokens, 32_000);
        assert_eq!(loaded.warnings.len(), 1);
        assert!(loaded.warnings[0].contains("request.max_tokens=204800"));
    }
}
