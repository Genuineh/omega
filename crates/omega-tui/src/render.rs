use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    Frame,
};

use omega_keymap::InteractionMode;

use crate::app::{App, MsgKind, Panel};
use crate::overlay::{overlay_area, ConfirmChoice, OverlayState};
use crate::sidebar::SidebarSection;

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
    let sidebar_pct: u16 = if term_width < 60 || app.sidebar.shell_collapsed {
        0
    } else if term_width < 100 {
        34
    } else {
        40
    };
    let resp_pct = 100 - sidebar_pct;

    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(resp_pct),
            Constraint::Percentage(sidebar_pct),
        ])
        .split(chunks[1]);

    app.response_rect = main_chunks[0];
    app.sidebar_rect = main_chunks[1];
    app.sidebar_rail_rect = ratatui::layout::Rect::default();
    app.todo_rect = ratatui::layout::Rect::default();
    app.logs_rect = ratatui::layout::Rect::default();
    app.normalize_focus();
    app.normalize_mode();

    let focus_label = if app.overlay_active() {
        "Overlay"
    } else {
        match app.focused_panel {
            Panel::Response => "Response",
            Panel::SidebarRail => "Sidebar rail",
            Panel::Todo => "Todos",
            Panel::Logs => "Logs",
        }
    };
    let mode_label = match app.interaction_mode {
        InteractionMode::Normal => "Normal",
        InteractionMode::Insert => "Insert",
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
    let todo_status = app.todo_status_text();
    let sidebar_status = app.sidebar_badge_text();
    let status_text = format!(
        " Omega Agent │ {} │ {} │ Mode: {} │ Focus: {} │ KM: {} │ {} │ {} ",
        model_name,
        agent_state,
        mode_label,
        focus_label,
        app.keymap_source,
        todo_status,
        sidebar_status
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
    let sidebar_border = if app.focused_panel == Panel::SidebarRail {
        Style::default()
            .fg(colors.focus_border)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(colors.border_dim)
    };
    let todo_border = if app.focused_panel == Panel::Todo {
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

    if app.sidebar_rect.width > 0 && app.sidebar_rect.height > 0 {
        let sidebar_title = if app.focused_panel == Panel::SidebarRail {
            " Sidebar ◆ "
        } else {
            " Sidebar "
        };
        let sidebar_block = Block::default()
            .title(sidebar_title)
            .borders(Borders::ALL)
            .border_style(sidebar_border);
        let sidebar_inner = sidebar_block.inner(app.sidebar_rect);
        frame.render_widget(sidebar_block, app.sidebar_rect);

        let sidebar_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(0)])
            .split(sidebar_inner);
        app.sidebar_rail_rect = sidebar_chunks[0];

        render_sidebar_rail(frame, app, &colors, sidebar_chunks[0]);
        render_sidebar_body(
            frame,
            app,
            &colors,
            sidebar_chunks[1],
            todo_border,
            logs_border,
        );
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
    match app.interaction_mode {
        InteractionMode::Normal => {
            if app.input_buffer.is_empty() {
                spans.push(Span::styled(
                    "Press Space jk to enter insert mode",
                    Style::default().fg(colors.hint_dim),
                ));
            } else {
                for ch in chars.iter().skip(scroll_offset).take(avail_w) {
                    spans.push(Span::styled(
                        ch.to_string(),
                        Style::default().fg(colors.hint_dim),
                    ));
                }
            }
        }
        InteractionMode::Insert => {
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
        }
    }

    let input_title = match (app.interaction_mode, app.is_running) {
        (InteractionMode::Normal, true) => " Input [Normal | Running…] ",
        (InteractionMode::Normal, false) => " Input [Normal] ",
        (InteractionMode::Insert, true) => " Input [Insert | Running…] ",
        (InteractionMode::Insert, false) => " Input [Insert] ",
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

    let hint_text = if app.overlay_active() {
        overlay_hint_text(app)
    } else if app.is_leader_pending() {
        " Leader pending: jk=Toggle mode  Tab=Focus  ↑/↓=Scroll  c=Interrupt  q=Quit  Esc=Cancel"
    } else if let Some(notice) = app.status_notice.as_deref() {
        notice
    } else if main_chunks[1].width == 0 {
        match app.interaction_mode {
            InteractionMode::Normal => {
                " Sidebar hidden. Space=Leader  Space jk=Toggle mode  Space Tab=Focus  Space b=Sidebar  Space /=Search  Space ↑/↓=Scroll"
            }
            InteractionMode::Insert => {
                " Sidebar hidden below 60 cols. Enter=Send  Space jk=Toggle mode  ←→/Home/End=Cursor  Del/Backspace=Delete"
            }
        }
    } else {
        match app.interaction_mode {
            InteractionMode::Normal => {
                if app.focused_panel == Panel::SidebarRail {
                    " Sidebar rail: ←/→ cycle  Enter focus  x collapse  Space b=Toggle sidebar  Space Tab=Next focus"
                } else {
                    " Space=Leader  Space jk=Toggle mode  Space Tab=Focus  Space b=Sidebar  Space /=Search  Space ↑/↓=Scroll"
                }
            }
            InteractionMode::Insert => {
                " Enter=Send  Space jk=Toggle mode  ←→/Home/End=Cursor  Del/Backspace=Delete"
            }
        }
    };
    let hint =
        Paragraph::new(hint_text).style(Style::default().fg(colors.hint_dim).bg(colors.status_bar));
    frame.render_widget(hint, chunks[3]);

    render_overlay(frame, app, &colors);
}

fn render_sidebar_rail(
    frame: &mut Frame,
    app: &mut App,
    colors: &ColorScheme,
    area: ratatui::layout::Rect,
) {
    let sections = [SidebarSection::Todos, SidebarSection::Logs];
    let mut spans = Vec::new();

    for (index, section) in sections.into_iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled(" | ", Style::default().fg(colors.border_dim)));
        }

        let selected = app.sidebar.rail_selection == section;
        let expanded = app.sidebar.is_expanded(section);
        let marker = if expanded { "▾" } else { "▸" };
        let style = if selected {
            Style::default()
                .fg(colors.focus_border)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(colors.text)
        };
        spans.push(Span::styled(
            format!("{marker} {} {}", section.label(), app.rail_badge(section)),
            style,
        ));
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_sidebar_body(
    frame: &mut Frame,
    app: &mut App,
    colors: &ColorScheme,
    area: ratatui::layout::Rect,
    todo_border: Style,
    logs_border: Style,
) {
    app.todo_rect = ratatui::layout::Rect::default();
    app.logs_rect = ratatui::layout::Rect::default();

    let sections = match (app.sidebar.todos_expanded, app.sidebar.logs_expanded) {
        (true, true) => Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
            .split(area)
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        (true, false) | (false, true) => Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(100)])
            .split(area)
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        (false, false) => Vec::new(),
    };

    let mut next_index = 0;
    if app.sidebar.todos_expanded {
        let rect = sections.get(next_index).copied().unwrap_or_default();
        next_index += 1;
        app.todo_rect = rect;
        let todo_title = app.todo_panel_title();
        let todo_inner_w = (rect.width as usize).saturating_sub(2).max(1);
        let todo_items: Vec<ListItem> = app
            .todo_lines
            .iter()
            .flat_map(|line| wrap_text(line, todo_inner_w).into_iter().map(ListItem::new))
            .collect();
        let todo_total = todo_items.len();
        app.todo_displayed_count = todo_total;
        if !app.todo_pinned && todo_total > 0 {
            app.todo_state.select(Some(todo_total - 1));
        }
        let todo_list = List::new(todo_items)
            .block(
                Block::default()
                    .title(todo_title)
                    .borders(Borders::ALL)
                    .border_style(todo_border),
            )
            .highlight_style(Style::default())
            .style(Style::default().fg(colors.text));
        frame.render_stateful_widget(todo_list, rect, &mut app.todo_state);
    } else {
        app.todo_displayed_count = 0;
    }

    if app.sidebar.logs_expanded {
        let rect = sections.get(next_index).copied().unwrap_or_default();
        app.logs_rect = rect;
        let logs_title = app.logs_panel_title();
        let logs_inner_w = (rect.width as usize).saturating_sub(2).max(1);
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
        frame.render_stateful_widget(log_list, rect, &mut app.logs_state);
    } else {
        app.logs_displayed_count = 0;
    }
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

fn render_overlay(frame: &mut Frame, app: &mut App, colors: &ColorScheme) {
    let Some(overlay) = app.overlay.as_ref() else {
        app.overlay_rect = ratatui::layout::Rect::default();
        return;
    };

    let full_area = frame.area();
    let overlay_rect = overlay_area(full_area, overlay.size());
    app.overlay_rect = overlay_rect;

    let mask = Block::default().style(
        Style::default()
            .bg(Color::Rgb(12, 12, 12))
            .add_modifier(Modifier::DIM),
    );
    frame.render_widget(mask, full_area);
    frame.render_widget(Clear, overlay_rect);

    match overlay {
        OverlayState::Search(search) => {
            let inner = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Length(1),
                    Constraint::Length(1),
                    Constraint::Min(1),
                ])
                .split(overlay_rect);
            let block = Block::default()
                .title(" Search ")
                .borders(Borders::ALL)
                .border_style(
                    Style::default()
                        .fg(colors.focus_border)
                        .add_modifier(Modifier::BOLD),
                )
                .style(Style::default().bg(colors.input_bg));
            frame.render_widget(block, overlay_rect);

            let input = Paragraph::new(Line::from(render_overlay_input(
                search.query.as_str(),
                search.cursor_pos,
                colors,
            )))
            .style(Style::default().fg(colors.input_text).bg(colors.input_bg));
            frame.render_widget(input, inner[0]);

            let (panel, count) = app
                .panel_search_match_count()
                .unwrap_or((search.target_panel, 0));
            let panel_name = match panel {
                Panel::Response => "Response",
                Panel::SidebarRail => "Sidebar",
                Panel::Todo => "Todos",
                Panel::Logs => "Logs",
            };
            frame.render_widget(
                Paragraph::new(format!(" Panel: {panel_name}"))
                    .style(Style::default().fg(colors.text).bg(colors.input_bg)),
                inner[1],
            );
            frame.render_widget(
                Paragraph::new(format!(
                    " Matches: {count} (highlight/jump lands in Task 15B-11)"
                ))
                .style(Style::default().fg(colors.hint_dim).bg(colors.input_bg)),
                inner[2],
            );
        }
        OverlayState::Confirm(confirm) => {
            let inner = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(2),
                    Constraint::Length(1),
                    Constraint::Length(1),
                ])
                .split(overlay_rect);
            let block = Block::default()
                .title(confirm.title.as_str())
                .borders(Borders::ALL)
                .border_style(
                    Style::default()
                        .fg(colors.focus_border)
                        .add_modifier(Modifier::BOLD),
                )
                .style(Style::default().bg(colors.input_bg));
            frame.render_widget(block, overlay_rect);
            frame.render_widget(
                Paragraph::new(confirm.message.as_str())
                    .style(Style::default().fg(colors.text).bg(colors.input_bg)),
                inner[0],
            );
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    button_span(
                        confirm.selected == ConfirmChoice::Cancel,
                        &confirm.cancel_label,
                    ),
                    Span::raw("  "),
                    button_span(
                        confirm.selected == ConfirmChoice::Confirm,
                        &confirm.confirm_label,
                    ),
                ]))
                .style(Style::default().bg(colors.input_bg)),
                inner[2],
            );
        }
        OverlayState::Detail(detail) => {
            let block = Block::default()
                .title(detail.title.as_str())
                .borders(Borders::ALL)
                .border_style(
                    Style::default()
                        .fg(colors.focus_border)
                        .add_modifier(Modifier::BOLD),
                )
                .style(Style::default().bg(colors.input_bg));
            let inner = block.inner(overlay_rect);
            frame.render_widget(block, overlay_rect);
            let items: Vec<ListItem> = detail
                .lines
                .iter()
                .skip(detail.scroll)
                .map(|line| ListItem::new(line.clone()))
                .collect();
            frame.render_widget(
                List::new(items).style(Style::default().fg(colors.text).bg(colors.input_bg)),
                inner,
            );
        }
        OverlayState::Picker(picker) => {
            let block = Block::default()
                .title(picker.title.as_str())
                .borders(Borders::ALL)
                .border_style(
                    Style::default()
                        .fg(colors.focus_border)
                        .add_modifier(Modifier::BOLD),
                )
                .style(Style::default().bg(colors.input_bg));
            let inner = block.inner(overlay_rect);
            frame.render_widget(block, overlay_rect);
            let items: Vec<ListItem> = picker
                .items
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    let prefix = if index == picker.selected {
                        "› "
                    } else {
                        "  "
                    };
                    let style = if index == picker.selected {
                        Style::default()
                            .fg(colors.focus_border)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(colors.text)
                    };
                    ListItem::new(Span::styled(format!("{prefix}{item}"), style))
                })
                .collect();
            frame.render_widget(
                List::new(items).style(Style::default().bg(colors.input_bg)),
                inner,
            );
        }
        OverlayState::InputPrompt(prompt) => {
            let inner = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1),
                    Constraint::Length(3),
                    Constraint::Min(1),
                ])
                .split(overlay_rect);
            let block = Block::default()
                .title(prompt.title.as_str())
                .borders(Borders::ALL)
                .border_style(
                    Style::default()
                        .fg(colors.focus_border)
                        .add_modifier(Modifier::BOLD),
                )
                .style(Style::default().bg(colors.input_bg));
            frame.render_widget(block, overlay_rect);
            frame.render_widget(
                Paragraph::new(prompt.prompt.as_str())
                    .style(Style::default().fg(colors.text).bg(colors.input_bg)),
                inner[0],
            );
            frame.render_widget(
                Paragraph::new(Line::from(render_overlay_input(
                    prompt.value.as_str(),
                    prompt.cursor_pos,
                    colors,
                )))
                .style(Style::default().fg(colors.input_text).bg(colors.input_bg)),
                inner[1],
            );
        }
    }
}

fn render_overlay_input(
    value: &str,
    cursor_pos: usize,
    colors: &ColorScheme,
) -> Vec<Span<'static>> {
    let chars: Vec<char> = value.chars().collect();
    let mut spans = vec![Span::styled(" > ", Style::default().fg(colors.input_text))];

    if chars.is_empty() {
        spans.push(Span::styled(" ", Style::default().bg(colors.input_text)));
        return spans;
    }

    for (index, ch) in chars.iter().enumerate() {
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

    if cursor_pos == chars.len() {
        spans.push(Span::styled(" ", Style::default().bg(colors.input_text)));
    }

    spans
}

fn button_span(selected: bool, label: &str) -> Span<'static> {
    let style = if selected {
        Style::default()
            .fg(Color::Rgb(30, 30, 30))
            .bg(Color::Rgb(78, 201, 176))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Rgb(212, 212, 212))
    };
    Span::styled(format!("[ {label} ]"), style)
}

fn overlay_hint_text(app: &App) -> &'static str {
    match app.overlay.as_ref() {
        Some(OverlayState::Search(_)) => {
            " Search popup: type to filter the focused panel  Enter=keep query  Esc=Close"
        }
        Some(OverlayState::Confirm(_)) => {
            " Confirm dialog: ←/→/Tab switch  Enter accepts selected action  Esc=Cancel"
        }
        Some(OverlayState::Detail(_)) => " Detail dialog: ↑/↓ scroll  Esc=Close",
        Some(OverlayState::Picker(_)) => " Picker popup: ↑/↓/Tab move  Enter=Select  Esc=Close",
        Some(OverlayState::InputPrompt(_)) => " Input prompt: type freely  Enter=Submit  Esc=Close",
        None => "",
    }
}

#[cfg(test)]
mod tests {
    use ratatui::{backend::TestBackend, Terminal};

    use crate::app::{App, Panel};

    use super::{render, wrap_text};

    #[test]
    fn wraps_unicode_text_by_character_width() {
        assert_eq!(wrap_text("你好世界", 2), vec!["你好", "世界"]);
    }

    #[test]
    fn collapsed_sidebar_hides_sections_and_restores_response_focus() {
        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new();
        app.sidebar.shell_collapsed = true;
        app.focused_panel = Panel::SidebarRail;

        terminal
            .draw(|frame| render(frame, &mut app, "test-model"))
            .unwrap();

        assert_eq!(app.focused_panel, Panel::Response);
        assert_eq!(app.sidebar_rect.width, 0);
        assert_eq!(app.todo_rect.width, 0);
        assert_eq!(app.logs_rect.width, 0);
    }

    #[test]
    fn single_activity_section_occupies_sidebar_body() {
        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new();
        app.sidebar.todos_expanded = false;
        app.sidebar.logs_expanded = true;

        terminal
            .draw(|frame| render(frame, &mut app, "test-model"))
            .unwrap();

        assert_eq!(app.todo_rect.height, 0);
        assert!(app.logs_rect.height > 0);
        assert_eq!(
            app.logs_rect.height + app.sidebar_rail_rect.height,
            app.sidebar_rect.height - 2
        );
    }

    #[test]
    fn narrow_terminal_forces_sidebar_hidden() {
        let backend = TestBackend::new(58, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new();
        app.focused_panel = Panel::Todo;

        terminal
            .draw(|frame| render(frame, &mut app, "test-model"))
            .unwrap();

        assert_eq!(app.focused_panel, Panel::Response);
        assert_eq!(app.sidebar_rect.width, 0);
    }
}
