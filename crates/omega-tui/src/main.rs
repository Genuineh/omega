use std::io::{self, Write};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use omega_core::{Agent, DynLlmClient, MinimaxClient, MinimaxConfig};
use ratatui::widgets::ListState;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame, Terminal,
};
use tokio::runtime::Handle;
use tracing::{error, info};
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;

/// Panel that can be focused for keyboard scrolling
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Panel {
    Response,
    Logs,
}

/// Message kind — controls color layering in the Response panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
enum MsgKind {
    User,      // user input line   — green
    Agent,     // agent reply       — default text
    Tool,      // tool call/output  — yellow
    Error,     // error output      — red
    Separator, // turn divider      — dim
}

/// A single stored message line (source, before wrap).
struct Msg {
    kind: MsgKind,
    text: String,
}

struct AgentSlot {
    turn_id: u64,
    agent: Option<Agent>,
}

/// Application state for multi-panel TUI
struct App {
    // Left panel: Agent response output (typed messages for color layering)
    output_msgs: Vec<Msg>,
    // Right panel: Trace/log lines
    log_lines: Vec<String>,
    // ListState for each panel (scrollable)
    response_state: ListState,
    logs_state: ListState,
    // Which panel has keyboard focus
    focused_panel: Panel,
    // Auto-scroll flags: true = follow new content; false = user has scrolled up (pinned)
    response_pinned: bool,
    logs_pinned: bool,
    // Panel rects from last render — used for accurate mouse-over detection
    response_rect: Rect,
    logs_rect: Rect,
    // Bottom: User input buffer
    input_buffer: String,
    // Cursor position within input_buffer (Unicode char index, not byte index)
    cursor_pos: usize,
    // Whether we're waiting for agent response
    is_running: bool,
    // Monotonic turn id. Every new turn or interrupt advances it so stale
    // worker-thread updates can be ignored without blocking the next send.
    active_turn_id: u64,
    // Spinner animation frame counter (incremented every render frame, wraps at 10)
    spinner_tick: u8,
    // Rendered (wrapped) line counts from the last frame — used by scroll_panel_down for max bounds
    response_displayed_count: usize,
    logs_displayed_count: usize,
}

impl App {
    fn new() -> Self {
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

    fn begin_turn(&mut self) -> u64 {
        self.active_turn_id = self.active_turn_id.wrapping_add(1);
        self.is_running = true;
        self.active_turn_id
    }

    fn interrupt_turn(&mut self) {
        self.active_turn_id = self.active_turn_id.wrapping_add(1);
        self.is_running = false;
    }

    fn is_current_turn(&self, turn_id: u64) -> bool {
        self.active_turn_id == turn_id
    }

    fn apply_log_update(&mut self, update: LogUpdate) {
        match update {
            LogUpdate::ToolLog { turn_id, log } if self.is_current_turn(turn_id) => {
                self.push_msg(MsgKind::Tool, &log);
            }
            LogUpdate::Output { turn_id, text } if self.is_current_turn(turn_id) => {
                self.push_msg(MsgKind::Agent, &text);
            }
            LogUpdate::Done { turn_id } if self.is_current_turn(turn_id) => {
                self.is_running = false;
            }
            _ => {}
        }
    }

    fn add_log(&mut self, line: String) {
        self.log_lines.push(strip_ansi(&line));
        // Auto-scroll is handled in render() each frame when logs_pinned == false.
    }

    /// Push one or more source lines into the Response panel with a given kind.
    /// Multi-line text is split and each line stored individually.
    fn push_msg(&mut self, kind: MsgKind, text: &str) {
        let clean = strip_ansi(text);
        for line in clean.lines() {
            self.output_msgs.push(Msg {
                kind,
                text: line.to_string(),
            });
        }
        // For empty text, push a blank line so callers don't need to special-case.
        if clean.is_empty() {
            self.output_msgs.push(Msg {
                kind,
                text: String::new(),
            });
        }
    }

    /// Scroll a panel up by `amount` items. Pins the view so auto-scroll stops.
    fn scroll_panel_up(&mut self, panel: Panel, amount: usize) {
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

    /// Scroll a panel down by `amount` items. Re-enables auto-scroll when the
    /// selected item reaches the last entry (i.e., user scrolled to the bottom).
    fn scroll_panel_down(&mut self, panel: Panel, amount: usize) {
        match panel {
            Panel::Response => {
                // Use the rendered (wrapped) count from last frame; fall back to source len.
                let last = self
                    .response_displayed_count
                    .max(self.output_msgs.len())
                    .saturating_sub(1);
                let current = self.response_state.selected().unwrap_or(last);
                let new_idx = (current + amount).min(last);
                self.response_state.select(Some(new_idx));
                if new_idx >= last {
                    self.response_pinned = false; // reached bottom → re-enable auto-scroll
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
                    self.logs_pinned = false; // reached bottom → re-enable auto-scroll
                }
            }
        }
    }

    /// Determine which panel the mouse is over using actual rendered Rects.
    fn panel_at(&self, col: u16) -> Panel {
        if col >= self.logs_rect.x {
            Panel::Logs
        } else {
            Panel::Response
        }
    }

    /// Number of Unicode characters in the input buffer.
    fn char_count(&self) -> usize {
        self.input_buffer.chars().count()
    }

    /// Byte offset in input_buffer for the current cursor_pos.
    fn cursor_byte_pos(&self) -> usize {
        self.input_buffer
            .char_indices()
            .nth(self.cursor_pos)
            .map(|(i, _)| i)
            .unwrap_or(self.input_buffer.len())
    }

    /// Insert a character at the cursor and advance the cursor by one.
    fn insert_char(&mut self, c: char) {
        let byte_pos = self.cursor_byte_pos();
        self.input_buffer.insert(byte_pos, c);
        self.cursor_pos += 1;
    }

    /// Delete the character immediately before the cursor (Backspace).
    fn delete_char_before(&mut self) {
        if self.cursor_pos > 0 {
            self.cursor_pos -= 1;
            let byte_pos = self.cursor_byte_pos();
            self.input_buffer.remove(byte_pos);
        }
    }

    /// Delete the character at the cursor (Delete key).
    fn delete_char_at(&mut self) {
        if self.cursor_pos < self.char_count() {
            let byte_pos = self.cursor_byte_pos();
            self.input_buffer.remove(byte_pos);
        }
    }

    fn move_cursor_left(&mut self) {
        self.cursor_pos = self.cursor_pos.saturating_sub(1);
    }
    fn move_cursor_right(&mut self) {
        let m = self.char_count();
        if self.cursor_pos < m {
            self.cursor_pos += 1;
        }
    }
    fn move_cursor_home(&mut self) {
        self.cursor_pos = 0;
    }
    fn move_cursor_end(&mut self) {
        self.cursor_pos = self.char_count();
    }

    /// Take the current input, clear the buffer, and reset the cursor. Used on Enter.
    fn take_input(&mut self) -> String {
        let s = self.input_buffer.clone();
        self.input_buffer.clear();
        self.cursor_pos = 0;
        s
    }
}

/// Log update message from agent thread
enum LogUpdate {
    ToolLog { turn_id: u64, log: String },
    Output { turn_id: u64, text: String },
    Done { turn_id: u64 },
}

/// Custom writer that sends log lines to the UI channel for the Logs panel.
/// Wraps a Mutex<Option<SyncSender>> so the sender can be taken and used inside
/// tracing-subscriber's MakeWriter closure.
struct UiWriter(Mutex<Option<mpsc::SyncSender<String>>>);

impl Write for UiWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let s = String::from_utf8_lossy(buf);
        for line in s.lines() {
            if !line.is_empty() {
                if let Some(tx) = self.0.lock().unwrap().as_ref() {
                    let _ = tx.send(line.to_string());
                }
            }
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Initialize the tracing subscriber with three layers:
/// 1. Terminal layer: compact human-readable, controlled by `OMEGA_LOG` env var
/// 2. File layer: JSON format → `~/.omega/logs/omega-YYYY-MM-DD.jsonl`
/// 3. UI layer: sends compact lines to the Logs panel
fn init_tracing(tx: Mutex<Option<mpsc::SyncSender<String>>>) -> anyhow::Result<()> {
    let env_filter =
        EnvFilter::try_from_env("OMEGA_LOG").unwrap_or_else(|_| EnvFilter::new("info"));

    // UI layer: sends compact lines to the Logs panel (ANSI disabled — UiWriter is not a TTY)
    let ui_layer = tracing_subscriber::fmt::layer()
        .compact()
        .with_ansi(false)
        .with_target(true)
        .with_thread_ids(false)
        .with_span_events(FmtSpan::CLOSE)
        .with_writer(Mutex::new(UiWriter(tx)));

    let file_layer = match create_log_file() {
        Ok(file) => {
            let writer = Mutex::new(file);
            let layer = tracing_subscriber::fmt::layer()
                .json()
                .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE)
                .with_writer(writer);
            Some(layer)
        }
        Err(e) => {
            eprintln!("warn: failed to create log file, file logging disabled: {e}");
            None
        }
    };

    tracing_subscriber::registry()
        .with(env_filter)
        .with(ui_layer)
        .with(file_layer)
        .init();

    Ok(())
}

fn create_log_file() -> anyhow::Result<std::fs::File> {
    let log_dir = match std::env::var("OMEGA_LOG_DIR") {
        Ok(dir) => std::path::PathBuf::from(dir),
        Err(_) => {
            let home = dirs::home_dir()
                .ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?;
            home.join(".omega").join("logs")
        }
    };

    let enabled = std::env::var("OMEGA_LOG_FILE")
        .map(|v| v != "false" && v != "0")
        .unwrap_or(true);
    if !enabled {
        return Err(anyhow::anyhow!("file logging disabled by OMEGA_LOG_FILE"));
    }

    std::fs::create_dir_all(&log_dir)?;

    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let filename = format!("omega-{today}.jsonl");
    let path = log_dir.join(&filename);

    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;

    Ok(file)
}

/// Strip ANSI escape sequences from a string so ratatui can render plain text
/// without leaking control codes into the terminal mid-frame.
/// Handles CSI (ESC [ ... letter), OSC (ESC ] ... BEL/ST), SS2/SS3, and
/// plain two-byte sequences (ESC + one char).
fn strip_ansi(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut result = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b {
            if i + 1 < bytes.len() && bytes[i + 1] == b'[' {
                // CSI sequence: ESC [ <params> <final-byte A-Za-z>
                i += 2;
                while i < bytes.len() && !bytes[i].is_ascii_alphabetic() {
                    i += 1;
                }
                if i < bytes.len() {
                    i += 1;
                }
            } else if i + 1 < bytes.len() && bytes[i + 1] == b']' {
                // OSC sequence: ESC ] ... BEL (0x07) or ESC \
                i += 2;
                while i < bytes.len() {
                    if bytes[i] == 0x07 {
                        i += 1;
                        break;
                    }
                    if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'\\' {
                        i += 2;
                        break;
                    }
                    i += 1;
                }
            } else {
                // Two-byte sequences (ESC + one char): skip both
                i += 2;
            }
        } else {
            result.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&result).into_owned()
}

/// Hard-wrap a string into segments of at most `width` Unicode characters.
/// Each stored source line may expand into multiple displayed rows.
fn wrap_text(line: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![line.to_string()];
    }
    if line.is_empty() {
        return vec![String::new()];
    }
    let chars: Vec<char> = line.chars().collect();
    let mut result = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        let end = (start + width).min(chars.len());
        result.push(chars[start..end].iter().collect());
        start = end;
    }
    result
}

/// Render the TUI with multiple panels
fn render(frame: &mut Frame, app: &mut App, model_name: &str) {
    // Define color scheme (dark theme)
    let colors = ColorScheme::dark();

    // Create the main layout
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Status bar (no border, color separates)
            Constraint::Min(0),    // Main content area
            Constraint::Length(3), // Input area
            Constraint::Length(1), // Hint bar
        ])
        .split(frame.area());

    // Responsive horizontal split: shrink or hide Logs panel on narrow terminals
    let term_width = frame.area().width;
    let (resp_pct, logs_pct): (u16, u16) = if term_width < 60 {
        (100, 0) // very narrow: full-width Response, Logs hidden
    } else if term_width < 100 {
        (70, 30) // narrow: shrink Logs to 30 %
    } else {
        (60, 40) // normal: 60 / 40 split
    };

    // Split main content into left (output) and right (logs)
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(resp_pct),
            Constraint::Percentage(logs_pct),
        ])
        .split(chunks[1]);

    // Store panel rects so mouse events can use actual boundaries (not hardcoded column offsets)
    app.response_rect = main_chunks[0];
    app.logs_rect = main_chunks[1];

    // ── Status Bar ── (1-row, no border — background color provides visual separation)
    let focus_label = match app.focused_panel {
        Panel::Response => "Response",
        Panel::Logs => "Logs",
    };
    const SPINNER_FRAMES: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
    let spinner_char = SPINNER_FRAMES[(app.spinner_tick as usize / 2) % SPINNER_FRAMES.len()];
    let agent_state_str; // storage for the formatted string below
    let agent_state = if app.is_running {
        agent_state_str = format!("{spinner_char} Running…");
        agent_state_str.as_str()
    } else {
        "● Idle"
    };
    let status_text = format!(
        " Omega Agent │ {} │ {} │ Focus: {} ",
        model_name, agent_state, focus_label
    );
    let status =
        Paragraph::new(status_text).style(Style::default().fg(colors.text).bg(colors.status_bar));
    frame.render_widget(status, chunks[0]);

    // ── Panel border styles based on focus ──
    let response_border = if app.focused_panel == Panel::Response {
        Style::default()
            .fg(colors.focus_border)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(colors.border_dim)
    };
    let logs_border = if app.focused_panel == Panel::Logs {
        Style::default()
            .fg(colors.focus_border)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(colors.border_dim)
    };

    // ── Left Panel: Agent Response ──
    let response_title = if app.focused_panel == Panel::Response {
        " Agent Response ◆ "
    } else {
        " Agent Response "
    };
    // Wrap long lines at the panel's inner width (subtract 2 border columns)
    let resp_inner_w = (main_chunks[0].width as usize).saturating_sub(2).max(1);
    let output_items: Vec<ListItem> = app
        .output_msgs
        .iter()
        .flat_map(|msg| {
            let style = match msg.kind {
                MsgKind::User => Style::default().fg(Color::Green),
                MsgKind::Agent => Style::default().fg(colors.text),
                MsgKind::Tool => Style::default().fg(colors.command),
                MsgKind::Error => Style::default().fg(Color::Red),
                MsgKind::Separator => Style::default().fg(colors.border_dim),
            };
            wrap_text(&msg.text, resp_inner_w)
                .into_iter()
                .map(move |wrapped| ListItem::new(Span::styled(wrapped, style)))
        })
        .collect();
    // Update rendered count and auto-scroll to bottom when not pinned
    let resp_total = output_items.len();
    app.response_displayed_count = resp_total;
    if !app.response_pinned && resp_total > 0 {
        app.response_state.select(Some(resp_total - 1));
    }
    let output_list = List::new(output_items)
        .block(
            Block::default()
                .title(response_title)
                .borders(Borders::ALL)
                .border_style(response_border),
        )
        .highlight_style(Style::default())
        .style(Style::default().fg(colors.text));
    frame.render_stateful_widget(output_list, main_chunks[0], &mut app.response_state);

    // ── Right Panel: Logs ──
    let logs_title = if app.focused_panel == Panel::Logs {
        " Logs ◆ "
    } else {
        " Logs "
    };
    // Only render if the panel has width (may be hidden on narrow terminals)
    let logs_inner_w = (main_chunks[1].width as usize).saturating_sub(2).max(1);
    let log_items: Vec<ListItem> = app
        .log_lines
        .iter()
        .flat_map(|line| wrap_text(line, logs_inner_w).into_iter().map(ListItem::new))
        .collect();
    // Update rendered count and auto-scroll to bottom when not pinned
    let logs_total = log_items.len();
    app.logs_displayed_count = logs_total;
    if !app.logs_pinned && logs_total > 0 {
        app.logs_state.select(Some(logs_total - 1));
    }
    let log_list = List::new(log_items)
        .block(
            Block::default()
                .title(logs_title)
                .borders(Borders::ALL)
                .border_style(logs_border),
        )
        .highlight_style(Style::default())
        .style(Style::default().fg(colors.text));
    if main_chunks[1].width > 0 {
        frame.render_stateful_widget(log_list, main_chunks[1], &mut app.logs_state);
    }

    // ── Input Area ──
    // Build Span-based display with inline block-cursor and horizontal scrolling.
    let chars: Vec<char> = app.input_buffer.chars().collect();
    let cursor_pos = app.cursor_pos;
    let char_count = chars.len();

    // Usable text columns = total widget width − 2 border columns − 3 prefix (" > ")
    let avail_w = (chunks[2].width as usize).saturating_sub(5).max(1);

    // Horizontal scroll: keep cursor always inside the visible window.
    let scroll_offset = if cursor_pos < avail_w {
        0
    } else {
        cursor_pos - avail_w + 1
    };

    // ◂>  prefix signals hidden text to the left; normal " > " otherwise.
    let prefix = if scroll_offset > 0 {
        "\u{25c2}> "
    } else {
        " > "
    };
    let prefix_span = Span::styled(prefix, Style::default().fg(colors.input_text));

    let mut spans: Vec<Span> = vec![prefix_span];
    if app.input_buffer.is_empty() {
        // Empty buffer: standing block cursor placeholder.
        spans.push(Span::styled(" ", Style::default().bg(colors.input_text)));
    } else {
        for (i, &ch) in chars.iter().enumerate().skip(scroll_offset).take(avail_w) {
            let style = if i == cursor_pos {
                // Block cursor: reverse video
                Style::default()
                    .fg(colors.input_bg)
                    .bg(colors.input_text)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(colors.input_text)
            };
            spans.push(Span::styled(ch.to_string(), style));
        }
        // Cursor at end-of-buffer: trailing block in remaining space.
        if cursor_pos == char_count && (char_count - scroll_offset) < avail_w {
            spans.push(Span::styled(" ", Style::default().bg(colors.input_text)));
        }
    }

    // Show running state in the block title to avoid obscuring the cursor.
    let input_title = if app.is_running {
        " Input [Running…] "
    } else {
        " Input "
    };
    let input = Paragraph::new(Line::from(spans))
        .style(Style::default().bg(colors.input_bg))
        .block(
            Block::default()
                .title(input_title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors.border)),
        );
    frame.render_widget(input, chunks[2]);

    // ── Hint Bar ──
    let hint_text = " Tab=Focus  ↑↓=Scroll  ←→=Cursor  Del=Delete  Ctrl+C=Interrupt  Ctrl+Q=Quit";
    let hint =
        Paragraph::new(hint_text).style(Style::default().fg(colors.hint_dim).bg(colors.status_bar));
    frame.render_widget(hint, chunks[3]);
}

/// Color scheme for dark theme
struct ColorScheme {
    #[allow(dead_code)]
    bg: Color,
    text: Color,
    border: Color,
    border_dim: Color,
    focus_border: Color,
    status_bar: Color,
    input_bg: Color,
    input_text: Color,
    hint_dim: Color,
    #[allow(dead_code)]
    highlight: Color,
    #[allow(dead_code)]
    command: Color,
}

impl ColorScheme {
    fn dark() -> Self {
        Self {
            bg: Color::Rgb(30, 30, 30),             // #1e1e1e
            text: Color::Rgb(212, 212, 212),        // #d4d4d4
            border: Color::Rgb(62, 62, 62),         // #3e3e3e
            border_dim: Color::Rgb(48, 48, 48),     // #303030 — unfocused panels
            focus_border: Color::Rgb(78, 201, 176), // #4ec9b0 — focused panel
            status_bar: Color::Rgb(45, 45, 45),
            input_bg: Color::Rgb(40, 40, 40),
            input_text: Color::Rgb(86, 156, 214), // #569cd6
            hint_dim: Color::Rgb(100, 100, 100),  // #646464 — hint bar text
            highlight: Color::Rgb(78, 201, 176),  // #4ec9b0
            command: Color::Rgb(220, 220, 170),   // #dcdcaa
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Channel for tracing log lines → Logs panel
    let (trace_tx, trace_rx) = mpsc::sync_channel::<String>(1024);
    init_tracing(Mutex::new(Some(trace_tx)))?;

    info!("omega starting with multi-panel TUI");

    let config = MinimaxConfig::from_env().map_err(|e| anyhow::anyhow!("{e}"))?;
    let model_name = config.model.clone();
    info!(model = %model_name, base_url = %config.base_url, "llm config loaded");

    let client: DynLlmClient =
        Arc::new(MinimaxClient::new(config).map_err(|e| anyhow::anyhow!("{e}"))?);

    let cwd = std::env::current_dir()?;
    let dispatcher = omega_core::create_default_tools(cwd.clone());
    info!(cwd = %cwd.display(), tool_count = dispatcher.len(), "tools registered");

    let system = format!(
        "You are a coding agent at {}. Use bash to solve tasks. Act, don't explain.",
        cwd.display()
    );

    let agent = Arc::new(Mutex::new(AgentSlot {
        turn_id: 0,
        agent: Some(Agent::new(client.clone(), system.clone(), dispatcher)?),
    }));
    let turn_checkpoint = Arc::new(Mutex::new(
        agent
            .lock()
            .unwrap()
            .agent
            .as_ref()
            .unwrap()
            .messages()
            .to_vec(),
    ));

    // Initialize terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let app = Arc::new(Mutex::new(App::new()));

    // Channel for sending log updates from agent thread
    let (tx, rx) = mpsc::channel::<LogUpdate>();

    // Main event loop
    loop {
        // Process agent events (max 20 per frame to keep UI responsive)
        for _ in 0..20 {
            if let Ok(update) = rx.try_recv() {
                let mut app = app.lock().unwrap();
                app.apply_log_update(update);
            } else {
                break;
            }
        }

        // Drain trace channel → Logs panel (non-blocking, max 20 per frame)
        for _ in 0..20 {
            if let Ok(line) = trace_rx.try_recv() {
                app.lock().unwrap().add_log(line);
            } else {
                break;
            }
        }

        // Advance spinner animation every frame
        {
            let mut app_guard = app.lock().unwrap();
            app_guard.spinner_tick = app_guard.spinner_tick.wrapping_add(1);
        }

        // Render the UI
        {
            let mut app_guard = app.lock().unwrap();
            let app_for_render = &mut *app_guard;
            terminal.draw(|f| render(f, app_for_render, &model_name))?;
        }

        // Check for events (non-blocking)
        if event::poll(std::time::Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    match key.code {
                        KeyCode::Char(c)
                            if key.modifiers == crossterm::event::KeyModifiers::CONTROL =>
                        {
                            match c {
                                'q' => {
                                    info!("user exit via Ctrl+Q");
                                    break;
                                }
                                'c' => {
                                    let mut app = app.lock().unwrap();
                                    if app.is_running {
                                        // Interrupt immediately by invalidating the current
                                        // turn id and restoring a fresh agent from the last
                                        // checkpoint before the interrupted user turn.
                                        app.interrupt_turn();
                                        app.push_msg(MsgKind::Error, "⚠ Interrupted");
                                        info!("user interrupted running task via Ctrl+C");

                                        let checkpoint = turn_checkpoint.lock().unwrap().clone();
                                        let turn_id = app.active_turn_id;
                                        let mut replacement = Agent::new(
                                            client.clone(),
                                            system.clone(),
                                            omega_core::create_default_tools(cwd.clone()),
                                        )?;
                                        replacement.set_messages(checkpoint);
                                        let mut slot = agent.lock().unwrap();
                                        slot.turn_id = turn_id;
                                        slot.agent = Some(replacement);
                                    }
                                    // Ctrl+C when idle is a no-op (use Ctrl+Q to quit)
                                }
                                _ => {}
                            }
                        }
                        KeyCode::Tab => {
                            // Cycle panel focus
                            let mut app = app.lock().unwrap();
                            app.focused_panel = match app.focused_panel {
                                Panel::Response => Panel::Logs,
                                Panel::Logs => Panel::Response,
                            };
                        }
                        KeyCode::Up => {
                            let mut app = app.lock().unwrap();
                            let panel = app.focused_panel;
                            app.scroll_panel_up(panel, 3);
                        }
                        KeyCode::Down => {
                            let mut app = app.lock().unwrap();
                            let panel = app.focused_panel;
                            app.scroll_panel_down(panel, 3);
                        }
                        KeyCode::Left => {
                            app.lock().unwrap().move_cursor_left();
                        }
                        KeyCode::Right => {
                            app.lock().unwrap().move_cursor_right();
                        }
                        KeyCode::Home => {
                            app.lock().unwrap().move_cursor_home();
                        }
                        KeyCode::End => {
                            app.lock().unwrap().move_cursor_end();
                        }
                        KeyCode::Delete => {
                            app.lock().unwrap().delete_char_at();
                        }
                        KeyCode::Char(c) => {
                            app.lock().unwrap().insert_char(c);
                        }
                        KeyCode::Backspace => {
                            app.lock().unwrap().delete_char_before();
                        }
                        KeyCode::Enter => {
                            // Peek agent availability BEFORE consuming input, so
                            // we never lose the user's text when the background
                            // thread from an interrupted turn hasn't finished yet.
                            let agent_ready = agent.lock().unwrap().agent.is_some();
                            let still_running = app.lock().unwrap().is_running;
                            if !agent_ready || still_running {
                                app.lock().unwrap().push_msg(
                                    MsgKind::Error,
                                    "⚠ Previous turn still finishing — please wait…",
                                );
                                continue;
                            }

                            let input = {
                                let mut app = app.lock().unwrap();
                                app.take_input()
                            };

                            if input == "q" || input == "exit" {
                                info!("user exit");
                                break;
                            }

                            if !input.is_empty() {
                                let turn_id = {
                                    let current_messages = agent
                                        .lock()
                                        .unwrap()
                                        .agent
                                        .as_ref()
                                        .map(|agent| agent.messages().to_vec())
                                        .unwrap_or_default();
                                    *turn_checkpoint.lock().unwrap() = current_messages;

                                    let mut app = app.lock().unwrap();
                                    if !app.output_msgs.is_empty() {
                                        app.push_msg(MsgKind::Separator, &"─".repeat(40));
                                    }
                                    app.push_msg(MsgKind::User, &format!("> {}", input));
                                    app.begin_turn()
                                };

                                {
                                    let mut slot = agent.lock().unwrap();
                                    slot.turn_id = turn_id;
                                }

                                // Take agent out of the shared slot.
                                // We pass the Arc itself into the thread so the agent
                                // can be put back when the turn completes, enabling
                                // subsequent turns to work correctly.
                                let agent_slot = agent.clone();
                                let mut agent_val = match agent.lock().unwrap().agent.take() {
                                    Some(a) => a,
                                    None => {
                                        // agent_ready check above should prevent this
                                        app.lock().unwrap().is_running = false;
                                        continue;
                                    }
                                };

                                // Two separate tx clones: one for the callback, one for results/done
                                let tx_callback = tx.clone();
                                let tx_result = tx.clone();
                                // Capture the tokio Handle from the current async context,
                                // then run the future in a plain thread using block_on.
                                let handle = Handle::current();
                                thread::spawn(move || {
                                    // Add user message
                                    agent_val.add_user_message(&input);

                                    // Run the async future using the captured handle
                                    let result = handle.block_on(agent_val.run_loop_with(
                                        move |name, tool_input, output| {
                                            if name == "bash" {
                                                if let Some(cmd) = tool_input["command"].as_str() {
                                                    let log = format!("$ {}", cmd);
                                                    let _ = tx_callback
                                                        .send(LogUpdate::ToolLog { turn_id, log });
                                                }
                                            }
                                            // Send tool output
                                            let preview = if output.len() > 100 {
                                                format!("{}...", &output[..100])
                                            } else {
                                                output.to_string()
                                            };
                                            let _ = tx_callback.send(LogUpdate::ToolLog {
                                                turn_id,
                                                log: preview,
                                            });
                                        },
                                    ));

                                    match result {
                                        Ok(text) => {
                                            if !text.is_empty() {
                                                let _ = tx_result
                                                    .send(LogUpdate::Output { turn_id, text });
                                            }
                                        }
                                        Err(e) => {
                                            error!(error = %e, "agent loop error");
                                            let _ = tx_result.send(LogUpdate::Output {
                                                turn_id,
                                                text: format!("Error: {e}"),
                                            });
                                        }
                                    }

                                    // Only restore the agent if this worker still belongs to the
                                    // active turn. Interrupted/stale workers must never overwrite
                                    // the fresh checkpoint-restored agent or a newer running turn.
                                    let mut slot = agent_slot.lock().unwrap();
                                    if slot.turn_id == turn_id {
                                        slot.agent = Some(agent_val);
                                    }

                                    let _ = tx_result.send(LogUpdate::Done { turn_id });
                                });
                            }
                        }
                        _ => {}
                    }
                }
                Event::Mouse(mouse) => {
                    use crossterm::event::MouseEventKind::*;
                    let mut app = app.lock().unwrap();
                    match mouse.kind {
                        ScrollUp => {
                            let panel = app.panel_at(mouse.column);
                            app.scroll_panel_up(panel, 3);
                        }
                        ScrollDown => {
                            let panel = app.panel_at(mouse.column);
                            app.scroll_panel_down(panel, 3);
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }

    // Drop app before restoring terminal
    drop(app);

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    info!("omega exiting");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interrupt_turn_invalidates_old_updates() {
        let mut app = App::new();
        let turn_id = app.begin_turn();
        app.interrupt_turn();

        app.apply_log_update(LogUpdate::Output {
            turn_id,
            text: "stale".to_string(),
        });
        app.apply_log_update(LogUpdate::Done { turn_id });

        assert!(app.output_msgs.is_empty());
        assert!(!app.is_running);
    }

    #[test]
    fn current_turn_updates_are_applied() {
        let mut app = App::new();
        let turn_id = app.begin_turn();

        app.apply_log_update(LogUpdate::ToolLog {
            turn_id,
            log: "$ echo hi".to_string(),
        });
        app.apply_log_update(LogUpdate::Output {
            turn_id,
            text: "hello".to_string(),
        });
        app.apply_log_update(LogUpdate::Done { turn_id });

        assert_eq!(app.output_msgs.len(), 2);
        assert!(!app.is_running);
    }
}
