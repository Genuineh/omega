use std::collections::BTreeMap;
use std::sync::mpsc;

use omega_core::{ChatEvent, CoreToolResult};
use omega_workflow::{WorkflowStep, WorkflowStepState};

use crate::runtime_ui::{
    ActivityTarget, ResponseSection, ResponseSectionDelta, ResponseSectionKind,
    ResponseSectionMetadata, ResponseSectionState, RuntimeUiEffect, RuntimeUiEnvelope,
    RuntimeUiMessage, StatusSlot, StatusValue, ToolRun, ToolRunDetail, ToolRunStatus, UiContent,
    UiMessageKind, UiPriority, UiSource, UiTarget, WorkflowRunRole,
};
use crate::session_state::SessionContext;
use crate::{preview_json_value, preview_text};

pub(crate) fn preview_tool_invocation(tool_name: &str, input: &serde_json::Value) -> String {
    match tool_name {
        "bash" => {
            let command_preview = input
                .get("command")
                .and_then(|value| value.as_str())
                .map(|command| format!("$ {}", preview_text(command, 80)));
            let description = input
                .get("description")
                .and_then(|value| value.as_str())
                .map(|value| preview_text(value.trim(), 40))
                .filter(|value| !value.is_empty());
            let workdir = input
                .get("workdir")
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty() && *value != ".")
                .map(|value| preview_text(value, 30));

            match (description, workdir, command_preview) {
                (Some(description), Some(workdir), Some(command)) => {
                    format!("{} @ {}: {}", description, workdir, command)
                }
                (Some(description), None, Some(command)) => format!("{}: {}", description, command),
                (None, Some(workdir), Some(command)) => format!("{} @ {}", command, workdir),
                (_, _, Some(command)) => command,
                _ => format!("{} {}", tool_name, preview_json_value(input, 80)),
            }
        }
        _ => input
            .get("path")
            .and_then(|value| value.as_str())
            .map(|path| preview_text(path, 80))
            .unwrap_or_else(|| preview_json_value(input, 80)),
    }
}

fn tool_result_preview(tool_result: &CoreToolResult, limit: usize) -> Option<String> {
    tool_result.preview.clone().or_else(|| {
        let preview = preview_text(&tool_result.output, limit);
        (!preview.is_empty()).then_some(preview)
    })
}

fn preview_lines(text: &str, max_lines: usize, max_chars: usize) -> Vec<String> {
    let mut lines = text
        .lines()
        .take(max_lines)
        .map(|line| preview_text(line, max_chars))
        .collect::<Vec<_>>();
    if text.lines().count() > max_lines {
        lines.push("...".to_string());
    }
    if lines.is_empty() {
        lines.push(preview_text(text, max_chars));
    }
    lines
}

fn build_tool_run_detail_lines(
    tool_name: &str,
    input: &serde_json::Value,
    tool_result: Option<&CoreToolResult>,
) -> Vec<String> {
    let mut lines = vec![
        format!("tool: {}", tool_name),
        format!("invoke: {}", preview_tool_invocation(tool_name, input)),
    ];

    if let Some(tool_result) = tool_result {
        if let Some(error_kind) = tool_result.error_kind {
            lines.push(format!("error_kind: {}", error_kind.as_str()));
        }
        if tool_result.truncated {
            lines.push("truncated: true".to_string());
        }
        if tool_result.has_metadata() {
            lines.push("metadata:".to_string());
            lines.extend(preview_lines(
                &serde_json::to_string_pretty(&tool_result.metadata)
                    .unwrap_or_else(|_| "{}".to_string()),
                12,
                160,
            ));
        }
        lines.push("result:".to_string());
        lines.extend(preview_lines(&tool_result.output, 12, 160));
    }

    lines
}

pub(crate) fn send_begin_tool_run(
    tx: &mpsc::Sender<RuntimeUiEnvelope>,
    turn_id: u64,
    tool_run: ToolRun,
) {
    let _ = tx.send(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::BeginToolRun { tool_run },
    ));
}

pub(crate) fn send_update_tool_run(
    tx: &mpsc::Sender<RuntimeUiEnvelope>,
    turn_id: u64,
    tool_run: ToolRun,
) {
    let _ = tx.send(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::UpdateToolRun { tool_run },
    ));
}

pub(crate) fn send_complete_tool_run(
    tx: &mpsc::Sender<RuntimeUiEnvelope>,
    turn_id: u64,
    id: &str,
    status: ToolRunStatus,
) {
    let _ = tx.send(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::CompleteToolRun {
            id: id.to_string(),
            status,
        },
    ));
}

fn find_markup_opening(text: &str) -> Option<(usize, &'static str)> {
    [
        ("<minimax:tool_call", "</minimax:tool_call>"),
        ("<invoke", "</invoke>"),
    ]
    .into_iter()
    .filter_map(|(opening, closing)| text.find(opening).map(|position| (position, closing)))
    .min_by_key(|(position, _)| *position)
}

fn suffix_prefix_len(text: &str, patterns: &[&str]) -> usize {
    let mut best = 0usize;
    for pattern in patterns {
        let max_len = pattern.len().min(text.len());
        for len in 1..=max_len {
            if text.ends_with(&pattern[..len]) {
                best = best.max(len);
            }
        }
    }
    best
}

fn tail_fragment(text: &str, keep: usize) -> String {
    if keep == 0 {
        String::new()
    } else {
        text[text.len() - keep..].to_string()
    }
}

pub(crate) fn send_workflow_step(
    tx: &mpsc::Sender<RuntimeUiEnvelope>,
    turn_id: u64,
    step: Option<WorkflowStepState>,
    workflow_id: &str,
    role: WorkflowRunRole,
) {
    let Some(step) = step else {
        return;
    };

    let _ = tx.send(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::SetStatusSlot {
            slot: StatusSlot::Workflow,
            value: StatusValue::WorkflowStep {
                workflow_id: workflow_id.to_string(),
                workflow_role: role,
                step_id: step.id.clone(),
                step_label: step.label.clone(),
                index: step.index,
                total: step.total,
            },
        },
    ));
    let _ = tx.send(RuntimeUiEnvelope::message(
        turn_id,
        RuntimeUiMessage {
            target: UiTarget::Activity(ActivityTarget::Log),
            source: UiSource::WorkflowStep {
                workflow_id: workflow_id.to_string(),
                workflow_role: role,
                step_id: step.id.clone(),
                step_label: step.label.clone(),
                index: step.index,
                total: step.total,
            },
            kind: UiMessageKind::Summary,
            content: UiContent::Text(step.label.clone()),
            priority: None,
        },
    ));
}

pub(crate) fn send_step_text(
    tx: &mpsc::Sender<RuntimeUiEnvelope>,
    turn_id: u64,
    workflow_id: &str,
    role: WorkflowRunRole,
    step: &WorkflowStep,
    text: &str,
) {
    let _ = tx.send(RuntimeUiEnvelope::message(
        turn_id,
        RuntimeUiMessage {
            target: UiTarget::Response,
            source: UiSource::WorkflowStep {
                workflow_id: workflow_id.to_string(),
                workflow_role: role,
                step_id: step.id.clone(),
                step_label: step.label.clone(),
                index: 0,
                total: 0,
            },
            kind: UiMessageKind::Narrative,
            content: UiContent::Text(text.to_string()),
            priority: None,
        },
    ));
}

pub(crate) fn send_assistant_text(tx: &mpsc::Sender<RuntimeUiEnvelope>, turn_id: u64, text: &str) {
    let _ = tx.send(RuntimeUiEnvelope::message(
        turn_id,
        RuntimeUiMessage {
            target: UiTarget::Response,
            source: UiSource::Assistant,
            kind: UiMessageKind::Result,
            content: UiContent::Text(text.to_string()),
            priority: None,
        },
    ));
}

pub(crate) fn send_error_text(tx: &mpsc::Sender<RuntimeUiEnvelope>, turn_id: u64, text: &str) {
    let _ = tx.send(RuntimeUiEnvelope::message(
        turn_id,
        RuntimeUiMessage {
            target: UiTarget::Response,
            source: UiSource::System,
            kind: UiMessageKind::Error,
            content: UiContent::Text(text.to_string()),
            priority: Some(UiPriority::High),
        },
    ));
}

pub(crate) fn send_warning_text(tx: &mpsc::Sender<RuntimeUiEnvelope>, turn_id: u64, text: &str) {
    let _ = tx.send(RuntimeUiEnvelope::message(
        turn_id,
        RuntimeUiMessage {
            target: UiTarget::Activity(ActivityTarget::Log),
            source: UiSource::System,
            kind: UiMessageKind::Warning,
            content: UiContent::Text(text.to_string()),
            priority: Some(UiPriority::Normal),
        },
    ));
}

pub(crate) fn send_system_log_text(tx: &mpsc::Sender<RuntimeUiEnvelope>, turn_id: u64, text: &str) {
    let _ = tx.send(RuntimeUiEnvelope::message(
        turn_id,
        RuntimeUiMessage {
            target: UiTarget::Activity(ActivityTarget::Log),
            source: UiSource::System,
            kind: UiMessageKind::Log,
            content: UiContent::Text(text.to_string()),
            priority: None,
        },
    ));
}

pub(crate) fn send_tool_call_preview(
    tx: &mpsc::Sender<RuntimeUiEnvelope>,
    turn_id: u64,
    tool_name: &str,
    command: Option<String>,
    preview: String,
) {
    let source = UiSource::Tool {
        tool_name: tool_name.to_string(),
    };

    if let Some(command) = command {
        let _ = tx.send(RuntimeUiEnvelope::message(
            turn_id,
            RuntimeUiMessage {
                target: UiTarget::Activity(ActivityTarget::Log),
                source: source.clone(),
                kind: UiMessageKind::Log,
                content: UiContent::Text(format!("$ {command}")),
                priority: None,
            },
        ));
    }

    let _ = tx.send(RuntimeUiEnvelope::message(
        turn_id,
        RuntimeUiMessage {
            target: UiTarget::Activity(ActivityTarget::Log),
            source,
            kind: UiMessageKind::Log,
            content: UiContent::Text(preview),
            priority: None,
        },
    ));
}

pub(crate) fn send_todo_snapshot(
    tx: &mpsc::Sender<RuntimeUiEnvelope>,
    turn_id: u64,
    rendered: &str,
) {
    let _ = tx.send(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::ReplacePanel {
            target: UiTarget::Todo,
            content: UiContent::Text(rendered.to_string()),
        },
    ));
}

pub(crate) fn send_begin_response_section(
    tx: &mpsc::Sender<RuntimeUiEnvelope>,
    turn_id: u64,
    section: ResponseSection,
) {
    let _ = tx.send(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::BeginResponseSection { section },
    ));
}

pub(crate) fn send_append_response_section(
    tx: &mpsc::Sender<RuntimeUiEnvelope>,
    turn_id: u64,
    id: &str,
    delta: ResponseSectionDelta,
) {
    let _ = tx.send(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::AppendResponseSection {
            id: id.to_string(),
            delta,
        },
    ));
}

pub(crate) fn send_complete_response_section(
    tx: &mpsc::Sender<RuntimeUiEnvelope>,
    turn_id: u64,
    id: &str,
    state: ResponseSectionState,
) {
    let _ = tx.send(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::CompleteResponseSection {
            id: id.to_string(),
            state,
        },
    ));
}

pub(crate) fn send_turn_finished(tx: &mpsc::Sender<RuntimeUiEnvelope>, turn_id: u64) {
    let _ = tx.send(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::ClearStatusSlot {
            slot: StatusSlot::Workflow,
        },
    ));
    let _ = tx.send(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::SetStatusSlot {
            slot: StatusSlot::Agent,
            value: StatusValue::Label("Idle".to_string()),
        },
    ));
}

pub(crate) fn send_session_status(
    tx: &mpsc::Sender<RuntimeUiEnvelope>,
    turn_id: u64,
    root_workflow_id: &str,
    session_context: &SessionContext,
) {
    let _ = tx.send(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::SetStatusSlot {
            slot: StatusSlot::Session,
            value: StatusValue::SessionRouting {
                root_workflow_id: root_workflow_id.to_string(),
                active_workflow_id: session_context.routing.active_workflow_id.clone(),
                active_workflow_role: session_context.routing.active_workflow_role,
                recognized_scene_id: session_context.routing.recognized_scene_id.clone(),
                selected_workflow_id: session_context.routing.selected_workflow_id.clone(),
            },
        },
    ));
}

pub(crate) fn send_routing_log(tx: &mpsc::Sender<RuntimeUiEnvelope>, turn_id: u64, text: String) {
    let _ = tx.send(RuntimeUiEnvelope::message(
        turn_id,
        RuntimeUiMessage {
            target: UiTarget::Activity(ActivityTarget::Log),
            source: UiSource::SessionRouting,
            kind: UiMessageKind::Summary,
            content: UiContent::Text(text),
            priority: None,
        },
    ));
}

pub(crate) struct StepResponseStreamer<'a> {
    tx: &'a mpsc::Sender<RuntimeUiEnvelope>,
    turn_id: u64,
    primary_section_id: String,
    thinking_section_id: String,
    primary_section: ResponseSection,
    thinking_section: ResponseSection,
    thinking_started: bool,
    primary_sanitizer: ProviderMarkupSanitizer,
    thinking_sanitizer: ProviderMarkupSanitizer,
}

impl<'a> StepResponseStreamer<'a> {
    pub(crate) fn new(
        tx: &'a mpsc::Sender<RuntimeUiEnvelope>,
        turn_id: u64,
        workflow_id: &str,
        role: WorkflowRunRole,
        step: &WorkflowStep,
        is_final_step: bool,
        scene_id: Option<&str>,
    ) -> Self {
        let base_id = format!(
            "turn-{turn_id}:{}:{}:{}",
            role.as_str(),
            workflow_id,
            step.id
        );
        let primary_kind = if role == WorkflowRunRole::Root {
            ResponseSectionKind::Routing
        } else if is_final_step {
            ResponseSectionKind::FinalAnswer
        } else {
            ResponseSectionKind::Step
        };
        let primary_title = if primary_kind == ResponseSectionKind::FinalAnswer {
            "Final Answer".to_string()
        } else {
            step.label.clone()
        };
        let metadata = ResponseSectionMetadata {
            scene_id: scene_id.map(ToOwned::to_owned),
            workflow_id: workflow_id.to_string(),
            workflow_role: role,
            step_id: Some(step.id.clone()),
            step_label: Some(step.label.clone()),
        };

        Self {
            tx,
            turn_id,
            primary_section_id: base_id.clone(),
            thinking_section_id: format!("{base_id}:thinking"),
            primary_section: ResponseSection {
                id: base_id,
                parent_id: None,
                kind: primary_kind,
                title: primary_title,
                state: ResponseSectionState::Streaming,
                metadata: metadata.clone(),
            },
            thinking_section: ResponseSection {
                id: String::new(),
                parent_id: None,
                kind: ResponseSectionKind::Thinking,
                title: "Thinking".to_string(),
                state: ResponseSectionState::Streaming,
                metadata,
            },
            thinking_started: false,
            primary_sanitizer: ProviderMarkupSanitizer::default(),
            thinking_sanitizer: ProviderMarkupSanitizer::default(),
        }
    }

    pub(crate) fn begin(&self) {
        send_begin_response_section(self.tx, self.turn_id, self.primary_section.clone());
    }

    pub(crate) fn primary_section_id(&self) -> &str {
        &self.primary_section_id
    }

    pub(crate) fn push_chat_event(&mut self, event: &ChatEvent) {
        match event {
            ChatEvent::TextDelta { text } if !text.is_empty() => {
                let sanitized = self.primary_sanitizer.push(text);
                self.append_primary_text(&sanitized);
            }
            ChatEvent::ThinkingDelta { thinking, .. } if !thinking.is_empty() => {
                let sanitized = self.thinking_sanitizer.push(thinking);
                self.append_thinking_text(&sanitized);
            }
            _ => {}
        }
    }

    pub(crate) fn complete(&mut self) {
        self.flush_pending_sanitized_text();
        if self.thinking_started {
            send_complete_response_section(
                self.tx,
                self.turn_id,
                &self.thinking_section_id,
                ResponseSectionState::Complete,
            );
        }
        send_complete_response_section(
            self.tx,
            self.turn_id,
            &self.primary_section_id,
            ResponseSectionState::Complete,
        );
    }

    pub(crate) fn fail(&mut self) {
        self.flush_pending_sanitized_text();
        if self.thinking_started {
            send_complete_response_section(
                self.tx,
                self.turn_id,
                &self.thinking_section_id,
                ResponseSectionState::Failed,
            );
        }
        send_complete_response_section(
            self.tx,
            self.turn_id,
            &self.primary_section_id,
            ResponseSectionState::Failed,
        );
    }

    fn append_primary_text(&self, text: &str) {
        if text.is_empty() {
            return;
        }
        send_append_response_section(
            self.tx,
            self.turn_id,
            &self.primary_section_id,
            ResponseSectionDelta::Text(text.to_string()),
        );
    }

    fn append_thinking_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if !self.thinking_started {
            self.thinking_started = true;
            self.thinking_section.id = self.thinking_section_id.clone();
            self.thinking_section.parent_id = Some(self.primary_section_id.clone());
            send_begin_response_section(self.tx, self.turn_id, self.thinking_section.clone());
        }
        send_append_response_section(
            self.tx,
            self.turn_id,
            &self.thinking_section_id,
            ResponseSectionDelta::Text(text.to_string()),
        );
    }

    fn flush_pending_sanitized_text(&mut self) {
        let remaining_primary = self.primary_sanitizer.finish();
        self.append_primary_text(&remaining_primary);

        let remaining_thinking = self.thinking_sanitizer.finish();
        self.append_thinking_text(&remaining_thinking);
    }
}

pub(crate) struct ToolRunTracker<'a> {
    tx: &'a mpsc::Sender<RuntimeUiEnvelope>,
    turn_id: u64,
    parent_section_id: String,
    tool_runs: BTreeMap<String, ToolRun>,
}

impl<'a> ToolRunTracker<'a> {
    pub(crate) fn new(
        tx: &'a mpsc::Sender<RuntimeUiEnvelope>,
        turn_id: u64,
        parent_section_id: String,
    ) -> Self {
        Self {
            tx,
            turn_id,
            parent_section_id,
            tool_runs: BTreeMap::new(),
        }
    }

    pub(crate) fn observe_chat_event(&mut self, event: &ChatEvent) {
        let ChatEvent::ToolUse { id, name, input } = event else {
            return;
        };

        let tool_run = ToolRun {
            id: id.clone(),
            parent_section_id: self.parent_section_id.clone(),
            tool_name: name.clone(),
            status: ToolRunStatus::Running,
            invocation_preview: preview_tool_invocation(name, input),
            result_preview: None,
            detail: ToolRunDetail {
                title: format!(" Tool: {} ", name),
                lines: build_tool_run_detail_lines(name, input, None),
            },
        };
        self.tool_runs.insert(id.clone(), tool_run.clone());
        send_begin_tool_run(self.tx, self.turn_id, tool_run);
    }

    pub(crate) fn complete_tool_run(
        &mut self,
        tool_use_id: &str,
        tool_name: &str,
        tool_input: &serde_json::Value,
        tool_result: &CoreToolResult,
    ) {
        let status = if tool_result.is_error() {
            ToolRunStatus::Failed
        } else {
            ToolRunStatus::Complete
        };

        let mut tool_run = self
            .tool_runs
            .remove(tool_use_id)
            .unwrap_or_else(|| ToolRun {
                id: tool_use_id.to_string(),
                parent_section_id: self.parent_section_id.clone(),
                tool_name: tool_name.to_string(),
                status: ToolRunStatus::Running,
                invocation_preview: preview_tool_invocation(tool_name, tool_input),
                result_preview: None,
                detail: ToolRunDetail {
                    title: format!(" Tool: {} ", tool_name),
                    lines: build_tool_run_detail_lines(tool_name, tool_input, None),
                },
            });

        tool_run.status = status;
        tool_run.result_preview = tool_result_preview(tool_result, 100);
        tool_run.detail = ToolRunDetail {
            title: format!(" Tool: {} ", tool_name),
            lines: build_tool_run_detail_lines(tool_name, tool_input, Some(tool_result)),
        };

        send_update_tool_run(self.tx, self.turn_id, tool_run.clone());
        send_complete_tool_run(self.tx, self.turn_id, &tool_run.id, status);
        self.tool_runs.insert(tool_run.id.clone(), tool_run);
    }
}

#[derive(Default)]
pub(crate) struct ProviderMarkupSanitizer {
    carry: String,
    stripping_until: Option<&'static str>,
}

impl ProviderMarkupSanitizer {
    pub(crate) fn push(&mut self, chunk: &str) -> String {
        let mut input = std::mem::take(&mut self.carry);
        input.push_str(chunk);
        let mut output = String::new();

        loop {
            if let Some(closing_marker) = self.stripping_until {
                if let Some(position) = input.find(closing_marker) {
                    input = input[position + closing_marker.len()..].to_string();
                    self.stripping_until = None;
                    continue;
                }

                let keep = suffix_prefix_len(&input, &[closing_marker]);
                self.carry = tail_fragment(&input, keep);
                return output;
            }

            if let Some((position, closing_marker)) = find_markup_opening(&input) {
                output.push_str(&input[..position]);
                let remainder = &input[position..];
                if let Some(tag_end) = remainder.find('>') {
                    input = remainder[tag_end + 1..].to_string();
                    self.stripping_until = Some(closing_marker);
                    continue;
                }

                self.carry = remainder.to_string();
                return output;
            }

            let keep = suffix_prefix_len(&input, &["<minimax:tool_call", "<invoke"]);
            if keep == 0 {
                output.push_str(&input);
                self.carry.clear();
            } else {
                output.push_str(&input[..input.len() - keep]);
                self.carry = tail_fragment(&input, keep);
            }
            return output;
        }
    }

    pub(crate) fn finish(&mut self) -> String {
        if self.stripping_until.is_some() {
            self.carry.clear();
            self.stripping_until = None;
            String::new()
        } else {
            std::mem::take(&mut self.carry)
        }
    }
}
