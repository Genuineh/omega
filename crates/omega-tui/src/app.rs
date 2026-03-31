use std::time::{Duration, Instant};

use crossterm::event::KeyEvent;
use omega_keymap::{InteractionMode, KeyFocus};
use ratatui::{layout::Rect, widgets::ListState};

use omega_observability::strip_ansi;
use omega_session::{
    OverlayTarget, ResponseSectionState, RuntimeUiEnvelope, StatusSlot, StatusValue,
    StepDiagnostics, StepOutputStatus, StepSubflowRef, StepSubflowStatus, ToolRun,
    ToolRunStatus, WorkflowRunRole,
};

use crate::overlay::{
    ConfirmChoice, ConfirmIntent, ConfirmOverlay, DetailOverlay, InputPromptOverlay, OverlayState,
    PickerOverlay, SearchOverlay, SearchResultsOverlay,
};
use crate::reducer::{session_status_from_status, workflow_summary_from_status, TuiUpdateReducer};
use crate::sidebar::{SidebarSection, SidebarState};

mod diagnostics;
mod response;
mod text;
mod todo;

pub(crate) use text::wrap_text_segments;
#[cfg(test)]
pub(crate) use todo::todo_empty_lines;
use todo::todo_unsynced_lines;

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
    pub subflow_ref: Option<StepSubflowRef>,
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
    OpenStepSubflowDetail(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResponseActivation {
    ThinkingCollapsed,
    ThinkingExpanded,
    ToolDetailOpened(String),
    StepSubflowDetailOpened(String),
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
    pub step_subflows: Vec<StepSubflowStatus>,
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
            step_subflows: vec![],
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
        self.step_subflows.clear();
        self.clear_step_diagnostics();
        self.active_turn_id
    }

    pub fn interrupt_turn(&mut self) {
        self.active_turn_id = self.active_turn_id.wrapping_add(1);
        self.is_running = false;
        self.workflow_summary = None;
        self.session_status = None;
        self.agent_status_label = Some("Idle".to_string());
        self.step_subflows.clear();
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
            | (OverlayTarget::Search, Some(OverlayState::SearchResults(_)))
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

    pub fn upsert_step_subflow(&mut self, subflow: StepSubflowStatus) {
        if let Some(existing) = self
            .step_subflows
            .iter_mut()
            .find(|existing| existing.workflow_id == subflow.workflow_id
                && existing.step_id == subflow.step_id
                && existing.subflow_id == subflow.subflow_id)
        {
            *existing = subflow;
        } else {
            self.step_subflows.push(subflow);
        }
        self.step_subflows
            .sort_by_key(|entry| (entry.workflow_id.clone(), entry.step_id.clone(), entry.item_index));
    }

    pub fn step_subflow_status_for_ref(
        &self,
        subflow_ref: &StepSubflowRef,
    ) -> Option<&StepSubflowStatus> {
        self.step_subflows.iter().find(|status| {
            status.workflow_id == subflow_ref.parent_workflow_id
                && status.step_id == subflow_ref.parent_step_id
                && status.subflow_id == subflow_ref.subflow_id
        })
    }

    pub fn active_step_subflow(&self) -> Option<&StepSubflowStatus> {
        self.step_subflows
            .iter()
            .find(|status| status.status == omega_session::StepSubflowState::Running)
            .or_else(|| {
                self.step_subflows
                    .iter()
                    .find(|status| status.status == omega_session::StepSubflowState::Failed)
            })
    }

    pub fn highlighted_todo_line_index(&self) -> Option<usize> {
        let active = self.active_step_subflow()?;
        let item_id = active.item_id.as_deref();

        self.todo_lines.iter().position(|line| {
            item_id.is_some_and(|id| line.contains(&format!("#{id}:")))
                || active
                    .item_label
                    .as_deref()
                    .is_some_and(|label| line.contains(label))
        })
    }

    pub fn open_step_subflow_detail(&mut self, id: &str) -> Option<String> {
        let message = self
            .output_msgs
            .iter()
            .find(|message| message.id.as_deref() == Some(id))?;
        let subflow_ref = message.subflow_ref.as_ref()?;
        let subflow = self.step_subflow_status_for_ref(subflow_ref).cloned();
        let mut lines = Vec::new();

        lines.push(format!(
            "subflow: {} ({}/{})",
            subflow_ref.subflow_id, subflow_ref.item_index, subflow_ref.item_total
        ));
        if let Some(item_id) = subflow_ref.item_id.as_deref() {
            lines.push(format!("todo: #{item_id}"));
        }
        if let Some(item_label) = subflow_ref.item_label.as_deref() {
            lines.push(format!("label: {item_label}"));
        }
        if let Some(status) = subflow.as_ref() {
            lines.push(format!("status: {}", status_label(status.status)));
            if status.repeat_count_for_item > 0 {
                lines.push(format!("repeats: {}", status.repeat_count_for_item));
            }
            if status.no_progress_streak_for_item > 0 {
                lines.push(format!("no-progress rounds: {}", status.no_progress_streak_for_item));
            }
            if let Some(source) = status.completion_source.as_deref() {
                lines.push(format!("completion: {source}"));
            }
        }

        let tool_runs = message
            .id
            .as_deref()
            .map(|section_id| {
                self.tool_runs
                    .iter()
                    .filter(|tool_run| tool_run.parent_section_id == section_id)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if !tool_runs.is_empty() {
            lines.push(String::new());
            lines.push(format!("tools: {}", tool_runs.len()));
            for tool_run in tool_runs {
                lines.push(format!(
                    "- {} [{}] {}",
                    tool_run.tool_name,
                    match tool_run.status {
                        ToolRunStatus::Running => "running",
                        ToolRunStatus::Complete => "done",
                        ToolRunStatus::Failed => "failed",
                    },
                    tool_run.invocation_preview,
                ));
            }
        }

        let body_preview = strip_ansi(&message.text);
        if !body_preview.trim().is_empty() {
            lines.push(String::new());
            lines.push("body:".to_string());
            lines.extend(body_preview.lines().take(8).map(ToOwned::to_owned));
        }

        let title = format!(" Subflow: {} ", subflow_ref.subflow_id);
        let label = subflow_ref
            .item_label
            .clone()
            .unwrap_or_else(|| subflow_ref.subflow_id.clone());
        self.open_detail_overlay(title, lines);
        Some(label)
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
            && row
                < self
                    .diagnostics_rect
                    .y
                    .saturating_add(self.diagnostics_rect.height)
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

    pub fn open_search_results_overlay(
        &mut self,
        title: impl Into<String>,
        lines: Vec<String>,
    ) {
        self.overlay = Some(OverlayState::SearchResults(SearchResultsOverlay {
            origin_panel: self.focused_panel,
            title: title.into(),
            lines,
            scroll: 0,
            dismiss_on_backdrop: true,
        }));
        self.clear_leader_pending();
        self.status_notice = Some("Search results overlay opened.".to_string());
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

fn status_label(status: omega_session::StepSubflowState) -> &'static str {
    match status {
        omega_session::StepSubflowState::Queued => "queued",
        omega_session::StepSubflowState::Running => "running",
        omega_session::StepSubflowState::Complete => "done",
        omega_session::StepSubflowState::Failed => "failed",
    }
}

#[cfg(test)]
#[path = "app_tests.rs"]
mod tests;
