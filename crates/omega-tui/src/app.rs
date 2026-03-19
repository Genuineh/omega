use ratatui::{layout::Rect, widgets::ListState};

use omega_observability::strip_ansi;
use omega_session::SessionUpdate;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Response,
    Logs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MsgKind {
    User,
    Agent,
    Tool,
    Error,
    Separator,
}

pub struct Msg {
    pub kind: MsgKind,
    pub text: String,
}

pub struct App {
    pub output_msgs: Vec<Msg>,
    pub log_lines: Vec<String>,
    pub response_state: ListState,
    pub logs_state: ListState,
    pub focused_panel: Panel,
    pub response_pinned: bool,
    pub logs_pinned: bool,
    pub response_rect: Rect,
    pub logs_rect: Rect,
    pub input_buffer: String,
    pub cursor_pos: usize,
    pub is_running: bool,
    pub active_turn_id: u64,
    pub spinner_tick: u8,
    pub response_displayed_count: usize,
    pub logs_displayed_count: usize,
}

impl App {
    pub fn new() -> Self {
        Self {
            output_msgs: vec![],
            log_lines: vec![],
            response_state: ListState::default(),
            logs_state: ListState::default(),
            focused_panel: Panel::Response,
            response_pinned: false,
            logs_pinned: false,
            response_rect: Rect::default(),
            logs_rect: Rect::default(),
            input_buffer: String::new(),
            cursor_pos: 0,
            is_running: false,
            active_turn_id: 0,
            spinner_tick: 0,
            response_displayed_count: 0,
            logs_displayed_count: 0,
        }
    }

    pub fn begin_turn(&mut self) -> u64 {
        self.active_turn_id = self.active_turn_id.wrapping_add(1);
        self.is_running = true;
        self.active_turn_id
    }

    pub fn interrupt_turn(&mut self) {
        self.active_turn_id = self.active_turn_id.wrapping_add(1);
        self.is_running = false;
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
                    self.push_msg(MsgKind::Tool, &format!("$ {}", command));
                }
                self.push_msg(MsgKind::Tool, &preview);
            }
            SessionUpdate::AssistantText { turn_id, text } if self.is_current_turn(turn_id) => {
                self.push_msg(MsgKind::Agent, &text);
            }
            SessionUpdate::TurnFinished { turn_id } if self.is_current_turn(turn_id) => {
                self.is_running = false;
            }
            _ => {}
        }
    }

    pub fn add_log(&mut self, line: String) {
        self.log_lines.push(strip_ansi(&line));
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
            Panel::Logs => {
                self.logs_pinned = true;
                let current = self.logs_state.selected().unwrap_or_else(|| {
                    self.logs_displayed_count
                        .max(self.log_lines.len())
                        .saturating_sub(1)
                });
                self.logs_state.select(Some(current.saturating_sub(amount)));
            }
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
        }
    }

    pub fn panel_at(&self, col: u16) -> Panel {
        if self.logs_rect.width > 0 && col >= self.logs_rect.x {
            Panel::Logs
        } else {
            Panel::Response
        }
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
