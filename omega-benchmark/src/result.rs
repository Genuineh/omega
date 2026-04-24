use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Result of executing a single benchmark case.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseResult {
    pub case_id: String,
    pub suite_id: String,
    pub status: CaseStatus,
    pub response: Option<String>,
    pub tool_trace: Vec<ToolTraceEntry>,
    pub delivery_summary: Option<serde_json::Value>,
    pub score: Option<ScoreBreakdown>,
    pub latency_ms: u64,
    pub total_tokens: u64,
    pub failure_reason: Option<String>,
}

/// Status of an individual case execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaseStatus {
    Pass,
    Fail,
    Error,
    Timeout,
    Skipped,
}

/// A tool invocation captured during case execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolTraceEntry {
    pub tool_name: String,
    pub arguments: serde_json::Value,
    pub result: Option<String>,
    pub is_error: bool,
}

/// Score breakdown for a case, varies by track.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreBreakdown {
    pub total: f64,
    pub metrics: BTreeMap<String, f64>,
}

impl ScoreBreakdown {
    pub fn single(name: &str, value: f64) -> Self {
        let mut metrics = BTreeMap::new();
        metrics.insert(name.to_string(), value);
        Self {
            total: value,
            metrics,
        }
    }

    pub fn from_metrics(metrics: BTreeMap<String, f64>) -> Self {
        let total = if metrics.is_empty() {
            0.0
        } else {
            metrics.values().sum::<f64>() / metrics.len() as f64
        };
        Self { total, metrics }
    }
}

/// Aggregate summary for a complete benchmark run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunSummary {
    pub run_id: String,
    pub model: String,
    pub timestamp: String,
    pub suites: Vec<SuiteSummary>,
    pub total_cases: usize,
    pub passed: usize,
    pub failed: usize,
    pub errors: usize,
    pub timeouts: usize,
    pub skipped: usize,
    pub aggregate_score: f64,
    pub total_latency_ms: u64,
    pub total_tokens: u64,
}

/// Per-suite summary within a run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuiteSummary {
    pub suite_id: String,
    pub track: String,
    pub case_count: usize,
    pub passed: usize,
    pub failed: usize,
    pub aggregate_score: f64,
    pub metrics: BTreeMap<String, f64>,
}

impl RunSummary {
    /// Build a run summary from a collection of case results.
    pub fn from_results(
        run_id: String,
        model: String,
        results: &[CaseResult],
        suite_track_map: &BTreeMap<String, String>,
    ) -> Self {
        let timestamp = chrono::Utc::now().to_rfc3339();
        let total_cases = results.len();
        let passed = results.iter().filter(|r| r.status == CaseStatus::Pass).count();
        let failed = results.iter().filter(|r| r.status == CaseStatus::Fail).count();
        let errors = results.iter().filter(|r| r.status == CaseStatus::Error).count();
        let timeouts = results.iter().filter(|r| r.status == CaseStatus::Timeout).count();
        let skipped = results.iter().filter(|r| r.status == CaseStatus::Skipped).count();
        let total_latency_ms: u64 = results.iter().map(|r| r.latency_ms).sum();
        let total_tokens: u64 = results.iter().map(|r| r.total_tokens).sum();

        let scored: Vec<f64> = results
            .iter()
            .filter_map(|r| r.score.as_ref().map(|s| s.total))
            .collect();
        let aggregate_score = if scored.is_empty() {
            0.0
        } else {
            scored.iter().sum::<f64>() / scored.len() as f64
        };

        // Group by suite
        let mut suite_results: BTreeMap<String, Vec<&CaseResult>> = BTreeMap::new();
        for r in results {
            suite_results.entry(r.suite_id.clone()).or_default().push(r);
        }

        let suites = suite_results
            .into_iter()
            .map(|(suite_id, cases)| {
                let case_count = cases.len();
                let s_passed = cases.iter().filter(|r| r.status == CaseStatus::Pass).count();
                let s_failed = cases.iter().filter(|r| r.status == CaseStatus::Fail).count();
                let s_scored: Vec<f64> = cases
                    .iter()
                    .filter_map(|r| r.score.as_ref().map(|s| s.total))
                    .collect();
                let s_aggregate = if s_scored.is_empty() {
                    0.0
                } else {
                    s_scored.iter().sum::<f64>() / s_scored.len() as f64
                };

                // Aggregate per-metric averages
                let mut metric_sums: BTreeMap<String, (f64, usize)> = BTreeMap::new();
                for c in &cases {
                    if let Some(score) = &c.score {
                        for (k, v) in &score.metrics {
                            let entry = metric_sums.entry(k.clone()).or_insert((0.0, 0));
                            entry.0 += v;
                            entry.1 += 1;
                        }
                    }
                }
                let metrics: BTreeMap<String, f64> = metric_sums
                    .into_iter()
                    .map(|(k, (sum, count))| (k, sum / count as f64))
                    .collect();

                let track = suite_track_map
                    .get(&suite_id)
                    .cloned()
                    .unwrap_or_else(|| "unknown".to_string());

                SuiteSummary {
                    suite_id,
                    track,
                    case_count,
                    passed: s_passed,
                    failed: s_failed,
                    aggregate_score: s_aggregate,
                    metrics,
                }
            })
            .collect();

        Self {
            run_id,
            model,
            timestamp,
            suites,
            total_cases,
            passed,
            failed,
            errors,
            timeouts,
            skipped,
            aggregate_score,
            total_latency_ms,
            total_tokens,
        }
    }
}
