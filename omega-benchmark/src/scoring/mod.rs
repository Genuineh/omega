pub mod assistant;
pub mod data_quality;
pub mod tool_calling;

use crate::manifest::BenchmarkCase;
use crate::result::ScoreBreakdown;
use crate::target::TargetOutput;

/// Trait for scoring a benchmark case result.
pub trait Scorer {
    /// Score the output against the expected outcome defined in the case.
    fn score(&self, case: &BenchmarkCase, output: &TargetOutput) -> ScoreResult;
}

/// Outcome of the scoring step.
pub struct ScoreResult {
    pub passed: bool,
    pub breakdown: ScoreBreakdown,
    pub failure_reason: Option<String>,
}
