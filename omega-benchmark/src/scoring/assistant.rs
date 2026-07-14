use std::collections::BTreeMap;

use crate::manifest::{BenchmarkCase, ExpectedOutcome};
use crate::result::ScoreBreakdown;
use crate::scoring::{ScoreResult, Scorer};
use crate::target::TargetOutput;

/// Scorer for GAIA-style general assistant evaluation.
///
/// Evaluates:
/// - `exact_match`: Response matches expected value exactly (normalized).
/// - `quasi_exact_match`: Response matches within defined tolerance.
/// - `task_completion`: Whether the agent produced a response at all.
/// - `evidence_completeness`: Whether tool usage supports the answer.
pub struct AssistantScorer;

impl Scorer for AssistantScorer {
    fn score(&self, case: &BenchmarkCase, output: &TargetOutput) -> ScoreResult {
        let expected = match &case.expected {
            Some(exp) => exp,
            None => {
                // No expected outcome — only score task completion.
                return score_completion_only(output);
            }
        };

        match expected {
            ExpectedOutcome::ExactMatch { value } => score_exact(value, output),
            ExpectedOutcome::QuasiExactMatch { value, tolerance } => {
                score_quasi_exact(value, tolerance.as_deref(), output)
            }
            ExpectedOutcome::JudgePass { rubric } => score_judge_stub(rubric, output),
            _ => ScoreResult {
                passed: false,
                breakdown: ScoreBreakdown::single("error", 0.0),
                failure_reason: Some("unexpected outcome type for assistant track".into()),
            },
        }
    }
}

fn normalize(s: &str) -> String {
    s.trim().to_lowercase()
}

fn score_exact(expected: &str, output: &TargetOutput) -> ScoreResult {
    let response = output.response.as_deref().unwrap_or("");
    let matched = normalize(response) == normalize(expected);
    let task_complete = !response.is_empty();
    let evidence = if output.tool_trace.is_empty() {
        0.5
    } else {
        1.0
    };

    let mut metrics = BTreeMap::new();
    metrics.insert("exact_match".into(), if matched { 1.0 } else { 0.0 });
    metrics.insert(
        "task_completion".into(),
        if task_complete { 1.0 } else { 0.0 },
    );
    metrics.insert("evidence_completeness".into(), evidence);

    ScoreResult {
        passed: matched,
        breakdown: ScoreBreakdown::from_metrics(metrics),
        failure_reason: if matched {
            None
        } else {
            Some(format!(
                "expected '{}', got '{}'",
                expected,
                truncate(response, 120)
            ))
        },
    }
}

fn score_quasi_exact(
    expected: &str,
    tolerance: Option<&str>,
    output: &TargetOutput,
) -> ScoreResult {
    let response = output.response.as_deref().unwrap_or("");
    let norm_expected = normalize(expected);
    let norm_response = normalize(response);

    // Exact match first
    if norm_response == norm_expected {
        return make_quasi_result(true, 1.0, output, None);
    }

    // Containment check: response contains the expected value
    if norm_response.contains(&norm_expected) {
        return make_quasi_result(true, 0.8, output, None);
    }

    // Numeric tolerance check
    if let Some(tol_str) = tolerance {
        if let (Ok(exp_num), Ok(resp_num)) =
            (norm_expected.parse::<f64>(), norm_response.parse::<f64>())
        {
            if let Ok(tol) = tol_str.parse::<f64>() {
                if (exp_num - resp_num).abs() <= tol {
                    return make_quasi_result(true, 0.9, output, None);
                }
            }
        }
    }

    make_quasi_result(
        false,
        0.0,
        output,
        Some(format!(
            "expected '{}', got '{}'",
            expected,
            truncate(response, 120)
        )),
    )
}

fn make_quasi_result(
    passed: bool,
    quasi_score: f64,
    output: &TargetOutput,
    failure_reason: Option<String>,
) -> ScoreResult {
    let response = output.response.as_deref().unwrap_or("");
    let task_complete = !response.is_empty();
    let evidence = if output.tool_trace.is_empty() {
        0.5
    } else {
        1.0
    };

    let mut metrics = BTreeMap::new();
    metrics.insert("quasi_exact_match".into(), quasi_score);
    metrics.insert(
        "task_completion".into(),
        if task_complete { 1.0 } else { 0.0 },
    );
    metrics.insert("evidence_completeness".into(), evidence);

    ScoreResult {
        passed,
        breakdown: ScoreBreakdown::from_metrics(metrics),
        failure_reason,
    }
}

fn score_judge_stub(rubric: &str, output: &TargetOutput) -> ScoreResult {
    // LLM-based judge is a placeholder until a real judge endpoint is wired.
    let response = output.response.as_deref().unwrap_or("");
    let task_complete = !response.is_empty();

    let mut metrics = BTreeMap::new();
    metrics.insert(
        "task_completion".into(),
        if task_complete { 1.0 } else { 0.0 },
    );
    metrics.insert("judge_score".into(), 0.0); // placeholder

    ScoreResult {
        passed: false,
        breakdown: ScoreBreakdown::from_metrics(metrics),
        failure_reason: Some(format!(
            "judge scoring not yet implemented; rubric: '{}'",
            truncate(rubric, 80)
        )),
    }
}

fn score_completion_only(output: &TargetOutput) -> ScoreResult {
    let response = output.response.as_deref().unwrap_or("");
    let complete = !response.is_empty();

    ScoreResult {
        passed: complete,
        breakdown: ScoreBreakdown::single("task_completion", if complete { 1.0 } else { 0.0 }),
        failure_reason: if complete {
            None
        } else {
            Some("no response produced".into())
        },
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::ExpectedOutcome;
    use crate::result::ToolTraceEntry;

    fn make_case(expected: ExpectedOutcome) -> BenchmarkCase {
        BenchmarkCase {
            id: "test".into(),
            prompt: "test".into(),
            expected: Some(expected),
            allowed_tools: Vec::new(),
            tags: Vec::new(),
            timeout_secs: None,
        }
    }

    fn make_output(response: &str, tools: usize) -> TargetOutput {
        TargetOutput {
            response: Some(response.into()),
            tool_trace: (0..tools)
                .map(|i| ToolTraceEntry {
                    tool_name: format!("tool_{i}"),
                    arguments: serde_json::json!({}),
                    result: None,
                    is_error: false,
                })
                .collect(),
            delivery_summary: None,
            total_tokens: 100,
            latency_ms: 50,
        }
    }

    #[test]
    fn exact_match_passes_with_normalization() {
        let case = make_case(ExpectedOutcome::ExactMatch {
            value: "Paris".into(),
        });
        let output = make_output("  paris  ", 1);
        let result = AssistantScorer.score(&case, &output);
        assert!(result.passed);
        assert_eq!(*result.breakdown.metrics.get("exact_match").unwrap(), 1.0);
    }

    #[test]
    fn exact_match_fails_on_mismatch() {
        let case = make_case(ExpectedOutcome::ExactMatch {
            value: "Paris".into(),
        });
        let output = make_output("London", 0);
        let result = AssistantScorer.score(&case, &output);
        assert!(!result.passed);
    }

    #[test]
    fn quasi_exact_containment() {
        let case = make_case(ExpectedOutcome::QuasiExactMatch {
            value: "42".into(),
            tolerance: None,
        });
        let output = make_output("The answer is 42.", 1);
        let result = AssistantScorer.score(&case, &output);
        assert!(result.passed);
        assert!(*result.breakdown.metrics.get("quasi_exact_match").unwrap() > 0.0);
    }

    #[test]
    fn quasi_exact_numeric_tolerance() {
        let case = make_case(ExpectedOutcome::QuasiExactMatch {
            value: "3.14".into(),
            tolerance: Some("0.01".into()),
        });
        let output = make_output("3.141", 0);
        let result = AssistantScorer.score(&case, &output);
        assert!(result.passed);
    }

    #[test]
    fn no_expected_scores_completion_only() {
        let case = BenchmarkCase {
            id: "test".into(),
            prompt: "test".into(),
            expected: None,
            allowed_tools: Vec::new(),
            tags: Vec::new(),
            timeout_secs: None,
        };
        let output = make_output("some response", 0);
        let result = AssistantScorer.score(&case, &output);
        assert!(result.passed);
    }
}
