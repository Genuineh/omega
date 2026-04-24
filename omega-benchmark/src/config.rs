use serde::{Deserialize, Serialize};

/// Configuration for a single benchmark run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunConfig {
    /// Model identifier to use for this run.
    pub model: String,
    /// Maximum number of agent turns allowed per case.
    #[serde(default = "default_max_turns")]
    pub max_turns: u32,
    /// Maximum tool invocations allowed per case.
    #[serde(default)]
    pub tool_budget: Option<u32>,
    /// Per-case timeout in seconds (overrides case-level timeout).
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    /// Optional fixed seed for deterministic execution.
    #[serde(default)]
    pub seed: Option<u64>,
    /// Suite IDs to include. Empty means all registered suites.
    #[serde(default)]
    pub suite_filter: Vec<String>,
    /// Track filter. Empty means all tracks.
    #[serde(default)]
    pub track_filter: Vec<String>,
    /// Tag filter applied to individual cases.
    #[serde(default)]
    pub tag_filter: Vec<String>,
}

fn default_max_turns() -> u32 {
    10
}

impl Default for RunConfig {
    fn default() -> Self {
        Self {
            model: "default".to_string(),
            max_turns: default_max_turns(),
            tool_budget: None,
            timeout_secs: None,
            seed: None,
            suite_filter: Vec::new(),
            track_filter: Vec::new(),
            tag_filter: Vec::new(),
        }
    }
}

impl RunConfig {
    /// Load a run config from a JSON file.
    pub fn load(path: &std::path::Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Self = serde_json::from_str(&content)?;
        Ok(config)
    }

    /// Effective timeout for a case, preferring config-level over case-level.
    pub fn effective_timeout(&self, case_timeout: Option<u64>) -> Option<u64> {
        self.timeout_secs.or(case_timeout)
    }
}
