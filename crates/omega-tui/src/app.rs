use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use crossterm::event::KeyEvent;
use omega_keymap::{InteractionMode, KeyFocus};
use ratatui::{layout::Rect, widgets::ListState};

use omega_observability::strip_ansi;
use omega_session::{
    ContextSupervisionSnapshot, OperatorPickerRequest, OverlayTarget, ResponseSectionState,
    RuntimeUiEnvelope, SkillLoadSummary, StatusSlot, StatusValue, StepDiagnostics,
    StepKnowledgeSummary, StepOutputStatus, StepSubflowRef, StepSubflowStatus, ToolRun,
    ToolRunStatus, WorkflowRunRole,
};
use omega_theme::RenderPalette;

use crate::overlay::{
    ConfirmChoice, ConfirmIntent, ConfirmOverlay, DetailOverlay, InputPromptOverlay, OverlayState,
    PickerOverlay, SearchOverlay, SearchResultsOverlay,
};
use crate::reducer::{session_status_from_status, workflow_summary_from_status, TuiUpdateReducer};
use crate::sidebar::{SidebarSection, SidebarState};

mod diagnostics;
mod delivery;
mod project;
mod response;
mod skills;
mod supervision;
mod text;
mod todo;

pub(crate) use project::project_badge_text;
pub(crate) use text::wrap_text_segments;
#[cfg(test)]
pub(crate) use todo::todo_empty_lines;
use skills::skill_placeholder_lines;
use delivery::{delivery_placeholder_lines, DeliverySummary};
use supervision::{knowledge_placeholder_lines, memory_placeholder_lines};
use todo::todo_unsynced_lines;

const TODO_UNSYNCED_LINES: &[&str] = &[
    "No todo snapshot yet.",
    "Call the todo tool to track the current task.",
];
const TODO_EMPTY_LINES: &[&str] = &[
    "Todo list is empty.",
    "Call the todo tool when the task splits into steps.",
];
const INPUT_VIEWPORT_PREFIX_WIDTH: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Response,
    SidebarRail,
    Diagnostics,
    Delivery,
    Skills,
    Project,
    Document,
    Memory,
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

#[derive(Debug, Clone)]
pub struct PendingKeySequenceState {
    pub started_at: Instant,
    pub timeout: Duration,
    pub key_events: Vec<KeyEvent>,
    pub replay_text: Option<String>,
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
    Command,
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
    pub tool_lane_collapsed: bool,
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
    /// Multi-span styled fragments. When non-empty, layout uses these instead of
    /// the single-style `text` field. Populated by the Markdown parser.
    pub spans: Vec<crate::render::markdown::StyledSpan>,
}

impl ResponseDisplayLine {
    pub fn plain(kind: MsgKind, text: String) -> Self {
        Self {
            kind,
            text,
            is_header: false,
            message_id: None,
            action: None,
            is_tool_line: false,
            tool_status: None,
            response_state: None,
            thinking_line_kind: None,
            spans: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResponseLineAction {
    ToggleThinkingSection(String),
    ToggleCommandSection(String),
    ToggleToolLane(String),
    OpenToolRunDetail(String),
    OpenStepSubflowDetail(String),
    OpenDeliveryDetail(u64),
    OpenSkillLoadDetail(String),
    OpenDocumentKnowledgeDetail(String),
    OpenMemoryKnowledgeDetail(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResponseActivation {
    ThinkingCollapsed,
    ThinkingExpanded,
    CommandCollapsed,
    CommandExpanded,
    ToolLaneCollapsed,
    ToolLaneExpanded,
    ToolDetailOpened(String),
    StepSubflowDetailOpened(String),
    DeliveryDetailOpened,
    SkillLoadDetailOpened,
    DocumentKnowledgeDetailOpened,
    MemoryKnowledgeDetailOpened,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResponseCardSectionKind {
    Meta,
    ResultsSummary,
    ChangesMade,
    Verification,
    Usage,
    OptionalNextStep,
    KeyPoints,
    RawDetail,
    Delivery,
    SkillLoad,
    Knowledge,
    ToolRuns,
    Thinking,
    Subflow,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResponseCardSection {
    pub kind: ResponseCardSectionKind,
    pub title: Option<String>,
    pub header_line: Option<ResponseDisplayLine>,
    pub lines: Vec<ResponseDisplayLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResponseCard {
    pub id: Option<String>,
    pub kind: MsgKind,
    pub prelude_lines: Vec<ResponseDisplayLine>,
    pub header_line: ResponseDisplayLine,
    pub sections: Vec<ResponseCardSection>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectStatusSummary {
    pub snapshot: omega_project::ProjectDetailSnapshot,
}

pub struct App {
    pub output_msgs: Vec<Msg>,
    pub tool_runs: Vec<ToolRun>,
    pub step_subflows: Vec<StepSubflowStatus>,
    pub delivery_summaries: BTreeMap<u64, DeliverySummary>,
    pub latest_delivery_turn_id: Option<u64>,
    pub skill_load_summaries: BTreeMap<String, SkillLoadSummary>,
    pub latest_skill_load_section_id: Option<String>,
    pub step_knowledge_summaries: BTreeMap<String, StepKnowledgeSummary>,
    step_diagnostics: Vec<StepDiagnostics>,
    context_supervision: Option<ContextSupervisionSnapshot>,
    diagnostics_lines: Vec<DiagnosticsLine>,
    pub delivery_lines: Vec<String>,
    pub skill_lines: Vec<String>,
    pub project_lines: Vec<String>,
    pub document_lines: Vec<String>,
    pub memory_lines: Vec<String>,
    pub todo_lines: Vec<String>,
    pub todo_status: TodoPanelStatus,
    pub todo_summary: Option<TodoSummary>,
    pub log_lines: Vec<String>,
    pub response_state: ListState,
    pub diagnostics_state: ListState,
    pub delivery_state: ListState,
    pub skills_state: ListState,
    pub project_state: ListState,
    pub document_state: ListState,
    pub memory_state: ListState,
    pub todo_state: ListState,
    pub logs_state: ListState,
    pub focused_panel: Panel,
    pub interaction_mode: InteractionMode,
    pub response_pinned: bool,
    pub diagnostics_pinned: bool,
    pub delivery_pinned: bool,
    pub skills_pinned: bool,
    pub project_pinned: bool,
    pub document_pinned: bool,
    pub memory_pinned: bool,
    pub todo_pinned: bool,
    pub logs_pinned: bool,
    pub response_rect: Rect,
    pub input_context_rect: Rect,
    pub input_gap_rect: Rect,
    pub input_rect: Rect,
    pub input_info_rect: Rect,
    pub sidebar_rect: Rect,
    pub sidebar_rail_rect: Rect,
    pub diagnostics_rect: Rect,
    pub delivery_rect: Rect,
    pub skills_rect: Rect,
    pub project_rect: Rect,
    pub document_rect: Rect,
    pub memory_rect: Rect,
    pub todo_rect: Rect,
    pub logs_rect: Rect,
    pub bottom_status_rect: Rect,
    pub input_buffer: String,
    pub cursor_pos: usize,
    pub input_scroll_top: usize,
    pub input_enabled: bool,
    pub show_thinking: bool,
    pub is_running: bool,
    pub active_turn_id: u64,
    pub workflow_summary: Option<WorkflowSummary>,
    pub agent_status_label: Option<String>,
    pub session_status: Option<SessionStatusSummary>,
    pub project_status: Option<ProjectStatusSummary>,
    pub last_todo_turn_id: Option<u64>,
    pub spinner_tick: u8,
    pub response_displayed_count: usize,
    pub diagnostics_displayed_count: usize,
    pub delivery_displayed_count: usize,
    pub skills_displayed_count: usize,
    pub project_displayed_count: usize,
    pub document_displayed_count: usize,
    pub memory_displayed_count: usize,
    pub todo_displayed_count: usize,
    pub logs_displayed_count: usize,
    pub pending_key_sequence: Option<PendingKeySequenceState>,
    pub keymap_source: String,
    pub command_hint: Option<String>,
    pub status_notice: Option<String>,
    pub text_selection: Option<PanelTextSelection>,
    pub mouse_selection_active: bool,
    pub input_cursor_column_goal: Option<usize>,
    pub sidebar: SidebarState,
    pub overlay: Option<OverlayState>,
    pub overlay_rect: Rect,
    pub cached_palette: Option<RenderPalette>,
    pub delivery_model_name: Option<String>,
}

impl App {
    pub fn new() -> Self {
        Self {
            output_msgs: vec![],
            tool_runs: vec![],
            step_subflows: vec![],
            delivery_summaries: BTreeMap::new(),
            latest_delivery_turn_id: None,
            skill_load_summaries: BTreeMap::new(),
            latest_skill_load_section_id: None,
            step_knowledge_summaries: BTreeMap::new(),
            step_diagnostics: vec![],
            context_supervision: None,
            diagnostics_lines: vec![],
            delivery_lines: delivery_placeholder_lines(),
            skill_lines: skill_placeholder_lines(),
            project_lines: project::project_placeholder_lines(),
            document_lines: knowledge_placeholder_lines(),
            memory_lines: memory_placeholder_lines(),
            todo_lines: todo_unsynced_lines(),
            todo_status: TodoPanelStatus::NeverSynced,
            todo_summary: None,
            log_lines: vec![],
            response_state: ListState::default(),
            diagnostics_state: ListState::default(),
            delivery_state: ListState::default(),
            skills_state: ListState::default(),
            project_state: ListState::default(),
            document_state: ListState::default(),
            memory_state: ListState::default(),
            todo_state: ListState::default(),
            logs_state: ListState::default(),
            focused_panel: Panel::Response,
            interaction_mode: InteractionMode::Normal,
            response_pinned: false,
            diagnostics_pinned: false,
            delivery_pinned: false,
            skills_pinned: false,
            project_pinned: false,
            document_pinned: false,
            memory_pinned: false,
            todo_pinned: false,
            logs_pinned: false,
            response_rect: Rect::default(),
            input_context_rect: Rect::default(),
            input_gap_rect: Rect::default(),
            input_rect: Rect::default(),
            input_info_rect: Rect::default(),
            sidebar_rect: Rect::default(),
            sidebar_rail_rect: Rect::default(),
            diagnostics_rect: Rect::default(),
            delivery_rect: Rect::default(),
            skills_rect: Rect::default(),
            project_rect: Rect::default(),
            document_rect: Rect::default(),
            memory_rect: Rect::default(),
            todo_rect: Rect::default(),
            logs_rect: Rect::default(),
            bottom_status_rect: Rect::default(),
            input_buffer: String::new(),
            cursor_pos: 0,
            input_scroll_top: 0,
            input_enabled: true,
            show_thinking: true,
            is_running: false,
            active_turn_id: 0,
            workflow_summary: None,
            agent_status_label: None,
            session_status: None,
            project_status: None,
            last_todo_turn_id: None,
            spinner_tick: 0,
            response_displayed_count: 0,
            diagnostics_displayed_count: 0,
            delivery_displayed_count: 0,
            skills_displayed_count: 0,
            project_displayed_count: 0,
            document_displayed_count: 0,
            memory_displayed_count: 0,
            todo_displayed_count: 0,
            logs_displayed_count: 0,
            pending_key_sequence: None,
            keymap_source: "builtin".to_string(),
            command_hint: None,
            status_notice: None,
            text_selection: None,
            mouse_selection_active: false,
            input_cursor_column_goal: None,
            sidebar: SidebarState::default(),
            overlay: None,
            overlay_rect: Rect::default(),
            cached_palette: None,
            delivery_model_name: None,
        }
    }

    pub fn begin_turn(&mut self) -> u64 {
        self.active_turn_id = self.active_turn_id.wrapping_add(1);
        self.is_running = true;
        self.workflow_summary = None;
        self.session_status = None;
        self.agent_status_label = Some("Running".to_string());
        self.step_subflows.clear();
        self.clear_skill_load_summaries();
        self.step_knowledge_summaries.clear();
        self.clear_step_diagnostics();
        self.clear_context_supervision();
        self.refresh_delivery_panel();
        self.active_turn_id
    }

    pub(crate) fn theme_palette(&self) -> RenderPalette {
        self.cached_palette
            .unwrap_or_else(|| omega_theme::OmegaTheme::dark().render_palette())
    }

    pub fn interrupt_turn(&mut self) {
        self.finalize_current_delivery_summary(true);
        self.fail_streaming_response_sections();
        self.fail_running_tool_runs();
        self.active_turn_id = self.active_turn_id.wrapping_add(1);
        self.is_running = false;
        self.workflow_summary = None;
        self.session_status = None;
        self.agent_status_label = Some("Idle".to_string());
        self.step_subflows.clear();
        self.clear_skill_load_summaries();
        self.step_knowledge_summaries.clear();
        self.clear_step_diagnostics();
        self.clear_context_supervision();
        self.refresh_delivery_panel();
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
                    let was_running = self.is_running;
                    self.is_running = label != "Idle";
                    self.agent_status_label = Some(label);
                    if was_running && !self.is_running {
                        self.finalize_current_delivery_summary(false);
                    }
                }
                StatusValue::Hidden => {
                    self.is_running = false;
                    self.agent_status_label = None;
                }
                StatusValue::SessionRouting { .. } => {}
                StatusValue::WorkflowStep { .. } => {}
                StatusValue::ProjectSelection { .. } => {}
            },
            StatusSlot::Session => match value {
                StatusValue::WorkflowStep { .. } => {}
                value => self.session_status = session_status_from_status(value),
            },
            StatusSlot::Project => match value {
                StatusValue::ProjectSelection { snapshot } => {
                    self.project_status = Some(ProjectStatusSummary { snapshot: *snapshot });
                    self.rebuild_project_lines();
                }
                StatusValue::Hidden => self.clear_project_status(),
                StatusValue::Label(_)
                | StatusValue::WorkflowStep { .. }
                | StatusValue::SessionRouting { .. } => {}
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
            StatusSlot::Project => self.clear_project_status(),
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
        if let Some(existing) = self.step_subflows.iter_mut().find(|existing| {
            existing.workflow_id == subflow.workflow_id
                && existing.step_id == subflow.step_id
                && existing.subflow_id == subflow.subflow_id
        }) {
            *existing = subflow;
        } else {
            self.step_subflows.push(subflow);
        }
        self.step_subflows.sort_by_key(|entry| {
            (
                entry.workflow_id.clone(),
                entry.step_id.clone(),
                entry.item_index,
            )
        });
    }

    pub fn upsert_step_knowledge_summary(
        &mut self,
        section_id: String,
        summary: StepKnowledgeSummary,
    ) {
        self.step_knowledge_summaries.insert(section_id, summary);
        self.refresh_delivery_panel();
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
                lines.push(format!(
                    "no-progress rounds: {}",
                    status.no_progress_streak_for_item
                ));
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
            Panel::Delivery => {
                self.delivery_pinned = true;
                let current = self.delivery_state.selected().unwrap_or_else(|| {
                    self.delivery_displayed_count
                        .max(self.delivery_lines.len())
                        .saturating_sub(1)
                });
                self.delivery_state.select(Some(current.saturating_sub(amount)));
            }
            Panel::Skills => {
                self.skills_pinned = true;
                let current = self.skills_state.selected().unwrap_or_else(|| {
                    self.skills_displayed_count
                        .max(self.skill_lines.len())
                        .saturating_sub(1)
                });
                self.skills_state.select(Some(current.saturating_sub(amount)));
            }
            Panel::Project => {
                self.project_pinned = true;
                let current = self.project_state.selected().unwrap_or_else(|| {
                    self.project_displayed_count
                        .max(self.project_lines.len())
                        .saturating_sub(1)
                });
                self.project_state.select(Some(current.saturating_sub(amount)));
            }
            Panel::Document => {
                self.document_pinned = true;
                let current = self.document_state.selected().unwrap_or_else(|| {
                    self.document_displayed_count
                        .max(self.document_lines.len())
                        .saturating_sub(1)
                });
                self.document_state
                    .select(Some(current.saturating_sub(amount)));
            }
            Panel::Memory => {
                self.memory_pinned = true;
                let current = self.memory_state.selected().unwrap_or_else(|| {
                    self.memory_displayed_count
                        .max(self.memory_lines.len())
                        .saturating_sub(1)
                });
                self.memory_state
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
            Panel::Delivery => {
                let last = self
                    .delivery_displayed_count
                    .max(self.delivery_lines.len())
                    .saturating_sub(1);
                let current = self.delivery_state.selected().unwrap_or(last);
                let new_idx = (current + amount).min(last);
                self.delivery_state.select(Some(new_idx));
                if new_idx >= last {
                    self.delivery_pinned = false;
                }
            }
            Panel::Skills => {
                let last = self
                    .skills_displayed_count
                    .max(self.skill_lines.len())
                    .saturating_sub(1);
                let current = self.skills_state.selected().unwrap_or(last);
                let new_idx = (current + amount).min(last);
                self.skills_state.select(Some(new_idx));
                if new_idx >= last {
                    self.skills_pinned = false;
                }
            }
            Panel::Project => {
                let last = self
                    .project_displayed_count
                    .max(self.project_lines.len())
                    .saturating_sub(1);
                let current = self.project_state.selected().unwrap_or(last);
                let new_idx = (current + amount).min(last);
                self.project_state.select(Some(new_idx));
                if new_idx >= last {
                    self.project_pinned = false;
                }
            }
            Panel::Document => {
                let last = self
                    .document_displayed_count
                    .max(self.document_lines.len())
                    .saturating_sub(1);
                let current = self.document_state.selected().unwrap_or(last);
                let new_idx = (current + amount).min(last);
                self.document_state.select(Some(new_idx));
                if new_idx >= last {
                    self.document_pinned = false;
                }
            }
            Panel::Memory => {
                let last = self
                    .memory_displayed_count
                    .max(self.memory_lines.len())
                    .saturating_sub(1);
                let current = self.memory_state.selected().unwrap_or(last);
                let new_idx = (current + amount).min(last);
                self.memory_state.select(Some(new_idx));
                if new_idx >= last {
                    self.memory_pinned = false;
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
            && col
                < self
                    .sidebar_rail_rect
                    .x
                    .saturating_add(self.sidebar_rail_rect.width)
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
            && col
                < self
                    .diagnostics_rect
                    .x
                    .saturating_add(self.diagnostics_rect.width)
            && row >= self.diagnostics_rect.y
            && row
                < self
                    .diagnostics_rect
                    .y
                    .saturating_add(self.diagnostics_rect.height)
        {
            Panel::Diagnostics
        } else if self.delivery_rect.width > 0
            && col >= self.delivery_rect.x
            && col < self.delivery_rect.x.saturating_add(self.delivery_rect.width)
            && row >= self.delivery_rect.y
            && row < self.delivery_rect.y.saturating_add(self.delivery_rect.height)
        {
            Panel::Delivery
        } else if self.skills_rect.width > 0
            && col >= self.skills_rect.x
            && col < self.skills_rect.x.saturating_add(self.skills_rect.width)
            && row >= self.skills_rect.y
            && row < self.skills_rect.y.saturating_add(self.skills_rect.height)
        {
            Panel::Skills
        } else if self.project_rect.width > 0
            && col >= self.project_rect.x
            && col < self.project_rect.x.saturating_add(self.project_rect.width)
            && row >= self.project_rect.y
            && row < self.project_rect.y.saturating_add(self.project_rect.height)
        {
            Panel::Project
        } else if self.document_rect.width > 0
            && col >= self.document_rect.x
            && col < self.document_rect.x.saturating_add(self.document_rect.width)
            && row >= self.document_rect.y
            && row < self.document_rect.y.saturating_add(self.document_rect.height)
        {
            Panel::Document
        } else if self.memory_rect.width > 0
            && col >= self.memory_rect.x
            && col < self.memory_rect.x.saturating_add(self.memory_rect.width)
            && row >= self.memory_rect.y
            && row < self.memory_rect.y.saturating_add(self.memory_rect.height)
        {
            Panel::Memory
        } else if self.logs_rect.width > 0
            && col >= self.logs_rect.x
            && col < self.logs_rect.x.saturating_add(self.logs_rect.width)
            && row >= self.logs_rect.y
            && row < self.logs_rect.y.saturating_add(self.logs_rect.height)
        {
            Panel::Logs
        } else if self.todo_rect.width > 0
            && col >= self.todo_rect.x
            && col < self.todo_rect.x.saturating_add(self.todo_rect.width)
            && row >= self.todo_rect.y
            && row < self.todo_rect.y.saturating_add(self.todo_rect.height)
        {
            Panel::Todo
        } else {
            Panel::Response
        }
    }

    pub fn select_sidebar_panel_item(&mut self, panel: Panel, index: usize) -> bool {
        match panel {
            Panel::Diagnostics if self.diagnostics_visible() && self.diagnostics_displayed_count > 0 => {
                self.focused_panel = Panel::Diagnostics;
                self.diagnostics_pinned = true;
                self.diagnostics_state.select(Some(index.min(self.diagnostics_displayed_count - 1)));
                true
            }
            Panel::Delivery if self.delivery_visible() && self.delivery_displayed_count > 0 => {
                self.focused_panel = Panel::Delivery;
                self.delivery_pinned = true;
                self.delivery_state.select(Some(index.min(self.delivery_displayed_count - 1)));
                true
            }
            Panel::Skills if self.skills_visible() && self.skills_displayed_count > 0 => {
                self.focused_panel = Panel::Skills;
                self.skills_pinned = true;
                self.skills_state.select(Some(index.min(self.skills_displayed_count - 1)));
                true
            }
            Panel::Project if self.project_visible() && self.project_displayed_count > 0 => {
                self.focused_panel = Panel::Project;
                self.project_pinned = true;
                self.project_state.select(Some(index.min(self.project_displayed_count - 1)));
                true
            }
            Panel::Document if self.document_visible() && self.document_displayed_count > 0 => {
                self.focused_panel = Panel::Document;
                self.document_pinned = true;
                self.document_state.select(Some(index.min(self.document_displayed_count - 1)));
                true
            }
            Panel::Memory if self.memory_visible() && self.memory_displayed_count > 0 => {
                self.focused_panel = Panel::Memory;
                self.memory_pinned = true;
                self.memory_state.select(Some(index.min(self.memory_displayed_count - 1)));
                true
            }
            Panel::Todo if self.todo_visible() && self.todo_displayed_count > 0 => {
                self.focused_panel = Panel::Todo;
                self.todo_pinned = true;
                self.todo_state.select(Some(index.min(self.todo_displayed_count - 1)));
                true
            }
            Panel::Logs if self.logs_visible() && self.logs_displayed_count > 0 => {
                self.focused_panel = Panel::Logs;
                self.logs_pinned = true;
                self.logs_state.select(Some(index.min(self.logs_displayed_count - 1)));
                true
            }
            _ => false,
        }
    }

    pub fn todo_visible(&self) -> bool {
        self.todo_rect.width > 0 && self.todo_rect.height > 0
    }

    pub fn diagnostics_visible(&self) -> bool {
        self.diagnostics_rect.width > 0 && self.diagnostics_rect.height > 0
    }

    pub fn delivery_visible(&self) -> bool {
        self.delivery_rect.width > 0 && self.delivery_rect.height > 0
    }

    pub fn skills_visible(&self) -> bool {
        self.skills_rect.width > 0 && self.skills_rect.height > 0
    }

    pub fn project_visible(&self) -> bool {
        self.project_rect.width > 0 && self.project_rect.height > 0
    }

    pub fn document_visible(&self) -> bool {
        self.document_rect.width > 0 && self.document_rect.height > 0
    }

    pub fn memory_visible(&self) -> bool {
        self.memory_rect.width > 0 && self.memory_rect.height > 0
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
        if self.focused_panel == Panel::Delivery && !self.delivery_visible() {
            self.focused_panel = Panel::Response;
        }
        if self.focused_panel == Panel::Skills && !self.skills_visible() {
            self.focused_panel = Panel::Response;
        }
        if self.focused_panel == Panel::Project && !self.project_visible() {
            self.focused_panel = Panel::Response;
        }
        if self.focused_panel == Panel::Document && !self.document_visible() {
            self.focused_panel = Panel::Response;
        }
        if self.focused_panel == Panel::Memory && !self.memory_visible() {
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
        if self.delivery_visible() {
            panels.push(Panel::Delivery);
        }
        if self.skills_visible() {
            panels.push(Panel::Skills);
        }
        if self.project_visible() {
            panels.push(Panel::Project);
        }
        if self.document_visible() {
            panels.push(Panel::Document);
        }
        if self.memory_visible() {
            panels.push(Panel::Memory);
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
            Panel::Delivery => KeyFocus::Activity,
            Panel::Skills => KeyFocus::Activity,
            Panel::Project => KeyFocus::Activity,
            Panel::Document => KeyFocus::Activity,
            Panel::Memory => KeyFocus::Activity,
            Panel::Todo => KeyFocus::Todo,
            Panel::Logs => KeyFocus::Activity,
        }
    }

    pub fn input_capable(&self) -> bool {
        self.input_enabled
    }

    pub fn is_leader_pending(&self) -> bool {
        self.pending_key_sequence.is_some()
    }

    pub fn pending_sequence_hint(&self) -> Option<String> {
        let pending = self.pending_key_sequence.as_ref()?;
        let keys = pending
            .key_events
            .iter()
            .map(describe_key_event)
            .collect::<Vec<_>>()
            .join(" ");

        match pending.replay_text.as_ref() {
            Some(replay_text) => Some(format!(
                " Pending keys: {keys}  (timeout replays {:?})  Esc=Cancel",
                replay_text
            )),
            None => Some(format!(
                " Leader pending: {keys}  jk=Toggle mode  Tab=Focus  ↑/↓=Scroll  c=Interrupt  q=Quit  Esc=Cancel"
            )),
        }
    }

    pub fn pending_key_events(&self) -> &[KeyEvent] {
        self.pending_key_sequence
            .as_ref()
            .map(|pending| pending.key_events.as_slice())
            .unwrap_or(&[])
    }

    pub fn begin_leader_pending(&mut self, leader_key: KeyEvent, timeout: Duration) {
        self.begin_pending_key_sequence(leader_key, None, timeout);
        self.status_notice = None;
    }

    pub fn begin_pending_key_sequence(
        &mut self,
        key: KeyEvent,
        replay_text: Option<String>,
        timeout: Duration,
    ) {
        self.pending_key_sequence = Some(PendingKeySequenceState {
            started_at: Instant::now(),
            timeout,
            key_events: vec![key],
            replay_text,
        });
    }

    pub fn extend_pending_sequence(
        &mut self,
        key: KeyEvent,
        replay_text: Option<String>,
        timeout: Duration,
    ) {
        if let Some(pending) = self.pending_key_sequence.as_mut() {
            pending.started_at = Instant::now();
            pending.timeout = timeout;
            pending.replay_text = replay_text;
            pending.key_events.push(key);
        } else {
            self.begin_pending_key_sequence(key, replay_text, timeout);
        }
        self.status_notice = None;
    }

    pub fn clear_leader_pending(&mut self) {
        self.pending_key_sequence = None;
    }

    pub fn expire_pending_key_sequence(&mut self) -> Option<String> {
        let Some(pending) = self.pending_key_sequence.as_ref() else {
            return None;
        };

        if pending.started_at.elapsed() < pending.timeout {
            return None;
        }

        let replay_text = pending.replay_text.clone();
        self.clear_leader_pending();
        if replay_text.is_none() {
            self.status_notice = Some("Pending key sequence timed out.".to_string());
        }

        replay_text
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

    pub fn set_command_hint(&mut self, hint: impl Into<String>) {
        self.command_hint = Some(hint.into());
    }

    pub fn clear_command_hint(&mut self) {
        self.command_hint = None;
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

    pub fn visible_sidebar_panels(&self) -> Vec<Panel> {
        let mut panels = Vec::new();
        if self.diagnostics_visible() {
            panels.push(Panel::Diagnostics);
        }
        if self.delivery_visible() {
            panels.push(Panel::Delivery);
        }
        if self.skills_visible() {
            panels.push(Panel::Skills);
        }
        if self.project_visible() {
            panels.push(Panel::Project);
        }
        if self.document_visible() {
            panels.push(Panel::Document);
        }
        if self.todo_visible() {
            panels.push(Panel::Todo);
        }
        if self.logs_visible() {
            panels.push(Panel::Logs);
        }
        panels
    }

    pub fn cycle_focused_sidebar_panel_next(&mut self) {
        let panels = self.visible_sidebar_panels();
        if panels.is_empty() {
            return;
        }
        let next = match panels.iter().position(|&p| p == self.focused_panel) {
            Some(i) => (i + 1) % panels.len(),
            None => 0,
        };
        self.focus_sidebar_panel(panels[next]);
    }

    pub fn cycle_focused_sidebar_panel_previous(&mut self) {
        let panels = self.visible_sidebar_panels();
        if panels.is_empty() {
            return;
        }
        let prev = match panels.iter().position(|&p| p == self.focused_panel) {
            Some(0) => panels.len() - 1,
            Some(i) => i - 1,
            None => panels.len() - 1,
        };
        self.focus_sidebar_panel(panels[prev]);
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
                self.focus_sidebar_panel(Panel::Diagnostics);
            }
            SidebarSection::Delivery => {
                if !self.sidebar.delivery_expanded {
                    self.sidebar.delivery_expanded = true;
                }
                self.focus_sidebar_panel(Panel::Delivery);
            }
            SidebarSection::Skills => {
                if !self.sidebar.skills_expanded {
                    self.sidebar.skills_expanded = true;
                }
                self.focus_sidebar_panel(Panel::Skills);
            }
            SidebarSection::Project => {
                if !self.sidebar.project_expanded {
                    self.sidebar.project_expanded = true;
                }
                self.focus_sidebar_panel(Panel::Project);
            }
            SidebarSection::Knowledge => {
                if !self.sidebar.knowledge_expanded {
                    self.sidebar.knowledge_expanded = true;
                }
                self.focus_sidebar_panel(Panel::Document);
            }
            SidebarSection::Todos => {
                if !self.sidebar.todos_expanded {
                    self.sidebar.todos_expanded = true;
                }
                self.focus_sidebar_panel(Panel::Todo);
            }
            SidebarSection::Logs => {
                if !self.sidebar.logs_expanded {
                    self.sidebar.logs_expanded = true;
                }
                self.focus_sidebar_panel(Panel::Logs);
            }
        }
    }

    fn focus_sidebar_panel(&mut self, panel: Panel) {
        self.focused_panel = panel;
        self.seed_sidebar_panel_selection(panel);
    }

    pub fn focus_sidebar_panel_from_pointer(&mut self, panel: Panel) {
        self.focus_sidebar_panel(panel);
    }

    fn seed_sidebar_panel_selection(&mut self, panel: Panel) {
        let total = self.panel_lines(panel).len();
        if total == 0 {
            return;
        }

        match panel {
            Panel::Diagnostics if self.diagnostics_state.selected().is_none() => {
                self.diagnostics_state.select(Some(0));
            }
            Panel::Delivery if self.delivery_state.selected().is_none() => {
                self.delivery_state.select(Some(0));
            }
            Panel::Skills if self.skills_state.selected().is_none() => {
                self.skills_state.select(Some(0));
            }
            Panel::Project if self.project_state.selected().is_none() => {
                self.project_state.select(Some(0));
            }
            Panel::Document if self.document_state.selected().is_none() => {
                self.document_state.select(Some(0));
            }
            Panel::Memory if self.memory_state.selected().is_none() => {
                self.memory_state.select(Some(0));
            }
            Panel::Todo if self.todo_state.selected().is_none() => {
                self.todo_state.select(Some(0));
            }
            Panel::Logs if self.logs_state.selected().is_none() => {
                self.logs_state.select(Some(0));
            }
            Panel::Response | Panel::SidebarRail | _ => {}
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
            SidebarSection::Delivery => match self.delivery_panel_summary() {
                Some(summary) => {
                    format!("V {}/{}", summary.llm_request_count, summary.changed_files.len())
                }
                None => "V --".to_string(),
            },
            SidebarSection::Skills => match self.latest_skill_load_section_id.as_deref() {
                Some(section_id) => self
                    .skill_load_summaries
                    .get(section_id)
                    .map(|summary| {
                        format!(
                            "S {}/{}",
                            summary.loaded_skill_ids.len(),
                            summary.recognized_skill_ids.len()
                        )
                    })
                    .unwrap_or_else(|| "S --".to_string()),
                None => "S --".to_string(),
            },
            SidebarSection::Project => self
                .project_status
                .as_ref()
                .map(|summary| {
                    format!(
                        "P {}/{}",
                        summary.snapshot.plan.current_task_count,
                        summary.snapshot.plan.history_task_count
                    )
                })
                .unwrap_or_else(|| "P --".to_string()),
            SidebarSection::Knowledge => match self.context_supervision.as_ref() {
                Some(snapshot) if !snapshot.document.enabled && !snapshot.memory.enabled => {
                    "K off".to_string()
                }
                Some(snapshot) => {
                    let doc = snapshot
                        .document
                        .current_hits
                        .as_ref()
                        .map(|hits| hits.result_count.to_string())
                        .unwrap_or_else(|| snapshot.document.readiness.as_str().to_string());
                    let mem = snapshot
                        .memory
                        .current_query
                        .as_ref()
                        .map(|query| query.result_count.to_string())
                        .unwrap_or_else(|| snapshot.memory.readiness.as_str().to_string());
                    format!("K {doc}/{mem}")
                }
                None => "K --".to_string(),
            },
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

    pub fn open_search_results_overlay(&mut self, title: impl Into<String>, lines: Vec<String>) {
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

    pub fn open_runtime_confirm_overlay(&mut self, message: impl Into<String>) {
        self.overlay = Some(OverlayState::Confirm(ConfirmOverlay {
            origin_panel: self.focused_panel,
            title: " Approval Required ".to_string(),
            message: message.into(),
            confirm_label: "Acknowledge".to_string(),
            cancel_label: "Close".to_string(),
            selected: ConfirmChoice::Confirm,
            intent: ConfirmIntent::Dismiss,
            dismiss_on_backdrop: true,
        }));
        self.clear_leader_pending();
        self.status_notice = Some("Approval overlay opened for the current tool action.".to_string());
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
    pub fn open_picker_overlay(&mut self, request: OperatorPickerRequest) {
        self.overlay = Some(OverlayState::Picker(PickerOverlay::new(
            self.focused_panel,
            request,
        )));
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

    pub fn overlay_viewport_lines(&self) -> usize {
        let height = self.overlay_rect.height.saturating_sub(4) as usize;
        height.max(8)
    }

    pub fn overlay_page_step(&self) -> usize {
        self.overlay_viewport_lines().saturating_sub(1).max(1)
    }

    pub fn scroll_active_overlay_up(&mut self, amount: usize) -> bool {
        match self.overlay.as_mut() {
            Some(OverlayState::SearchResults(overlay)) => {
                overlay.scroll = overlay.scroll.saturating_sub(amount);
                true
            }
            Some(OverlayState::Detail(overlay)) => {
                overlay.scroll = overlay.scroll.saturating_sub(amount);
                true
            }
            _ => false,
        }
    }

    pub fn scroll_active_overlay_down(&mut self, amount: usize) -> bool {
        match self.overlay.as_mut() {
            Some(OverlayState::SearchResults(overlay)) => {
                overlay.scroll = overlay.scroll.saturating_add(amount);
                true
            }
            Some(OverlayState::Detail(overlay)) => {
                overlay.scroll = overlay.scroll.saturating_add(amount);
                true
            }
            _ => false,
        }
    }

    pub fn scroll_active_overlay_to_start(&mut self) -> bool {
        match self.overlay.as_mut() {
            Some(OverlayState::SearchResults(overlay)) => {
                overlay.scroll = 0;
                true
            }
            Some(OverlayState::Detail(overlay)) => {
                overlay.scroll = 0;
                true
            }
            _ => false,
        }
    }

    pub fn scroll_active_overlay_to_end(&mut self) -> bool {
        let viewport_lines = self.overlay_viewport_lines();
        match self.overlay.as_mut() {
            Some(OverlayState::SearchResults(overlay)) => {
                overlay.scroll = overlay.lines.len().saturating_sub(viewport_lines);
                true
            }
            Some(OverlayState::Detail(overlay)) => {
                overlay.scroll = overlay.lines.len().saturating_sub(viewport_lines);
                true
            }
            _ => false,
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

    pub fn input_viewport_top(&self, total_lines: usize) -> usize {
        let visible_height = self.input_visible_height();
        if visible_height == 0 {
            return 0;
        }

        self.input_scroll_top
            .min(total_lines.saturating_sub(visible_height))
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
        self.clear_input_cursor_column_goal();
        self.ensure_input_cursor_visible();
    }

    pub fn insert_text(&mut self, text: &str) {
        for character in text.chars() {
            self.insert_char(character);
        }
    }

    pub fn insert_newline(&mut self) {
        self.insert_char('\n');
    }

    pub fn delete_char_before(&mut self) {
        if self.cursor_pos > 0 {
            self.cursor_pos -= 1;
            let byte_pos = self.cursor_byte_pos();
            self.input_buffer.remove(byte_pos);
            self.clear_input_cursor_column_goal();
            self.ensure_input_cursor_visible();
        }
    }

    pub fn delete_char_at(&mut self) {
        if self.cursor_pos < self.char_count() {
            let byte_pos = self.cursor_byte_pos();
            self.input_buffer.remove(byte_pos);
            self.clear_input_cursor_column_goal();
            self.ensure_input_cursor_visible();
        }
    }

    pub fn move_cursor_left(&mut self) {
        self.cursor_pos = self.cursor_pos.saturating_sub(1);
        self.clear_input_cursor_column_goal();
        self.ensure_input_cursor_visible();
    }

    pub fn move_cursor_right(&mut self) {
        let count = self.char_count();
        if self.cursor_pos < count {
            self.cursor_pos += 1;
        }
        self.clear_input_cursor_column_goal();
        self.ensure_input_cursor_visible();
    }

    pub fn move_cursor_up(&mut self) {
        self.move_cursor_vertical(-1);
    }

    pub fn move_cursor_down(&mut self) {
        self.move_cursor_vertical(1);
    }

    pub fn move_cursor_home(&mut self) {
        self.cursor_pos = 0;
        self.clear_input_cursor_column_goal();
        self.ensure_input_cursor_visible();
    }

    pub fn move_cursor_end(&mut self) {
        self.cursor_pos = self.char_count();
        self.clear_input_cursor_column_goal();
        self.ensure_input_cursor_visible();
    }

    pub fn scroll_input_up(&mut self, amount: usize) {
        self.input_scroll_top = self.input_scroll_top.saturating_sub(amount);
    }

    pub fn scroll_input_down(&mut self, amount: usize) {
        let total_lines = self.input_total_visual_lines();
        self.input_scroll_top = (self.input_scroll_top + amount).min(
            total_lines.saturating_sub(self.input_visible_height()),
        );
    }

    pub fn take_input(&mut self) -> String {
        let input = self.input_buffer.clone();
        self.input_buffer.clear();
        self.cursor_pos = 0;
        self.input_scroll_top = 0;
        self.clear_input_cursor_column_goal();
        self.command_hint = None;
        input
    }

    fn input_visible_height(&self) -> usize {
        self.input_rect.height as usize
    }

    fn input_content_width(&self) -> usize {
        (self.input_rect.width as usize)
            .saturating_sub(INPUT_VIEWPORT_PREFIX_WIDTH)
            .max(1)
    }

    fn input_total_visual_lines(&self) -> usize {
        self.input_cursor_visual_positions()
            .last()
            .map(|(line, _)| line.saturating_add(1))
            .unwrap_or(1)
    }

    fn input_cursor_visual_positions(&self) -> Vec<(usize, usize)> {
        let mut positions = Vec::with_capacity(self.char_count().saturating_add(1));
        let mut line = 0usize;
        let mut column = 0usize;
        let content_width = self.input_content_width();

        positions.push((line, column));
        for character in self.input_buffer.chars() {
            if character == '\n' {
                line = line.saturating_add(1);
                column = 0;
                positions.push((line, column));
                continue;
            }

            if column == content_width {
                line = line.saturating_add(1);
                column = 0;
            }
            column = column.saturating_add(1);
            positions.push((line, column));
        }

        positions
    }

    fn clear_input_cursor_column_goal(&mut self) {
        self.input_cursor_column_goal = None;
    }

    fn ensure_input_cursor_visible(&mut self) {
        let visible_height = self.input_visible_height();
        if visible_height == 0 {
            self.input_scroll_top = 0;
            return;
        }

        let positions = self.input_cursor_visual_positions();
        let (cursor_line, _) = positions.get(self.cursor_pos).copied().unwrap_or((0, 0));
        let max_top = positions
            .last()
            .map(|(line, _)| line.saturating_add(1))
            .unwrap_or(1)
            .saturating_sub(visible_height);

        if cursor_line < self.input_scroll_top {
            self.input_scroll_top = cursor_line;
        } else if cursor_line >= self.input_scroll_top.saturating_add(visible_height) {
            self.input_scroll_top = cursor_line.saturating_add(1).saturating_sub(visible_height);
        }

        self.input_scroll_top = self.input_scroll_top.min(max_top);
    }

    fn move_cursor_vertical(&mut self, direction: isize) {
        let positions = self.input_cursor_visual_positions();
        let Some((current_line, current_column)) = positions.get(self.cursor_pos).copied() else {
            return;
        };
        let last_line = positions.last().map(|(line, _)| *line).unwrap_or(0);
        let target_line = match direction {
            -1 if current_line > 0 => current_line - 1,
            1 if current_line < last_line => current_line + 1,
            _ => return,
        };
        let desired_column = self
            .input_cursor_column_goal
            .unwrap_or(current_column);
        let mut fallback_index = None;

        for (index, (line, column)) in positions.iter().copied().enumerate() {
            if line < target_line {
                continue;
            }
            if line > target_line {
                break;
            }

            fallback_index = Some(index);
            if column == desired_column {
                self.cursor_pos = index;
                self.input_cursor_column_goal = Some(desired_column);
                self.ensure_input_cursor_visible();
                return;
            }
        }

        if let Some(index) = fallback_index {
            self.cursor_pos = index;
            self.input_cursor_column_goal = Some(desired_column);
            self.ensure_input_cursor_visible();
        }
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

fn describe_key_event(key: &KeyEvent) -> String {
    match key.code {
        crossterm::event::KeyCode::Char(' ') => "<space>".to_string(),
        crossterm::event::KeyCode::Char(character) => character.to_string(),
        crossterm::event::KeyCode::Esc => "Esc".to_string(),
        crossterm::event::KeyCode::Tab => "Tab".to_string(),
        crossterm::event::KeyCode::Enter => "Enter".to_string(),
        crossterm::event::KeyCode::Backspace => "Backspace".to_string(),
        crossterm::event::KeyCode::Delete => "Delete".to_string(),
        crossterm::event::KeyCode::Left => "Left".to_string(),
        crossterm::event::KeyCode::Right => "Right".to_string(),
        crossterm::event::KeyCode::Up => "Up".to_string(),
        crossterm::event::KeyCode::Down => "Down".to_string(),
        crossterm::event::KeyCode::Home => "Home".to_string(),
        crossterm::event::KeyCode::End => "End".to_string(),
        other => format!("{other:?}"),
    }
}

#[cfg(test)]
#[path = "app_tests.rs"]
mod tests;
