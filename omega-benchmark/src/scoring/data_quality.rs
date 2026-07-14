use std::collections::BTreeMap;

use crate::manifest::{BenchmarkCase, ExpectedOutcome};
use crate::result::ScoreBreakdown;
use crate::scoring::{ScoreResult, Scorer};
use crate::target::TargetOutput;

/// Scorer for data generation quality evaluation.
///
/// Evaluates:
/// - `schema_validity`: Output conforms to the expected JSON schema.
/// - `judge_score`: LLM-based quality judge (placeholder for v1).
/// - `win_rate`: Pairwise comparison win rate (placeholder for v1).
/// - `human_audit_pass_rate`: Human review pass rate (recorded externally).
pub struct DataQualityScorer;

impl Scorer for DataQualityScorer {
    fn score(&self, case: &BenchmarkCase, output: &TargetOutput) -> ScoreResult {
        let expected = match &case.expected {
            Some(exp) => exp,
            None => {
                return score_existence_only(output);
            }
        };

        match expected {
            ExpectedOutcome::SchemaValid { schema } => score_schema(schema, output),
            ExpectedOutcome::JudgePass { rubric } => score_judge_stub(rubric, output),
            _ => ScoreResult {
                passed: false,
                breakdown: ScoreBreakdown::single("error", 0.0),
                failure_reason: Some("unexpected outcome type for data quality track".into()),
            },
        }
    }
}

fn score_schema(schema: &serde_json::Value, output: &TargetOutput) -> ScoreResult {
    let response = output.response.as_deref().unwrap_or("");

    // Step 1: Is the response valid JSON?
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(response);
    let json_val = match parsed {
        Ok(val) => val,
        Err(e) => {
            return ScoreResult {
                passed: false,
                breakdown: ScoreBreakdown::single("schema_validity", 0.0),
                failure_reason: Some(format!("response is not valid JSON: {e}")),
            };
        }
    };

    // Step 2: Validate against schema (structural check).
    let valid = validate_against_schema(&json_val, schema);

    let mut metrics = BTreeMap::new();
    metrics.insert("schema_validity".into(), if valid { 1.0 } else { 0.0 });
    // Placeholder slots for judge and win_rate
    metrics.insert("judge_score".into(), 0.0);
    metrics.insert("win_rate".into(), 0.0);

    ScoreResult {
        passed: valid,
        breakdown: ScoreBreakdown::from_metrics(metrics),
        failure_reason: if valid {
            None
        } else {
            Some("response does not match expected schema".into())
        },
    }
}

/// Structural schema validation.
///
/// Checks that the response contains at least the keys specified in the
/// schema's `properties` (if the schema is an object type). This is a
/// lightweight v1 check — full JSON Schema validation can be added later.
fn validate_against_schema(value: &serde_json::Value, schema: &serde_json::Value) -> bool {
    use serde_json::Value;

    // If schema specifies required properties, check they exist.
    if let Some(required) = schema.get("required").and_then(Value::as_array) {
        if let Value::Object(obj) = value {
            for req in required {
                if let Some(key) = req.as_str() {
                    if !obj.contains_key(key) {
                        return false;
                    }
                }
            }
        } else {
            return false;
        }
    }

    // If schema specifies type, check it.
    if let Some(ty) = schema.get("type").and_then(Value::as_str) {
        match ty {
            "object" => {
                if !value.is_object() {
                    return false;
                }
            }
            "array" => {
                if !value.is_array() {
                    return false;
                }
            }
            "string" => {
                if !value.is_string() {
                    return false;
                }
            }
            "number" | "integer" => {
                if !value.is_number() {
                    return false;
                }
            }
            "boolean" => {
                if !value.is_boolean() {
                    return false;
                }
            }
            _ => {}
        }
    }

    true
}

fn score_judge_stub(rubric: &str, output: &TargetOutput) -> ScoreResult {
    let response = output.response.as_deref().unwrap_or("");
    let has_output = !response.is_empty();

    let mut metrics = BTreeMap::new();
    metrics.insert("judge_score".into(), 0.0);
    metrics.insert("win_rate".into(), 0.0);

    ScoreResult {
        passed: false,
        breakdown: ScoreBreakdown::from_metrics(metrics),
        failure_reason: Some(format!(
            "judge scoring not yet implemented; rubric: '{}'; has_output: {has_output}",
            truncate(rubric, 80)
        )),
    }
}

fn score_existence_only(output: &TargetOutput) -> ScoreResult {
    let response = output.response.as_deref().unwrap_or("");
    let has_output = !response.is_empty();

    ScoreResult {
        passed: has_output,
        breakdown: ScoreBreakdown::single("output_present", if has_output { 1.0 } else { 0.0 }),
        failure_reason: if has_output {
            None
        } else {
            Some("no output produced".into())
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

    fn make_output(response: &str) -> TargetOutput {
        TargetOutput {
            response: Some(response.into()),
            tool_trace: Vec::new(),
            delivery_summary: None,
            total_tokens: 50,
            latency_ms: 20,
        }
    }

    #[test]
    fn schema_valid_passes_with_required_keys() {
        let case = make_case(ExpectedOutcome::SchemaValid {
            schema: serde_json::json!({
                "type": "object",
                "required": ["name", "age"]
            }),
        });
        let output = make_output(r#"{"name": "Alice", "age": 30}"#);
        let result = DataQualityScorer.score(&case, &output);
        assert!(result.passed);
        assert_eq!(
            *result.breakdown.metrics.get("schema_validity").unwrap(),
            1.0
        );
    }

    #[test]
    fn schema_valid_fails_missing_key() {
        let case = make_case(ExpectedOutcome::SchemaValid {
            schema: serde_json::json!({
                "type": "object",
                "required": ["name", "age"]
            }),
        });
        let output = make_output(r#"{"name": "Alice"}"#);
        let result = DataQualityScorer.score(&case, &output);
        assert!(!result.passed);
    }

    #[test]
    fn schema_valid_fails_on_invalid_json() {
        let case = make_case(ExpectedOutcome::SchemaValid {
            schema: serde_json::json!({"type": "object"}),
        });
        let output = make_output("not json at all");
        let result = DataQualityScorer.score(&case, &output);
        assert!(!result.passed);
    }

    #[test]
    fn schema_valid_fails_wrong_type() {
        let case = make_case(ExpectedOutcome::SchemaValid {
            schema: serde_json::json!({"type": "array"}),
        });
        let output = make_output(r#"{"key": "value"}"#);
        let result = DataQualityScorer.score(&case, &output);
        assert!(!result.passed);
    }
}
