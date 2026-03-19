use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    Frame,
};

use crate::app::{App, MsgKind, Panel};

pub fn render(frame: &mut Frame, app: &mut App, model_name: &str) {
    let colors = ColorScheme::dark();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(frame.area());

    let term_width = frame.area().width;
    let (resp_pct, logs_pct): (u16, u16) = if term_width < 60 {
        (100, 0)
    } else if term_width < 100 {
        (70, 30)
    } else {
        (60, 40)
    };

    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(resp_pct),
            Constraint::Percentage(logs_pct),
        ])
        .split(chunks[1]);

    app.response_rect = main_chunks[0];
    app.logs_rect = main_chunks[1];

    let focus_label = match app.focused_panel {
        Panel::Response => "Response",
        Panel::Logs => "Logs",
    };
    const SPINNER_FRAMES: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
    let spinner_char = SPINNER_FRAMES[(app.spinner_tick as usize / 2) % SPINNER_FRAMES.len()];
    let agent_state_owned;
    let agent_state = if app.is_running {
        agent_state_owned = format!("{spinner_char} Running…");
        agent_state_owned.as_str()
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

    let response_title = if app.focused_panel == Panel::Response {
        " Agent Response ◆ "
    } else {
        " Agent Response "
    };
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

    let logs_title = if app.focused_panel == Panel::Logs {
        " Logs ◆ "
    } else {
        " Logs "
    };
    let logs_inner_w = (main_chunks[1].width as usize).saturating_sub(2).max(1);
    let log_items: Vec<ListItem> = app
        .log_lines
        .iter()
        .flat_map(|line| wrap_text(line, logs_inner_w).into_iter().map(ListItem::new))
        .collect();
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

    let chars: Vec<char> = app.input_buffer.chars().collect();
    let cursor_pos = app.cursor_pos;
    let char_count = chars.len();
    let avail_w = (chunks[2].width as usize).saturating_sub(5).max(1);
    let scroll_offset = if cursor_pos < avail_w {
        0
    } else {
        cursor_pos - avail_w + 1
    };

    let prefix = if scroll_offset > 0 { "◂> " } else { " > " };
    let mut spans = vec![Span::styled(prefix, Style::default().fg(colors.input_text))];
    if app.input_buffer.is_empty() {
        spans.push(Span::styled(" ", Style::default().bg(colors.input_text)));
    } else {
        for (index, ch) in chars.iter().enumerate().skip(scroll_offset).take(avail_w) {
            let style = if index == cursor_pos {
                Style::default()
                    .fg(colors.input_bg)
                    .bg(colors.input_text)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(colors.input_text)
            };
            spans.push(Span::styled(ch.to_string(), style));
        }
        if cursor_pos == char_count && (char_count - scroll_offset) < avail_w {
            spans.push(Span::styled(" ", Style::default().bg(colors.input_text)));
        }
    }

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

    let hint_text = " Tab=Focus  ↑↓=Scroll  ←→=Cursor  Del=Delete  Ctrl+C=Interrupt  Ctrl+Q=Quit";
    let hint =
        Paragraph::new(hint_text).style(Style::default().fg(colors.hint_dim).bg(colors.status_bar));
    frame.render_widget(hint, chunks[3]);
}

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

struct ColorScheme {
    text: Color,
    border: Color,
    border_dim: Color,
    focus_border: Color,
    status_bar: Color,
    input_bg: Color,
    input_text: Color,
    hint_dim: Color,
    command: Color,
}

impl ColorScheme {
    fn dark() -> Self {
        Self {
            text: Color::Rgb(212, 212, 212),
            border: Color::Rgb(62, 62, 62),
            border_dim: Color::Rgb(48, 48, 48),
            focus_border: Color::Rgb(78, 201, 176),
            status_bar: Color::Rgb(45, 45, 45),
            input_bg: Color::Rgb(40, 40, 40),
            input_text: Color::Rgb(86, 156, 214),
            hint_dim: Color::Rgb(100, 100, 100),
            command: Color::Rgb(220, 220, 170),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::wrap_text;

    #[test]
    fn wraps_unicode_text_by_character_width() {
        assert_eq!(wrap_text("你好世界", 2), vec!["你好", "世界"]);
    }
}
