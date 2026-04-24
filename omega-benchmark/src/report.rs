use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::result::{CaseResult, RunSummary};

/// Manages benchmark run artifacts and baseline comparisons.
pub struct ReportStore {
    /// Directory for per-run result files.
    runs_dir: PathBuf,
    /// Directory for committed baseline summaries.
    baselines_dir: PathBuf,
}

/// Comparison between a current run and a baseline.
#[derive(Debug, serde::Serialize)]
pub struct BaselineDiff {
    pub run_id: String,
    pub baseline_id: String,
    pub overall: DiffDirection,
    pub score_delta: f64,
    pub suite_diffs: Vec<SuiteDiff>,
}

#[derive(Debug, serde::Serialize)]
pub struct SuiteDiff {
    pub suite_id: String,
    pub direction: DiffDirection,
    pub score_delta: f64,
    pub metric_deltas: BTreeMap<String, f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffDirection {
    Improved,
    Regressed,
    Unchanged,
}

impl ReportStore {
    pub fn new(runs_dir: PathBuf, baselines_dir: PathBuf) -> Self {
        Self {
            runs_dir,
            baselines_dir,
        }
    }

    /// Initialize from a benchmark root directory.
    ///
    /// Uses `<root>/baselines/` for committed baselines and
    /// a state directory for run artifacts.
    pub fn from_root(benchmark_root: &Path, state_dir: &Path) -> Self {
        Self {
            runs_dir: state_dir.join("benchmark").join("runs"),
            baselines_dir: benchmark_root.join("baselines"),
        }
    }

    /// Save per-case results for a run.
    pub fn save_results(&self, run_id: &str, results: &[CaseResult]) -> anyhow::Result<PathBuf> {
        std::fs::create_dir_all(&self.runs_dir)?;
        let path = self.runs_dir.join(format!("{run_id}-results.json"));
        let json = serde_json::to_string_pretty(results)?;
        std::fs::write(&path, json)?;
        Ok(path)
    }

    /// Save a run summary.
    pub fn save_summary(&self, summary: &RunSummary) -> anyhow::Result<PathBuf> {
        std::fs::create_dir_all(&self.runs_dir)?;
        let path = self.runs_dir.join(format!("{}-summary.json", summary.run_id));
        let json = serde_json::to_string_pretty(summary)?;
        std::fs::write(&path, json)?;
        Ok(path)
    }

    /// Save a run summary as the new baseline for comparison.
    pub fn save_baseline(&self, summary: &RunSummary) -> anyhow::Result<PathBuf> {
        std::fs::create_dir_all(&self.baselines_dir)?;
        let path = self.baselines_dir.join(format!("{}.json", summary.run_id));
        let json = serde_json::to_string_pretty(summary)?;
        std::fs::write(&path, json)?;
        Ok(path)
    }

    /// Load the latest baseline summary, if any exist.
    pub fn load_latest_baseline(&self) -> anyhow::Result<Option<RunSummary>> {
        if !self.baselines_dir.is_dir() {
            return Ok(None);
        }

        let mut entries: Vec<_> = std::fs::read_dir(&self.baselines_dir)?
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .map_or(false, |ext| ext == "json")
            })
            .collect();

        if entries.is_empty() {
            return Ok(None);
        }

        // Sort by filename descending to get the latest
        entries.sort_by(|a, b| b.file_name().cmp(&a.file_name()));

        let content = std::fs::read_to_string(entries[0].path())?;
        let summary: RunSummary = serde_json::from_str(&content)?;
        Ok(Some(summary))
    }

    /// Load a specific baseline by run ID.
    pub fn load_baseline(&self, run_id: &str) -> anyhow::Result<Option<RunSummary>> {
        let path = self.baselines_dir.join(format!("{run_id}.json"));
        if !path.is_file() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(path)?;
        let summary: RunSummary = serde_json::from_str(&content)?;
        Ok(Some(summary))
    }

    /// Compare a run summary against a baseline.
    pub fn compare(&self, current: &RunSummary, baseline: &RunSummary) -> BaselineDiff {
        let score_delta = current.aggregate_score - baseline.aggregate_score;
        let overall = classify_delta(score_delta);

        let baseline_suites: BTreeMap<String, &crate::result::SuiteSummary> = baseline
            .suites
            .iter()
            .map(|s| (s.suite_id.clone(), s))
            .collect();

        let suite_diffs: Vec<SuiteDiff> = current
            .suites
            .iter()
            .map(|cs| {
                let (dir, sd, metric_deltas) = match baseline_suites.get(&cs.suite_id) {
                    Some(bs) => {
                        let delta = cs.aggregate_score - bs.aggregate_score;
                        let mut md = BTreeMap::new();
                        for (k, v) in &cs.metrics {
                            let base_v = bs.metrics.get(k).copied().unwrap_or(0.0);
                            md.insert(k.clone(), v - base_v);
                        }
                        (classify_delta(delta), delta, md)
                    }
                    None => (DiffDirection::Unchanged, 0.0, BTreeMap::new()),
                };
                SuiteDiff {
                    suite_id: cs.suite_id.clone(),
                    direction: dir,
                    score_delta: sd,
                    metric_deltas,
                }
            })
            .collect();

        BaselineDiff {
            run_id: current.run_id.clone(),
            baseline_id: baseline.run_id.clone(),
            overall,
            score_delta,
            suite_diffs,
        }
    }

    /// Format a baseline diff for terminal display.
    pub fn format_diff(diff: &BaselineDiff) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "Baseline comparison: {} vs {}\n",
            diff.run_id, diff.baseline_id
        ));
        out.push_str(&format!(
            "Overall: {:?} ({:+.3})\n",
            diff.overall, diff.score_delta
        ));
        out.push('\n');

        for sd in &diff.suite_diffs {
            out.push_str(&format!(
                "  {}: {:?} ({:+.3})\n",
                sd.suite_id, sd.direction, sd.score_delta
            ));
            for (k, v) in &sd.metric_deltas {
                if *v != 0.0 {
                    out.push_str(&format!("    {k}: {:+.3}\n", v));
                }
            }
        }

        out
    }
}

fn classify_delta(delta: f64) -> DiffDirection {
    const THRESHOLD: f64 = 0.001;
    if delta > THRESHOLD {
        DiffDirection::Improved
    } else if delta < -THRESHOLD {
        DiffDirection::Regressed
    } else {
        DiffDirection::Unchanged
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::result::{RunSummary, SuiteSummary};

    fn make_summary(run_id: &str, score: f64, suite_score: f64) -> RunSummary {
        let mut metrics = BTreeMap::new();
        metrics.insert("accuracy".into(), suite_score);
        RunSummary {
            run_id: run_id.into(),
            model: "test-model".into(),
            timestamp: "2026-04-16T00:00:00Z".into(),
            suites: vec![SuiteSummary {
                suite_id: "tool-basic".into(),
                track: "tool-calling".into(),
                case_count: 10,
                passed: (suite_score * 10.0) as usize,
                failed: 10 - (suite_score * 10.0) as usize,
                aggregate_score: suite_score,
                metrics,
            }],
            total_cases: 10,
            passed: (score * 10.0) as usize,
            failed: 10 - (score * 10.0) as usize,
            errors: 0,
            timeouts: 0,
            skipped: 0,
            aggregate_score: score,
            total_latency_ms: 1000,
            total_tokens: 5000,
        }
    }

    #[test]
    fn detects_improvement() {
        let store = ReportStore::new(PathBuf::from("/tmp/test-runs"), PathBuf::from("/tmp/test-baselines"));
        let baseline = make_summary("run-001", 0.7, 0.7);
        let current = make_summary("run-002", 0.85, 0.85);
        let diff = store.compare(&current, &baseline);
        assert_eq!(diff.overall, DiffDirection::Improved);
        assert!(diff.score_delta > 0.0);
    }

    #[test]
    fn detects_regression() {
        let store = ReportStore::new(PathBuf::from("/tmp/test-runs"), PathBuf::from("/tmp/test-baselines"));
        let baseline = make_summary("run-001", 0.9, 0.9);
        let current = make_summary("run-002", 0.75, 0.75);
        let diff = store.compare(&current, &baseline);
        assert_eq!(diff.overall, DiffDirection::Regressed);
        assert!(diff.score_delta < 0.0);
    }

    #[test]
    fn detects_unchanged() {
        let store = ReportStore::new(PathBuf::from("/tmp/test-runs"), PathBuf::from("/tmp/test-baselines"));
        let baseline = make_summary("run-001", 0.8, 0.8);
        let current = make_summary("run-002", 0.8, 0.8);
        let diff = store.compare(&current, &baseline);
        assert_eq!(diff.overall, DiffDirection::Unchanged);
    }

    #[test]
    fn format_diff_produces_output() {
        let store = ReportStore::new(PathBuf::from("/tmp/test-runs"), PathBuf::from("/tmp/test-baselines"));
        let baseline = make_summary("run-001", 0.7, 0.7);
        let current = make_summary("run-002", 0.85, 0.85);
        let diff = store.compare(&current, &baseline);
        let text = ReportStore::format_diff(&diff);
        assert!(text.contains("run-002"));
        assert!(text.contains("run-001"));
        assert!(text.contains("Improved"));
    }
}
