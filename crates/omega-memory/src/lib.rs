#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepSummary {
    pub workflow_id: String,
    pub step_id: String,
    pub title: String,
    pub summary: String,
    pub estimated_tokens: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepContextHint {
    pub step_id: String,
    pub input_sources: Vec<String>,
    pub active_workflow_id: String,
    pub report_step_id: String,
    pub execute_step_id: String,
    pub plan_step_id: String,
    pub scene_recognition_step_id: String,
    pub select_workflow_step_id: String,
    pub root_workflow_id: String,
    pub has_execute_item: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummaryPriority {
    Medium,
    Low,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SummaryCandidate {
    pub summary: StepSummary,
    pub original_index: usize,
    pub priority: SummaryPriority,
    pub score: u32,
    pub compacted: bool,
}

pub const CONTEXT_COMPACTION_THRESHOLD_PERCENT: u32 = 70;
pub const MAX_UNCOMPACTED_SUMMARIES: usize = 5;
pub const COMPACTED_SUMMARY_CHAR_LIMIT: usize = 480;
pub const AGGRESSIVE_COMPACTED_SUMMARY_CHAR_LIMIT: usize = 240;

pub fn should_trigger_context_compaction(
    base_input_tokens: u32,
    available_input_budget: u32,
    summary_count: usize,
) -> bool {
    let threshold_tokens =
        available_input_budget.saturating_mul(CONTEXT_COMPACTION_THRESHOLD_PERCENT) / 100;
    base_input_tokens >= threshold_tokens || summary_count > MAX_UNCOMPACTED_SUMMARIES
}

pub fn rank_summary_candidates(
    step_summaries: &[StepSummary],
    hint: &StepContextHint,
    compaction_triggered: bool,
) -> Vec<SummaryCandidate> {
    let total = step_summaries.len();
    let mut candidates = step_summaries
        .iter()
        .enumerate()
        .map(|(index, summary)| {
            let priority = classify_summary_priority(summary, hint);
            let score = summary_relevance_score(summary, hint, total.saturating_sub(index));
            let compacted_summary =
                maybe_compact_summary(summary, priority, compaction_triggered, index, total);
            SummaryCandidate {
                compacted: compacted_summary.summary != summary.summary,
                summary: compacted_summary,
                original_index: index,
                priority,
                score,
            }
        })
        .collect::<Vec<_>>();

    candidates.sort_by(|left, right| {
        priority_rank(right.priority)
            .cmp(&priority_rank(left.priority))
            .then_with(|| right.score.cmp(&left.score))
            .then_with(|| right.original_index.cmp(&left.original_index))
            .then_with(|| left.compacted.cmp(&right.compacted))
    });
    candidates
}

pub fn compact_summary_text(text: &str, limit: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= limit {
        return trimmed.to_string();
    }
    if limit <= 16 {
        return truncate_chars(trimmed, limit);
    }

    let head_len = limit.saturating_mul(2) / 3;
    let tail_len = limit.saturating_sub(head_len).saturating_sub(3);
    let head = truncate_chars(trimmed, head_len);
    let tail = tail_chars(trimmed, tail_len);
    format!("{}...{}", head.trim_end(), tail.trim_start())
}

fn priority_rank(priority: SummaryPriority) -> u8 {
    match priority {
        SummaryPriority::Medium => 2,
        SummaryPriority::Low => 1,
    }
}

fn classify_summary_priority(summary: &StepSummary, hint: &StepContextHint) -> SummaryPriority {
    if is_input_source_summary(summary, hint)
        || (hint.step_id == hint.execute_step_id
            && matches!(summary.step_id.as_str(), step if step == hint.plan_step_id || step == hint.execute_step_id))
        || (hint.step_id == hint.report_step_id
            && matches!(summary.step_id.as_str(), step if step == hint.plan_step_id || step == hint.execute_step_id))
    {
        SummaryPriority::Medium
    } else if summary.workflow_id == hint.active_workflow_id {
        SummaryPriority::Medium
    } else {
        SummaryPriority::Low
    }
}

fn is_input_source_summary(summary: &StepSummary, hint: &StepContextHint) -> bool {
    hint.input_sources
        .iter()
        .any(|source| source == &summary.step_id)
}

fn is_root_routing_summary(summary: &StepSummary, hint: &StepContextHint) -> bool {
    summary.workflow_id == hint.root_workflow_id
        && (summary.step_id == hint.scene_recognition_step_id
            || summary.step_id == hint.select_workflow_step_id)
}

fn summary_relevance_score(
    summary: &StepSummary,
    hint: &StepContextHint,
    recency_score: usize,
) -> u32 {
    let mut score = recency_score as u32;
    if summary.workflow_id == hint.active_workflow_id {
        score += 20;
    }
    if is_input_source_summary(summary, hint) {
        score += 80;
    }
    if hint.step_id == hint.execute_step_id {
        if summary.step_id == hint.plan_step_id {
            score += 70;
        } else if summary.step_id == hint.execute_step_id {
            score += 55;
        }
    }
    if hint.step_id == hint.report_step_id {
        if summary.step_id == hint.execute_step_id {
            score += 80;
        } else if summary.step_id == hint.plan_step_id {
            score += 50;
        }
    }
    if hint.has_execute_item
        && (summary.step_id == hint.plan_step_id || summary.step_id == hint.execute_step_id)
    {
        score += 15;
    }
    if is_root_routing_summary(summary, hint) {
        score = score.saturating_sub(40);
    }
    score
}

fn maybe_compact_summary(
    summary: &StepSummary,
    priority: SummaryPriority,
    compaction_triggered: bool,
    index: usize,
    total: usize,
) -> StepSummary {
    let target_limit = match (priority, compaction_triggered) {
        (SummaryPriority::Medium, true) if index + 2 < total => Some(COMPACTED_SUMMARY_CHAR_LIMIT),
        (SummaryPriority::Low, true) => Some(AGGRESSIVE_COMPACTED_SUMMARY_CHAR_LIMIT),
        _ => None,
    };

    let Some(target_limit) = target_limit else {
        return summary.clone();
    };
    let compacted = compact_summary_text(&summary.summary, target_limit);
    if compacted == summary.summary {
        return summary.clone();
    }

    StepSummary {
        workflow_id: summary.workflow_id.clone(),
        step_id: summary.step_id.clone(),
        title: summary.title.clone(),
        estimated_tokens: estimate_tokens(&compacted),
        summary: compacted,
    }
}

fn estimate_tokens(text: &str) -> u32 {
    text.chars().count().div_ceil(4) as u32
}

fn truncate_chars(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

fn tail_chars(text: &str, limit: usize) -> String {
    if limit == 0 {
        return String::new();
    }
    let chars = text.chars().collect::<Vec<_>>();
    if chars.len() <= limit {
        return text.to_string();
    }
    chars[chars.len() - limit..].iter().collect()
}

#[cfg(test)]
mod tests {
    use super::{
        compact_summary_text, rank_summary_candidates, should_trigger_context_compaction,
        StepContextHint, StepSummary, SummaryPriority, AGGRESSIVE_COMPACTED_SUMMARY_CHAR_LIMIT,
    };

    fn hint(step_id: &str, input_sources: Vec<&str>, has_execute_item: bool) -> StepContextHint {
        StepContextHint {
            step_id: step_id.to_string(),
            input_sources: input_sources.into_iter().map(ToOwned::to_owned).collect(),
            active_workflow_id: "feature".to_string(),
            report_step_id: "report".to_string(),
            execute_step_id: "execute".to_string(),
            plan_step_id: "plan".to_string(),
            scene_recognition_step_id: "scene-recognition".to_string(),
            select_workflow_step_id: "select-workflow".to_string(),
            root_workflow_id: "root".to_string(),
            has_execute_item,
        }
    }

    fn summary(workflow_id: &str, step_id: &str, text: &str) -> StepSummary {
        StepSummary {
            workflow_id: workflow_id.to_string(),
            step_id: step_id.to_string(),
            title: step_id.to_string(),
            summary: text.to_string(),
            estimated_tokens: 1,
        }
    }

    #[test]
    fn ranks_plan_and_input_summaries_ahead_of_root_routing_history() {
        let summaries = vec![
            summary("feature", "explore", "Explore root cause."),
            summary("feature", "plan", "Plan slot budget update."),
            summary("root", "select-workflow", "Selected workflow: feature."),
        ];
        let ranked = rank_summary_candidates(
            &summaries,
            &hint("execute", vec!["explore", "plan", "execute"], true),
            false,
        );

        assert_eq!(ranked[0].summary.step_id, "plan");
        assert_eq!(ranked[0].priority, SummaryPriority::Medium);
        assert!(
            ranked
                .iter()
                .position(|item| item.summary.step_id == "plan")
                .unwrap()
                < ranked
                    .iter()
                    .position(|item| item.summary.step_id == "select-workflow")
                    .unwrap()
        );
    }

    #[test]
    fn compaction_trigger_fires_for_budget_or_backlog() {
        assert!(should_trigger_context_compaction(710, 1_000, 2));
        assert!(should_trigger_context_compaction(120, 1_000, 6));
        assert!(!should_trigger_context_compaction(500, 1_000, 3));
    }

    #[test]
    fn compact_summary_preserves_head_and_tail() {
        let text = format!("head-marker {} tail-marker", "body ".repeat(80));
        let compacted = compact_summary_text(&text, AGGRESSIVE_COMPACTED_SUMMARY_CHAR_LIMIT);
        assert!(compacted.contains("head-marker"));
        assert!(compacted.contains("tail-marker"));
        assert!(compacted.contains("..."));
    }
}
