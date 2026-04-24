use std::collections::BTreeMap;

use crate::manifest::{BenchmarkCase, ExpectedOutcome, ExpectedToolCall};
use crate::result::ScoreBreakdown;
use crate::scoring::{ScoreResult, Scorer};
use crate::target::TargetOutput;

/// Scorer for BFCL-style tool calling evaluation.
///
/// Evaluates:
/// - `tool_selection_accuracy`: Did the agent pick the right tool(s)?
/// - `argument_exact_match`: Are the arguments structurally correct?
/// - `parallel_call_validity`: For multi-tool cases, are all calls present?
/// - `irrelevance_rejection_rate`: For no-tool cases, did the agent avoid calling tools?
pub struct ToolCallingScorer;

impl Scorer for ToolCallingScorer {
    fn score(&self, case: &BenchmarkCase, output: &TargetOutput) -> ScoreResult {
        let expected = match &case.expected {
            Some(exp) => exp,
            None => {
                return ScoreResult {
                    passed: false,
                    breakdown: ScoreBreakdown::single("error", 0.0),
                    failure_reason: Some("no expected outcome defined".into()),
                };
            }
        };

        match expected {
            ExpectedOutcome::ToolCalls { calls } => score_tool_calls(calls, output),
            ExpectedOutcome::NoToolCall => score_no_tool_call(output),
            _ => ScoreResult {
                passed: false,
                breakdown: ScoreBreakdown::single("error", 0.0),
                failure_reason: Some("unexpected outcome type for tool calling track".into()),
            },
        }
    }
}

fn score_tool_calls(expected: &[ExpectedToolCall], output: &TargetOutput) -> ScoreResult {
    let actual = &output.tool_trace;

    if expected.is_empty() {
        return ScoreResult {
            passed: actual.is_empty(),
            breakdown: ScoreBreakdown::single("tool_selection_accuracy", if actual.is_empty() { 1.0 } else { 0.0 }),
            failure_reason: if actual.is_empty() { None } else { Some("expected no tools but got calls".into()) },
        };
    }

    // Tool selection: how many expected tools were called?
    let mut selection_hits = 0usize;
    let mut arg_match_hits = 0usize;
    let mut reasons: Vec<String> = Vec::new();

    for exp in expected {
        let matching = actual.iter().find(|a| a.tool_name == exp.name);
        match matching {
            Some(trace) => {
                selection_hits += 1;
                if arguments_match(&exp.arguments, &trace.arguments) {
                    arg_match_hits += 1;
                } else {
                    reasons.push(format!(
                        "tool '{}': argument mismatch",
                        exp.name
                    ));
                }
            }
            None => {
                reasons.push(format!("tool '{}' was not called", exp.name));
            }
        }
    }

    let tool_selection_accuracy = selection_hits as f64 / expected.len() as f64;
    let argument_exact_match = arg_match_hits as f64 / expected.len() as f64;

    // Parallel call validity: all expected calls present and no extraneous
    let expected_names: Vec<&str> = expected.iter().map(|e| e.name.as_str()).collect();
    let actual_names: Vec<&str> = actual.iter().map(|a| a.tool_name.as_str()).collect();
    let extraneous = actual_names
        .iter()
        .filter(|n| !expected_names.contains(n))
        .count();
    let parallel_valid = if expected.len() > 1 {
        if selection_hits == expected.len() && extraneous == 0 {
            1.0
        } else {
            0.0
        }
    } else {
        // Single-tool case: parallel_call_validity is N/A but we report 1.0 if correct
        if selection_hits == 1 && extraneous == 0 { 1.0 } else { 0.0 }
    };

    let mut metrics = BTreeMap::new();
    metrics.insert("tool_selection_accuracy".into(), tool_selection_accuracy);
    metrics.insert("argument_exact_match".into(), argument_exact_match);
    metrics.insert("parallel_call_validity".into(), parallel_valid);

    let passed = tool_selection_accuracy == 1.0 && argument_exact_match == 1.0;
    let failure_reason = if reasons.is_empty() {
        None
    } else {
        Some(reasons.join("; "))
    };

    ScoreResult {
        passed,
        breakdown: ScoreBreakdown::from_metrics(metrics),
        failure_reason,
    }
}

fn score_no_tool_call(output: &TargetOutput) -> ScoreResult {
    let rejected = output.tool_trace.is_empty();
    let score = if rejected { 1.0 } else { 0.0 };

    let mut metrics = BTreeMap::new();
    metrics.insert("irrelevance_rejection_rate".into(), score);

    ScoreResult {
        passed: rejected,
        breakdown: ScoreBreakdown::from_metrics(metrics),
        failure_reason: if rejected {
            None
        } else {
            Some(format!(
                "expected no tool calls but {} tools were invoked",
                output.tool_trace.len()
            ))
        },
    }
}

/// Check if actual arguments match expected arguments.
///
/// For `Value::Null` expected, any actual value matches (wildcard).
/// For objects, checks that all expected keys are present with matching values.
fn arguments_match(expected: &serde_json::Value, actual: &serde_json::Value) -> bool {
    use serde_json::Value;

    match (expected, actual) {
        (Value::Null, _) => true, // wildcard
        (Value::Object(exp), Value::Object(act)) => {
            for (key, exp_val) in exp {
                match act.get(key) {
                    Some(act_val) => {
                        if !arguments_match(exp_val, act_val) {
                            return false;
                        }
                    }
                    None => return false,
                }
            }
            true
        }
        (Value::Array(exp), Value::Array(act)) => {
            if exp.len() != act.len() {
                return false;
            }
            exp.iter().zip(act.iter()).all(|(e, a)| arguments_match(e, a))
        }
        _ => expected == actual,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::ExpectedOutcome;
    use crate::result::ToolTraceEntry;

    fn make_case(expected: ExpectedOutcome) -> BenchmarkCase {
        BenchmarkCase {
            id: "test-case".into(),
            prompt: "test".into(),
            expected: Some(expected),
            allowed_tools: Vec::new(),
            tags: Vec::new(),
            timeout_secs: None,
        }
    }

    #[test]
    fn scores_correct_single_tool_call() {
        let case = make_case(ExpectedOutcome::ToolCalls {
            calls: vec![ExpectedToolCall {
                name: "read_file".into(),
                arguments: serde_json::json!({"path": "/tmp/test.txt"}),
                order: None,
            }],
        });

        let output = TargetOutput {
            response: Some("done".into()),
            tool_trace: vec![ToolTraceEntry {
                tool_name: "read_file".into(),
                arguments: serde_json::json!({"path": "/tmp/test.txt"}),
                result: Some("content".into()),
                is_error: false,
            }],
            delivery_summary: None,
            total_tokens: 100,
            latency_ms: 50,
        };

        let result = ToolCallingScorer.score(&case, &output);
        assert!(result.passed);
        assert_eq!(*result.breakdown.metrics.get("tool_selection_accuracy").unwrap(), 1.0);
        assert_eq!(*result.breakdown.metrics.get("argument_exact_match").unwrap(), 1.0);
    }

    #[test]
    fn fails_on_wrong_tool() {
        let case = make_case(ExpectedOutcome::ToolCalls {
            calls: vec![ExpectedToolCall {
                name: "read_file".into(),
                arguments: serde_json::json!(null),
                order: None,
            }],
        });

        let output = TargetOutput {
            response: None,
            tool_trace: vec![ToolTraceEntry {
                tool_name: "write_file".into(),
                arguments: serde_json::json!({}),
                result: None,
                is_error: false,
            }],
            delivery_summary: None,
            total_tokens: 50,
            latency_ms: 30,
        };

        let result = ToolCallingScorer.score(&case, &output);
        assert!(!result.passed);
        assert_eq!(*result.breakdown.metrics.get("tool_selection_accuracy").unwrap(), 0.0);
    }

    #[test]
    fn scores_irrelevance_rejection() {
        let case = make_case(ExpectedOutcome::NoToolCall);

        let output = TargetOutput {
            response: Some("I cannot help with that.".into()),
            tool_trace: Vec::new(),
            delivery_summary: None,
            total_tokens: 20,
            latency_ms: 10,
        };

        let result = ToolCallingScorer.score(&case, &output);
        assert!(result.passed);
        assert_eq!(
            *result.breakdown.metrics.get("irrelevance_rejection_rate").unwrap(),
            1.0
        );
    }

    #[test]
    fn fails_irrelevance_when_tools_called() {
        let case = make_case(ExpectedOutcome::NoToolCall);

        let output = TargetOutput {
            response: None,
            tool_trace: vec![ToolTraceEntry {
                tool_name: "search".into(),
                arguments: serde_json::json!({}),
                result: None,
                is_error: false,
            }],
            delivery_summary: None,
            total_tokens: 30,
            latency_ms: 15,
        };

        let result = ToolCallingScorer.score(&case, &output);
        assert!(!result.passed);
    }

    #[test]
    fn argument_wildcard_matches_anything() {
        assert!(arguments_match(&serde_json::json!(null), &serde_json::json!({"a": 1})));
    }

    #[test]
    fn argument_partial_object_match() {
        let expected = serde_json::json!({"path": "/tmp/x"});
        let actual = serde_json::json!({"path": "/tmp/x", "encoding": "utf-8"});
        assert!(arguments_match(&expected, &actual));
    }

    #[test]
    fn argument_mismatch_value() {
        let expected = serde_json::json!({"path": "/tmp/a"});
        let actual = serde_json::json!({"path": "/tmp/b"});
        assert!(!arguments_match(&expected, &actual));
    }
}
