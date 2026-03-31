use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

pub const DEFAULT_ENV_CONFIG_PATH: &str = ".omega/env.toml";

const DEFAULT_ENV_CONFIG_TOML: &str = r#"# Repository-local environment overrides loaded by omega-app at startup.
#
# Existing process environment wins over this file. Use this file to provide
# repo-scoped defaults for provider bootstrap, logging, and other startup-time
# settings that downstream crates read via from_env-style config.
[env]
# OMEGA_API_KEY = ""
# OMEGA_MODEL_ID = "MiniMax-M1"
# OMEGA_LOG = "info"
"#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppEnvConfig {
    entries: Vec<(String, String)>,
}

impl Default for AppEnvConfig {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedAppEnvConfig {
    pub source: AppEnvConfigSource,
    pub warnings: Vec<String>,
    pub applied_keys: Vec<String>,
    pub skipped_existing_keys: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppEnvConfigSource {
    BuiltinDefault,
    File(PathBuf),
    FileWithFallback(PathBuf),
}

impl AppEnvConfigSource {
    pub fn source_label(&self) -> String {
        match self {
            Self::BuiltinDefault => "builtin".to_string(),
            Self::File(path) | Self::FileWithFallback(path) => path.display().to_string(),
        }
    }
}

impl LoadedAppEnvConfig {
    pub fn source_label(&self) -> String {
        self.source.source_label()
    }
}

impl AppEnvConfig {
    pub fn load_and_apply(root: &Path) -> LoadedAppEnvConfig {
        let path = root.join(DEFAULT_ENV_CONFIG_PATH);
        if !path.exists() {
            return match Self::write_default_file(&path) {
                Ok(()) => match Self::load_from_file(&path) {
                    Ok(config) => config.apply(AppEnvConfigSource::File(path)),
                    Err(error) => LoadedAppEnvConfig {
                        source: AppEnvConfigSource::BuiltinDefault,
                        warnings: vec![format!(
                            "Default env config at {} was created but failed to load: {error}. Continuing without env overrides.",
                            path.display()
                        )],
                        applied_keys: Vec::new(),
                        skipped_existing_keys: Vec::new(),
                    },
                },
                Err(error) => LoadedAppEnvConfig {
                    source: AppEnvConfigSource::BuiltinDefault,
                    warnings: vec![format!(
                        "Failed to create default env config at {}: {error}. Continuing without env overrides.",
                        path.display()
                    )],
                    applied_keys: Vec::new(),
                    skipped_existing_keys: Vec::new(),
                },
            };
        }

        match Self::load_from_file(&path) {
            Ok(config) => config.apply(AppEnvConfigSource::File(path)),
            Err(error) => LoadedAppEnvConfig {
                source: AppEnvConfigSource::FileWithFallback(path.clone()),
                warnings: vec![format!(
                    "Invalid env config at {}: {error}. Continuing without env overrides.",
                    path.display()
                )],
                applied_keys: Vec::new(),
                skipped_existing_keys: Vec::new(),
            },
        }
    }

    fn load_from_file(path: &Path) -> Result<Self> {
        let contents = fs::read_to_string(path)
            .with_context(|| format!("failed to read env config {}", path.display()))?;
        let file: AppEnvConfigFile = toml::from_str(&contents)
            .with_context(|| format!("failed to parse env config {}", path.display()))?;

        let entries = normalize_entries(file.env.unwrap_or_default())?;
        Ok(Self { entries })
    }

    fn write_default_file(path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create env config dir {}", parent.display()))?;
        }
        fs::write(path, DEFAULT_ENV_CONFIG_TOML)
            .with_context(|| format!("failed to write env config {}", path.display()))?;
        Ok(())
    }

    fn apply(self, source: AppEnvConfigSource) -> LoadedAppEnvConfig {
        let mut applied_keys = Vec::new();
        let mut skipped_existing_keys = Vec::new();

        for (key, value) in self.entries {
            if std::env::var_os(&key).is_some() {
                skipped_existing_keys.push(key);
                continue;
            }

            std::env::set_var(&key, value);
            applied_keys.push(key);
        }

        LoadedAppEnvConfig {
            source,
            warnings: Vec::new(),
            applied_keys,
            skipped_existing_keys,
        }
    }
}

fn normalize_entries(entries: BTreeMap<String, String>) -> Result<Vec<(String, String)>> {
    let mut normalized = Vec::new();

    for (key, value) in entries {
        validate_env_key(&key)?;
        normalized.push((key, value));
    }

    Ok(normalized)
}

fn validate_env_key(key: &str) -> Result<()> {
    if key.is_empty() {
        anyhow::bail!("env keys must not be empty");
    }

    let mut chars = key.chars();
    let first = chars.next().expect("validated non-empty env key");
    if !(first.is_ascii_alphabetic() || first == '_') {
        anyhow::bail!("env key '{key}' must start with an ASCII letter or underscore");
    }

    if !chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_') {
        anyhow::bail!("env key '{key}' must contain only ASCII letters, digits, or underscores");
    }

    Ok(())
}

#[derive(Debug, Deserialize)]
struct AppEnvConfigFile {
    #[serde(default)]
    env: Option<BTreeMap<String, String>>,
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{AppEnvConfig, AppEnvConfigSource, DEFAULT_ENV_CONFIG_PATH};

    fn temp_root(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("omega-env-config-{}-{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn unique_env_key(name: &str) -> String {
        format!(
            "OMEGA_TEST_ENV_{}_{}",
            name.to_ascii_uppercase(),
            std::process::id()
        )
    }

    #[test]
    fn missing_file_writes_default_template() {
        let root = temp_root("missing");

        let loaded = AppEnvConfig::load_and_apply(&root);
        let written = std::fs::read_to_string(root.join(DEFAULT_ENV_CONFIG_PATH)).unwrap();

        assert!(loaded.warnings.is_empty());
        assert!(loaded.applied_keys.is_empty());
        assert!(loaded.skipped_existing_keys.is_empty());
        assert!(written.contains("[env]"));
        assert!(written.contains("OMEGA_API_KEY"));
    }

    #[test]
    fn env_file_applies_missing_variables() {
        let root = temp_root("apply");
        let omega_dir = root.join(".omega");
        let env_key = unique_env_key("apply");
        std::fs::create_dir_all(&omega_dir).unwrap();
        std::fs::write(
            omega_dir.join("env.toml"),
            format!("[env]\n{env_key} = \"configured\"\n"),
        )
        .unwrap();
        std::env::remove_var(&env_key);

        let loaded = AppEnvConfig::load_and_apply(&root);

        assert!(loaded.warnings.is_empty());
        assert_eq!(std::env::var(&env_key).unwrap(), "configured");
        assert_eq!(loaded.applied_keys, vec![env_key.clone()]);
        assert!(loaded.skipped_existing_keys.is_empty());

        std::env::remove_var(env_key);
    }

    #[test]
    fn existing_process_env_wins_over_env_file() {
        let root = temp_root("existing-wins");
        let omega_dir = root.join(".omega");
        let env_key = unique_env_key("existing");
        std::fs::create_dir_all(&omega_dir).unwrap();
        std::fs::write(
            omega_dir.join("env.toml"),
            format!("[env]\n{env_key} = \"from-file\"\n"),
        )
        .unwrap();
        std::env::set_var(&env_key, "from-shell");

        let loaded = AppEnvConfig::load_and_apply(&root);

        assert!(loaded.warnings.is_empty());
        assert_eq!(std::env::var(&env_key).unwrap(), "from-shell");
        assert!(loaded.applied_keys.is_empty());
        assert_eq!(loaded.skipped_existing_keys, vec![env_key.clone()]);

        std::env::remove_var(env_key);
    }

    #[test]
    fn invalid_env_key_falls_back_without_applying_anything() {
        let root = temp_root("invalid-key");
        let omega_dir = root.join(".omega");
        std::fs::create_dir_all(&omega_dir).unwrap();
        std::fs::write(omega_dir.join("env.toml"), "[env]\n1INVALID = \"value\"\n").unwrap();

        let loaded = AppEnvConfig::load_and_apply(&root);

        assert!(matches!(
            loaded.source,
            AppEnvConfigSource::FileWithFallback(_)
        ));
        assert!(loaded.applied_keys.is_empty());
        assert!(loaded.warnings[0].contains("must start with an ASCII letter or underscore"));
    }
}
