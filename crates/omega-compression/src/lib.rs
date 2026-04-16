use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::Result;
use omega_project::{
	OmegaProjectHandle, ProjectSessionSnapshot, SessionContextRecord, SessionContextRecordKind,
};

pub const DEFAULT_SESSION_CONTEXT_BUDGET_TOKENS: usize = 400_000;

static SESSION_CONTEXT_BUDGET_TOKENS: AtomicUsize =
	AtomicUsize::new(DEFAULT_SESSION_CONTEXT_BUDGET_TOKENS);

pub fn session_context_budget_tokens() -> usize {
	SESSION_CONTEXT_BUDGET_TOKENS.load(Ordering::Relaxed)
}

pub fn set_session_context_budget_tokens(max_tokens: usize) {
	let normalized = if max_tokens == 0 {
		DEFAULT_SESSION_CONTEXT_BUDGET_TOKENS
	} else {
		max_tokens
	};
	SESSION_CONTEXT_BUDGET_TOKENS.store(normalized, Ordering::Relaxed);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionContextLoadGoal {
	ResumeContext,
	PromptAssembly,
	HistoricalSearch,
	UiHydration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionContextLoadRequest {
	pub session_id: String,
	pub max_tokens: usize,
	pub goal: SessionContextLoadGoal,
	pub query: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SessionContextLoadResult {
	pub recent_records: Vec<SessionContextRecord>,
	pub checkpoint_records: Vec<SessionContextRecord>,
	pub matched_records: Vec<SessionContextRecord>,
	pub reconstructed_working_set: Option<ProjectSessionSnapshot>,
	pub estimated_tokens: u32,
	pub truncated_history: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSearchRequest {
	pub session_id: String,
	pub query: String,
	pub max_results: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionCompactionRequest {
	pub session_id: String,
	pub max_tokens: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SessionCompactionResult {
	pub checkpoint_records: Vec<SessionContextRecord>,
	pub estimated_tokens: u32,
}

pub trait SessionContextCompressor: Send + Sync {
	fn load(&self, request: SessionContextLoadRequest) -> Result<SessionContextLoadResult>;
	fn compact(&self, request: SessionCompactionRequest) -> Result<SessionCompactionResult>;
	fn search(&self, request: SessionSearchRequest) -> Result<Vec<SessionContextRecord>>;
}

pub struct LedgerSessionContextCompressor {
	project_handle: Arc<OmegaProjectHandle>,
	default_max_tokens: usize,
}

impl LedgerSessionContextCompressor {
	pub fn new(project_handle: Arc<OmegaProjectHandle>) -> Self {
		Self::with_budget(project_handle, session_context_budget_tokens())
	}

	pub fn with_budget(project_handle: Arc<OmegaProjectHandle>, default_max_tokens: usize) -> Self {
		Self {
			project_handle,
			default_max_tokens,
		}
	}
}

impl SessionContextCompressor for LedgerSessionContextCompressor {
	fn load(&self, request: SessionContextLoadRequest) -> Result<SessionContextLoadResult> {
		let all_records = self.project_handle.load_context_records(&request.session_id)?;
		let reconstructed_working_set = all_records.iter().rev().find_map(|record| match &record.record {
			SessionContextRecordKind::WorkingSetSnapshot { snapshot } => Some(snapshot.clone()),
			SessionContextRecordKind::ReplayEntry { .. }
			| SessionContextRecordKind::CompressionCheckpoint { .. } => None,
		});

		let max_tokens = if request.max_tokens == 0 {
			self.default_max_tokens
		} else {
			request.max_tokens
		};

		let mut estimated_tokens = 0u32;
		let mut recent_records = Vec::new();
		for record in all_records.iter().rev() {
			let record_tokens = estimate_record_tokens(record);
			if !recent_records.is_empty()
				&& estimated_tokens.saturating_add(record_tokens) > max_tokens as u32
			{
				break;
			}
			estimated_tokens = estimated_tokens.saturating_add(record_tokens);
			recent_records.push(record.clone());
		}
		recent_records.reverse();

		let matched_records = if matches!(request.goal, SessionContextLoadGoal::HistoricalSearch)
			|| request.query.as_deref().is_some_and(|value| !value.trim().is_empty())
		{
			self.search(SessionSearchRequest {
				session_id: request.session_id.clone(),
				query: request.query.unwrap_or_default(),
				max_results: 8,
			})?
		} else {
			Vec::new()
		};

		let recent_sequences = recent_records
			.iter()
			.map(|record| record.sequence)
			.collect::<std::collections::BTreeSet<_>>();
		let checkpoint_records = if matches!(
			request.goal,
			SessionContextLoadGoal::ResumeContext | SessionContextLoadGoal::PromptAssembly
		) {
			all_records
				.iter()
				.rev()
				.filter(|record| !recent_sequences.contains(&record.sequence))
				.filter(|record| {
					matches!(
						record.record,
						SessionContextRecordKind::CompressionCheckpoint { .. }
					)
				})
				.take(1)
				.cloned()
				.collect::<Vec<_>>()
		} else {
			Vec::new()
		};
		let selected_sequences = recent_sequences
			.iter()
			.copied()
			.chain(checkpoint_records.iter().map(|record| record.sequence))
			.collect::<std::collections::BTreeSet<_>>();
		let matched_records = matched_records
			.into_iter()
			.filter(|record| !selected_sequences.contains(&record.sequence))
			.collect::<Vec<_>>();
		let checkpoint_tokens = checkpoint_records
			.iter()
			.map(estimate_record_tokens)
			.fold(0u32, u32::saturating_add);
		let matched_tokens = matched_records
			.iter()
			.map(estimate_record_tokens)
			.fold(0u32, u32::saturating_add);
		let truncated_history = all_records.iter().any(|record| {
			!selected_sequences.contains(&record.sequence)
				&& !matched_records
					.iter()
					.any(|selected| selected.sequence == record.sequence)
		});

		Ok(SessionContextLoadResult {
			recent_records,
			checkpoint_records,
			matched_records,
			reconstructed_working_set,
			estimated_tokens: estimated_tokens
				.saturating_add(checkpoint_tokens)
				.saturating_add(matched_tokens),
			truncated_history,
		})
	}

	fn compact(&self, request: SessionCompactionRequest) -> Result<SessionCompactionResult> {
		let result = self.load(SessionContextLoadRequest {
			session_id: request.session_id.clone(),
			max_tokens: request.max_tokens,
			goal: SessionContextLoadGoal::PromptAssembly,
			query: None,
		})?;
		if !result.truncated_history {
			return Ok(SessionCompactionResult {
				checkpoint_records: Vec::new(),
				estimated_tokens: result.estimated_tokens,
			});
		}

		let all_records = self.project_handle.load_context_records(&request.session_id)?;
		let existing_checkpoints = all_records
			.iter()
			.filter_map(|record| match &record.record {
				SessionContextRecordKind::CompressionCheckpoint {
					source_sequence_start,
					source_sequence_end,
					..
				} => Some((*source_sequence_start, *source_sequence_end)),
				SessionContextRecordKind::WorkingSetSnapshot { .. }
				| SessionContextRecordKind::ReplayEntry { .. } => None,
			})
			.collect::<std::collections::BTreeSet<_>>();
		let recent_sequences = result
			.recent_records
			.iter()
			.map(|record| record.sequence)
			.collect::<std::collections::BTreeSet<_>>();
		let source_records = all_records
			.into_iter()
			.filter(|record| !recent_sequences.contains(&record.sequence))
			.filter(|record| {
				!matches!(record.record, SessionContextRecordKind::CompressionCheckpoint { .. })
			})
			.collect::<Vec<_>>();
		if source_records.is_empty() {
			return Ok(SessionCompactionResult {
				checkpoint_records: Vec::new(),
				estimated_tokens: result.estimated_tokens,
			});
		}

		let summary = summarize_checkpoint_records(&source_records);
		let keywords = checkpoint_keywords(&source_records);
		let retained_facts = checkpoint_retained_facts(&source_records);
		let source_sequence_start = source_records.first().map(|record| record.sequence).unwrap_or(0);
		let source_sequence_end = source_records.last().map(|record| record.sequence).unwrap_or(0);
		if existing_checkpoints.contains(&(source_sequence_start, source_sequence_end)) {
			return Ok(SessionCompactionResult {
				checkpoint_records: Vec::new(),
				estimated_tokens: result.estimated_tokens,
			});
		}
		let recorded_at = source_records.last().map(|record| record.recorded_at).unwrap_or(0);
		let token_count = ((summary.chars().count() + 3) / 4) as u32;
		Ok(SessionCompactionResult {
			checkpoint_records: vec![SessionContextRecord {
				schema_version: 1,
				session_id: request.session_id,
				sequence: 0,
				recorded_at,
				token_estimate: Some(token_count),
				record: SessionContextRecordKind::CompressionCheckpoint {
					checkpoint_id: format!("checkpoint:{}-{}", source_sequence_start, source_sequence_end),
					source_sequence_start,
					source_sequence_end,
					summary,
					keywords,
					retained_facts,
					token_count,
				},
			}],
			estimated_tokens: token_count,
		})
	}

	fn search(&self, request: SessionSearchRequest) -> Result<Vec<SessionContextRecord>> {
		let query = request.query.trim().to_ascii_lowercase();
		if query.is_empty() {
			return Ok(Vec::new());
		}

		let mut matches = self
			.project_handle
			.load_context_records(&request.session_id)?
			.into_iter()
			.filter(|record| searchable_record_text(record).contains(&query))
			.collect::<Vec<_>>();
		matches.sort_by(|left, right| right.sequence.cmp(&left.sequence));
		matches.truncate(request.max_results);
		Ok(matches)
	}
}

fn estimate_record_tokens(record: &SessionContextRecord) -> u32 {
	let text = match &record.record {
		SessionContextRecordKind::WorkingSetSnapshot { snapshot } => {
			let mut parts = Vec::new();
			if let Some(latest_user_turn) = snapshot.latest_user_turn.as_deref() {
				parts.push(latest_user_turn.to_string());
			}
			parts.extend(
				snapshot
					.recent_turn_summaries
					.iter()
					.map(|summary| summary.user_intent.clone()),
			);
			parts.extend(
				snapshot
					.step_summaries
					.iter()
					.map(|summary| format!("{} {}", summary.title, summary.summary)),
			);
			parts.extend(snapshot.todo_items.iter().map(|item| item.text.clone()));
			parts.join("\n")
		}
		SessionContextRecordKind::ReplayEntry { entry } => {
			format!("{}\n{}", entry.title.as_deref().unwrap_or(""), entry.body)
		}
		SessionContextRecordKind::CompressionCheckpoint {
			summary,
			keywords,
			retained_facts,
			..
		} => {
			let mut parts = vec![summary.clone()];
			parts.extend(keywords.iter().cloned());
			parts.extend(retained_facts.iter().cloned());
			parts.join("\n")
		}
	};
	((text.chars().count() + 3) / 4) as u32
}

fn searchable_record_text(record: &SessionContextRecord) -> String {
	match &record.record {
		SessionContextRecordKind::WorkingSetSnapshot { snapshot } => {
			let mut parts = Vec::new();
			if let Some(latest_user_turn) = snapshot.latest_user_turn.as_deref() {
				parts.push(latest_user_turn.to_ascii_lowercase());
			}
			parts.extend(
				snapshot
					.recent_turn_summaries
					.iter()
					.map(|summary| summary.user_intent.to_ascii_lowercase()),
			);
			parts.extend(
				snapshot
					.step_summaries
					.iter()
					.map(|summary| {
						format!("{} {}", summary.title, summary.summary).to_ascii_lowercase()
					}),
			);
			parts.extend(snapshot.todo_items.iter().map(|item| item.text.to_ascii_lowercase()));
			parts.join("\n")
		}
		SessionContextRecordKind::ReplayEntry { entry } => format!(
			"{}\n{}",
			entry.title.as_deref().unwrap_or("").to_ascii_lowercase(),
			entry.body.to_ascii_lowercase()
		),
		SessionContextRecordKind::CompressionCheckpoint {
			summary,
			keywords,
			retained_facts,
			..
		} => {
			let mut parts = vec![summary.to_ascii_lowercase()];
			parts.extend(keywords.iter().map(|keyword| keyword.to_ascii_lowercase()));
			parts.extend(retained_facts.iter().map(|fact| fact.to_ascii_lowercase()));
			parts.join("\n")
		}
	}
}

fn summarize_checkpoint_records(records: &[SessionContextRecord]) -> String {
	let mut snippets = records
		.iter()
		.filter_map(checkpoint_summary_snippet)
		.filter(|snippet| !snippet.is_empty())
		.collect::<Vec<_>>();
	if snippets.is_empty() {
		return format!("Compacted {} historical ledger records.", records.len());
	}
	snippets.truncate(4);
	format!(
		"Compacted {} historical ledger records: {}",
		records.len(),
		snippets.join(" | ")
	)
}

fn checkpoint_summary_snippet(record: &SessionContextRecord) -> Option<String> {
	match &record.record {
		SessionContextRecordKind::WorkingSetSnapshot { snapshot } => snapshot
			.latest_user_turn
			.as_deref()
			.map(preview_checkpoint_text),
		SessionContextRecordKind::ReplayEntry { entry } => Some(preview_checkpoint_text(&entry.body)),
		SessionContextRecordKind::CompressionCheckpoint { summary, .. } => {
			Some(preview_checkpoint_text(summary))
		}
	}
}

fn checkpoint_keywords(records: &[SessionContextRecord]) -> Vec<String> {
	let mut keywords = std::collections::BTreeSet::new();
	for record in records {
		match &record.record {
			SessionContextRecordKind::WorkingSetSnapshot { snapshot } => {
				for summary in &snapshot.step_summaries {
					if keywords.len() >= 6 {
						break;
					}
					keywords.insert(summary.workflow_id.clone());
					if keywords.len() >= 6 {
						break;
					}
					keywords.insert(summary.step_id.clone());
				}
			}
			SessionContextRecordKind::ReplayEntry { entry } => {
				if let Some(title) = entry.title.as_deref() {
					if keywords.len() < 6 {
						keywords.insert(title.to_string());
					}
				}
			}
			SessionContextRecordKind::CompressionCheckpoint { keywords: existing, .. } => {
				for keyword in existing {
					if keywords.len() >= 6 {
						break;
					}
					keywords.insert(keyword.clone());
				}
			}
		}
		if keywords.len() >= 6 {
			break;
		}
	}
	keywords.into_iter().collect()
}

fn checkpoint_retained_facts(records: &[SessionContextRecord]) -> Vec<String> {
	let mut facts = Vec::new();
	for record in records.iter().rev() {
		match &record.record {
			SessionContextRecordKind::WorkingSetSnapshot { snapshot } => {
				if let Some(latest_user_turn) = snapshot.latest_user_turn.as_deref() {
					facts.push(format!("latest_user_turn: {}", preview_checkpoint_text(latest_user_turn)));
				}
				for item in snapshot.todo_items.iter().take(2) {
					facts.push(format!("todo: {}", item.text));
				}
			}
			SessionContextRecordKind::ReplayEntry { entry } => {
				if matches!(entry.kind, omega_project::SessionReplayEntryKind::UserTurn) {
					facts.push(format!("user_turn: {}", preview_checkpoint_text(&entry.body)));
				}
			}
			SessionContextRecordKind::CompressionCheckpoint { retained_facts, .. } => {
				facts.extend(retained_facts.iter().cloned());
			}
		}
		if facts.len() >= 4 {
			break;
		}
	}
	facts.truncate(4);
	facts
}

fn preview_checkpoint_text(text: &str) -> String {
	let trimmed = text.trim();
	if trimmed.chars().count() <= 80 {
		trimmed.to_string()
	} else {
		format!("{}...", trimmed.chars().take(77).collect::<String>())
	}
}

#[cfg(test)]
mod tests {
	use std::collections::BTreeMap;

	use omega_project::{
		ProjectRegistry, ProjectResolutionInput, ProjectSessionRoutingSnapshot,
		ProjectSessionSnapshot, ProjectSessionStatus, ProjectSessionTodoItem,
		ProjectSessionTodoStatus, ProjectSessionTurnSummary, ProjectSessionUpdate,
		ProjectSkillRoutingSnapshot, SessionReplayEntry, SessionReplayEntryKind,
	};

	use super::*;

	#[test]
	fn load_prefers_recent_records_within_budget_and_reconstructs_snapshot() {
		let temp_dir = tempfile::tempdir().unwrap();
		let root = temp_dir.path().join("workspace");
		std::fs::create_dir_all(&root).unwrap();

		let handle = ProjectRegistry::new()
			.resolve(ProjectResolutionInput {
				current_file_path: None,
				cwd: root.clone(),
				explicit_root: Some(root.clone()),
			})
			.unwrap();
		handle
			.upsert_session(ProjectSessionUpdate {
				session_id: "session-a".to_string(),
				title: Some("Session A".to_string()),
				status: ProjectSessionStatus::Idle,
				turn_count: 2,
				last_user_turn_preview: Some("recent work".to_string()),
				archived_turn_count: Some(0),
			})
			.unwrap();
		handle
			.save_session_snapshot(&ProjectSessionSnapshot {
				schema_version: 1,
				project_id: handle.project_id(),
				session_id: "session-a".to_string(),
				saved_at: 2,
				last_completed_turn_id: Some(2),
				latest_user_turn: Some("recent work".to_string()),
				recent_turn_summaries: vec![ProjectSessionTurnSummary {
					turn_id: 2,
					workflow_id: "feature".to_string(),
					user_intent: "recent work".to_string(),
					summary_count: 1,
				}],
				routing: ProjectSessionRoutingSnapshot {
					recognized_scene_id: Some("feature".to_string()),
					selected_workflow_id: Some("feature".to_string()),
					active_workflow_id: "feature".to_string(),
					active_workflow_role: "child".to_string(),
				},
				skill_routing: ProjectSkillRoutingSnapshot {
					selected_skill_ids: vec!["review".to_string()],
					loaded_skill_ids: vec!["review".to_string()],
					ignored_skill_ids: Vec::new(),
					selection_reason: None,
					source_step_id: None,
				},
				step_summaries: Vec::new(),
				step_outputs: BTreeMap::new(),
				governance_events: Vec::new(),
				todo_items: vec![ProjectSessionTodoItem {
					id: "1".to_string(),
					text: "finish report".to_string(),
					status: ProjectSessionTodoStatus::Pending,
					active_form: None,
				}],
				structured_input: None,
				selected_task_id: None,
				last_known_cwd: Some(root.clone()),
			})
			.unwrap();
		handle
			.append_replay_entries(
				"session-a",
				&[
					SessionReplayEntry {
						session_id: "session-a".to_string(),
						recorded_at: 1,
						kind: SessionReplayEntryKind::UserTurn,
						title: Some("User".to_string()),
						body: "older question".to_string(),
						state: None,
					},
					SessionReplayEntry {
						session_id: "session-a".to_string(),
						recorded_at: 3,
						kind: SessionReplayEntryKind::AssistantResponse,
						title: Some("Assistant".to_string()),
						body: "recent answer with feature details".to_string(),
						state: Some("complete".to_string()),
					},
				],
			)
			.unwrap();

		let compressor = LedgerSessionContextCompressor::with_budget(handle, 12);
		let result = compressor
			.load(SessionContextLoadRequest {
				session_id: "session-a".to_string(),
				max_tokens: 12,
				goal: SessionContextLoadGoal::ResumeContext,
				query: None,
			})
			.unwrap();

		assert!(result.reconstructed_working_set.is_some());
		assert!(!result.recent_records.is_empty());
		assert!(result.truncated_history);
	}

	#[test]
	fn resume_load_backfills_latest_checkpoint_when_history_is_truncated() {
		let temp_dir = tempfile::tempdir().unwrap();
		let root = temp_dir.path().join("workspace");
		std::fs::create_dir_all(&root).unwrap();

		let handle = ProjectRegistry::new()
			.resolve(ProjectResolutionInput {
				current_file_path: None,
				cwd: root.clone(),
				explicit_root: Some(root.clone()),
			})
			.unwrap();
		handle
			.upsert_session(ProjectSessionUpdate {
				session_id: "session-a".to_string(),
				title: Some("Session A".to_string()),
				status: ProjectSessionStatus::Idle,
				turn_count: 3,
				last_user_turn_preview: Some("recent work".to_string()),
				archived_turn_count: Some(1),
			})
			.unwrap();
		handle
			.append_replay_entries(
				"session-a",
				&[
					SessionReplayEntry {
						session_id: "session-a".to_string(),
						recorded_at: 1,
						kind: SessionReplayEntryKind::UserTurn,
						title: Some("User".to_string()),
						body: "older question about cache invalidation".to_string(),
						state: None,
					},
					SessionReplayEntry {
						session_id: "session-a".to_string(),
						recorded_at: 2,
						kind: SessionReplayEntryKind::AssistantResponse,
						title: Some("Assistant".to_string()),
						body: "older answer with retained cache fact".to_string(),
						state: Some("complete".to_string()),
					},
					SessionReplayEntry {
						session_id: "session-a".to_string(),
						recorded_at: 3,
						kind: SessionReplayEntryKind::AssistantResponse,
						title: Some("Assistant".to_string()),
						body: "recent answer that should stay visible".to_string(),
						state: Some("complete".to_string()),
					},
				],
			)
			.unwrap();

		let checkpoint_records = LedgerSessionContextCompressor::with_budget(handle.clone(), 12)
			.compact(SessionCompactionRequest {
				session_id: "session-a".to_string(),
				max_tokens: 12,
			})
			.unwrap()
			.checkpoint_records;
		assert_eq!(checkpoint_records.len(), 1);
		handle
			.append_context_records("session-a", &checkpoint_records)
			.unwrap();
		handle
			.append_replay_entries(
				"session-a",
				&[SessionReplayEntry {
					session_id: "session-a".to_string(),
					recorded_at: 4,
					kind: SessionReplayEntryKind::AssistantResponse,
					title: Some("Assistant".to_string()),
					body: "newest answer after checkpoint".to_string(),
					state: Some("complete".to_string()),
				}],
			)
			.unwrap();

		let result = LedgerSessionContextCompressor::with_budget(handle, 12)
			.load(SessionContextLoadRequest {
				session_id: "session-a".to_string(),
				max_tokens: 12,
				goal: SessionContextLoadGoal::ResumeContext,
				query: None,
			})
			.unwrap();

		assert!(result.truncated_history);
		assert_eq!(result.checkpoint_records.len(), 1);
		assert!(matches!(
			result.checkpoint_records[0].record,
			SessionContextRecordKind::CompressionCheckpoint { .. }
		));
	}

	#[test]
	fn search_returns_matching_ledger_records() {
		let temp_dir = tempfile::tempdir().unwrap();
		let root = temp_dir.path().join("workspace");
		std::fs::create_dir_all(&root).unwrap();

		let handle = ProjectRegistry::new()
			.resolve(ProjectResolutionInput {
				current_file_path: None,
				cwd: root.clone(),
				explicit_root: Some(root.clone()),
			})
			.unwrap();
		handle
			.upsert_session(ProjectSessionUpdate {
				session_id: "session-a".to_string(),
				title: Some("Session A".to_string()),
				status: ProjectSessionStatus::Idle,
				turn_count: 1,
				last_user_turn_preview: Some("feature recall".to_string()),
				archived_turn_count: Some(0),
			})
			.unwrap();
		handle
			.append_replay_entries(
				"session-a",
				&[SessionReplayEntry {
					session_id: "session-a".to_string(),
					recorded_at: 1,
					kind: SessionReplayEntryKind::AssistantResponse,
					title: Some("Assistant".to_string()),
					body: "feature recall signal".to_string(),
					state: Some("complete".to_string()),
				}],
			)
			.unwrap();

		let compressor = LedgerSessionContextCompressor::new(handle);
		let matches = compressor
			.search(SessionSearchRequest {
				session_id: "session-a".to_string(),
				query: "recall".to_string(),
				max_results: 4,
			})
			.unwrap();

		assert_eq!(matches.len(), 1);
	}

	#[test]
	fn compact_emits_checkpoint_for_truncated_history() {
		let temp_dir = tempfile::tempdir().unwrap();
		let root = temp_dir.path().join("workspace");
		std::fs::create_dir_all(&root).unwrap();

		let handle = ProjectRegistry::new()
			.resolve(ProjectResolutionInput {
				current_file_path: None,
				cwd: root.clone(),
				explicit_root: Some(root.clone()),
			})
			.unwrap();
		handle
			.upsert_session(ProjectSessionUpdate {
				session_id: "session-a".to_string(),
				title: Some("Session A".to_string()),
				status: ProjectSessionStatus::Idle,
				turn_count: 3,
				last_user_turn_preview: Some("recent work".to_string()),
				archived_turn_count: Some(1),
			})
			.unwrap();
		handle
			.append_replay_entries(
				"session-a",
				&[
					SessionReplayEntry {
						session_id: "session-a".to_string(),
						recorded_at: 1,
						kind: SessionReplayEntryKind::UserTurn,
						title: Some("User".to_string()),
						body: "older question about compaction".to_string(),
						state: None,
					},
					SessionReplayEntry {
						session_id: "session-a".to_string(),
						recorded_at: 2,
						kind: SessionReplayEntryKind::AssistantResponse,
						title: Some("Assistant".to_string()),
						body: "older answer with retained fact".to_string(),
						state: Some("complete".to_string()),
					},
					SessionReplayEntry {
						session_id: "session-a".to_string(),
						recorded_at: 3,
						kind: SessionReplayEntryKind::AssistantResponse,
						title: Some("Assistant".to_string()),
						body: "recent answer that should stay visible".to_string(),
						state: Some("complete".to_string()),
					},
				],
			)
			.unwrap();

		let compressor = LedgerSessionContextCompressor::with_budget(handle, 12);
		let result = compressor
			.compact(SessionCompactionRequest {
				session_id: "session-a".to_string(),
				max_tokens: 12,
			})
			.unwrap();

		assert_eq!(result.checkpoint_records.len(), 1);
		let checkpoint = &result.checkpoint_records[0];
		match &checkpoint.record {
			SessionContextRecordKind::CompressionCheckpoint {
				checkpoint_id,
				source_sequence_start,
				source_sequence_end,
				summary,
				keywords,
				retained_facts,
				token_count,
			} => {
				assert!(checkpoint_id.starts_with("checkpoint:"));
				assert!(*source_sequence_start >= 1);
				assert!(*source_sequence_end >= *source_sequence_start);
				assert!(summary.contains("Compacted"));
				assert!(!keywords.is_empty());
				assert!(!retained_facts.is_empty());
				assert!(*token_count > 0);
			}
			_ => panic!("expected compression checkpoint record"),
		}
	}
}
