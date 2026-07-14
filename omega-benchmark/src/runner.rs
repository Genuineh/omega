use crate::config::RunConfig;
use crate::manifest::BenchmarkCase;
use crate::registry::SuiteRegistry;
use crate::result::{CaseResult, CaseStatus};
use crate::target::BenchmarkTarget;

/// Runs benchmark cases against a target and scores results.
pub struct CaseRunner<'a> {
    target: &'a dyn BenchmarkTarget,
    config: &'a RunConfig,
}

impl<'a> CaseRunner<'a> {
    pub fn new(target: &'a dyn BenchmarkTarget, config: &'a RunConfig) -> Self {
        Self { target, config }
    }

    /// Run a single case, execute against the target, and score.
    pub fn run_case(
        &self,
        suite_id: &str,
        case: &BenchmarkCase,
        scorer: &dyn crate::scoring::Scorer,
    ) -> CaseResult {
        let start = std::time::Instant::now();

        let output = match self.target.execute(case, self.config) {
            Ok(out) => out,
            Err(e) => {
                return CaseResult {
                    case_id: case.id.clone(),
                    suite_id: suite_id.to_string(),
                    status: CaseStatus::Error,
                    response: None,
                    tool_trace: Vec::new(),
                    delivery_summary: None,
                    score: None,
                    latency_ms: start.elapsed().as_millis() as u64,
                    total_tokens: 0,
                    failure_reason: Some(format!("execution error: {e}")),
                };
            }
        };

        let score_result = scorer.score(case, &output);

        let status = if score_result.passed {
            CaseStatus::Pass
        } else {
            CaseStatus::Fail
        };

        CaseResult {
            case_id: case.id.clone(),
            suite_id: suite_id.to_string(),
            status,
            response: output.response,
            tool_trace: output.tool_trace,
            delivery_summary: output.delivery_summary,
            score: Some(score_result.breakdown),
            latency_ms: output.latency_ms.max(start.elapsed().as_millis() as u64),
            total_tokens: output.total_tokens,
            failure_reason: score_result.failure_reason,
        }
    }

    /// Run all cases in all matching suites from the registry.
    pub fn run_all(&self, registry: &SuiteRegistry) -> Vec<CaseResult> {
        let mut results = Vec::new();

        for suite_id in registry.suite_ids() {
            if !self.config.suite_filter.is_empty() && !self.config.suite_filter.contains(&suite_id)
            {
                continue;
            }

            let suite = match registry.get(&suite_id) {
                Some(s) => s,
                None => continue,
            };

            if !self.config.track_filter.is_empty() {
                let track_str = suite.manifest.track.to_string();
                if !self.config.track_filter.contains(&track_str) {
                    continue;
                }
            }

            for case in &suite.manifest.cases {
                if !self.config.tag_filter.is_empty()
                    && !case.tags.iter().any(|t| self.config.tag_filter.contains(t))
                {
                    results.push(CaseResult {
                        case_id: case.id.clone(),
                        suite_id: suite_id.clone(),
                        status: CaseStatus::Skipped,
                        response: None,
                        tool_trace: Vec::new(),
                        delivery_summary: None,
                        score: None,
                        latency_ms: 0,
                        total_tokens: 0,
                        failure_reason: Some("filtered by tags".into()),
                    });
                    continue;
                }

                let result = self.run_case(&suite_id, case, suite.scorer.as_ref());
                results.push(result);
            }
        }

        results
    }
}
