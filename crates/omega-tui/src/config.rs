use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use omega_project_layout::{OmegaProjectLayout, TUI_CONFIG_PATH};
use serde::Deserialize;

pub const DEFAULT_TUI_CONFIG_PATH: &str = TUI_CONFIG_PATH;

const DEFAULT_TUI_CONFIG_TOML: &str = r#"# Default omega-tui behavior overrides
[response]
show_thinking = true
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TuiBehaviorConfig {
    pub show_thinking: bool,
}

impl Default for TuiBehaviorConfig {
    fn default() -> Self {
        Self {
            show_thinking: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedTuiBehaviorConfig {
    pub config: TuiBehaviorConfig,
    pub source: TuiConfigSource,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TuiConfigSource {
    BuiltinDefault,
    File(PathBuf),
    FileWithFallback(PathBuf),
}

impl TuiConfigSource {
    pub fn source_label(&self) -> String {
        match self {
            Self::BuiltinDefault => "builtin".to_string(),
            Self::File(path) | Self::FileWithFallback(path) => path.display().to_string(),
        }
    }
}

impl LoadedTuiBehaviorConfig {
    pub fn source_label(&self) -> String {
        self.source.source_label()
    }
}

impl TuiBehaviorConfig {
    pub fn load(root: &Path) -> LoadedTuiBehaviorConfig {
        let path = OmegaProjectLayout::new(root.to_path_buf()).tui_config_path();
        if !path.exists() {
            match Self::write_default_file(&path) {
                Ok(()) => {
                    return match Self::load_from_file(&path) {
                        Ok(config) => LoadedTuiBehaviorConfig {
                            config,
                            source: TuiConfigSource::File(path),
                            warnings: Vec::new(),
                        },
                        Err(error) => LoadedTuiBehaviorConfig {
                            config: Self::default(),
                            source: TuiConfigSource::BuiltinDefault,
                            warnings: vec![format!(
                                "Default TUI config at {} was created but failed to load: {error}. Falling back to built-in defaults.",
                                path.display()
                            )],
                        },
                    };
                }
                Err(error) => {
                    return LoadedTuiBehaviorConfig {
                        config: Self::default(),
                        source: TuiConfigSource::BuiltinDefault,
                        warnings: vec![format!(
                            "Failed to create default TUI config at {}: {error}. Falling back to built-in defaults.",
                            path.display()
                        )],
                    };
                }
            }
        }

        match Self::load_from_file(&path) {
            Ok(config) => LoadedTuiBehaviorConfig {
                config,
                source: TuiConfigSource::File(path),
                warnings: Vec::new(),
            },
            Err(error) => LoadedTuiBehaviorConfig {
                config: Self::default(),
                source: TuiConfigSource::FileWithFallback(path.clone()),
                warnings: vec![format!(
                    "Invalid TUI config at {}: {error}. Falling back to built-in defaults.",
                    path.display()
                )],
            },
        }
    }

    fn load_from_file(path: &Path) -> Result<Self> {
        let contents = fs::read_to_string(path)
            .with_context(|| format!("failed to read TUI config {}", path.display()))?;
        let file: TuiConfigFile = toml::from_str(&contents)
            .with_context(|| format!("failed to parse TUI config {}", path.display()))?;

        let mut config = Self::default();
        if let Some(response) = file.response {
            if let Some(show_thinking) = response.show_thinking {
                config.show_thinking = show_thinking;
            }
        }

        Ok(config)
    }

    fn write_default_file(path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create TUI config directory {}", parent.display())
            })?;
        }
        fs::write(path, DEFAULT_TUI_CONFIG_TOML)
            .with_context(|| format!("failed to write TUI config {}", path.display()))?;
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct TuiConfigFile {
    #[serde(default)]
    response: Option<ResponseConfigFile>,
}

#[derive(Debug, Deserialize)]
struct ResponseConfigFile {
    #[serde(default)]
    show_thinking: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::{TuiBehaviorConfig, DEFAULT_TUI_CONFIG_PATH};

    fn temp_root(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("omega-tui-config-{}-{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    use std::path::PathBuf;

    #[test]
    fn missing_file_writes_default_config() {
        let root = temp_root("missing");

        let loaded = TuiBehaviorConfig::load(&root);
        let written = std::fs::read_to_string(root.join(DEFAULT_TUI_CONFIG_PATH)).unwrap();

        assert!(loaded.warnings.is_empty());
        assert!(loaded.config.show_thinking);
        assert!(written.contains("show_thinking = true"));
    }

    #[test]
    fn file_override_can_hide_thinking() {
        let root = temp_root("override");
        let omega_dir = root.join(".omega");
        std::fs::create_dir_all(&omega_dir).unwrap();
        std::fs::write(
            omega_dir.join("tui.toml"),
            "[response]\nshow_thinking = false\n",
        )
        .unwrap();

        let loaded = TuiBehaviorConfig::load(&root);

        assert!(loaded.warnings.is_empty());
        assert!(!loaded.config.show_thinking);
    }
}
