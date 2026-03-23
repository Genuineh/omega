use std::time::{Duration, Instant};

use crossterm::event::KeyEvent;
use omega_keymap::{InteractionMode, KeyFocus};
use ratatui::{layout::Rect, widgets::ListState};

use omega_observability::strip_ansi;
use omega_session::{
    OverlayTarget, ResponseSection, ResponseSectionKind, ResponseSectionState, RuntimeUiEnvelope,
    StatusSlot, StatusValue, StepContextWrite, StepContextWriteKind, StepDiagnostics,
    StepInputStatus, StepOutputStatus, ToolRun, ToolRunStatus, WorkflowRunRole,
};

use crate::overlay::{
    ConfirmChoice, ConfirmIntent, ConfirmOverlay, DetailOverlay, InputPromptOverlay, OverlayState,
    PickerOverlay, SearchOverlay,
};
use crate::reducer::{session_status_from_status, workflow_summary_from_status, TuiUpdateReducer};
use crate::sidebar::{SidebarSection, SidebarState};

const TODO_UNSYNCED_LINES: &[&str] = &[
    "No todo snapshot yet.",
    "Call the todo tool to track the current task.",
];
const TODO_EMPTY_LINES: &[&str] = &[
    "Todo list is empty.",
    "Call the todo tool when the task splits into steps.",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Response,
    SidebarRail,
    Diagnostics,
    Todo,
    Logs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PanelTextPoint {
    pub line_index: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanelTextSelection {
    pub panel: Panel,
    pub anchor: PanelTextPoint,
    pub focus: PanelTextPoint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrappedPanelLine {
    pub source_line_index: usize,
    pub source_column_start: usize,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MsgKind {
    User,
    Agent,
    Error,
    Separator,
    Routing,
    Step,
    FinalAnswer,
    Thinking,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Msg {
    pub kind: MsgKind,
    pub text: String,
    pub id: Option<String>,
    pub parent_id: Option<String>,
    pub title: Option<String>,
    pub state: Option<ResponseSectionState>,
    pub workflow_id: Option<String>,
    pub workflow_role: Option<WorkflowRunRole>,
    pub scene_id: Option<String>,
    pub collapsed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseDisplayLine {
    pub kind: MsgKind,
    pub text: String,
    pub is_header: bool,
    pub message_id: Option<String>,
    pub action: Option<ResponseLineAction>,
    pub is_tool_line: bool,
    pub tool_status: Option<ToolRunStatus>,
    pub response_state: Option<ResponseSectionState>,
    pub thinking_line_kind: Option<ThinkingLineKind>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResponseLineAction {
    ToggleThinkingSection(String),
    OpenToolRunDetail(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResponseActivation {
    ThinkingCollapsed,
    ThinkingExpanded,
    ToolDetailOpened(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DiagnosticsLine {
    text: String,
    diagnostic_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThinkingLineKind {
    Summary,
    Body,
    Placeholder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TodoPanelStatus {
    NeverSynced,
    Empty,
    Ready,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TodoSummary {
    pub completed: usize,
    pub total: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowSummary {
    pub workflow_id: String,
    pub workflow_role: WorkflowRunRole,
    pub id: String,
    pub label: String,
    pub index: usize,
    pub total: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRoutingSummary {
    pub root_workflow_id: String,
    pub active_workflow_id: String,
    pub active_workflow_role: WorkflowRunRole,
    pub recognized_scene_id: Option<String>,
    pub selected_workflow_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionStatusSummary {
    Label(String),
    Routing(SessionRoutingSummary),
}

pub struct App {
    pub output_msgs: Vec<Msg>,
    pub tool_runs: Vec<ToolRun>,
    step_diagnostics: Vec<StepDiagnostics>,
    diagnostics_lines: Vec<DiagnosticsLine>,
    pub todo_lines: Vec<String>,
    pub todo_status: TodoPanelStatus,
    pub todo_summary: Option<TodoSummary>,
    pub log_lines: Vec<String>,
    pub response_state: ListState,
    pub diagnostics_state: ListState,
    pub todo_state: ListState,
    pub logs_state: ListState,
    pub focused_panel: Panel,
    pub interaction_mode: InteractionMode,
    pub response_pinned: bool,
    pub diagnostics_pinned: bool,
    pub todo_pinned: bool,
    pub logs_pinned: bool,
    pub response_rect: Rect,
    pub input_context_rect: Rect,
    pub input_gap_rect: Rect,
    pub input_rect: Rect,
    pub sidebar_rect: Rect,
    pub sidebar_rail_rect: Rect,
    pub diagnostics_rect: Rect,
    pub todo_rect: Rect,
    pub logs_rect: Rect,
    pub bottom_status_rect: Rect,
    pub input_buffer: String,
    pub cursor_pos: usize,
    pub input_enabled: bool,
    pub show_thinking: bool,
    pub is_running: bool,
    pub active_turn_id: u64,
    pub workflow_summary: Option<WorkflowSummary>,
    pub agent_status_label: Option<String>,
    pub session_status: Option<SessionStatusSummary>,
    pub last_todo_turn_id: Option<u64>,
    pub spinner_tick: u8,
    pub response_displayed_count: usize,
    pub diagnostics_displayed_count: usize,
    pub todo_displayed_count: usize,
    pub logs_displayed_count: usize,
    pub leader_pending_since: Option<Instant>,
    pub pending_key_events: Vec<KeyEvent>,
    pub keymap_source: String,
    pub status_notice: Option<String>,
    pub text_selection: Option<PanelTextSelection>,
    pub mouse_selection_active: bool,
    pub sidebar: SidebarState,
    pub overlay: Option<OverlayState>,
    pub overlay_rect: Rect,
}

impl App {
    pub fn new() -> Self {
        Self {
            output_msgs: vec![],
            tool_runs: vec![],
            step_diagnostics: vec![],
            diagnostics_lines: vec![],
            todo_lines: todo_unsynced_lines(),
            todo_status: TodoPanelStatus::NeverSynced,
            todo_summary: None,
            log_lines: vec![],
            response_state: ListState::default(),
            diagnostics_state: ListState::default(),
            todo_state: ListState::default(),
            logs_state: ListState::default(),
            focused_panel: Panel::Response,
            interaction_mode: InteractionMode::Normal,
            response_pinned: false,
            diagnostics_pinned: false,
            todo_pinned: false,
            logs_pinned: false,
            response_rect: Rect::default(),
            input_context_rect: Rect::default(),
            input_gap_rect: Rect::default(),
            input_rect: Rect::default(),
            sidebar_rect: Rect::default(),
            sidebar_rail_rect: Rect::default(),
            diagnostics_rect: Rect::default(),
            todo_rect: Rect::default(),
            logs_rect: Rect::default(),
            bottom_status_rect: Rect::default(),
            input_buffer: String::new(),
            cursor_pos: 0,
            input_enabled: true,
            show_thinking: true,
            is_running: false,
            active_turn_id: 0,
            workflow_summary: None,
            agent_status_label: None,
            session_status: None,
            last_todo_turn_id: None,
            spinner_tick: 0,
            response_displayed_count: 0,
            diagnostics_displayed_count: 0,
            todo_displayed_count: 0,
            logs_displayed_count: 0,
            leader_pending_since: None,
            pending_key_events: Vec::new(),
            keymap_source: "builtin".to_string(),
            status_notice: None,
            text_selection: None,
            mouse_selection_active: false,
            sidebar: SidebarState::default(),
            overlay: None,
            overlay_rect: Rect::default(),
        }
    }

    pub fn begin_turn(&mut self) -> u64 {
        self.active_turn_id = self.active_turn_id.wrapping_add(1);
        self.is_running = true;
        self.workflow_summary = None;
        self.session_status = None;
        self.agent_status_label = Some("Running".to_string());
        self.clear_step_diagnostics();
        self.active_turn_id
    }

    pub fn interrupt_turn(&mut self) {
        self.active_turn_id = self.active_turn_id.wrapping_add(1);
        self.is_running = false;
        self.workflow_summary = None;
        self.session_status = None;
        self.agent_status_label = Some("Idle".to_string());
        self.clear_step_diagnostics();
    }

    pub fn is_current_turn(&self, turn_id: u64) -> bool {
        self.active_turn_id == turn_id
    }

    pub fn apply_runtime_envelope(&mut self, envelope: RuntimeUiEnvelope) {
        TuiUpdateReducer::apply(self, envelope);
    }

    pub fn set_status_slot(&mut self, slot: StatusSlot, value: StatusValue) {
        match slot {
            StatusSlot::Workflow => {
                self.workflow_summary = workflow_summary_from_status(value);
            }
            StatusSlot::Agent => match value {
                StatusValue::Label(label) => {
                    self.is_running = label != "Idle";
                    self.agent_status_label = Some(label);
                }
                StatusValue::Hidden => {
                    self.is_running = false;
                    self.agent_status_label = None;
                }
                StatusValue::SessionRouting { .. } => {}
                StatusValue::WorkflowStep { .. } => {}
            },
            StatusSlot::Session => match value {
                StatusValue::WorkflowStep { .. } => {}
                value => self.session_status = session_status_from_status(value),
            },
        }
    }

    pub fn clear_status_slot(&mut self, slot: StatusSlot) {
        match slot {
            StatusSlot::Workflow => self.workflow_summary = None,
            StatusSlot::Agent => {
                self.is_running = false;
                self.agent_status_label = None;
            }
            StatusSlot::Session => self.session_status = None,
        }
    }

    pub fn hide_overlay_target(&mut self, target: OverlayTarget) {
        match (target, self.overlay.as_ref()) {
            (OverlayTarget::Search, Some(OverlayState::Search(_)))
            | (OverlayTarget::Confirm, Some(OverlayState::Confirm(_)))
            | (OverlayTarget::Detail, Some(OverlayState::Detail(_)))
            | (OverlayTarget::Picker, Some(OverlayState::Picker(_)))
            | (OverlayTarget::InputPrompt, Some(OverlayState::InputPrompt(_))) => {
                self.close_overlay();
            }
            _ => {}
        }
    }

    pub fn add_log(&mut self, line: String) {
        self.log_lines.push(strip_ansi(&line));
    }

    pub fn upsert_step_diagnostics(&mut self, diagnostics: StepDiagnostics) {
        let sanitized = sanitize_step_diagnostics(diagnostics);
        if let Some(existing) = self
            .step_diagnostics
            .iter_mut()
            .find(|existing| existing.id == sanitized.id)
        {
            *existing = sanitized;
        } else {
            self.step_diagnostics.push(sanitized);
        }
        self.step_diagnostics.sort_by(|left, right| {
            (left.workflow_role.as_str(), left.workflow_id.as_str(), left.index, left.step_id.as_str())
                .cmp(&(right.workflow_role.as_str(), right.workflow_id.as_str(), right.index, right.step_id.as_str()))
        });
        self.rebuild_diagnostics_lines();
    }

    fn clear_step_diagnostics(&mut self) {
        self.step_diagnostics.clear();
        self.diagnostics_lines.clear();
        self.diagnostics_state.select(None);
        self.diagnostics_displayed_count = 0;
        self.diagnostics_pinned = false;
    }

    fn rebuild_diagnostics_lines(&mut self) {
        self.diagnostics_lines = self
            .step_diagnostics
            .iter()
            .flat_map(build_diagnostics_lines)
            .collect();
    }

    pub fn set_todo_snapshot(&mut self, turn_id: u64, rendered: &str) {
        let clean = strip_ansi(rendered);
        self.last_todo_turn_id = Some(turn_id);

        if clean.trim().is_empty() || clean.trim() == "No todos." {
            self.todo_status = TodoPanelStatus::Empty;
            self.todo_summary = Some(TodoSummary {
                completed: 0,
                total: 0,
            });
            self.todo_lines = todo_empty_lines();
            return;
        }

        let mut lines: Vec<String> = clean.lines().map(ToOwned::to_owned).collect();
        let summary = extract_summary(&mut lines).or_else(|| summarize_todo_lines(&lines));
        if lines.last().is_some_and(|line| line.is_empty()) {
            lines.pop();
        }

        self.todo_status = TodoPanelStatus::Ready;
        self.todo_summary = summary;
        self.todo_lines = lines;
    }

    pub fn push_msg(&mut self, kind: MsgKind, text: &str) {
        let clean = strip_ansi(text);
        self.output_msgs.push(Msg::plain(kind, clean));
    }

    pub fn begin_response_section(&mut self, section: ResponseSection) {
        self.output_msgs.push(Msg::from_response_section(section));
    }

    pub fn begin_tool_run(&mut self, tool_run: ToolRun) {
        self.upsert_tool_run(tool_run, true);
    }

    pub fn update_tool_run(&mut self, tool_run: ToolRun) {
        self.upsert_tool_run(tool_run, true);
    }

    pub fn complete_tool_run(&mut self, id: &str, status: ToolRunStatus) {
        if let Some(tool_run) = self.tool_runs.iter_mut().find(|tool_run| tool_run.id == id) {
            tool_run.status = status;
        }
    }

    pub fn append_response_section(&mut self, id: &str, delta: &str) {
        if let Some(message) = self
            .output_msgs
            .iter_mut()
            .find(|message| message.id.as_deref() == Some(id))
        {
            message.text.push_str(&strip_ansi(delta));
        }
    }

    pub fn complete_response_section(&mut self, id: &str, state: ResponseSectionState) {
        if let Some(message) = self
            .output_msgs
            .iter_mut()
            .find(|message| message.id.as_deref() == Some(id))
        {
            message.state = Some(state);
            if message.kind == MsgKind::Thinking {
                message.collapsed = true;
            }
        }
    }

    pub fn set_show_thinking(&mut self, show_thinking: bool) {
        self.show_thinking = show_thinking;
    }

    #[cfg(test)]
    pub fn toggle_selected_thinking_section(&mut self) -> Option<bool> {
        let selected = self.response_state.selected()?;
        let lines = self.response_display_lines();
        let message_id = lines.get(selected)?.message_id.as_deref()?;

        self.toggle_thinking_section(message_id)
    }

    pub fn activate_selected_response_item(&mut self) -> Option<ResponseActivation> {
        let selected = self.response_state.selected()?;
        let lines = self.response_display_lines();
        let action = lines.get(selected)?.action.clone()?;

        match action {
            ResponseLineAction::ToggleThinkingSection(id) => {
                let collapsed = self.toggle_thinking_section(&id)?;
                if collapsed {
                    Some(ResponseActivation::ThinkingCollapsed)
                } else {
                    Some(ResponseActivation::ThinkingExpanded)
                }
            }
            ResponseLineAction::OpenToolRunDetail(id) => self
                .open_tool_run_detail(&id)
                .map(ResponseActivation::ToolDetailOpened),
        }
    }

    fn toggle_thinking_section(&mut self, id: &str) -> Option<bool> {
        let message = self.output_msgs.iter_mut().find(|message| {
            message.id.as_deref() == Some(id) && message.kind == MsgKind::Thinking
        })?;
        message.collapsed = !message.collapsed;
        Some(message.collapsed)
    }

    pub fn response_lines(&self) -> Vec<String> {
        self.response_display_lines()
            .into_iter()
            .map(|line| line.text)
            .collect()
    }

    pub fn response_display_lines(&self) -> Vec<ResponseDisplayLine> {
        let mut lines = Vec::new();
        for message in &self.output_msgs {
            if message.kind == MsgKind::Thinking && !self.show_thinking {
                continue;
            }
            lines.extend(self.render_message_lines(message));
        }
        lines
    }

    fn render_message_lines(&self, message: &Msg) -> Vec<ResponseDisplayLine> {
        match message.kind {
            MsgKind::User | MsgKind::Agent | MsgKind::Error | MsgKind::Separator => {
                split_or_empty(&message.text)
                    .into_iter()
                    .map(|text| ResponseDisplayLine {
                        kind: message.kind,
                        text,
                        is_header: false,
                        message_id: message.id.clone(),
                        action: None,
                        is_tool_line: false,
                        tool_status: None,
                        response_state: None,
                        thinking_line_kind: None,
                    })
                    .collect()
            }
            MsgKind::Routing | MsgKind::Step | MsgKind::FinalAnswer | MsgKind::Thinking => {
                let mut lines = Vec::new();
                let message_state = message.state.unwrap_or(ResponseSectionState::Complete);
                let default_action = if message.kind == MsgKind::Thinking {
                    message
                        .id
                        .clone()
                        .map(ResponseLineAction::ToggleThinkingSection)
                } else {
                    None
                };
                lines.push(ResponseDisplayLine {
                    kind: message.kind,
                    text: format_response_header(message),
                    is_header: true,
                    message_id: message.id.clone(),
                    action: default_action.clone(),
                    is_tool_line: false,
                    tool_status: None,
                    response_state: message.state,
                    thinking_line_kind: None,
                });

                if message.kind != MsgKind::Thinking {
                    if let Some(scene_id) = message.scene_id.as_deref() {
                        lines.push(ResponseDisplayLine {
                            kind: message.kind,
                            text: format!("  scene {scene_id}"),
                            is_header: false,
                            message_id: message.id.clone(),
                            action: None,
                            is_tool_line: false,
                            tool_status: None,
                            response_state: None,
                            thinking_line_kind: None,
                        });
                    }
                }

                match message.kind {
                    MsgKind::Routing => {
                        if let Some(preview) = first_non_empty_line(&message.text) {
                            lines.push(ResponseDisplayLine {
                                kind: message.kind,
                                text: format!("  result {preview}"),
                                is_header: false,
                                message_id: message.id.clone(),
                                action: None,
                                is_tool_line: false,
                                tool_status: None,
                                response_state: None,
                                thinking_line_kind: None,
                            });
                        }
                    }
                    MsgKind::Step | MsgKind::FinalAnswer => {
                        let tool_runs = message
                            .id
                            .as_deref()
                            .map(|section_id| self.tool_runs_for_section(section_id))
                            .unwrap_or_default();
                        let body_lines = split_or_empty(&message.text);
                        if body_lines.len() == 1 && body_lines[0].is_empty() && tool_runs.is_empty()
                        {
                            lines.push(ResponseDisplayLine {
                                kind: message.kind,
                                text: "  …".to_string(),
                                is_header: false,
                                message_id: message.id.clone(),
                                action: None,
                                is_tool_line: false,
                                tool_status: None,
                                response_state: None,
                                thinking_line_kind: None,
                            });
                        } else if !(body_lines.len() == 1 && body_lines[0].is_empty()) {
                            lines.extend(body_lines.into_iter().map(|line| ResponseDisplayLine {
                                kind: message.kind,
                                text: format!("  {line}"),
                                is_header: false,
                                message_id: message.id.clone(),
                                action: None,
                                is_tool_line: false,
                                tool_status: None,
                                response_state: None,
                                thinking_line_kind: None,
                            }));
                        }
                        if !tool_runs.is_empty() {
                            lines.push(ResponseDisplayLine {
                                kind: message.kind,
                                text: format_tool_lane_header(&tool_runs),
                                is_header: false,
                                message_id: message.id.clone(),
                                action: None,
                                is_tool_line: true,
                                tool_status: None,
                                response_state: None,
                                thinking_line_kind: None,
                            });
                            lines.extend(tool_runs.into_iter().map(|tool_run| {
                                ResponseDisplayLine {
                                    kind: message.kind,
                                    text: format_tool_summary(tool_run),
                                    is_header: false,
                                    message_id: message.id.clone(),
                                    action: Some(ResponseLineAction::OpenToolRunDetail(
                                        tool_run.id.clone(),
                                    )),
                                    is_tool_line: true,
                                    tool_status: Some(tool_run.status),
                                    response_state: None,
                                    thinking_line_kind: None,
                                }
                            }));
                        }
                    }
                    MsgKind::Thinking => {
                        if message.collapsed {
                            lines.push(ResponseDisplayLine {
                                kind: message.kind,
                                text: format!(
                                    "    = {}",
                                    summarize_thinking_text(&message.text, message_state)
                                ),
                                is_header: false,
                                message_id: message.id.clone(),
                                action: default_action.clone(),
                                is_tool_line: false,
                                tool_status: None,
                                response_state: Some(message_state),
                                thinking_line_kind: Some(ThinkingLineKind::Summary),
                            });
                        } else {
                            let body_lines = split_or_empty(&message.text);
                            if body_lines.len() == 1 && body_lines[0].is_empty() {
                                lines.push(ResponseDisplayLine {
                                    kind: message.kind,
                                    text: format!(
                                        "    | {}",
                                        thinking_placeholder_text(message_state)
                                    ),
                                    is_header: false,
                                    message_id: message.id.clone(),
                                    action: default_action.clone(),
                                    is_tool_line: false,
                                    tool_status: None,
                                    response_state: Some(message_state),
                                    thinking_line_kind: Some(ThinkingLineKind::Placeholder),
                                });
                            } else {
                                lines.extend(body_lines.into_iter().map(|line| {
                                    ResponseDisplayLine {
                                        kind: message.kind,
                                        text: format!("    | {line}"),
                                        is_header: false,
                                        message_id: message.id.clone(),
                                        action: default_action.clone(),
                                        is_tool_line: false,
                                        tool_status: None,
                                        response_state: Some(message_state),
                                        thinking_line_kind: Some(ThinkingLineKind::Body),
                                    }
                                }));
                            }
                        }
                    }
                    _ => {}
                }

                lines
            }
        }
    }

    pub fn activate_selected_diagnostics_item(&mut self) -> Option<String> {
        let selected = self.diagnostics_state.selected()?;
        let width = (self.diagnostics_rect.width as usize).saturating_sub(2).max(1);
        let line = self
            .wrapped_panel_lines(Panel::Diagnostics, width)
            .get(selected)
            .cloned()?;
        let diagnostic_id = self
            .diagnostics_lines
            .get(line.source_line_index)
            .and_then(|line| line.diagnostic_id.clone())?;
        self.open_step_diagnostics_detail(&diagnostic_id)
    }

    pub fn scroll_panel_up(&mut self, panel: Panel, amount: usize) {
        match panel {
            Panel::Response => {
                self.response_pinned = true;
                let current = self.response_state.selected().unwrap_or_else(|| {
                    self.response_displayed_count
                        .max(self.output_msgs.len())
                        .saturating_sub(1)
                });
                self.response_state
                    .select(Some(current.saturating_sub(amount)));
            }
            Panel::Diagnostics => {
                self.diagnostics_pinned = true;
                let current = self.diagnostics_state.selected().unwrap_or_else(|| {
                    self.diagnostics_displayed_count
                        .max(self.diagnostics_lines.len())
                        .saturating_sub(1)
                });
                self.diagnostics_state
                    .select(Some(current.saturating_sub(amount)));
            }
            Panel::Todo => {
                self.todo_pinned = true;
                let current = self.todo_state.selected().unwrap_or_else(|| {
                    self.todo_displayed_count
                        .max(self.todo_lines.len())
                        .saturating_sub(1)
                });
                self.todo_state.select(Some(current.saturating_sub(amount)));
            }
            Panel::Logs => {
                self.logs_pinned = true;
                let current = self.logs_state.selected().unwrap_or_else(|| {
                    self.logs_displayed_count
                        .max(self.log_lines.len())
                        .saturating_sub(1)
                });
                self.logs_state.select(Some(current.saturating_sub(amount)));
            }
            Panel::SidebarRail => {}
        }
    }

    pub fn scroll_panel_down(&mut self, panel: Panel, amount: usize) {
        match panel {
            Panel::Response => {
                let last = self
                    .response_displayed_count
                    .max(self.output_msgs.len())
                    .saturating_sub(1);
                let current = self.response_state.selected().unwrap_or(last);
                let new_idx = (current + amount).min(last);
                self.response_state.select(Some(new_idx));
                if new_idx >= last {
                    self.response_pinned = false;
                }
            }
            Panel::Diagnostics => {
                let last = self
                    .diagnostics_displayed_count
                    .max(self.diagnostics_lines.len())
                    .saturating_sub(1);
                let current = self.diagnostics_state.selected().unwrap_or(last);
                let new_idx = (current + amount).min(last);
                self.diagnostics_state.select(Some(new_idx));
                if new_idx >= last {
                    self.diagnostics_pinned = false;
                }
            }
            Panel::Todo => {
                let last = self
                    .todo_displayed_count
                    .max(self.todo_lines.len())
                    .saturating_sub(1);
                let current = self.todo_state.selected().unwrap_or(last);
                let new_idx = (current + amount).min(last);
                self.todo_state.select(Some(new_idx));
                if new_idx >= last {
                    self.todo_pinned = false;
                }
            }
            Panel::Logs => {
                let last = self
                    .logs_displayed_count
                    .max(self.log_lines.len())
                    .saturating_sub(1);
                let current = self.logs_state.selected().unwrap_or(last);
                let new_idx = (current + amount).min(last);
                self.logs_state.select(Some(new_idx));
                if new_idx >= last {
                    self.logs_pinned = false;
                }
            }
            Panel::SidebarRail => {}
        }
    }

    pub fn panel_at(&self, col: u16, row: u16) -> Panel {
        if self.sidebar_rail_rect.width > 0
            && col >= self.sidebar_rail_rect.x
            && row >= self.sidebar_rail_rect.y
            && row
                < self
                    .sidebar_rail_rect
                    .y
                    .saturating_add(self.sidebar_rail_rect.height)
        {
            Panel::SidebarRail
        } else if self.diagnostics_rect.width > 0
            && col >= self.diagnostics_rect.x
            && row >= self.diagnostics_rect.y
            && row < self.diagnostics_rect.y.saturating_add(self.diagnostics_rect.height)
        {
            Panel::Diagnostics
        } else if self.logs_rect.width > 0
            && col >= self.logs_rect.x
            && row >= self.logs_rect.y
            && row < self.logs_rect.y.saturating_add(self.logs_rect.height)
        {
            Panel::Logs
        } else if self.todo_rect.width > 0
            && col >= self.todo_rect.x
            && row >= self.todo_rect.y
            && row < self.todo_rect.y.saturating_add(self.todo_rect.height)
        {
            Panel::Todo
        } else {
            Panel::Response
        }
    }

    pub fn todo_visible(&self) -> bool {
        self.todo_rect.width > 0 && self.todo_rect.height > 0
    }

    pub fn diagnostics_visible(&self) -> bool {
        self.diagnostics_rect.width > 0 && self.diagnostics_rect.height > 0
    }

    pub fn logs_visible(&self) -> bool {
        self.logs_rect.width > 0 && self.logs_rect.height > 0
    }

    pub fn sidebar_visible(&self) -> bool {
        self.sidebar_rect.width > 0 && self.sidebar_rect.height > 0
    }

    pub fn normalize_focus(&mut self) {
        if self.focused_panel == Panel::SidebarRail && !self.sidebar_visible() {
            self.focused_panel = Panel::Response;
        }
        if self.focused_panel == Panel::Diagnostics && !self.diagnostics_visible() {
            self.focused_panel = Panel::Response;
        }
        if self.focused_panel == Panel::Todo && !self.todo_visible() {
            self.focused_panel = Panel::Response;
        }
        if self.focused_panel == Panel::Logs && !self.logs_visible() {
            self.focused_panel = Panel::Response;
        }
    }

    pub fn normalize_mode(&mut self) {
        if self.interaction_mode == InteractionMode::Insert && !self.input_capable() {
            self.interaction_mode = InteractionMode::Normal;
            self.status_notice =
                Some("Insert mode is unavailable in the current context.".to_string());
        }
    }

    pub fn next_focus_panel(&self) -> Panel {
        let mut panels = vec![Panel::Response];
        if self.sidebar_visible() {
            panels.push(Panel::SidebarRail);
        }
        if self.diagnostics_visible() {
            panels.push(Panel::Diagnostics);
        }
        if self.todo_visible() {
            panels.push(Panel::Todo);
        }
        if self.logs_visible() {
            panels.push(Panel::Logs);
        }

        let current_index = panels
            .iter()
            .position(|panel| *panel == self.focused_panel)
            .unwrap_or(0);
        panels[(current_index + 1) % panels.len()]
    }

    pub fn todo_refresh_pending(&self) -> bool {
        self.is_running && self.last_todo_turn_id != Some(self.active_turn_id)
    }

    pub fn key_focus(&self) -> KeyFocus {
        match self.focused_panel {
            Panel::Response => KeyFocus::Response,
            Panel::SidebarRail => KeyFocus::SidebarRail,
            Panel::Diagnostics => KeyFocus::Activity,
            Panel::Todo => KeyFocus::Todo,
            Panel::Logs => KeyFocus::Activity,
        }
    }

    pub fn input_capable(&self) -> bool {
        self.input_enabled
    }

    pub fn is_leader_pending(&self) -> bool {
        self.leader_pending_since.is_some()
    }

    pub fn pending_key_events(&self) -> &[KeyEvent] {
        &self.pending_key_events
    }

    pub fn begin_leader_pending(&mut self, leader_key: KeyEvent) {
        self.leader_pending_since = Some(Instant::now());
        self.pending_key_events.clear();
        self.pending_key_events.push(leader_key);
        self.status_notice = None;
    }

    pub fn extend_pending_sequence(&mut self, key: KeyEvent) {
        self.leader_pending_since = Some(Instant::now());
        self.pending_key_events.push(key);
        self.status_notice = None;
    }

    pub fn clear_leader_pending(&mut self) {
        self.leader_pending_since = None;
        self.pending_key_events.clear();
    }

    pub fn expire_leader_pending(&mut self, timeout: Duration) {
        if let Some(start) = self.leader_pending_since {
            if start.elapsed() >= timeout {
                self.clear_leader_pending();
                self.status_notice = Some("Leader sequence timed out.".to_string());
            }
        }
    }

    pub fn enter_normal_mode(&mut self) {
        self.interaction_mode = InteractionMode::Normal;
        self.clear_leader_pending();
    }

    pub fn enter_insert_mode(&mut self) -> bool {
        if !self.input_capable() {
            return false;
        }

        self.interaction_mode = InteractionMode::Insert;
        self.clear_leader_pending();
        true
    }

    pub fn set_status_notice(&mut self, notice: impl Into<String>) {
        self.status_notice = Some(notice.into());
    }

    pub fn clear_status_notice(&mut self) {
        self.status_notice = None;
    }

    pub fn set_keymap_source(&mut self, source: impl Into<String>) {
        self.keymap_source = source.into();
    }

    pub fn toggle_sidebar_shell(&mut self) {
        self.sidebar.toggle_shell();
        if self.sidebar.shell_collapsed {
            self.focused_panel = Panel::Response;
        }
    }

    pub fn focus_sidebar_rail(&mut self) {
        if self.sidebar_visible() {
            self.focused_panel = Panel::SidebarRail;
        }
    }

    pub fn cycle_sidebar_rail_next(&mut self) {
        self.sidebar.cycle_next();
    }

    pub fn cycle_sidebar_rail_previous(&mut self) {
        self.sidebar.cycle_previous();
    }

    pub fn toggle_selected_sidebar_section(&mut self) {
        if !self.sidebar.toggle_selected_section() {
            self.set_status_notice("At least one sidebar section must remain open.");
        }
        self.normalize_focus();
    }

    pub fn activate_sidebar_selection(&mut self) {
        match self.sidebar.rail_selection {
            SidebarSection::Diagnostics => {
                if !self.sidebar.diagnostics_expanded {
                    self.sidebar.diagnostics_expanded = true;
                }
                if self.diagnostics_visible() {
                    self.focused_panel = Panel::Diagnostics;
                }
            }
            SidebarSection::Todos => {
                if !self.sidebar.todos_expanded {
                    self.sidebar.todos_expanded = true;
                }
                if self.todo_visible() {
                    self.focused_panel = Panel::Todo;
                }
            }
            SidebarSection::Logs => {
                if !self.sidebar.logs_expanded {
                    self.sidebar.logs_expanded = true;
                }
                if self.logs_visible() {
                    self.focused_panel = Panel::Logs;
                }
            }
        }
    }

    pub fn logs_panel_title(&self) -> String {
        let mut title = " Activity & Logs ".to_string();
        if self.focused_panel == Panel::Logs {
            title.push('◆');
            title.push(' ');
        }
        title
    }

    pub fn diagnostics_panel_title(&self) -> String {
        let invalid = self
            .step_diagnostics
            .iter()
            .filter(|diagnostics| diagnostics.output.status == StepOutputStatus::Invalid)
            .count();
        let mut title = if invalid > 0 {
            format!(" Contract Diagnostics (!{}) ", invalid)
        } else {
            " Contract Diagnostics ".to_string()
        };
        if self.focused_panel == Panel::Diagnostics {
            title.push('◆');
            title.push(' ');
        }
        title
    }

    pub fn rail_badge(&self, section: SidebarSection) -> String {
        match section {
            SidebarSection::Diagnostics => {
                let total = self.step_diagnostics.len();
                let invalid = self
                    .step_diagnostics
                    .iter()
                    .filter(|diagnostics| diagnostics.output.status == StepOutputStatus::Invalid)
                    .count();
                let pending = self
                    .step_diagnostics
                    .iter()
                    .filter(|diagnostics| diagnostics.output.status == StepOutputStatus::Pending)
                    .count();
                if invalid > 0 {
                    format!("D !{}", invalid)
                } else if pending > 0 {
                    format!("D …{}", pending)
                } else if total > 0 {
                    format!("D {}", total)
                } else {
                    "D --".to_string()
                }
            }
            SidebarSection::Todos => match self.todo_summary {
                Some(summary) => format!("T {}/{}", summary.completed, summary.total),
                None => "T --".to_string(),
            },
            SidebarSection::Logs => {
                format!("{}", self.log_lines.len())
            }
        }
    }

    pub fn overlay_active(&self) -> bool {
        self.overlay.is_some()
    }

    pub fn open_search_overlay(&mut self) {
        self.overlay = Some(OverlayState::Search(SearchOverlay {
            origin_panel: self.focused_panel,
            target_panel: self.focused_panel,
            query: String::new(),
            cursor_pos: 0,
        }));
        self.clear_leader_pending();
        self.status_notice = Some("Search popup opened for the focused panel.".to_string());
    }

    pub fn open_interrupt_confirm_overlay(&mut self, turn_id: u64) {
        self.overlay = Some(OverlayState::Confirm(ConfirmOverlay {
            origin_panel: self.focused_panel,
            title: " Confirm interrupt ".to_string(),
            message: "Stop the running turn and keep the current transcript?".to_string(),
            confirm_label: "Interrupt".to_string(),
            cancel_label: "Keep running".to_string(),
            selected: ConfirmChoice::Cancel,
            intent: ConfirmIntent::InterruptTurn { turn_id },
            dismiss_on_backdrop: false,
        }));
        self.clear_leader_pending();
        self.status_notice =
            Some("Confirm interrupt in the overlay before stopping the turn.".to_string());
    }

    #[allow(dead_code)]
    pub fn open_detail_overlay(&mut self, title: impl Into<String>, lines: Vec<String>) {
        self.overlay = Some(OverlayState::Detail(DetailOverlay {
            origin_panel: self.focused_panel,
            title: title.into(),
            lines,
            scroll: 0,
            dismiss_on_backdrop: true,
        }));
        self.clear_leader_pending();
    }

    fn open_tool_run_detail(&mut self, id: &str) -> Option<String> {
        let tool_run = self.tool_runs.iter().find(|tool_run| tool_run.id == id)?;
        let title = tool_run.detail.title.clone();
        let lines = tool_run.detail.lines.clone();
        let tool_name = tool_run.tool_name.clone();
        self.open_detail_overlay(title, lines);
        Some(tool_name)
    }

    fn open_step_diagnostics_detail(&mut self, id: &str) -> Option<String> {
        let diagnostics = self
            .step_diagnostics
            .iter()
            .find(|diagnostics| diagnostics.id == id)?;
        let title = format!(
            " Contract Diagnostics {}:{} {} ",
            diagnostics.workflow_role.as_str(),
            diagnostics.workflow_id,
            diagnostics.step_label
        );
        let lines = build_step_diagnostics_detail_lines(diagnostics);
        let label = diagnostics.step_label.clone();
        self.open_detail_overlay(title, lines);
        Some(label)
    }

    fn upsert_tool_run(&mut self, tool_run: ToolRun, append_if_missing: bool) {
        let sanitized = sanitize_tool_run(tool_run);
        if let Some(existing) = self
            .tool_runs
            .iter_mut()
            .find(|existing| existing.id == sanitized.id)
        {
            *existing = sanitized;
        } else if append_if_missing {
            self.tool_runs.push(sanitized);
        }
    }

    fn tool_runs_for_section(&self, section_id: &str) -> Vec<&ToolRun> {
        self.tool_runs
            .iter()
            .filter(|tool_run| tool_run.parent_section_id == section_id)
            .collect()
    }

    #[allow(dead_code)]
    pub fn open_picker_overlay(&mut self, title: impl Into<String>, items: Vec<String>) {
        self.overlay = Some(OverlayState::Picker(PickerOverlay {
            origin_panel: self.focused_panel,
            title: title.into(),
            items,
            selected: 0,
            dismiss_on_backdrop: true,
        }));
        self.clear_leader_pending();
    }

    #[allow(dead_code)]
    pub fn open_input_prompt_overlay(
        &mut self,
        title: impl Into<String>,
        prompt: impl Into<String>,
    ) {
        self.overlay = Some(OverlayState::InputPrompt(InputPromptOverlay {
            origin_panel: self.focused_panel,
            title: title.into(),
            prompt: prompt.into(),
            value: String::new(),
            cursor_pos: 0,
            dismiss_on_backdrop: true,
        }));
        self.clear_leader_pending();
    }

    pub fn close_overlay(&mut self) {
        if let Some(overlay) = self.overlay.take() {
            self.focused_panel = overlay.origin_panel();
            self.normalize_focus();
        }
    }

    pub fn panel_search_match_count(&self) -> Option<(Panel, usize)> {
        let OverlayState::Search(overlay) = self.overlay.as_ref()? else {
            return None;
        };

        let query = overlay.query.trim();
        if query.is_empty() {
            return Some((overlay.target_panel, 0));
        }

        let query = query.to_ascii_lowercase();
        let count = self
            .panel_lines(overlay.target_panel)
            .into_iter()
            .map(|line| line.to_ascii_lowercase().matches(&query).count())
            .sum();

        Some((overlay.target_panel, count))
    }

    pub fn panel_lines(&self, panel: Panel) -> Vec<String> {
        match panel {
            Panel::Response => self.response_lines(),
            Panel::SidebarRail => Vec::new(),
            Panel::Diagnostics => self
                .diagnostics_lines
                .iter()
                .map(|line| line.text.clone())
                .collect(),
            Panel::Todo => self.todo_lines.clone(),
            Panel::Logs => self.log_lines.clone(),
        }
    }

    pub fn wrapped_panel_lines(&self, panel: Panel, width: usize) -> Vec<WrappedPanelLine> {
        self.panel_lines(panel)
            .into_iter()
            .enumerate()
            .flat_map(|(source_line_index, line)| {
                wrap_text_segments(&line, width).into_iter().map(
                    move |(source_column_start, text)| WrappedPanelLine {
                        source_line_index,
                        source_column_start,
                        text,
                    },
                )
            })
            .collect()
    }

    pub fn selection_for_panel(&self, panel: Panel) -> Option<(PanelTextPoint, PanelTextPoint)> {
        let selection = self.text_selection?;
        if selection.panel != panel {
            return None;
        }

        Some(normalize_points(selection.anchor, selection.focus))
    }

    pub fn selection_range_for_segment(
        &self,
        panel: Panel,
        source_line_index: usize,
        source_column_start: usize,
        source_column_end: usize,
    ) -> Option<(usize, usize)> {
        let (selection_start, selection_end) = self.selection_for_panel(panel)?;
        if selection_start == selection_end {
            return None;
        }

        let segment_start = PanelTextPoint {
            line_index: source_line_index,
            column: source_column_start,
        };
        let segment_end = PanelTextPoint {
            line_index: source_line_index,
            column: source_column_end,
        };

        let overlap_start = max_text_point(selection_start, segment_start);
        let overlap_end = min_text_point(selection_end, segment_end);
        if overlap_start >= overlap_end {
            return None;
        }

        Some((
            overlap_start.column.saturating_sub(source_column_start),
            overlap_end.column.saturating_sub(source_column_start),
        ))
    }

    pub fn clear_text_selection(&mut self) {
        self.text_selection = None;
        self.mouse_selection_active = false;
    }

    pub fn begin_mouse_selection(&mut self, panel: Panel, col: u16, row: u16) -> bool {
        let Some(point) = self.panel_text_point_at(panel, col, row) else {
            self.clear_text_selection();
            return false;
        };

        self.text_selection = Some(PanelTextSelection {
            panel,
            anchor: point,
            focus: point,
        });
        self.mouse_selection_active = true;
        true
    }

    pub fn update_mouse_selection(&mut self, col: u16, row: u16) -> bool {
        let Some(selection) = self.text_selection else {
            return false;
        };
        if !self.mouse_selection_active {
            return false;
        }

        let Some(point) = self.panel_text_point_at(selection.panel, col, row) else {
            return false;
        };

        if let Some(selection) = self.text_selection.as_mut() {
            selection.focus = point;
        }
        true
    }

    pub fn finish_mouse_selection(&mut self, col: u16, row: u16) -> Option<String> {
        if !self.mouse_selection_active {
            return None;
        }

        self.update_mouse_selection(col, row);
        self.mouse_selection_active = false;

        let text = self.selected_text();
        if text.as_deref().is_some_and(|value| !value.is_empty()) {
            text
        } else {
            self.text_selection = None;
            None
        }
    }

    pub fn selected_text(&self) -> Option<String> {
        let selection = self.text_selection?;
        let (start, end) = normalize_points(selection.anchor, selection.focus);
        if start == end {
            return None;
        }

        let lines = self.panel_lines(selection.panel);
        if start.line_index >= lines.len() {
            return None;
        }

        let final_line = end.line_index.min(lines.len().saturating_sub(1));
        let mut selected = Vec::new();

        for (offset, line) in lines[start.line_index..=final_line].iter().enumerate() {
            let line_index = start.line_index + offset;
            let line_len = line.chars().count();
            let start_column = if line_index == start.line_index {
                start.column.min(line_len)
            } else {
                0
            };
            let end_column = if line_index == final_line {
                end.column.min(line_len)
            } else {
                line_len
            };

            if line_index == final_line && start_column >= end_column {
                continue;
            }

            selected.push(slice_chars(line, start_column, end_column));
        }

        Some(selected.join("\n"))
    }

    pub fn panel_text_point_at(&self, panel: Panel, col: u16, row: u16) -> Option<PanelTextPoint> {
        let inner = self.panel_inner_rect(panel)?;
        if inner.width == 0 || inner.height == 0 {
            return None;
        }
        if col < inner.x
            || col >= inner.x.saturating_add(inner.width)
            || row < inner.y
            || row >= inner.y.saturating_add(inner.height)
        {
            return None;
        }

        let wrapped_lines = self.wrapped_panel_lines(panel, inner.width as usize);
        let viewport_index = self.panel_scroll_offset(panel) + (row - inner.y) as usize;
        let wrapped_line = wrapped_lines.get(viewport_index)?;
        let line_len = wrapped_line.text.chars().count();
        let column = (col - inner.x) as usize;

        Some(PanelTextPoint {
            line_index: wrapped_line.source_line_index,
            column: wrapped_line.source_column_start + column.min(line_len),
        })
    }

    pub fn panel_scroll_offset(&self, panel: Panel) -> usize {
        match panel {
            Panel::Response => self.response_state.offset(),
            Panel::Todo => self.todo_state.offset(),
            Panel::Logs => self.logs_state.offset(),
            Panel::Diagnostics => self.diagnostics_state.offset(),
            Panel::SidebarRail => 0,
        }
    }

    pub fn panel_inner_rect(&self, panel: Panel) -> Option<Rect> {
        let rect = match panel {
            Panel::Response => self.response_rect,
            Panel::Todo => self.todo_rect,
            Panel::Logs => self.logs_rect,
            Panel::Diagnostics => self.diagnostics_rect,
            Panel::SidebarRail => return None,
        };

        if rect.width < 2 || rect.height < 2 {
            return None;
        }

        Some(Rect::new(
            rect.x.saturating_add(1),
            rect.y.saturating_add(1),
            rect.width.saturating_sub(2),
            rect.height.saturating_sub(2),
        ))
    }

    pub fn todo_panel_title(&self) -> String {
        let mut title = match self.todo_status {
            TodoPanelStatus::NeverSynced => " Todos ".to_string(),
            TodoPanelStatus::Empty => " Todos empty ".to_string(),
            TodoPanelStatus::Ready => match self.todo_summary {
                Some(summary) => format!(" Todos {}/{} ", summary.completed, summary.total),
                None => " Todos ".to_string(),
            },
        };

        if self.todo_refresh_pending() {
            title = format!("{}(stale) ", title.trim_end());
        }

        if self.focused_panel == Panel::Todo {
            title.push('◆');
            title.push(' ');
        }

        title
    }

    pub fn char_count(&self) -> usize {
        self.input_buffer.chars().count()
    }

    pub fn cursor_byte_pos(&self) -> usize {
        self.input_buffer
            .char_indices()
            .nth(self.cursor_pos)
            .map(|(i, _)| i)
            .unwrap_or(self.input_buffer.len())
    }

    pub fn insert_char(&mut self, c: char) {
        let byte_pos = self.cursor_byte_pos();
        self.input_buffer.insert(byte_pos, c);
        self.cursor_pos += 1;
    }

    pub fn delete_char_before(&mut self) {
        if self.cursor_pos > 0 {
            self.cursor_pos -= 1;
            let byte_pos = self.cursor_byte_pos();
            self.input_buffer.remove(byte_pos);
        }
    }

    pub fn delete_char_at(&mut self) {
        if self.cursor_pos < self.char_count() {
            let byte_pos = self.cursor_byte_pos();
            self.input_buffer.remove(byte_pos);
        }
    }

    pub fn move_cursor_left(&mut self) {
        self.cursor_pos = self.cursor_pos.saturating_sub(1);
    }

    pub fn move_cursor_right(&mut self) {
        let count = self.char_count();
        if self.cursor_pos < count {
            self.cursor_pos += 1;
        }
    }

    pub fn move_cursor_home(&mut self) {
        self.cursor_pos = 0;
    }

    pub fn move_cursor_end(&mut self) {
        self.cursor_pos = self.char_count();
    }

    pub fn take_input(&mut self) -> String {
        let input = self.input_buffer.clone();
        self.input_buffer.clear();
        self.cursor_pos = 0;
        input
    }
}

fn todo_unsynced_lines() -> Vec<String> {
    TODO_UNSYNCED_LINES
        .iter()
        .map(|line| (*line).to_string())
        .collect()
}

fn todo_empty_lines() -> Vec<String> {
    TODO_EMPTY_LINES
        .iter()
        .map(|line| (*line).to_string())
        .collect()
}

fn extract_summary(lines: &mut Vec<String>) -> Option<TodoSummary> {
    let last_line = lines.last()?.trim();
    let inner = last_line.strip_prefix('(')?.strip_suffix(" completed)")?;
    let (completed, total) = inner.split_once('/')?;
    let summary = TodoSummary {
        completed: completed.trim().parse().ok()?,
        total: total.trim().parse().ok()?,
    };

    lines.pop();
    if lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }

    Some(summary)
}

fn summarize_todo_lines(lines: &[String]) -> Option<TodoSummary> {
    let mut total = 0;
    let mut completed = 0;

    for line in lines {
        let trimmed = line.trim_start();
        if trimmed.starts_with("[ ]") || trimmed.starts_with("[>]") || trimmed.starts_with("[x]") {
            total += 1;
            if trimmed.starts_with("[x]") {
                completed += 1;
            }
        }
    }

    if total == 0 {
        None
    } else {
        Some(TodoSummary { completed, total })
    }
}

pub fn wrap_text_segments(line: &str, width: usize) -> Vec<(usize, String)> {
    if width == 0 {
        return vec![(0, line.to_string())];
    }
    if line.is_empty() {
        return vec![(0, String::new())];
    }

    let chars: Vec<char> = line.chars().collect();
    let mut result = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        let end = (start + width).min(chars.len());
        result.push((start, chars[start..end].iter().collect()));
        start = end;
    }
    result
}

fn normalize_points(
    first: PanelTextPoint,
    second: PanelTextPoint,
) -> (PanelTextPoint, PanelTextPoint) {
    if first <= second {
        (first, second)
    } else {
        (second, first)
    }
}

fn max_text_point(first: PanelTextPoint, second: PanelTextPoint) -> PanelTextPoint {
    if first >= second {
        first
    } else {
        second
    }
}

fn min_text_point(first: PanelTextPoint, second: PanelTextPoint) -> PanelTextPoint {
    if first <= second {
        first
    } else {
        second
    }
}

fn slice_chars(text: &str, start: usize, end: usize) -> String {
    if start >= end {
        return String::new();
    }

    text.chars().skip(start).take(end - start).collect()
}

impl Msg {
    fn plain(kind: MsgKind, text: String) -> Self {
        Self {
            kind,
            text,
            id: None,
            parent_id: None,
            title: None,
            state: None,
            workflow_id: None,
            workflow_role: None,
            scene_id: None,
            collapsed: false,
        }
    }

    fn from_response_section(section: ResponseSection) -> Self {
        Self {
            kind: match section.kind {
                ResponseSectionKind::Routing => MsgKind::Routing,
                ResponseSectionKind::Step => MsgKind::Step,
                ResponseSectionKind::FinalAnswer => MsgKind::FinalAnswer,
                ResponseSectionKind::Thinking => MsgKind::Thinking,
            },
            text: String::new(),
            id: Some(section.id),
            parent_id: section.parent_id,
            title: Some(section.title),
            state: Some(section.state),
            workflow_id: Some(section.metadata.workflow_id),
            workflow_role: Some(section.metadata.workflow_role),
            scene_id: section.metadata.scene_id,
            collapsed: false,
        }
    }
}

fn split_or_empty(text: &str) -> Vec<String> {
    if text.is_empty() {
        vec![String::new()]
    } else {
        text.lines().map(ToOwned::to_owned).collect()
    }
}

fn sanitize_tool_run(mut tool_run: ToolRun) -> ToolRun {
    tool_run.invocation_preview = strip_ansi(&tool_run.invocation_preview);
    tool_run.result_preview = tool_run.result_preview.map(|text| strip_ansi(&text));
    tool_run.detail.title = strip_ansi(&tool_run.detail.title);
    tool_run.detail.lines = tool_run
        .detail
        .lines
        .into_iter()
        .map(|line| strip_ansi(&line))
        .collect();
    tool_run
}

fn sanitize_step_diagnostics(mut diagnostics: StepDiagnostics) -> StepDiagnostics {
    diagnostics.input.structured_input_preview = diagnostics
        .input
        .structured_input_preview
        .map(|text| strip_ansi(&text));
    diagnostics.input.todo_state_preview = diagnostics
        .input
        .todo_state_preview
        .map(|text| strip_ansi(&text));
    diagnostics.input.error = diagnostics.input.error.map(|text| strip_ansi(&text));
    diagnostics.output.extracted_preview = diagnostics
        .output
        .extracted_preview
        .map(|text| strip_ansi(&text));
    diagnostics.output.error = diagnostics.output.error.map(|text| strip_ansi(&text));
    diagnostics.session_writes = diagnostics
        .session_writes
        .into_iter()
        .map(|write| StepContextWrite {
            path: strip_ansi(&write.path),
            kind: write.kind,
            before_preview: write.before_preview.map(|text| strip_ansi(&text)),
            after_preview: write.after_preview.map(|text| strip_ansi(&text)),
        })
        .collect();
    diagnostics
}

fn build_diagnostics_lines(diagnostics: &StepDiagnostics) -> Vec<DiagnosticsLine> {
    let header = format!(
        "{}:{} {}/{} {}",
        diagnostics.workflow_role.as_str(),
        diagnostics.workflow_id,
        diagnostics.index,
        diagnostics.total,
        diagnostics.step_label
    );
    let input = format!(
        "  input {} · summaries={} · structured={}{}",
        diagnostics_input_status_label(diagnostics.input.status),
        diagnostics.input.summary_sources.len(),
        diagnostics.input.resolved_structured_sources.len(),
        if diagnostics.input.todo_state_preview.is_some() {
            " · todo"
        } else {
            ""
        }
    );
    let output = format!(
        "  output {} · retries={}/{} · writes={}",
        diagnostics_output_status_label(diagnostics.output.status),
        diagnostics.output.retry_count,
        diagnostics.output.max_retries,
        diagnostics.session_writes.len()
    );
    let mut lines = vec![DiagnosticsLine {
        text: header,
        diagnostic_id: Some(diagnostics.id.clone()),
    }, DiagnosticsLine {
        text: input,
        diagnostic_id: Some(diagnostics.id.clone()),
    }, DiagnosticsLine {
        text: output,
        diagnostic_id: Some(diagnostics.id.clone()),
    }];
    if let Some(error) = diagnostics
        .input
        .error
        .as_deref()
        .or(diagnostics.output.error.as_deref())
    {
        lines.push(DiagnosticsLine {
            text: format!("  error {}", truncate_preview(error, 96)),
            diagnostic_id: Some(diagnostics.id.clone()),
        });
    }
    lines
}

fn build_step_diagnostics_detail_lines(diagnostics: &StepDiagnostics) -> Vec<String> {
    let mut lines = vec![
        format!(
            "step: {}:{} {} {}/{}",
            diagnostics.workflow_role.as_str(),
            diagnostics.workflow_id,
            diagnostics.step_label,
            diagnostics.index,
            diagnostics.total
        ),
        format!("step_id: {}", diagnostics.step_id),
        format!(
            "input: {}",
            diagnostics_input_status_label(diagnostics.input.status)
        ),
    ];

    if diagnostics.input.summary_sources.is_empty() {
        lines.push("summary_sources: none".to_string());
    } else {
        lines.push(format!(
            "summary_sources: {}",
            diagnostics
                .input
                .summary_sources
                .iter()
                .map(|source| format!("{}:{} ({})", source.workflow_id, source.step_id, source.title))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    if diagnostics.input.expected_structured_sources.is_empty() {
        lines.push("structured_sources: none".to_string());
    } else {
        lines.push(format!(
            "structured_expected: {}",
            diagnostics.input.expected_structured_sources.join(", ")
        ));
        lines.push(format!(
            "structured_resolved: {}",
            if diagnostics.input.resolved_structured_sources.is_empty() {
                "none".to_string()
            } else {
                diagnostics.input.resolved_structured_sources.join(", ")
            }
        ));
        if !diagnostics.input.missing_structured_sources.is_empty() {
            lines.push(format!(
                "structured_missing: {}",
                diagnostics.input.missing_structured_sources.join(", ")
            ));
        }
    }

    if let Some(preview) = diagnostics.input.structured_input_preview.as_deref() {
        lines.push("structured_input_preview:".to_string());
        lines.extend(preview.lines().map(|line| format!("  {line}")));
    }
    if let Some(preview) = diagnostics.input.todo_state_preview.as_deref() {
        lines.push("todo_state_preview:".to_string());
        lines.extend(preview.lines().map(|line| format!("  {line}")));
    }
    if let Some(error) = diagnostics.input.error.as_deref() {
        lines.push(format!("input_error: {error}"));
    }

    lines.push(format!(
        "output: {}",
        diagnostics_output_status_label(diagnostics.output.status)
    ));
    lines.push(format!(
        "output_contract: {}{}{}",
        diagnostics_output_contract_label(diagnostics.output.status, diagnostics.output.format.as_deref()),
        diagnostics
            .output
            .schema_path
            .as_deref()
            .map(|path| format!(" · schema={path}"))
            .unwrap_or_default(),
        if diagnostics.output.max_retries > 0 {
            format!(
                " · attempts={} · retries={}/{}",
                diagnostics.output.attempts,
                diagnostics.output.retry_count,
                diagnostics.output.max_retries
            )
        } else if diagnostics.output.attempts > 0 {
            format!(" · attempts={}", diagnostics.output.attempts)
        } else {
            String::new()
        }
    ));
    if let Some(preview) = diagnostics.output.extracted_preview.as_deref() {
        lines.push("structured_output_preview:".to_string());
        lines.extend(preview.lines().map(|line| format!("  {line}")));
    }
    if let Some(error) = diagnostics.output.error.as_deref() {
        lines.push(format!("output_error: {error}"));
    }

    if diagnostics.session_writes.is_empty() {
        lines.push("session_writes: none".to_string());
    } else {
        lines.push("session_writes:".to_string());
        for write in &diagnostics.session_writes {
            lines.push(format!(
                "  {} ({})",
                write.path,
                diagnostics_write_kind_label(write.kind)
            ));
            if let Some(preview) = write.before_preview.as_deref() {
                lines.push(format!("    before {}", truncate_preview(preview, 140)));
            }
            if let Some(preview) = write.after_preview.as_deref() {
                lines.push(format!("    after  {}", truncate_preview(preview, 140)));
            }
        }
    }

    lines
}

fn diagnostics_input_status_label(status: StepInputStatus) -> &'static str {
    match status {
        StepInputStatus::None => "none",
        StepInputStatus::Ready => "ready",
        StepInputStatus::OptionalEmpty => "optional-empty",
        StepInputStatus::MissingRequired => "missing-required",
    }
}

fn diagnostics_output_status_label(status: StepOutputStatus) -> &'static str {
    match status {
        StepOutputStatus::None => "none",
        StepOutputStatus::Pending => "pending",
        StepOutputStatus::Valid => "valid",
        StepOutputStatus::Invalid => "invalid",
        StepOutputStatus::Skipped => "skipped",
    }
}

fn diagnostics_output_contract_label(status: StepOutputStatus, format: Option<&str>) -> String {
    match format {
        Some(format) => format!("format={format} · status={}", diagnostics_output_status_label(status)),
        None => format!("status={}", diagnostics_output_status_label(status)),
    }
}

fn diagnostics_write_kind_label(kind: StepContextWriteKind) -> &'static str {
    match kind {
        StepContextWriteKind::Added => "added",
        StepContextWriteKind::Updated => "updated",
        StepContextWriteKind::Cleared => "cleared",
    }
}

fn format_tool_lane_header(tool_runs: &[&ToolRun]) -> String {
    let running = tool_runs
        .iter()
        .filter(|tool_run| tool_run.status == ToolRunStatus::Running)
        .count();
    let failed = tool_runs
        .iter()
        .filter(|tool_run| tool_run.status == ToolRunStatus::Failed)
        .count();
    let total = tool_runs.len();

    if running > 0 {
        format!("  tools  {total} total · {running} running")
    } else if failed > 0 {
        format!("  tools  {total} total · {failed} failed")
    } else {
        format!("  tools  {total} total")
    }
}

fn format_tool_summary(tool_run: &ToolRun) -> String {
    let mut summary = format!(
        "    {}  [{}]  {}",
        tool_run.tool_name,
        tool_run_status_label(tool_run.status),
        tool_run.invocation_preview
    );
    if let Some(result_preview) = tool_run.result_preview.as_deref() {
        summary.push_str(" -> ");
        summary.push_str(result_preview);
    }
    summary
}

fn tool_run_status_label(status: ToolRunStatus) -> &'static str {
    match status {
        ToolRunStatus::Running => "running",
        ToolRunStatus::Complete => "done",
        ToolRunStatus::Failed => "failed",
    }
}

fn first_non_empty_line(text: &str) -> Option<&str> {
    text.lines().find(|line| !line.trim().is_empty())
}

fn format_response_header(message: &Msg) -> String {
    let state = message.state.unwrap_or(ResponseSectionState::Complete);
    let badge = match message.kind {
        MsgKind::Routing => "route",
        MsgKind::Step => "step",
        MsgKind::FinalAnswer => "final",
        MsgKind::Thinking => "  reasoning",
        _ => "msg",
    };
    let workflow_role = message
        .workflow_role
        .map(WorkflowRunRole::as_str)
        .unwrap_or("unknown");
    let workflow_id = message.workflow_id.as_deref().unwrap_or("workflow");
    let title = match message.kind {
        MsgKind::Thinking => thinking_header_title(state),
        _ => message.title.as_deref().unwrap_or("Section"),
    };
    let state = match state {
        ResponseSectionState::Streaming => "streaming",
        ResponseSectionState::Complete => "done",
        ResponseSectionState::Failed => "failed",
    };

    format!("{badge}  {workflow_role}:{workflow_id}  {title}  [{state}]")
}

fn summarize_thinking_text(text: &str, state: ResponseSectionState) -> String {
    let preview = first_non_empty_line(text)
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| truncate_preview(line, 56))
        .unwrap_or_else(|| thinking_placeholder_text(state).to_string());
    let line_count = text.lines().filter(|line| !line.trim().is_empty()).count();
    let label = thinking_summary_label(state);

    if line_count == 0 {
        format!("{label} · {preview}")
    } else if line_count == 1 {
        format!("{label} · 1 line · {preview}")
    } else {
        format!("{label} · {line_count} lines · {preview}")
    }
}

fn thinking_header_title(state: ResponseSectionState) -> &'static str {
    match state {
        ResponseSectionState::Streaming => "Reasoning live",
        ResponseSectionState::Complete => "Reasoning",
        ResponseSectionState::Failed => "Reasoning failed",
    }
}

fn thinking_summary_label(state: ResponseSectionState) -> &'static str {
    match state {
        ResponseSectionState::Streaming => "reasoning live",
        ResponseSectionState::Complete => "reasoning",
        ResponseSectionState::Failed => "reasoning failed",
    }
}

fn thinking_placeholder_text(state: ResponseSectionState) -> &'static str {
    match state {
        ResponseSectionState::Streaming => "waiting for reasoning...",
        ResponseSectionState::Complete => "no reasoning captured",
        ResponseSectionState::Failed => "reasoning ended before content arrived",
    }
}

fn truncate_preview(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let preview: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{preview}...")
    } else {
        preview
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use omega_session::{
        ActivityTarget, OverlayRequest, ResponseSectionDelta, ResponseSectionMetadata,
        RuntimeUiEffect, RuntimeUiMessage, StepDiagnostics, StepInputDiagnostics,
        StepInputStatus, StepOutputContractMode, StepOutputDiagnostics, StepOutputStatus,
        StepSummarySource, ToolRunDetail, UiContent, UiMessageKind, UiSource, UiTarget,
        WorkflowRunRole,
    };
    use ratatui::layout::Rect;

    fn sample_step_diagnostics() -> StepDiagnostics {
        StepDiagnostics {
            id: "child:feature:plan".to_string(),
            workflow_id: "feature".to_string(),
            workflow_role: WorkflowRunRole::Child,
            step_id: "plan".to_string(),
            step_label: "Plan".to_string(),
            index: 2,
            total: 4,
            input: StepInputDiagnostics {
                status: StepInputStatus::Ready,
                summary_sources: vec![StepSummarySource {
                    workflow_id: "feature".to_string(),
                    step_id: "analysis".to_string(),
                    title: "Analysis".to_string(),
                }],
                expected_structured_sources: vec!["analysis".to_string()],
                resolved_structured_sources: vec!["analysis".to_string()],
                missing_structured_sources: vec![],
                structured_input_preview: Some("{\"analysis\":{\"objective\":\"Ship\"}}".to_string()),
                todo_state_preview: None,
                error: None,
            },
            output: StepOutputDiagnostics {
                contract_mode: StepOutputContractMode::Required,
                format: Some("json".to_string()),
                schema_path: Some(".omega/schema/step/plan.json".to_string()),
                status: StepOutputStatus::Valid,
                extracted_preview: Some("{\"tasks\":[{\"id\":\"task-1\"}]}".to_string()),
                attempts: 2,
                retry_count: 1,
                max_retries: 2,
                error: Some("missing validation_targets".to_string()),
            },
            session_writes: vec![StepContextWrite {
                path: "step_outputs.plan".to_string(),
                kind: StepContextWriteKind::Added,
                before_preview: None,
                after_preview: Some("{\"tasks\":[{\"id\":\"task-1\"}]}".to_string()),
            }],
        }
    }

    #[test]
    fn input_editing_uses_character_indices() {
        let mut app = App::new();
        app.insert_char('你');
        app.insert_char('好');
        app.move_cursor_left();
        app.insert_char('们');

        assert_eq!(app.input_buffer, "你们好");
        assert_eq!(app.cursor_pos, 2);
    }

    #[test]
    fn add_log_strips_ansi_sequences() {
        let mut app = App::new();
        app.add_log("\u{1b}[32mhello\u{1b}[0m".to_string());

        assert_eq!(app.log_lines, vec!["hello"]);
    }

    #[test]
    fn upserting_step_diagnostics_builds_sidebar_lines() {
        let mut app = App::new();

        app.upsert_step_diagnostics(sample_step_diagnostics());

        assert_eq!(app.step_diagnostics.len(), 1);
        assert!(!app.diagnostics_lines.is_empty());
        assert!(app
            .diagnostics_lines
            .iter()
            .any(|line| line.text.contains("child:feature 2/4 Plan")));
        assert_eq!(app.rail_badge(SidebarSection::Diagnostics), "D 1");
    }

    #[test]
    fn activating_diagnostics_item_opens_detail_overlay() {
        let mut app = App::new();
        app.upsert_step_diagnostics(sample_step_diagnostics());
        app.focused_panel = Panel::Diagnostics;
        app.diagnostics_rect = Rect::new(0, 0, 80, 8);
        app.diagnostics_state.select(Some(0));

        let opened = app.activate_selected_diagnostics_item();

        assert_eq!(opened, Some("Plan".to_string()));
        match app.overlay.as_ref() {
            Some(OverlayState::Detail(detail)) => {
                assert!(detail.title.contains("Plan"));
                assert!(detail.lines.iter().any(|line| line.contains("step_outputs.plan")));
                assert!(detail.lines.iter().any(|line| line.contains("(added)")));
                assert!(detail.lines.iter().any(|line| line.contains("after  {\"tasks\"")));
            }
            other => panic!("expected detail overlay, got {other:?}"),
        }
    }

    #[test]
    fn todo_snapshot_replaces_lines() {
        let mut app = App::new();
        app.set_todo_snapshot(1, "[ ] #1: Plan\n[>] #2: Code\n\n(0/2 completed)");

        assert_eq!(app.todo_lines, vec!["[ ] #1: Plan", "[>] #2: Code"]);
        assert_eq!(
            app.todo_summary,
            Some(TodoSummary {
                completed: 0,
                total: 2
            })
        );
    }

    #[test]
    fn empty_todo_snapshot_uses_actionable_copy() {
        let mut app = App::new();
        app.set_todo_snapshot(2, "No todos.");

        assert_eq!(app.todo_status, TodoPanelStatus::Empty);
        assert_eq!(app.todo_lines, todo_empty_lines());
        assert_eq!(
            app.todo_summary,
            Some(TodoSummary {
                completed: 0,
                total: 0
            })
        );
    }

    #[test]
    fn running_turn_marks_todo_as_stale_until_snapshot_arrives() {
        let mut app = App::new();
        let first_turn = app.begin_turn();
        app.set_todo_snapshot(first_turn, "[>] #1: Code\n\n(0/1 completed)");
        app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
            first_turn,
            RuntimeUiEffect::SetStatusSlot {
                slot: StatusSlot::Agent,
                value: StatusValue::Label("Idle".to_string()),
            },
        ));

        app.begin_turn();

        assert!(app.todo_refresh_pending());
        assert!(app.todo_panel_title().contains("stale"));
    }

    #[test]
    fn current_turn_workflow_updates_replace_summary_and_clear_on_finish() {
        let mut app = App::new();
        let turn_id = app.begin_turn();

        app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::SetStatusSlot {
                slot: StatusSlot::Workflow,
                value: StatusValue::WorkflowStep {
                    workflow_id: "feature".to_string(),
                    workflow_role: WorkflowRunRole::Child,
                    step_id: "plan".to_string(),
                    step_label: "Plan".to_string(),
                    index: 2,
                    total: 4,
                },
            },
        ));
        app.apply_runtime_envelope(RuntimeUiEnvelope::message(
            turn_id,
            RuntimeUiMessage {
                target: UiTarget::Activity(ActivityTarget::Log),
                source: UiSource::WorkflowStep {
                    workflow_id: "feature".to_string(),
                    workflow_role: WorkflowRunRole::Child,
                    step_id: "plan".to_string(),
                    step_label: "Plan".to_string(),
                    index: 2,
                    total: 4,
                },
                kind: UiMessageKind::Summary,
                content: UiContent::Text("Plan".to_string()),
                priority: None,
            },
        ));

        assert_eq!(
            app.workflow_summary,
            Some(WorkflowSummary {
                workflow_id: "feature".to_string(),
                workflow_role: WorkflowRunRole::Child,
                id: "plan".to_string(),
                label: "Plan".to_string(),
                index: 2,
                total: 4,
            })
        );
        assert_eq!(app.log_lines, vec!["[child:feature 2/4] Plan (plan)"]);

        app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::ClearStatusSlot {
                slot: StatusSlot::Workflow,
            },
        ));

        assert!(app.workflow_summary.is_none());
    }

    #[test]
    fn tool_preview_routes_to_logs_instead_of_response() {
        let mut app = App::new();
        let turn_id = app.begin_turn();

        app.apply_runtime_envelope(RuntimeUiEnvelope::message(
            turn_id,
            RuntimeUiMessage {
                target: UiTarget::Activity(ActivityTarget::Log),
                source: UiSource::Tool {
                    tool_name: "bash".to_string(),
                },
                kind: UiMessageKind::Log,
                content: UiContent::Text("$ echo hi".to_string()),
                priority: None,
            },
        ));
        app.apply_runtime_envelope(RuntimeUiEnvelope::message(
            turn_id,
            RuntimeUiMessage {
                target: UiTarget::Activity(ActivityTarget::Log),
                source: UiSource::Tool {
                    tool_name: "bash".to_string(),
                },
                kind: UiMessageKind::Log,
                content: UiContent::Text("hi".to_string()),
                priority: None,
            },
        ));

        assert!(app.output_msgs.is_empty());
        assert_eq!(app.log_lines, vec!["[tool] $ echo hi", "[tool] hi"]);
    }

    #[test]
    fn step_text_routes_to_response_with_step_label() {
        let mut app = App::new();
        let turn_id = app.begin_turn();

        app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::BeginResponseSection {
                section: ResponseSection {
                    id: "turn-1:child:feature:plan".to_string(),
                    parent_id: None,
                    kind: ResponseSectionKind::Step,
                    title: "Plan".to_string(),
                    state: ResponseSectionState::Streaming,
                    metadata: ResponseSectionMetadata {
                        scene_id: Some("feature".to_string()),
                        workflow_id: "feature".to_string(),
                        workflow_role: WorkflowRunRole::Child,
                        step_id: Some("plan".to_string()),
                        step_label: Some("Plan".to_string()),
                    },
                },
            },
        ));
        app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::AppendResponseSection {
                id: "turn-1:child:feature:plan".to_string(),
                delta: ResponseSectionDelta::Text("Line one\nLine two".to_string()),
            },
        ));
        app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::CompleteResponseSection {
                id: "turn-1:child:feature:plan".to_string(),
                state: ResponseSectionState::Complete,
            },
        ));

        assert_eq!(
            app.response_lines(),
            vec![
                "step  child:feature  Plan  [done]".to_string(),
                "  scene feature".to_string(),
                "  Line one".to_string(),
                "  Line two".to_string(),
            ]
        );
        assert!(app.log_lines.is_empty());
    }

    #[test]
    fn routing_and_final_answer_sections_form_response_timeline() {
        let mut app = App::new();
        let turn_id = app.begin_turn();

        app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::BeginResponseSection {
                section: ResponseSection {
                    id: "turn-7:root:root:scene-recognition".to_string(),
                    parent_id: None,
                    kind: ResponseSectionKind::Routing,
                    title: "Scene Recognition".to_string(),
                    state: ResponseSectionState::Streaming,
                    metadata: ResponseSectionMetadata {
                        scene_id: None,
                        workflow_id: "root".to_string(),
                        workflow_role: WorkflowRunRole::Root,
                        step_id: Some("scene-recognition".to_string()),
                        step_label: Some("Scene Recognition".to_string()),
                    },
                },
            },
        ));
        app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::AppendResponseSection {
                id: "turn-7:root:root:scene-recognition".to_string(),
                delta: ResponseSectionDelta::Text("chat".to_string()),
            },
        ));
        app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::CompleteResponseSection {
                id: "turn-7:root:root:scene-recognition".to_string(),
                state: ResponseSectionState::Complete,
            },
        ));
        app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::BeginResponseSection {
                section: ResponseSection {
                    id: "turn-7:child:chat:chat".to_string(),
                    parent_id: None,
                    kind: ResponseSectionKind::FinalAnswer,
                    title: "Final Answer".to_string(),
                    state: ResponseSectionState::Streaming,
                    metadata: ResponseSectionMetadata {
                        scene_id: Some("chat".to_string()),
                        workflow_id: "chat".to_string(),
                        workflow_role: WorkflowRunRole::Child,
                        step_id: Some("chat".to_string()),
                        step_label: Some("Chat".to_string()),
                    },
                },
            },
        ));
        app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::AppendResponseSection {
                id: "turn-7:child:chat:chat".to_string(),
                delta: ResponseSectionDelta::Text("hello".to_string()),
            },
        ));
        app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::CompleteResponseSection {
                id: "turn-7:child:chat:chat".to_string(),
                state: ResponseSectionState::Complete,
            },
        ));

        assert_eq!(
            app.response_lines(),
            vec![
                "route  root:root  Scene Recognition  [done]".to_string(),
                "  result chat".to_string(),
                "final  child:chat  Final Answer  [done]".to_string(),
                "  scene chat".to_string(),
                "  hello".to_string(),
            ]
        );
    }

    #[test]
    fn thinking_sections_stream_then_collapse_on_complete() {
        let mut app = App::new();
        let turn_id = app.begin_turn();

        app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::BeginResponseSection {
                section: ResponseSection {
                    id: "turn-9:child:chat:chat".to_string(),
                    parent_id: None,
                    kind: ResponseSectionKind::FinalAnswer,
                    title: "Final Answer".to_string(),
                    state: ResponseSectionState::Streaming,
                    metadata: ResponseSectionMetadata {
                        scene_id: Some("chat".to_string()),
                        workflow_id: "chat".to_string(),
                        workflow_role: WorkflowRunRole::Child,
                        step_id: Some("chat".to_string()),
                        step_label: Some("Chat".to_string()),
                    },
                },
            },
        ));
        app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::BeginResponseSection {
                section: ResponseSection {
                    id: "turn-9:child:chat:chat:thinking".to_string(),
                    parent_id: Some("turn-9:child:chat:chat".to_string()),
                    kind: ResponseSectionKind::Thinking,
                    title: "Thinking".to_string(),
                    state: ResponseSectionState::Streaming,
                    metadata: ResponseSectionMetadata {
                        scene_id: Some("chat".to_string()),
                        workflow_id: "chat".to_string(),
                        workflow_role: WorkflowRunRole::Child,
                        step_id: Some("chat".to_string()),
                        step_label: Some("Chat".to_string()),
                    },
                },
            },
        ));
        app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::AppendResponseSection {
                id: "turn-9:child:chat:chat:thinking".to_string(),
                delta: ResponseSectionDelta::Text("outline answer\ncheck tone".to_string()),
            },
        ));

        assert_eq!(
            app.response_lines(),
            vec![
                "final  child:chat  Final Answer  [streaming]".to_string(),
                "  scene chat".to_string(),
                "  …".to_string(),
                "  reasoning  child:chat  Reasoning live  [streaming]".to_string(),
                "    | outline answer".to_string(),
                "    | check tone".to_string(),
            ]
        );

        app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::CompleteResponseSection {
                id: "turn-9:child:chat:chat:thinking".to_string(),
                state: ResponseSectionState::Complete,
            },
        ));

        assert_eq!(
            app.response_lines(),
            vec![
                "final  child:chat  Final Answer  [streaming]".to_string(),
                "  scene chat".to_string(),
                "  …".to_string(),
                "  reasoning  child:chat  Reasoning  [done]".to_string(),
                "    = reasoning · 2 lines · outline answer".to_string(),
            ]
        );

        let thinking_index = app
            .response_display_lines()
            .iter()
            .position(|line| line.text == "  reasoning  child:chat  Reasoning  [done]")
            .unwrap();
        app.response_state.select(Some(thinking_index));

        assert_eq!(app.toggle_selected_thinking_section(), Some(false));
        assert_eq!(
            app.response_lines(),
            vec![
                "final  child:chat  Final Answer  [streaming]".to_string(),
                "  scene chat".to_string(),
                "  …".to_string(),
                "  reasoning  child:chat  Reasoning  [done]".to_string(),
                "    | outline answer".to_string(),
                "    | check tone".to_string(),
            ]
        );
    }

    #[test]
    fn failed_thinking_sections_surface_failure_summary() {
        let mut app = App::new();
        let turn_id = app.begin_turn();

        app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::BeginResponseSection {
                section: ResponseSection {
                    id: "turn-10:child:chat:chat:thinking".to_string(),
                    parent_id: Some("turn-10:child:chat:chat".to_string()),
                    kind: ResponseSectionKind::Thinking,
                    title: "Thinking".to_string(),
                    state: ResponseSectionState::Streaming,
                    metadata: ResponseSectionMetadata {
                        scene_id: Some("chat".to_string()),
                        workflow_id: "chat".to_string(),
                        workflow_role: WorkflowRunRole::Child,
                        step_id: Some("chat".to_string()),
                        step_label: Some("Chat".to_string()),
                    },
                },
            },
        ));
        app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::AppendResponseSection {
                id: "turn-10:child:chat:chat:thinking".to_string(),
                delta: ResponseSectionDelta::Text("tool result mismatched".to_string()),
            },
        ));
        app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::CompleteResponseSection {
                id: "turn-10:child:chat:chat:thinking".to_string(),
                state: ResponseSectionState::Failed,
            },
        ));

        assert_eq!(
            app.response_lines(),
            vec![
                "  reasoning  child:chat  Reasoning failed  [failed]".to_string(),
                "    = reasoning failed · 1 line · tool result mismatched".to_string(),
            ]
        );
    }

    #[test]
    fn thinking_sections_can_be_hidden_by_config() {
        let mut app = App::new();
        app.set_show_thinking(false);
        let turn_id = app.begin_turn();

        app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::BeginResponseSection {
                section: ResponseSection {
                    id: "turn-12:child:chat:chat:thinking".to_string(),
                    parent_id: Some("turn-12:child:chat:chat".to_string()),
                    kind: ResponseSectionKind::Thinking,
                    title: "Thinking".to_string(),
                    state: ResponseSectionState::Streaming,
                    metadata: ResponseSectionMetadata {
                        scene_id: Some("chat".to_string()),
                        workflow_id: "chat".to_string(),
                        workflow_role: WorkflowRunRole::Child,
                        step_id: Some("chat".to_string()),
                        step_label: Some("Chat".to_string()),
                    },
                },
            },
        ));
        app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::AppendResponseSection {
                id: "turn-12:child:chat:chat:thinking".to_string(),
                delta: ResponseSectionDelta::Text("hidden reasoning".to_string()),
            },
        ));

        assert!(app.response_lines().is_empty());
    }

    #[test]
    fn tool_run_effects_render_inside_step_block() {
        let mut app = App::new();
        let turn_id = app.begin_turn();

        app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::BeginResponseSection {
                section: ResponseSection {
                    id: "turn-12:child:feature:execute".to_string(),
                    parent_id: None,
                    kind: ResponseSectionKind::Step,
                    title: "Execute".to_string(),
                    state: ResponseSectionState::Streaming,
                    metadata: ResponseSectionMetadata {
                        scene_id: Some("feature".to_string()),
                        workflow_id: "feature".to_string(),
                        workflow_role: WorkflowRunRole::Child,
                        step_id: Some("execute".to_string()),
                        step_label: Some("Execute".to_string()),
                    },
                },
            },
        ));
        app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::BeginToolRun {
                tool_run: ToolRun {
                    id: "tool-1".to_string(),
                    parent_section_id: "turn-12:child:feature:execute".to_string(),
                    tool_name: "bash".to_string(),
                    status: ToolRunStatus::Running,
                    invocation_preview: "$ echo hi".to_string(),
                    result_preview: None,
                    detail: ToolRunDetail {
                        title: " Tool: bash ".to_string(),
                        lines: vec!["tool: bash".to_string(), "invoke: $ echo hi".to_string()],
                    },
                },
            },
        ));
        app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::UpdateToolRun {
                tool_run: ToolRun {
                    id: "tool-1".to_string(),
                    parent_section_id: "turn-12:child:feature:execute".to_string(),
                    tool_name: "bash".to_string(),
                    status: ToolRunStatus::Complete,
                    invocation_preview: "$ echo hi".to_string(),
                    result_preview: Some("hi".to_string()),
                    detail: ToolRunDetail {
                        title: " Tool: bash ".to_string(),
                        lines: vec![
                            "tool: bash".to_string(),
                            "invoke: $ echo hi".to_string(),
                            "result:".to_string(),
                            "hi".to_string(),
                        ],
                    },
                },
            },
        ));
        app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::CompleteToolRun {
                id: "tool-1".to_string(),
                status: ToolRunStatus::Complete,
            },
        ));

        assert_eq!(
            app.response_lines(),
            vec![
                "step  child:feature  Execute  [streaming]".to_string(),
                "  scene feature".to_string(),
                "  tools  1 total".to_string(),
                "    bash  [done]  $ echo hi -> hi".to_string(),
            ]
        );
    }

    #[test]
    fn activating_tool_summary_opens_detail_overlay() {
        let mut app = App::new();
        let turn_id = app.begin_turn();

        app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::BeginResponseSection {
                section: ResponseSection {
                    id: "turn-13:child:feature:execute".to_string(),
                    parent_id: None,
                    kind: ResponseSectionKind::Step,
                    title: "Execute".to_string(),
                    state: ResponseSectionState::Streaming,
                    metadata: ResponseSectionMetadata {
                        scene_id: Some("feature".to_string()),
                        workflow_id: "feature".to_string(),
                        workflow_role: WorkflowRunRole::Child,
                        step_id: Some("execute".to_string()),
                        step_label: Some("Execute".to_string()),
                    },
                },
            },
        ));
        app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::BeginToolRun {
                tool_run: ToolRun {
                    id: "tool-2".to_string(),
                    parent_section_id: "turn-13:child:feature:execute".to_string(),
                    tool_name: "read_file".to_string(),
                    status: ToolRunStatus::Complete,
                    invocation_preview: "src/main.rs".to_string(),
                    result_preview: Some("12 lines".to_string()),
                    detail: ToolRunDetail {
                        title: " Tool: read_file ".to_string(),
                        lines: vec![
                            "tool: read_file".to_string(),
                            "invoke: src/main.rs".to_string(),
                            "result:".to_string(),
                            "12 lines".to_string(),
                        ],
                    },
                },
            },
        ));

        let selected_index = app
            .response_display_lines()
            .iter()
            .position(|line| line.text == "    read_file  [done]  src/main.rs -> 12 lines")
            .unwrap();
        app.response_state.select(Some(selected_index));

        assert_eq!(
            app.activate_selected_response_item(),
            Some(ResponseActivation::ToolDetailOpened(
                "read_file".to_string()
            ))
        );

        match app.overlay.as_ref() {
            Some(OverlayState::Detail(detail)) => {
                assert_eq!(detail.title, " Tool: read_file ");
                assert_eq!(
                    detail.lines,
                    vec![
                        "tool: read_file".to_string(),
                        "invoke: src/main.rs".to_string(),
                        "result:".to_string(),
                        "12 lines".to_string(),
                    ]
                );
            }
            other => panic!("expected detail overlay, got {other:?}"),
        }
    }

    #[test]
    fn status_bar_session_target_updates_session_slot() {
        let mut app = App::new();
        let turn_id = app.begin_turn();

        app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::SetStatusSlot {
                slot: StatusSlot::Session,
                value: StatusValue::SessionRouting {
                    root_workflow_id: "root".to_string(),
                    active_workflow_id: "chat".to_string(),
                    active_workflow_role: WorkflowRunRole::Child,
                    recognized_scene_id: Some("chat".to_string()),
                    selected_workflow_id: Some("chat".to_string()),
                },
            },
        ));

        assert_eq!(
            app.session_status,
            Some(SessionStatusSummary::Routing(SessionRoutingSummary {
                root_workflow_id: "root".to_string(),
                active_workflow_id: "chat".to_string(),
                active_workflow_role: WorkflowRunRole::Child,
                recognized_scene_id: Some("chat".to_string()),
                selected_workflow_id: Some("chat".to_string()),
            }))
        );
    }

    #[test]
    fn focus_hint_routes_to_visible_logs_panel() {
        let mut app = App::new();
        let turn_id = app.begin_turn();
        app.logs_rect = Rect::new(60, 10, 20, 8);

        app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::FocusHint {
                target: UiTarget::Activity(ActivityTarget::Log),
            },
        ));

        assert_eq!(app.focused_panel, Panel::Logs);
    }

    #[test]
    fn detail_overlay_target_can_be_shown_and_hidden() {
        let mut app = App::new();
        let turn_id = app.begin_turn();

        app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::ShowOverlay(OverlayRequest {
                target: OverlayTarget::Detail,
                content: UiContent::Text("first\nsecond".to_string()),
            }),
        ));

        match app.overlay.as_ref() {
            Some(OverlayState::Detail(detail)) => {
                assert_eq!(
                    detail.lines,
                    vec!["first".to_string(), "second".to_string()]
                );
            }
            other => panic!("expected detail overlay, got {other:?}"),
        }

        app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::HideOverlay {
                target: OverlayTarget::Detail,
            },
        ));

        assert!(app.overlay.is_none());
    }

    #[test]
    fn panel_hit_testing_distinguishes_todo_and_logs() {
        let mut app = App::new();
        app.todo_rect = Rect::new(60, 1, 20, 8);
        app.logs_rect = Rect::new(60, 9, 20, 10);

        assert_eq!(app.panel_at(10, 5), Panel::Response);
        assert_eq!(app.panel_at(65, 4), Panel::Todo);
        assert_eq!(app.panel_at(65, 12), Panel::Logs);
    }

    #[test]
    fn normalize_focus_returns_to_response_when_sidebar_hides() {
        let mut app = App::new();
        app.focused_panel = Panel::Todo;
        app.todo_rect = Rect::default();
        app.logs_rect = Rect::default();

        app.normalize_focus();

        assert_eq!(app.focused_panel, Panel::Response);
        assert_eq!(app.next_focus_panel(), Panel::Response);
    }

    #[test]
    fn normalize_mode_returns_to_normal_when_input_is_disabled() {
        let mut app = App::new();
        app.interaction_mode = InteractionMode::Insert;
        app.input_enabled = false;

        app.normalize_mode();

        assert_eq!(app.interaction_mode, InteractionMode::Normal);
        assert!(app
            .status_notice
            .as_deref()
            .is_some_and(|notice| notice.contains("Insert mode")));
    }

    #[test]
    fn overlay_close_restores_origin_focus() {
        let mut app = App::new();
        app.focused_panel = Panel::Logs;
        app.logs_rect = Rect::new(60, 10, 20, 8);

        app.open_search_overlay();
        app.focused_panel = Panel::Response;
        app.close_overlay();

        assert_eq!(app.focused_panel, Panel::Logs);
        assert!(!app.overlay_active());
    }

    #[test]
    fn search_match_count_uses_overlay_target_panel() {
        let mut app = App::new();
        app.log_lines = vec!["alpha beta".to_string(), "alpha".to_string()];
        app.focused_panel = Panel::Logs;
        app.open_search_overlay();

        if let Some(OverlayState::Search(overlay)) = app.overlay.as_mut() {
            overlay.query = "alpha".to_string();
            overlay.cursor_pos = 5;
        }

        assert_eq!(app.panel_search_match_count(), Some((Panel::Logs, 2)));
    }

    #[test]
    fn wrapped_selection_copies_without_soft_newlines() {
        let mut app = App::new();
        app.logs_rect = Rect::new(0, 0, 7, 5);
        app.log_lines = vec!["abcdefg".to_string()];

        assert!(app.begin_mouse_selection(Panel::Logs, 2, 1));
        assert!(app.update_mouse_selection(3, 2));

        assert_eq!(app.selected_text().as_deref(), Some("bcdefg"));
    }

    #[test]
    fn panel_text_point_accounts_for_scroll_offset() {
        let mut app = App::new();
        app.logs_rect = Rect::new(0, 0, 8, 5);
        app.log_lines = vec![
            "first".to_string(),
            "second".to_string(),
            "third".to_string(),
        ];
        *app.logs_state.offset_mut() = 1;

        let point = app.panel_text_point_at(Panel::Logs, 1, 1).unwrap();

        assert_eq!(point.line_index, 1);
        assert_eq!(point.column, 0);
    }
}
