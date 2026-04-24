use serde::{Deserialize, Serialize};

/// Benchmark evaluation track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Track {
    ToolCalling,
    Assistant,
    DataQuality,
}

impl Track {
    pub fn as_str(&self) -> &'static str {
        match self {
            Track::ToolCalling => "tool-calling",
            Track::Assistant => "assistant",
            Track::DataQuality => "data-quality",
        }
    }
}

impl std::fmt::Display for Track {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Top-level manifest for a benchmark suite.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuiteManifest {
    pub id: String,
    pub track: Track,
    pub description: String,
    pub cases: Vec<BenchmarkCase>,
    #[serde(default)]
    pub default_scorer: Option<String>,
    #[serde(default)]
    pub fixture_root: Option<String>,
}

/// A single benchmark case within a suite.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkCase {
    pub id: String,
    pub prompt: String,
    #[serde(default)]
    pub expected: Option<ExpectedOutcome>,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

/// Expected outcome for a benchmark case.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExpectedOutcome {
    ExactMatch { value: String },
    QuasiExactMatch { value: String, tolerance: Option<String> },
    ToolCalls { calls: Vec<ExpectedToolCall> },
    SchemaValid { schema: serde_json::Value },
    JudgePass { rubric: String },
    NoToolCall,
}

/// An expected tool call for tool-calling track validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpectedToolCall {
    pub name: String,
    #[serde(default)]
    pub arguments: serde_json::Value,
    #[serde(default)]
    pub order: Option<u32>,
}

impl SuiteManifest {
    /// Load a suite manifest from a JSON file path.
    pub fn load(path: &std::path::Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let manifest: Self = serde_json::from_str(&content)?;
        Ok(manifest)
    }

    /// Number of cases in this suite.
    pub fn case_count(&self) -> usize {
        self.cases.len()
    }
}
