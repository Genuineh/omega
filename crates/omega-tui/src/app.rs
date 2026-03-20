use std::time::{Duration, Instant};

use crossterm::event::KeyEvent;
use omega_keymap::{InteractionMode, KeyFocus};
use ratatui::{layout::Rect, widgets::ListState};

use omega_observability::strip_ansi;
use omega_session::SessionUpdate;

use crate::overlay::{
    ConfirmChoice, ConfirmIntent, ConfirmOverlay, DetailOverlay, InputPromptOverlay, OverlayState,
    PickerOverlay, SearchOverlay,
};
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
    Todo,
    Logs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MsgKind {
    User,
    Agent,
    Error,
    Separator,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Msg {
    pub kind: MsgKind,
    pub text: String,
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
    pub id: String,
    pub label: String,
    pub index: usize,
    pub total: usize,
}

pub struct App {
    pub output_msgs: Vec<Msg>,
    pub todo_lines: Vec<String>,
    pub todo_status: TodoPanelStatus,
    pub todo_summary: Option<TodoSummary>,
    pub log_lines: Vec<String>,
    pub response_state: ListState,
    pub todo_state: ListState,
    pub logs_state: ListState,
    pub focused_panel: Panel,
    pub interaction_mode: InteractionMode,
    pub response_pinned: bool,
    pub todo_pinned: bool,
    pub logs_pinned: bool,
    pub response_rect: Rect,
    pub input_context_rect: Rect,
    pub input_gap_rect: Rect,
    pub input_rect: Rect,
    pub sidebar_rect: Rect,
    pub sidebar_rail_rect: Rect,
    pub todo_rect: Rect,
    pub logs_rect: Rect,
    pub bottom_status_rect: Rect,
    pub input_buffer: String,
    pub cursor_pos: usize,
    pub input_enabled: bool,
    pub is_running: bool,
    pub active_turn_id: u64,
    pub workflow_summary: Option<WorkflowSummary>,
    pub last_todo_turn_id: Option<u64>,
    pub spinner_tick: u8,
    pub response_displayed_count: usize,
    pub todo_displayed_count: usize,
    pub logs_displayed_count: usize,
    pub leader_pending_since: Option<Instant>,
    pub pending_key_events: Vec<KeyEvent>,
    pub keymap_source: String,
    pub status_notice: Option<String>,
    pub sidebar: SidebarState,
    pub overlay: Option<OverlayState>,
    pub overlay_rect: Rect,
}

impl App {
    pub fn new() -> Self {
        Self {
            output_msgs: vec![],
            todo_lines: todo_unsynced_lines(),
            todo_status: TodoPanelStatus::NeverSynced,
            todo_summary: None,
            log_lines: vec![],
            response_state: ListState::default(),
            todo_state: ListState::default(),
            logs_state: ListState::default(),
            focused_panel: Panel::Response,
            interaction_mode: InteractionMode::Normal,
            response_pinned: false,
            todo_pinned: false,
            logs_pinned: false,
            response_rect: Rect::default(),
            input_context_rect: Rect::default(),
            input_gap_rect: Rect::default(),
            input_rect: Rect::default(),
            sidebar_rect: Rect::default(),
            sidebar_rail_rect: Rect::default(),
            todo_rect: Rect::default(),
            logs_rect: Rect::default(),
            bottom_status_rect: Rect::default(),
            input_buffer: String::new(),
            cursor_pos: 0,
            input_enabled: true,
            is_running: false,
            active_turn_id: 0,
            workflow_summary: None,
            last_todo_turn_id: None,
            spinner_tick: 0,
            response_displayed_count: 0,
            todo_displayed_count: 0,
            logs_displayed_count: 0,
            leader_pending_since: None,
            pending_key_events: Vec::new(),
            keymap_source: "builtin".to_string(),
            status_notice: None,
            sidebar: SidebarState::default(),
            overlay: None,
            overlay_rect: Rect::default(),
        }
    }

    pub fn begin_turn(&mut self) -> u64 {
        self.active_turn_id = self.active_turn_id.wrapping_add(1);
        self.is_running = true;
        self.workflow_summary = None;
        self.active_turn_id
    }

    pub fn interrupt_turn(&mut self) {
        self.active_turn_id = self.active_turn_id.wrapping_add(1);
        self.is_running = false;
        self.workflow_summary = None;
    }

    pub fn is_current_turn(&self, turn_id: u64) -> bool {
        self.active_turn_id == turn_id
    }

    pub fn apply_session_update(&mut self, update: SessionUpdate) {
        match update {
            SessionUpdate::ToolCallPreview {
                turn_id,
                command,
                preview,
            } if self.is_current_turn(turn_id) => {
                if let Some(command) = command {
                    self.add_log(format!("[tool] $ {}", command));
                }
                self.add_log(format!("[tool] {}", preview));
            }
            SessionUpdate::TodoSnapshot { turn_id, rendered } if self.is_current_turn(turn_id) => {
                self.set_todo_snapshot(turn_id, &rendered);
            }
            SessionUpdate::WorkflowStepChanged {
                turn_id,
                step_id,
                step_label,
                index,
                total,
            } if self.is_current_turn(turn_id) => {
                self.add_log(format!(
                    "[flow {}/{}] {} ({})",
                    index, total, step_label, step_id
                ));
                self.workflow_summary = Some(WorkflowSummary {
                    id: step_id,
                    label: step_label,
                    index,
                    total,
                });
            }
            SessionUpdate::StepText {
                turn_id,
                step_label,
                text,
                ..
            } if self.is_current_turn(turn_id) => {
                self.push_step_result(&step_label, &text);
            }
            SessionUpdate::AssistantText { turn_id, text } if self.is_current_turn(turn_id) => {
                self.push_msg(MsgKind::Agent, &text);
            }
            SessionUpdate::TurnFinished { turn_id } if self.is_current_turn(turn_id) => {
                self.is_running = false;
                self.workflow_summary = None;
            }
            _ => {}
        }
    }

    pub fn add_log(&mut self, line: String) {
        self.log_lines.push(strip_ansi(&line));
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
        for line in clean.lines() {
            self.output_msgs.push(Msg {
                kind,
                text: line.to_string(),
            });
        }
        if clean.is_empty() {
            self.output_msgs.push(Msg {
                kind,
                text: String::new(),
            });
        }
    }

    pub fn push_step_result(&mut self, step_label: &str, text: &str) {
        let clean = strip_ansi(text);
        if clean.is_empty() {
            self.output_msgs.push(Msg {
                kind: MsgKind::Agent,
                text: format!("[{}]", step_label),
            });
            return;
        }

        for line in clean.lines() {
            self.output_msgs.push(Msg {
                kind: MsgKind::Agent,
                text: format!("[{}] {}", step_label, line),
            });
        }
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

    pub fn panel_lines(&self, panel: Panel) -> Vec<&str> {
        match panel {
            Panel::Response => self
                .output_msgs
                .iter()
                .map(|msg| msg.text.as_str())
                .collect(),
            Panel::SidebarRail => Vec::new(),
            Panel::Todo => self.todo_lines.iter().map(String::as_str).collect(),
            Panel::Logs => self.log_lines.iter().map(String::as_str).collect(),
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;

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
        app.apply_session_update(SessionUpdate::TurnFinished {
            turn_id: first_turn,
        });

        app.begin_turn();

        assert!(app.todo_refresh_pending());
        assert!(app.todo_panel_title().contains("stale"));
    }

    #[test]
    fn current_turn_workflow_updates_replace_summary_and_clear_on_finish() {
        let mut app = App::new();
        let turn_id = app.begin_turn();

        app.apply_session_update(SessionUpdate::WorkflowStepChanged {
            turn_id,
            step_id: "plan".to_string(),
            step_label: "Plan".to_string(),
            index: 2,
            total: 4,
        });

        assert_eq!(
            app.workflow_summary,
            Some(WorkflowSummary {
                id: "plan".to_string(),
                label: "Plan".to_string(),
                index: 2,
                total: 4,
            })
        );
        assert_eq!(app.log_lines, vec!["[flow 2/4] Plan (plan)"]);

        app.apply_session_update(SessionUpdate::TurnFinished { turn_id });

        assert!(app.workflow_summary.is_none());
    }

    #[test]
    fn tool_preview_routes_to_logs_instead_of_response() {
        let mut app = App::new();
        let turn_id = app.begin_turn();

        app.apply_session_update(SessionUpdate::ToolCallPreview {
            turn_id,
            command: Some("echo hi".to_string()),
            preview: "hi".to_string(),
        });

        assert!(app.output_msgs.is_empty());
        assert_eq!(app.log_lines, vec!["[tool] $ echo hi", "[tool] hi"]);
    }

    #[test]
    fn step_text_routes_to_response_with_step_label() {
        let mut app = App::new();
        let turn_id = app.begin_turn();

        app.apply_session_update(SessionUpdate::StepText {
            turn_id,
            step_id: "plan".to_string(),
            step_label: "Plan".to_string(),
            text: "Line one\nLine two".to_string(),
        });

        assert_eq!(
            app.output_msgs,
            vec![
                Msg {
                    kind: MsgKind::Agent,
                    text: "[Plan] Line one".to_string(),
                },
                Msg {
                    kind: MsgKind::Agent,
                    text: "[Plan] Line two".to_string(),
                },
            ]
        );
        assert!(app.log_lines.is_empty());
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
}
