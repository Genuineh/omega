use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetentionProfile {
    ProjectFacts,
    DeveloperPreferences,
    OpenThreads,
    EphemeralDebug,
}

impl RetentionProfile {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ProjectFacts => "project_facts",
            Self::DeveloperPreferences => "developer_preferences",
            Self::OpenThreads => "open_threads",
            Self::EphemeralDebug => "ephemeral_debug",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TurnRetentionSignals {
    pub changed_paths: Vec<String>,
    pub completed_tasks: Vec<String>,
    pub open_tasks: Vec<String>,
    pub validation_targets: Vec<String>,
    pub developer_preferences: Vec<String>,
    pub governance_events: Vec<GovernanceEventSignal>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GovernanceEventSignal {
    pub label: String,
    pub at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RetentionEvidenceRef {
    StepSummary {
        workflow_id: String,
        step_id: String,
        title: String,
    },
    ChangedPath {
        path: String,
    },
    ValidationTarget {
        target: String,
    },
    GovernanceEvent {
        label: String,
        at: u64,
    },
    UserIntent {
        text: String,
    },
    TaskRef {
        task_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetentionCandidate {
    pub profile: RetentionProfile,
    pub text: String,
    pub evidence_refs: Vec<RetentionEvidenceRef>,
    pub accepted: bool,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchivedTurnInput {
    pub turn_id: u64,
    pub workflow_id: String,
    pub user_intent: String,
    pub summaries: Vec<StepSummary>,
    pub signals: TurnRetentionSignals,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchivedTurnRecord {
    pub turn_id: u64,
    pub workflow_id: String,
    pub user_intent: String,
    pub summary_count: usize,
    pub summaries: Vec<StepSummary>,
    pub retention_candidates: Vec<RetentionCandidate>,
    pub signals: TurnRetentionSignals,
    pub archived_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MemoryQuery {
    pub text: Option<String>,
    pub queries: Vec<String>,
    pub raw_query: Option<String>,
    pub profiles: Vec<RetentionProfile>,
    pub max_results: usize,
    pub rewrite_reason: Option<String>,
    pub rewrite_queries: Vec<String>,
    pub recovery_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryQueryHit {
    pub profile: RetentionProfile,
    pub title: String,
    pub preview: String,
    pub evidence_refs: Vec<RetentionEvidenceRef>,
    pub last_updated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ObservationQuery {
    pub text: Option<String>,
    pub queries: Vec<String>,
    pub raw_query: Option<String>,
    pub max_results: usize,
    pub include_stale: bool,
    pub rewrite_reason: Option<String>,
    pub rewrite_queries: Vec<String>,
    pub recovery_path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationFreshness {
    Fresh,
    Stale,
    Superseded,
    Corrected,
}

impl ObservationFreshness {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fresh => "fresh",
            Self::Stale => "stale",
            Self::Superseded => "superseded",
            Self::Corrected => "corrected",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservationEvidenceRef {
    pub kind: String,
    pub locator: String,
    pub observed_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectObservation {
    pub id: String,
    pub profile: RetentionProfile,
    pub title: String,
    pub summary: String,
    pub freshness: ObservationFreshness,
    pub created_at: u64,
    pub updated_at: u64,
    pub effective_at: Option<u64>,
    pub evidence_refs: Vec<ObservationEvidenceRef>,
    pub supersedes: Vec<String>,
    pub corrected_by: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MemoryStoreStats {
    pub total_turns_archived: u64,
    pub retained_candidates_accepted: u64,
    pub retained_candidates_dropped: u64,
    pub dropped_by_profile: BTreeMap<String, u64>,
    pub observation_totals: ObservationTotals,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ObservationTotals {
    pub total: u64,
    pub fresh: u64,
    pub stale: u64,
    pub superseded: u64,
    pub corrected: u64,
    pub correction_activity: u64,
}

pub struct LocalMemoryStore {
    root: PathBuf,
    io_lock: Mutex<()>,
}

impl LocalMemoryStore {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            io_lock: Mutex::new(()),
        }
    }

    pub fn archive_turn(&self, input: ArchivedTurnInput) -> Result<ArchivedTurnRecord> {
        let _guard = self.io_lock.lock().unwrap();
        let archived_at = current_unix_timestamp();
        let retention_candidates = build_retention_candidates(&input);
        let record = ArchivedTurnRecord {
            turn_id: input.turn_id,
            workflow_id: input.workflow_id,
            user_intent: input.user_intent,
            summary_count: input.summaries.len(),
            summaries: input.summaries,
            retention_candidates,
            signals: input.signals,
            archived_at,
        };

        fs::create_dir_all(self.turns_dir())
            .with_context(|| format!("create {}", self.turns_dir().display()))?;
        let turn_path = self.turn_path(record.turn_id);
        let turn_json = serde_json::to_string_pretty(&record)?;
        fs::write(&turn_path, turn_json)
            .with_context(|| format!("write archived turn {}", turn_path.display()))?;

        let mut observations = self.read_observations_locked()?;
        update_observations(&mut observations, &record);
        self.write_observations_locked(&observations)?;
        Ok(record)
    }

    pub fn get_turn_history(&self, limit: usize) -> Result<Vec<ArchivedTurnRecord>> {
        let _guard = self.io_lock.lock().unwrap();
        let mut turns = self.read_turns_locked()?;
        turns.sort_by(|left, right| {
            right
                .archived_at
                .cmp(&left.archived_at)
                .then_with(|| right.turn_id.cmp(&left.turn_id))
        });
        if limit == 0 {
            turns.clear();
        } else {
            turns.truncate(limit);
        }
        Ok(turns)
    }

    pub fn query(&self, query: &MemoryQuery) -> Result<Vec<MemoryQueryHit>> {
        let _guard = self.io_lock.lock().unwrap();
        let planned_queries = planned_query_texts(query.text.as_deref(), &query.queries);
        let queries_empty = planned_queries.is_empty();
        let allowed_profiles = allowed_profiles(&query.profiles);
        let mut hits = self
            .read_turns_locked()?
            .into_iter()
            .flat_map(|turn| {
                let planned_queries = planned_queries.clone();
                turn.retention_candidates
                    .into_iter()
                    .filter(|candidate| candidate.accepted)
                    .filter(|candidate| allowed_profiles.contains(candidate.profile.as_str()))
                    .map(move |candidate| {
                        let title = derive_candidate_title(candidate.profile.as_str(), &candidate.text);
                        let score = memory_query_score(
                            &title,
                            &candidate.text,
                            &turn.user_intent,
                            &planned_queries,
                        );
                        (
                            score,
                            MemoryQueryHit {
                                profile: candidate.profile,
                                title,
                                preview: preview_text(&candidate.text, 180),
                                evidence_refs: candidate.evidence_refs,
                                last_updated_at: turn.archived_at,
                            },
                        )
                    })
            })
                    .filter(|(score, _)| *score > 0 || queries_empty)
            .collect::<Vec<_>>();

        hits.sort_by(|left, right| {
            right
                .0
                .cmp(&left.0)
                .then_with(|| right.1.last_updated_at.cmp(&left.1.last_updated_at))
                .then_with(|| left.1.title.cmp(&right.1.title))
        });

        Ok(hits
            .into_iter()
            .take(normalized_max_results(query.max_results))
            .map(|(_, hit)| hit)
            .collect())
    }

    pub fn query_observations(&self, query: &ObservationQuery) -> Result<Vec<ProjectObservation>> {
        let _guard = self.io_lock.lock().unwrap();
        let planned_queries = planned_query_texts(query.text.as_deref(), &query.queries);
        let mut hits = self
            .read_observations_locked()?
            .into_iter()
            .map(refresh_observation_freshness)
            .filter(|observation| {
                query.include_stale
                    || !matches!(
                        observation.freshness,
                        ObservationFreshness::Superseded | ObservationFreshness::Corrected
                    )
            })
            .map(|observation| {
                let score = observation_query_score(
                    &observation.title,
                    &observation.summary,
                    &planned_queries,
                );
                (score, observation)
            })
            .filter(|(score, _)| *score > 0 || planned_queries.is_empty())
            .collect::<Vec<_>>();

        hits.sort_by(|left, right| {
            right
                .0
                .cmp(&left.0)
                .then_with(|| right.1.updated_at.cmp(&left.1.updated_at))
                .then_with(|| left.1.title.cmp(&right.1.title))
        });

        Ok(hits
            .into_iter()
            .take(normalized_max_results(query.max_results))
            .map(|(_, observation)| observation)
            .collect())
    }

    pub fn stats(&self) -> Result<MemoryStoreStats> {
        let _guard = self.io_lock.lock().unwrap();
        let turns = self.read_turns_locked()?;
        let observations = self
            .read_observations_locked()?
            .into_iter()
            .map(refresh_observation_freshness)
            .collect::<Vec<_>>();

        let mut dropped_by_profile = BTreeMap::new();
        let mut retained_candidates_accepted = 0_u64;
        let mut retained_candidates_dropped = 0_u64;
        for turn in &turns {
            for candidate in &turn.retention_candidates {
                if candidate.accepted {
                    retained_candidates_accepted = retained_candidates_accepted.saturating_add(1);
                } else {
                    retained_candidates_dropped = retained_candidates_dropped.saturating_add(1);
                    *dropped_by_profile
                        .entry(candidate.profile.as_str().to_string())
                        .or_default() += 1;
                }
            }
        }

        let mut observation_totals = ObservationTotals {
            total: observations.len() as u64,
            ..ObservationTotals::default()
        };
        for observation in observations {
            match observation.freshness {
                ObservationFreshness::Fresh => observation_totals.fresh += 1,
                ObservationFreshness::Stale => observation_totals.stale += 1,
                ObservationFreshness::Superseded => observation_totals.superseded += 1,
                ObservationFreshness::Corrected => observation_totals.corrected += 1,
            }
            if observation.corrected_by.is_some() || !observation.supersedes.is_empty() {
                observation_totals.correction_activity += 1;
            }
        }

        Ok(MemoryStoreStats {
            total_turns_archived: turns.len() as u64,
            retained_candidates_accepted,
            retained_candidates_dropped,
            dropped_by_profile,
            observation_totals,
        })
    }

    pub fn compact_archived_turns(&self, keep_recent_turns: usize) -> Result<bool> {
        let _guard = self.io_lock.lock().unwrap();
        let mut turns = self.read_turns_locked()?;
        if turns.len() <= keep_recent_turns {
            return Ok(false);
        }

        turns.sort_by(|left, right| {
            right
                .archived_at
                .cmp(&left.archived_at)
                .then_with(|| right.turn_id.cmp(&left.turn_id))
        });

        let mut changed = false;
        for turn in turns.iter_mut().skip(keep_recent_turns) {
            if compact_archived_turn_record(turn) {
                changed = true;
            }
        }

        if !changed {
            return Ok(false);
        }

        turns.sort_by(|left, right| {
            left.archived_at
                .cmp(&right.archived_at)
                .then_with(|| left.turn_id.cmp(&right.turn_id))
        });

        for turn in &turns {
            self.write_turn_locked(turn)?;
        }

        let mut observations = Vec::new();
        for turn in &turns {
            update_observations(&mut observations, turn);
        }
        self.write_observations_locked(&observations)?;
        Ok(true)
    }

    fn turns_dir(&self) -> PathBuf {
        self.root.join(".omega/memory/turns")
    }

    fn turn_path(&self, turn_id: u64) -> PathBuf {
        self.turns_dir().join(format!("turn-{turn_id:020}.json"))
    }

    fn observations_path(&self) -> PathBuf {
        self.root.join(".omega/memory/observations.jsonl")
    }

    fn write_turn_locked(&self, record: &ArchivedTurnRecord) -> Result<()> {
        fs::create_dir_all(self.turns_dir())
            .with_context(|| format!("create {}", self.turns_dir().display()))?;
        let turn_path = self.turn_path(record.turn_id);
        let turn_json = serde_json::to_string_pretty(record)?;
        fs::write(&turn_path, turn_json)
            .with_context(|| format!("write archived turn {}", turn_path.display()))
    }

    fn read_turns_locked(&self) -> Result<Vec<ArchivedTurnRecord>> {
        let Ok(entries) = fs::read_dir(self.turns_dir()) else {
            return Ok(Vec::new());
        };
        let mut turns = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let raw = fs::read_to_string(&path)
                .with_context(|| format!("read archived turn {}", path.display()))?;
            turns.push(
                serde_json::from_str(&raw)
                    .with_context(|| format!("parse archived turn {}", path.display()))?,
            );
        }
        Ok(turns)
    }

    fn read_observations_locked(&self) -> Result<Vec<ProjectObservation>> {
        let path = self.observations_path();
        let Ok(raw) = fs::read_to_string(&path) else {
            return Ok(Vec::new());
        };
        raw.lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| serde_json::from_str(line).context("parse project observation"))
            .collect()
    }

    fn write_observations_locked(&self, observations: &[ProjectObservation]) -> Result<()> {
        let path = self.observations_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create {}", parent.display()))?;
        }
        let payload = observations
            .iter()
            .map(serde_json::to_string)
            .collect::<std::result::Result<Vec<_>, _>>()?
            .join("\n");
        let payload = if payload.is_empty() {
            payload
        } else {
            format!("{payload}\n")
        };
        fs::write(&path, payload).with_context(|| format!("write {}", path.display()))
    }
}

const OBSERVATION_STALE_AFTER_SECONDS: u64 = 7 * 24 * 60 * 60;

fn build_retention_candidates(input: &ArchivedTurnInput) -> Vec<RetentionCandidate> {
    let mut candidates = Vec::new();

    for summary in &input.summaries {
        let summary_text = summary.summary.trim();
        if summary_text.is_empty() {
            continue;
        }

        let text = format!("{}: {}", summary.title.trim(), summary_text);
        let evidence = vec![RetentionEvidenceRef::StepSummary {
            workflow_id: summary.workflow_id.clone(),
            step_id: summary.step_id.clone(),
            title: summary.title.clone(),
        }];

        if looks_like_noise(summary.title.as_str(), summary_text) {
            candidates.push(RetentionCandidate {
                profile: RetentionProfile::EphemeralDebug,
                text,
                evidence_refs: evidence,
                accepted: false,
                reason: "noise_gate: debug_only_summary".to_string(),
            });
        } else if looks_like_preference(summary_text) {
            candidates.push(RetentionCandidate {
                profile: RetentionProfile::DeveloperPreferences,
                text,
                evidence_refs: evidence,
                accepted: true,
                reason: "accepted: preference_like_summary".to_string(),
            });
        }
    }

    for path in dedupe_strings(&input.signals.changed_paths) {
        candidates.push(RetentionCandidate {
            profile: RetentionProfile::ProjectFacts,
            text: format!("Repository changes touched {path}"),
            evidence_refs: vec![RetentionEvidenceRef::ChangedPath { path }],
            accepted: true,
            reason: "accepted: changed_path".to_string(),
        });
    }

    for target in dedupe_strings(&input.signals.validation_targets) {
        candidates.push(RetentionCandidate {
            profile: RetentionProfile::ProjectFacts,
            text: format!("Validation target executed: {target}"),
            evidence_refs: vec![RetentionEvidenceRef::ValidationTarget { target }],
            accepted: true,
            reason: "accepted: validation_target".to_string(),
        });
    }

    for event in dedupe_governance_events(&input.signals.governance_events) {
        candidates.push(RetentionCandidate {
            profile: RetentionProfile::ProjectFacts,
            text: format!("Governance event: {}", event.label),
            evidence_refs: vec![RetentionEvidenceRef::GovernanceEvent {
                label: event.label,
                at: event.at,
            }],
            accepted: true,
            reason: "accepted: governance_event".to_string(),
        });
    }

    for task in dedupe_strings(&input.signals.open_tasks) {
        candidates.push(RetentionCandidate {
            profile: RetentionProfile::OpenThreads,
            text: format!("Open thread: {task}"),
            evidence_refs: vec![RetentionEvidenceRef::TaskRef { task_id: task }],
            accepted: true,
            reason: "accepted: open_task".to_string(),
        });
    }

    for preference in dedupe_strings(&input.signals.developer_preferences) {
        candidates.push(RetentionCandidate {
            profile: RetentionProfile::DeveloperPreferences,
            text: preference,
            evidence_refs: vec![RetentionEvidenceRef::UserIntent {
                text: input.user_intent.clone(),
            }],
            accepted: true,
            reason: "accepted: explicit_preference".to_string(),
        });
    }

    if !looks_like_noise("user_intent", &input.user_intent)
        && (!input.signals.changed_paths.is_empty() || !input.signals.open_tasks.is_empty())
    {
        candidates.push(RetentionCandidate {
            profile: RetentionProfile::ProjectFacts,
            text: format!("Turn intent: {}", input.user_intent.trim()),
            evidence_refs: vec![RetentionEvidenceRef::UserIntent {
                text: input.user_intent.clone(),
            }],
            accepted: true,
            reason: "accepted: turn_intent_with_repo_signal".to_string(),
        });
    }

    for task in dedupe_strings(&input.signals.completed_tasks) {
        candidates.push(RetentionCandidate {
            profile: RetentionProfile::EphemeralDebug,
            text: format!("Closed thread: {task}"),
            evidence_refs: vec![RetentionEvidenceRef::TaskRef { task_id: task }],
            accepted: false,
            reason: "noise_gate: completed_task_recorded_in_turn_archive".to_string(),
        });
    }

    candidates
}

fn compact_archived_turn_record(record: &mut ArchivedTurnRecord) -> bool {
    let mut changed = false;
    for summary in &mut record.summaries {
        let compacted = compact_summary_text(&summary.summary, AGGRESSIVE_COMPACTED_SUMMARY_CHAR_LIMIT);
        if compacted != summary.summary {
            summary.summary = compacted;
            changed = true;
        }
    }

    if changed {
        record.summary_count = record.summaries.len();
        record.retention_candidates = build_retention_candidates(&ArchivedTurnInput {
            turn_id: record.turn_id,
            workflow_id: record.workflow_id.clone(),
            user_intent: record.user_intent.clone(),
            summaries: record.summaries.clone(),
            signals: record.signals.clone(),
        });
    }

    changed
}

fn update_observations(observations: &mut Vec<ProjectObservation>, record: &ArchivedTurnRecord) {
    let now = record.archived_at;
    let mut active_open_threads = BTreeSet::new();

    for (index, candidate) in record
        .retention_candidates
        .iter()
        .enumerate()
        .filter(|(_, candidate)| candidate.accepted)
    {
        let title = derive_candidate_title(candidate.profile.as_str(), &candidate.text);
        let key = canonical_key(candidate.profile.as_str(), &title);
        if matches!(candidate.profile, RetentionProfile::OpenThreads) {
            active_open_threads.insert(key.clone());
        }
        let evidence_refs = candidate
            .evidence_refs
            .iter()
            .map(|evidence| observation_evidence_from_retention(evidence, now))
            .collect::<Vec<_>>();

        if let Some(existing_index) = observations.iter().position(|observation| {
            canonical_key(observation.profile.as_str(), &observation.title) == key
                && !matches!(observation.freshness, ObservationFreshness::Superseded)
        }) {
            let existing = &mut observations[existing_index];
            if normalized_text(&existing.summary) == normalized_text(&candidate.text) {
                existing.updated_at = now;
                existing.effective_at = Some(now);
                existing.evidence_refs = merge_observation_evidence(&existing.evidence_refs, &evidence_refs);
                if matches!(existing.freshness, ObservationFreshness::Stale) {
                    existing.freshness = ObservationFreshness::Fresh;
                }
            } else {
                let previous_id = existing.id.clone();
                existing.freshness = ObservationFreshness::Corrected;
                existing.updated_at = now;
                existing.corrected_by = Some(observation_id(record.turn_id, index));
                observations.push(ProjectObservation {
                    id: observation_id(record.turn_id, index),
                    profile: candidate.profile.clone(),
                    title,
                    summary: candidate.text.clone(),
                    freshness: ObservationFreshness::Fresh,
                    created_at: now,
                    updated_at: now,
                    effective_at: Some(now),
                    evidence_refs,
                    supersedes: vec![previous_id],
                    corrected_by: None,
                });
            }
        } else {
            observations.push(ProjectObservation {
                id: observation_id(record.turn_id, index),
                profile: candidate.profile.clone(),
                title,
                summary: candidate.text.clone(),
                freshness: ObservationFreshness::Fresh,
                created_at: now,
                updated_at: now,
                effective_at: Some(now),
                evidence_refs,
                supersedes: Vec::new(),
                corrected_by: None,
            });
        }
    }

    let completed_tasks = record
        .signals
        .completed_tasks
        .iter()
        .map(|task| canonical_key("open_threads", &derive_candidate_title("open_threads", &format!("Open thread: {task}"))))
        .collect::<BTreeSet<_>>();

    for observation in observations.iter_mut() {
        if observation.profile != RetentionProfile::OpenThreads {
            continue;
        }
        let key = canonical_key(observation.profile.as_str(), &observation.title);
        if completed_tasks.contains(&key) {
            observation.freshness = ObservationFreshness::Superseded;
            observation.updated_at = now;
        } else if !active_open_threads.contains(&key)
            && matches!(observation.freshness, ObservationFreshness::Fresh)
            && now.saturating_sub(observation.updated_at) > OBSERVATION_STALE_AFTER_SECONDS
        {
            observation.freshness = ObservationFreshness::Stale;
        }
    }

    observations.sort_by(|left, right| {
        right
            .updated_at
            .cmp(&left.updated_at)
            .then_with(|| left.title.cmp(&right.title))
    });
}

fn merge_observation_evidence(
    existing: &[ObservationEvidenceRef],
    incoming: &[ObservationEvidenceRef],
) -> Vec<ObservationEvidenceRef> {
    let mut merged = existing.to_vec();
    for evidence in incoming {
        if !merged.iter().any(|current| current == evidence) {
            merged.push(evidence.clone());
        }
    }
    merged
}

fn refresh_observation_freshness(mut observation: ProjectObservation) -> ProjectObservation {
    if matches!(observation.freshness, ObservationFreshness::Fresh)
        && current_unix_timestamp().saturating_sub(observation.updated_at)
            > OBSERVATION_STALE_AFTER_SECONDS
    {
        observation.freshness = ObservationFreshness::Stale;
    }
    observation
}

fn observation_evidence_from_retention(
    evidence: &RetentionEvidenceRef,
    observed_at: u64,
) -> ObservationEvidenceRef {
    match evidence {
        RetentionEvidenceRef::StepSummary {
            workflow_id,
            step_id,
            title,
        } => ObservationEvidenceRef {
            kind: "step_summary".to_string(),
            locator: format!("{workflow_id}:{step_id}:{title}"),
            observed_at,
        },
        RetentionEvidenceRef::ChangedPath { path } => ObservationEvidenceRef {
            kind: "changed_path".to_string(),
            locator: path.clone(),
            observed_at,
        },
        RetentionEvidenceRef::ValidationTarget { target } => ObservationEvidenceRef {
            kind: "validation_target".to_string(),
            locator: target.clone(),
            observed_at,
        },
        RetentionEvidenceRef::GovernanceEvent { label, at } => ObservationEvidenceRef {
            kind: "governance_event".to_string(),
            locator: label.clone(),
            observed_at: *at,
        },
        RetentionEvidenceRef::UserIntent { text } => ObservationEvidenceRef {
            kind: "user_intent".to_string(),
            locator: preview_text(text, 160),
            observed_at,
        },
        RetentionEvidenceRef::TaskRef { task_id } => ObservationEvidenceRef {
            kind: "task_ref".to_string(),
            locator: task_id.clone(),
            observed_at,
        },
    }
}

fn looks_like_noise(title: &str, text: &str) -> bool {
    let haystack = normalized_text(&format!("{title}\n{text}"));
    ["debug", "trace", "stderr", "stdout", "panic", "warning", "log"]
        .iter()
        .any(|needle| haystack.contains(needle))
}

fn looks_like_preference(text: &str) -> bool {
    let haystack = normalized_text(text);
    ["prefer", "always", "never", "must", "should", "keep using"]
        .iter()
        .any(|needle| haystack.contains(needle))
}

fn derive_candidate_title(profile: &str, text: &str) -> String {
    let trimmed = text.trim();
    let base = trimmed
        .split_once(':')
        .map(|(_, rest)| rest.trim())
        .filter(|rest| !rest.is_empty())
        .unwrap_or(trimmed);
    let label = match profile {
        "project_facts" => "Project fact",
        "developer_preferences" => "Developer preference",
        "open_threads" => "Open thread",
        "ephemeral_debug" => "Ephemeral debug",
        _ => "Memory note",
    };
    format!("{label}: {}", preview_text(base, 72))
}

fn observation_id(turn_id: u64, index: usize) -> String {
    format!("obs-{turn_id:020}-{index:04}")
}

fn canonical_key(profile: &str, title: &str) -> String {
    format!("{profile}:{}", normalized_text(title))
}

fn normalized_text(text: &str) -> String {
    let mut normalized = String::new();
    let mut pending_space = false;
    for ch in text.chars().flat_map(|ch| ch.to_lowercase()) {
        if ch.is_alphanumeric() {
            if pending_space && !normalized.is_empty() {
                normalized.push(' ');
            }
            normalized.push(ch);
            pending_space = false;
        } else if !normalized.is_empty() {
            pending_space = true;
        }
    }
    normalized
}

fn query_terms(text: Option<&str>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut terms = Vec::new();
    let text = text.unwrap_or_default().trim();
    if text.is_empty() {
        return terms;
    }

    push_query_term(text, &mut seen, &mut terms);
    for segment in text.split_whitespace() {
        push_query_term(segment, &mut seen, &mut terms);
        push_cjk_bigrams(segment, &mut seen, &mut terms);
    }
    if text.chars().any(is_cjk_char) {
        push_cjk_bigrams(text, &mut seen, &mut terms);
    }
    terms
}

fn planned_query_texts(primary: Option<&str>, queries: &[String]) -> Vec<String> {
    let mut values = queries.iter().cloned().collect::<Vec<_>>();
    if let Some(primary) = primary {
        values.insert(0, primary.to_string());
    }
    dedupe_strings(&values)
}

fn memory_query_score(title: &str, summary: &str, user_intent: &str, queries: &[String]) -> u32 {
    aggregate_query_scores(queries, |query| {
        field_weighted_match_score(
            query,
            &[(title, 14), (summary, 8), (user_intent, 4)],
        )
    })
}

fn observation_query_score(title: &str, summary: &str, queries: &[String]) -> u32 {
    aggregate_query_scores(queries, |query| {
        field_weighted_match_score(query, &[(title, 12), (summary, 7)])
    })
}

fn aggregate_query_scores(queries: &[String], mut scorer: impl FnMut(&str) -> u32) -> u32 {
    if queries.is_empty() {
        return 1;
    }
    let mut scores = queries
        .iter()
        .map(|query| scorer(query))
        .filter(|score| *score > 0)
        .collect::<Vec<_>>();
    if scores.is_empty() {
        return 0;
    }
    scores.sort_unstable_by(|left, right| right.cmp(left));
    let bonus = scores.iter().skip(1).take(2).sum::<u32>() / 4;
    scores[0]
        .saturating_add(bonus)
        .saturating_add((scores.len().saturating_sub(1) as u32).min(2))
}

fn field_weighted_match_score(query: &str, fields: &[(&str, u32)]) -> u32 {
    let terms = query_terms(Some(query));
    if terms.is_empty() {
        return 0;
    }

    let normalized_query = normalized_text(query);
    let mut total = 0_u32;
    for (field, weight) in fields {
        let normalized_field = normalized_text(field);
        if normalized_field.is_empty() {
            continue;
        }
        if !normalized_query.is_empty() && normalized_field.contains(&normalized_query) {
            total = total.saturating_add(weight.saturating_mul(3));
        }
        let matches = terms
            .iter()
            .filter(|term| normalized_field.contains(term.as_str()))
            .count() as u32;
        if matches == 0 {
            continue;
        }
        total = total.saturating_add(matches.saturating_mul(*weight));
        total = total.saturating_add(
            matches
                .saturating_mul(weight.saturating_mul(2))
                .div_ceil(terms.len() as u32),
        );
    }
    total
}

fn push_query_term(text: &str, seen: &mut BTreeSet<String>, terms: &mut Vec<String>) {
    let normalized = normalized_text(text);
    if !normalized.is_empty() && seen.insert(normalized.clone()) {
        terms.push(normalized);
    }
}

fn push_cjk_bigrams(text: &str, seen: &mut BTreeSet<String>, terms: &mut Vec<String>) {
    let mut sequence = Vec::new();
    for ch in text.chars() {
        if is_cjk_char(ch) {
            sequence.push(ch);
        } else {
            flush_cjk_sequence(&sequence, seen, terms);
            sequence.clear();
        }
    }
    flush_cjk_sequence(&sequence, seen, terms);
}

fn flush_cjk_sequence(sequence: &[char], seen: &mut BTreeSet<String>, terms: &mut Vec<String>) {
    if sequence.is_empty() {
        return;
    }
    if sequence.len() == 1 {
        let value = sequence[0].to_string();
        if seen.insert(value.clone()) {
            terms.push(value);
        }
        return;
    }
    for window in sequence.windows(2) {
        let value = window.iter().collect::<String>();
        if seen.insert(value.clone()) {
            terms.push(value);
        }
    }
}

fn is_cjk_char(ch: char) -> bool {
    matches!(
        ch,
        '\u{3400}'..='\u{4DBF}'
            | '\u{4E00}'..='\u{9FFF}'
            | '\u{3040}'..='\u{309F}'
            | '\u{30A0}'..='\u{30FF}'
            | '\u{AC00}'..='\u{D7AF}'
    )
}

fn normalized_max_results(max_results: usize) -> usize {
    max_results.max(1)
}

fn allowed_profiles(profiles: &[RetentionProfile]) -> BTreeSet<&'static str> {
    if profiles.is_empty() {
        return [
            RetentionProfile::ProjectFacts.as_str(),
            RetentionProfile::DeveloperPreferences.as_str(),
            RetentionProfile::OpenThreads.as_str(),
            RetentionProfile::EphemeralDebug.as_str(),
        ]
        .into_iter()
        .collect();
    }
    profiles.iter().map(RetentionProfile::as_str).collect()
}

fn dedupe_strings(values: &[String]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut deduped = Vec::new();
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() || !seen.insert(trimmed.to_string()) {
            continue;
        }
        deduped.push(trimmed.to_string());
    }
    deduped
}

fn dedupe_governance_events(values: &[GovernanceEventSignal]) -> Vec<GovernanceEventSignal> {
    let mut seen = BTreeSet::new();
    let mut deduped = Vec::new();
    for value in values {
        let label = value.label.trim();
        if label.is_empty() || !seen.insert((label.to_string(), value.at)) {
            continue;
        }
        deduped.push(GovernanceEventSignal {
            label: label.to_string(),
            at: value.at,
        });
    }
    deduped
}

fn preview_text(text: &str, limit: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= limit {
        return trimmed.to_string();
    }
    format!("{}...", truncate_chars(trimmed, limit.saturating_sub(3)))
}

fn current_unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{
        compact_summary_text, rank_summary_candidates, should_trigger_context_compaction,
        ArchivedTurnInput, ArchivedTurnRecord, GovernanceEventSignal, LocalMemoryStore,
        MemoryQuery, ObservationFreshness, ObservationQuery, RetentionProfile,
        StepContextHint, StepSummary, SummaryPriority, TurnRetentionSignals,
        AGGRESSIVE_COMPACTED_SUMMARY_CHAR_LIMIT,
    };
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("omega-memory-test-{nanos}"));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

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

    #[test]
    fn archive_turn_persists_retention_profiles_and_noise_gate() {
        let root = temp_root();
        let store = LocalMemoryStore::new(root.clone());

        let record = store
            .archive_turn(ArchivedTurnInput {
                turn_id: 7,
                workflow_id: "feature".to_string(),
                user_intent: "Prefer patch-sized edits and keep validation narrow".to_string(),
                summaries: vec![
                    summary("feature", "plan", "Prefer narrow validation when possible."),
                    summary("feature", "debug", "debug log dump and trace output"),
                ],
                signals: TurnRetentionSignals {
                    changed_paths: vec!["crates/omega-context/src/lib.rs".to_string()],
                    completed_tasks: vec!["task-1".to_string()],
                    open_tasks: vec!["task-2".to_string()],
                    validation_targets: vec!["cargo test -p omega-context".to_string()],
                    developer_preferences: vec![
                        "Prefer patch-sized edits and focused validation".to_string(),
                    ],
                    governance_events: vec![GovernanceEventSignal {
                        label: "document.archive docs/specs/old.md".to_string(),
                        at: 42,
                    }],
                },
            })
            .unwrap();

        assert!(record
            .retention_candidates
            .iter()
            .any(|candidate| candidate.profile == RetentionProfile::ProjectFacts && candidate.accepted));
        assert!(record
            .retention_candidates
            .iter()
            .any(|candidate| candidate.profile == RetentionProfile::DeveloperPreferences && candidate.accepted));
        assert!(record
            .retention_candidates
            .iter()
            .any(|candidate| candidate.profile == RetentionProfile::OpenThreads && candidate.accepted));
        assert!(record
            .retention_candidates
            .iter()
            .any(|candidate| candidate.reason == "accepted: governance_event"));
        assert!(record.retention_candidates.iter().any(|candidate| {
            candidate.profile == RetentionProfile::EphemeralDebug && !candidate.accepted
        }));

        let stats = store.stats().unwrap();
        assert_eq!(stats.total_turns_archived, 1);
        assert!(stats.retained_candidates_accepted >= 3);
        assert!(stats.retained_candidates_dropped >= 1);
        assert!(root.join(".omega/memory/turns/turn-00000000000000000007.json").exists());
    }

        #[test]
        fn legacy_archived_turn_without_governance_events_still_parses() {
                let archived = serde_json::from_str::<ArchivedTurnRecord>(
                        r#"{
    "turn_id": 3,
    "workflow_id": "research",
    "user_intent": "analyze current project",
    "summary_count": 1,
    "summaries": [
        {
            "workflow_id": "research",
            "step_id": "report",
            "title": "Report",
            "summary": "analysis output",
            "estimated_tokens": 42
        }
    ],
    "retention_candidates": [],
    "signals": {
        "changed_paths": [],
        "completed_tasks": [],
        "open_tasks": [],
        "validation_targets": [],
        "developer_preferences": []
    },
    "archived_at": 1775540230
}"#,
                )
                .unwrap();

                assert!(archived.signals.governance_events.is_empty());
        }

    #[test]
    fn query_returns_retained_candidates_and_observations() {
        let root = temp_root();
        let store = LocalMemoryStore::new(root);
        store
            .archive_turn(ArchivedTurnInput {
                turn_id: 11,
                workflow_id: "feature".to_string(),
                user_intent: "Wire memory query into the planner".to_string(),
                summaries: vec![summary("feature", "plan", "Wire memory query into the planner")],
                signals: TurnRetentionSignals {
                    changed_paths: vec!["crates/omega-context/src/lib.rs".to_string()],
                    completed_tasks: Vec::new(),
                    open_tasks: vec!["task-memory-query".to_string()],
                    validation_targets: vec!["cargo test -p omega-context".to_string()],
                    developer_preferences: Vec::new(),
                    governance_events: Vec::new(),
                },
            })
            .unwrap();

        let hits = store
            .query(&MemoryQuery {
                text: Some("planner memory query".to_string()),
                queries: vec!["planner memory query".to_string()],
                raw_query: Some("Wire memory query into the planner".to_string()),
                profiles: vec![RetentionProfile::ProjectFacts, RetentionProfile::OpenThreads],
                max_results: 5,
                rewrite_reason: None,
                rewrite_queries: Vec::new(),
                recovery_path: None,
            })
            .unwrap();
        assert!(!hits.is_empty());
        assert!(hits.iter().any(|hit| hit.preview.contains("planner")));

        let observations = store
            .query_observations(&ObservationQuery {
                text: Some("memory query".to_string()),
                queries: vec!["memory query".to_string()],
                raw_query: Some("Wire memory query into the planner".to_string()),
                max_results: 5,
                include_stale: true,
                rewrite_reason: None,
                rewrite_queries: Vec::new(),
                recovery_path: None,
            })
            .unwrap();
        assert!(!observations.is_empty());
        assert!(observations
            .iter()
            .any(|observation| observation.summary.contains("memory query")));
    }

    #[test]
    fn completed_open_thread_marks_previous_observation_superseded() {
        let root = temp_root();
        let store = LocalMemoryStore::new(root);

        store
            .archive_turn(ArchivedTurnInput {
                turn_id: 21,
                workflow_id: "feature".to_string(),
                user_intent: "Keep task-keep-open active".to_string(),
                summaries: vec![summary("feature", "execute", "Open thread remains active")],
                signals: TurnRetentionSignals {
                    changed_paths: Vec::new(),
                    completed_tasks: Vec::new(),
                    open_tasks: vec!["task-keep-open".to_string()],
                    validation_targets: Vec::new(),
                    developer_preferences: Vec::new(),
                    governance_events: Vec::new(),
                },
            })
            .unwrap();
        store
            .archive_turn(ArchivedTurnInput {
                turn_id: 22,
                workflow_id: "feature".to_string(),
                user_intent: "Close task-keep-open".to_string(),
                summaries: vec![summary("feature", "execute", "Thread closed")],
                signals: TurnRetentionSignals {
                    changed_paths: Vec::new(),
                    completed_tasks: vec!["task-keep-open".to_string()],
                    open_tasks: Vec::new(),
                    validation_targets: Vec::new(),
                    developer_preferences: Vec::new(),
                    governance_events: Vec::new(),
                },
            })
            .unwrap();

        let observations = store
            .query_observations(&ObservationQuery {
                text: Some("task keep open".to_string()),
                queries: vec!["task keep open".to_string()],
                raw_query: Some("Close task-keep-open".to_string()),
                max_results: 5,
                include_stale: true,
                rewrite_reason: None,
                rewrite_queries: Vec::new(),
                recovery_path: None,
            })
            .unwrap();
        assert!(observations.iter().any(|observation| {
            observation.freshness == ObservationFreshness::Superseded
        }));
    }

    #[test]
    fn query_tokenization_handles_cjk_memory_queries() {
        let root = temp_root();
        let store = LocalMemoryStore::new(root);
        store
            .archive_turn(ArchivedTurnInput {
                turn_id: 31,
                workflow_id: "feature".to_string(),
                user_intent: "修复记忆查询规划空命中问题".to_string(),
                summaries: vec![summary("feature", "plan", "修复记忆查询规划")],
                signals: TurnRetentionSignals {
                    changed_paths: Vec::new(),
                    completed_tasks: Vec::new(),
                    open_tasks: Vec::new(),
                    validation_targets: Vec::new(),
                    developer_preferences: vec!["保持记忆查询规划稳定".to_string()],
                    governance_events: Vec::new(),
                },
            })
            .unwrap();

        let hits = store
            .query(&MemoryQuery {
                text: Some("记忆查询规划".to_string()),
                queries: vec!["记忆查询规划".to_string()],
                raw_query: Some("记忆查询规划".to_string()),
                profiles: vec![RetentionProfile::DeveloperPreferences],
                max_results: 5,
                rewrite_reason: None,
                rewrite_queries: Vec::new(),
                recovery_path: None,
            })
            .unwrap();

        assert!(!hits.is_empty());
        assert!(hits[0].preview.contains("记忆查询规划"));
    }

    #[test]
    fn query_prefers_title_and_summary_matches_over_user_intent_only_matches() {
        let root = temp_root();
        let store = LocalMemoryStore::new(root);
        store
            .archive_turn(ArchivedTurnInput {
                turn_id: 41,
                workflow_id: "feature".to_string(),
                user_intent: "Investigate planner follow-up".to_string(),
                summaries: vec![summary("feature", "plan", "Use recall bundle naming")],
                signals: TurnRetentionSignals {
                    changed_paths: Vec::new(),
                    completed_tasks: Vec::new(),
                    open_tasks: Vec::new(),
                    validation_targets: Vec::new(),
                    developer_preferences: vec!["Prefer recall bundle naming in diagnostics".to_string()],
                    governance_events: Vec::new(),
                },
            })
            .unwrap();
        store
            .archive_turn(ArchivedTurnInput {
                turn_id: 42,
                workflow_id: "feature".to_string(),
                user_intent: "Need recall bundle naming for the final report".to_string(),
                summaries: vec![summary("feature", "report", "Close out remaining work")],
                signals: TurnRetentionSignals {
                    changed_paths: Vec::new(),
                    completed_tasks: Vec::new(),
                    open_tasks: vec!["task-generic".to_string()],
                    validation_targets: Vec::new(),
                    developer_preferences: Vec::new(),
                    governance_events: Vec::new(),
                },
            })
            .unwrap();

        let hits = store
            .query(&MemoryQuery {
                text: Some("recall bundle naming".to_string()),
                queries: vec!["recall bundle naming".to_string()],
                raw_query: Some("recall bundle naming".to_string()),
                profiles: vec![RetentionProfile::DeveloperPreferences, RetentionProfile::OpenThreads],
                max_results: 5,
                rewrite_reason: None,
                rewrite_queries: Vec::new(),
                recovery_path: None,
            })
            .unwrap();

        assert!(hits.len() >= 2);
        assert!(hits[0].title.contains("recall bundle naming"));
    }
}