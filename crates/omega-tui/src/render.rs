use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::{Line, Span},
    Frame,
};

use omega_keymap::InteractionMode;
use omega_session::ResponseSectionState;
use omega_theme::{OmegaTheme, RenderPalette as ColorScheme};

use crate::app::{
    wrap_text_segments, App, MsgKind, Panel, ResponseDisplayLine, SessionRoutingSummary,
    SessionStatusSummary, ThinkingLineKind,
};
use crate::overlay::{overlay_area, ConfirmChoice, OverlayState};
use crate::sidebar::SidebarSection;

pub fn render(frame: &mut Frame, app: &mut App, model_name: &str, theme: &OmegaTheme) {
    let colors = theme.render_palette();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(1),
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
        .split(chunks[0]);

    app.response_rect = main_chunks[0];
    app.input_context_rect = chunks[1];
    app.input_gap_rect = ratatui::layout::Rect::default();
    app.input_rect = chunks[2];
    app.sidebar_rect = main_chunks[1];
    app.sidebar_rail_rect = ratatui::layout::Rect::default();
    app.todo_rect = ratatui::layout::Rect::default();
    app.logs_rect = ratatui::layout::Rect::default();
    app.bottom_status_rect = chunks[3];
    app.normalize_focus();
    app.normalize_mode();

    const SPINNER_FRAMES: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
    let status = Paragraph::new(bottom_status_line(app, model_name, SPINNER_FRAMES, &colors))
        .style(Style::default().bg(colors.status_bar_bg));
    frame.render_widget(status, chunks[3]);

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
    let diagnostics_border = if app.focused_panel == Panel::Diagnostics {
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
    let app_ref: &App = &*app;
    let resp_inner_w = (main_chunks[0].width as usize).saturating_sub(2).max(1);
    let response_lines = app_ref.response_display_lines();
    let output_items: Vec<ListItem> = response_lines
        .iter()
        .enumerate()
        .flat_map(|(line_index, line)| {
            let style = response_line_style(line, &colors);
            wrap_text_segments(&line.text, resp_inner_w)
                .into_iter()
                .map(move |(source_column_start, wrapped)| {
                    list_item_with_selection(
                        &wrapped,
                        style,
                        app_ref.selection_range_for_segment(
                            Panel::Response,
                            line_index,
                            source_column_start,
                            source_column_start + wrapped.chars().count(),
                        ),
                    )
                })
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
                .border_type(colors.panel_border_type)
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
            .border_type(colors.panel_border_type)
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
            diagnostics_border,
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
                    Style::default().fg(colors.input_placeholder),
                ));
            } else {
                for ch in chars.iter().skip(scroll_offset).take(avail_w) {
                    spans.push(Span::styled(
                        ch.to_string(),
                        Style::default().fg(colors.input_placeholder),
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

    let input_border_color = match app.interaction_mode {
        InteractionMode::Normal => colors.mode_normal_fg,
        InteractionMode::Insert => colors.mode_insert_fg,
    };

    let input = Paragraph::new(Line::from(spans))
        .style(Style::default().bg(colors.input_bg))
        .block(
            Block::default()
                .border_type(colors.input_border_type)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(input_border_color)),
        );
    frame.render_widget(input, chunks[2]);

    let context = Paragraph::new(input_context_line(app, main_chunks[1].width == 0, &colors))
        .style(Style::default().bg(colors.context_bar_bg));
    frame.render_widget(context, chunks[1]);

    render_overlay(frame, app, &colors);
}

fn response_line_style(line: &ResponseDisplayLine, colors: &ColorScheme) -> Style {
    match line.kind {
        MsgKind::User => Style::default().fg(colors.user_message),
        MsgKind::Agent => Style::default().fg(colors.agent_message),
        MsgKind::Error => Style::default().fg(colors.error_message),
        MsgKind::Separator => Style::default().fg(colors.separator_message),
        MsgKind::Routing => {
            if line.is_header {
                Style::default()
                    .fg(colors.context_label)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(colors.context_hint)
            }
        }
        MsgKind::Step => {
            if line.is_tool_line {
                match line.tool_status {
                    None => Style::default()
                        .fg(colors.context_label)
                        .add_modifier(Modifier::BOLD),
                    Some(omega_session::ToolRunStatus::Running) => {
                        Style::default().fg(colors.focus_border)
                    }
                    Some(omega_session::ToolRunStatus::Failed) => {
                        Style::default().fg(colors.error_message)
                    }
                    _ => Style::default().fg(colors.context_hint),
                }
            } else if line.is_header {
                Style::default()
                    .fg(colors.focus_border)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(colors.agent_message)
            }
        }
        MsgKind::FinalAnswer => {
            if line.is_tool_line {
                match line.tool_status {
                    None => Style::default()
                        .fg(colors.context_label)
                        .add_modifier(Modifier::BOLD),
                    Some(omega_session::ToolRunStatus::Running) => {
                        Style::default().fg(colors.focus_border)
                    }
                    Some(omega_session::ToolRunStatus::Failed) => {
                        Style::default().fg(colors.error_message)
                    }
                    _ => Style::default().fg(colors.context_hint),
                }
            } else if line.is_header {
                Style::default()
                    .fg(colors.mode_insert_fg)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(colors.text)
            }
        }
        MsgKind::Thinking => {
            let state = line
                .response_state
                .unwrap_or(ResponseSectionState::Complete);
            if line.is_header {
                thinking_header_style(state, colors)
            } else {
                match line.thinking_line_kind {
                    Some(ThinkingLineKind::Summary) => thinking_summary_style(state, colors),
                    Some(ThinkingLineKind::Placeholder) => {
                        thinking_placeholder_style(state, colors)
                    }
                    _ => thinking_body_style(state, colors),
                }
            }
        }
    }
}

fn thinking_header_style(state: ResponseSectionState, colors: &ColorScheme) -> Style {
    match state {
        ResponseSectionState::Streaming => Style::default()
            .fg(colors.focus_border)
            .add_modifier(Modifier::BOLD),
        ResponseSectionState::Complete => Style::default()
            .fg(colors.context_label)
            .add_modifier(Modifier::BOLD),
        ResponseSectionState::Failed => Style::default()
            .fg(colors.error_message)
            .add_modifier(Modifier::BOLD),
    }
}

fn thinking_summary_style(state: ResponseSectionState, colors: &ColorScheme) -> Style {
    match state {
        ResponseSectionState::Streaming => Style::default()
            .fg(colors.focus_border)
            .add_modifier(Modifier::BOLD),
        ResponseSectionState::Complete => Style::default()
            .fg(colors.context_label)
            .add_modifier(Modifier::BOLD),
        ResponseSectionState::Failed => Style::default()
            .fg(colors.error_message)
            .add_modifier(Modifier::BOLD),
    }
}

fn thinking_body_style(state: ResponseSectionState, colors: &ColorScheme) -> Style {
    match state {
        ResponseSectionState::Failed => Style::default().fg(colors.error_message),
        ResponseSectionState::Streaming | ResponseSectionState::Complete => {
            Style::default().fg(colors.context_hint)
        }
    }
}

fn thinking_placeholder_style(state: ResponseSectionState, colors: &ColorScheme) -> Style {
    thinking_body_style(state, colors).add_modifier(Modifier::ITALIC)
}

fn input_context_text(app: &App, sidebar_hidden: bool) -> &str {
    if app.overlay_active() {
        overlay_hint_text(app)
    } else if app.is_leader_pending() {
        " Leader pending: jk=Toggle mode  Tab=Focus  ↑/↓=Scroll  c=Interrupt  q=Quit  Esc=Cancel"
    } else if let Some(notice) = app.status_notice.as_deref() {
        notice
    } else if sidebar_hidden {
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
                } else if app.focused_panel == Panel::Diagnostics {
                    " Diagnostics: Enter/x=Open detail  Space Tab=Focus  Space b=Sidebar  Space /=Search  Space ↑/↓=Scroll"
                } else if app.focused_panel == Panel::Response && app.show_thinking {
                    " Response: Enter/x=Toggle thinking or open tool detail  Space Tab=Focus  Space b=Sidebar  Space /=Search  Space ↑/↓=Scroll"
                } else {
                    " Space=Leader  Space jk=Toggle mode  Space Tab=Focus  Space b=Sidebar  Space /=Search  Space ↑/↓=Scroll"
                }
            }
            InteractionMode::Insert => {
                " Enter=Send  Space jk=Toggle mode  ←→/Home/End=Cursor  Del/Backspace=Delete"
            }
        }
    }
}

fn input_context_line(app: &App, sidebar_hidden: bool, colors: &ColorScheme) -> Line<'static> {
    let hint_val = input_context_text(app, sidebar_hidden).to_string();

    Line::from(vec![
        Span::styled(
            " keys ",
            Style::default()
                .fg(colors.context_label)
                .bg(colors.context_bar_bg),
        ),
        Span::styled(
            hint_val,
            Style::default()
                .fg(colors.context_hint)
                .bg(colors.context_bar_bg),
        ),
    ])
}

#[cfg(test)]
fn bottom_status_text(app: &App, model_name: &str, spinner_frames: &[char]) -> String {
    let segments = bottom_status_segments(app, model_name, spinner_frames);
    let mut rendered = vec![segments.model, segments.state];
    if let Some(flow) = segments.flow {
        rendered.push(flow);
    }
    if let Some(aux) = segments.aux {
        rendered.push(aux.value);
    }

    format!(" {} ", rendered.join(" │ "))
}

fn bottom_status_line(
    app: &App,
    model_name: &str,
    spinner_frames: &[char],
    colors: &ColorScheme,
) -> Line<'static> {
    let segments = bottom_status_segments(app, model_name, spinner_frames);
    let runtime_active = app.is_running;
    let mode_text = match app.interaction_mode {
        InteractionMode::Normal => "NORMAL",
        InteractionMode::Insert => "INSERT",
    };
    let mode_color = match app.interaction_mode {
        InteractionMode::Normal => colors.mode_normal_fg,
        InteractionMode::Insert => colors.mode_insert_fg,
    };

    let mut spans = vec![
        Span::styled(
            " mode ",
            Style::default()
                .fg(colors.status_label)
                .bg(colors.status_bar_bg),
        ),
        Span::styled(
            mode_text,
            Style::default()
                .fg(mode_color)
                .bg(colors.status_bar_bg)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "  ·  ",
            Style::default()
                .fg(colors.bar_divider)
                .bg(colors.status_bar_bg),
        ),
        Span::styled(
            " model ",
            Style::default()
                .fg(colors.status_label)
                .bg(colors.status_bar_bg),
        ),
        Span::styled(
            segments.model,
            Style::default()
                .fg(colors.text)
                .bg(colors.status_bar_bg)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "  ·  ",
            Style::default()
                .fg(colors.bar_divider)
                .bg(colors.status_bar_bg),
        ),
        Span::styled(
            " state ",
            Style::default()
                .fg(colors.status_label)
                .bg(colors.status_bar_bg),
        ),
        Span::styled(
            segments.state,
            Style::default()
                .fg(if runtime_active {
                    colors.status_running_fg
                } else {
                    colors.status_idle_fg
                })
                .bg(colors.status_bar_bg)
                .add_modifier(Modifier::BOLD),
        ),
    ];

    if let Some(flow) = segments.flow {
        spans.push(Span::styled(
            "  ·  ",
            Style::default()
                .fg(colors.bar_divider)
                .bg(colors.status_bar_bg),
        ));
        spans.push(Span::styled(
            " flow ",
            Style::default()
                .fg(colors.status_label)
                .bg(colors.status_bar_bg),
        ));
        spans.push(Span::styled(
            flow,
            Style::default()
                .fg(colors.focus_border)
                .bg(colors.status_bar_bg)
                .add_modifier(Modifier::BOLD),
        ));
    }

    if let Some(aux) = segments.aux {
        spans.push(Span::styled(
            "  ·  ",
            Style::default()
                .fg(colors.bar_divider)
                .bg(colors.status_bar_bg),
        ));
        spans.push(Span::styled(
            format!(" {} ", aux.label),
            Style::default()
                .fg(colors.status_label)
                .bg(colors.status_bar_bg),
        ));
        spans.push(Span::styled(
            aux.value,
            Style::default()
                .fg(colors.text)
                .bg(colors.status_bar_bg)
                .add_modifier(Modifier::BOLD),
        ));
    }

    Line::from(spans)
}

struct BottomStatusSegments {
    model: String,
    state: String,
    flow: Option<String>,
    aux: Option<BottomStatusBadge>,
}

struct BottomStatusBadge {
    label: &'static str,
    value: String,
}

fn bottom_status_segments(
    app: &App,
    model_name: &str,
    spinner_frames: &[char],
) -> BottomStatusSegments {
    let spinner_char = spinner_frames[(app.spinner_tick as usize / 2) % spinner_frames.len()];
    let state = if app.is_running {
        format!("{spinner_char} Running…")
    } else if let Some(label) = app.agent_status_label.as_deref() {
        if label == "Idle" {
            "● Idle".to_string()
        } else {
            label.to_string()
        }
    } else {
        "● Idle".to_string()
    };

    let flow = if app.is_running {
        app.workflow_summary.as_ref().map(|workflow| {
            format!(
                "{}:{} {} {}/{}",
                workflow.workflow_role.as_str(),
                workflow.workflow_id,
                workflow.label,
                workflow.index,
                workflow.total
            )
        })
    } else {
        None
    };

    let aux = match app.session_status.as_ref() {
        Some(SessionStatusSummary::Label(label)) => Some(BottomStatusBadge {
            label: "session",
            value: label.clone(),
        }),
        Some(SessionStatusSummary::Routing(routing)) => Some(BottomStatusBadge {
            label: "route",
            value: format_routing_badge(routing),
        }),
        None => None,
    };

    BottomStatusSegments {
        model: model_name.to_string(),
        state,
        flow,
        aux,
    }
}

fn format_routing_badge(routing: &SessionRoutingSummary) -> String {
    match (
        routing.recognized_scene_id.as_deref(),
        routing.selected_workflow_id.as_deref(),
    ) {
        (None, None) => format!(
            "{} via {}",
            routing.active_workflow_role.as_str(),
            routing.root_workflow_id
        ),
        (Some(scene_id), None) => format!("{} -> selecting", scene_id),
        (Some(scene_id), Some(workflow_id)) => format!("{} -> {}", scene_id, workflow_id),
        (None, Some(workflow_id)) => format!("pending -> {}", workflow_id),
    }
}

fn render_sidebar_rail(
    frame: &mut Frame,
    app: &mut App,
    colors: &ColorScheme,
    area: ratatui::layout::Rect,
) {
    let sections = [
        SidebarSection::Diagnostics,
        SidebarSection::Todos,
        SidebarSection::Logs,
    ];
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
    diagnostics_border: Style,
    todo_border: Style,
    logs_border: Style,
) {
    app.diagnostics_rect = ratatui::layout::Rect::default();
    app.todo_rect = ratatui::layout::Rect::default();
    app.logs_rect = ratatui::layout::Rect::default();

    let expanded_sections = [
        app.sidebar.diagnostics_expanded,
        app.sidebar.todos_expanded,
        app.sidebar.logs_expanded,
    ]
    .into_iter()
    .filter(|expanded| *expanded)
    .count();
    let sections = if expanded_sections == 0 {
        Vec::new()
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![Constraint::Ratio(1, expanded_sections as u32); expanded_sections])
            .split(area)
            .iter()
            .copied()
            .collect::<Vec<_>>()
    };

    let mut next_index = 0;
    if app.sidebar.diagnostics_expanded {
        let rect = sections.get(next_index).copied().unwrap_or_default();
        next_index += 1;
        app.diagnostics_rect = rect;
        let diagnostics_title = app.diagnostics_panel_title();
        let diagnostics_inner_w = (rect.width as usize).saturating_sub(2).max(1);
        let app_ref: &App = &*app;
        let diagnostics_items: Vec<ListItem> = app_ref
            .wrapped_panel_lines(Panel::Diagnostics, diagnostics_inner_w)
            .into_iter()
            .map(|line| {
                let line_len = line.text.chars().count();
                list_item_with_selection(
                    &line.text,
                    Style::default().fg(colors.text),
                    app_ref.selection_range_for_segment(
                        Panel::Diagnostics,
                        line.source_line_index,
                        line.source_column_start,
                        line.source_column_start + line_len,
                    ),
                )
            })
            .collect();
        let diagnostics_total = diagnostics_items.len();
        app.diagnostics_displayed_count = diagnostics_total;
        if !app.diagnostics_pinned && diagnostics_total > 0 {
            app.diagnostics_state.select(Some(diagnostics_total - 1));
        }
        let diagnostics_list = List::new(diagnostics_items)
            .block(
                Block::default()
                    .border_type(colors.panel_border_type)
                    .title(diagnostics_title)
                    .borders(Borders::ALL)
                    .border_style(diagnostics_border),
            )
            .highlight_style(Style::default())
            .style(Style::default().fg(colors.text));
        frame.render_stateful_widget(diagnostics_list, rect, &mut app.diagnostics_state);
    } else {
        app.diagnostics_displayed_count = 0;
    }

    if app.sidebar.todos_expanded {
        let rect = sections.get(next_index).copied().unwrap_or_default();
        next_index += 1;
        app.todo_rect = rect;
        let todo_title = app.todo_panel_title();
        let todo_inner_w = (rect.width as usize).saturating_sub(2).max(1);
        let app_ref: &App = &*app;
        let todo_items: Vec<ListItem> = app_ref
            .wrapped_panel_lines(Panel::Todo, todo_inner_w)
            .into_iter()
            .map(|line| {
                let line_len = line.text.chars().count();
                list_item_with_selection(
                    &line.text,
                    Style::default().fg(colors.text),
                    app_ref.selection_range_for_segment(
                        Panel::Todo,
                        line.source_line_index,
                        line.source_column_start,
                        line.source_column_start + line_len,
                    ),
                )
            })
            .collect();
        let todo_total = todo_items.len();
        app.todo_displayed_count = todo_total;
        if !app.todo_pinned && todo_total > 0 {
            app.todo_state.select(Some(todo_total - 1));
        }
        let todo_list = List::new(todo_items)
            .block(
                Block::default()
                    .border_type(colors.panel_border_type)
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
        let app_ref: &App = &*app;
        let log_items: Vec<ListItem> = app_ref
            .wrapped_panel_lines(Panel::Logs, logs_inner_w)
            .into_iter()
            .map(|line| {
                let line_len = line.text.chars().count();
                list_item_with_selection(
                    &line.text,
                    Style::default().fg(colors.text),
                    app_ref.selection_range_for_segment(
                        Panel::Logs,
                        line.source_line_index,
                        line.source_column_start,
                        line.source_column_start + line_len,
                    ),
                )
            })
            .collect();
        let logs_total = log_items.len();
        app.logs_displayed_count = logs_total;
        if !app.logs_pinned && logs_total > 0 {
            app.logs_state.select(Some(logs_total - 1));
        }
        let log_list = List::new(log_items)
            .block(
                Block::default()
                    .border_type(colors.panel_border_type)
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

#[cfg(test)]
fn wrap_text(line: &str, width: usize) -> Vec<String> {
    wrap_text_segments(line, width)
        .into_iter()
        .map(|(_, segment)| segment)
        .collect()
}

fn list_item_with_selection(
    text: &str,
    base_style: Style,
    selection: Option<(usize, usize)>,
) -> ListItem<'static> {
    let Some((selection_start, selection_end)) = selection else {
        return ListItem::new(Span::styled(text.to_string(), base_style));
    };

    let text_len = text.chars().count();
    if selection_start >= selection_end || selection_start >= text_len {
        return ListItem::new(Span::styled(text.to_string(), base_style));
    }

    let selection_end = selection_end.min(text_len);
    let before: String = text.chars().take(selection_start).collect();
    let selected: String = text
        .chars()
        .skip(selection_start)
        .take(selection_end - selection_start)
        .collect();
    let after: String = text.chars().skip(selection_end).collect();

    let mut spans = Vec::new();
    if !before.is_empty() {
        spans.push(Span::styled(before, base_style));
    }
    if !selected.is_empty() {
        spans.push(Span::styled(
            selected,
            base_style.add_modifier(Modifier::REVERSED),
        ));
    }
    if !after.is_empty() {
        spans.push(Span::styled(after, base_style));
    }

    if spans.is_empty() {
        spans.push(Span::styled(String::new(), base_style));
    }

    ListItem::new(Line::from(spans))
}

fn render_overlay(frame: &mut Frame, app: &mut App, colors: &ColorScheme) {
    let Some(overlay) = app.overlay.as_ref() else {
        app.overlay_rect = ratatui::layout::Rect::default();
        return;
    };

    let full_area = frame.area();
    let overlay_rect = overlay_area(full_area, overlay.size());
    app.overlay_rect = overlay_rect;

    let mask = Block::default()
        .border_type(colors.overlay_border_type)
        .style(
            Style::default()
                .bg(colors.overlay_mask_bg)
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
                .border_type(colors.overlay_border_type)
                .title(" Search ")
                .borders(Borders::ALL)
                .border_style(
                    Style::default()
                        .fg(colors.focus_border)
                        .add_modifier(Modifier::BOLD),
                )
                .style(Style::default().bg(colors.overlay_bg));
            frame.render_widget(block, overlay_rect);

            let input = Paragraph::new(Line::from(render_overlay_input(
                search.query.as_str(),
                search.cursor_pos,
                colors,
            )))
            .style(Style::default().fg(colors.input_text).bg(colors.overlay_bg));
            frame.render_widget(input, inner[0]);

            let (panel, count) = app
                .panel_search_match_count()
                .unwrap_or((search.target_panel, 0));
            let panel_name = match panel {
                Panel::Response => "Response",
                Panel::SidebarRail => "Sidebar",
                Panel::Diagnostics => "Diagnostics",
                Panel::Todo => "Todos",
                Panel::Logs => "Logs",
            };
            frame.render_widget(
                Paragraph::new(format!(" Panel: {panel_name}"))
                    .style(Style::default().fg(colors.text).bg(colors.overlay_bg)),
                inner[1],
            );
            frame.render_widget(
                Paragraph::new(format!(
                    " Matches: {count} (highlight/jump lands in Task 15B-11)"
                ))
                .style(
                    Style::default()
                        .fg(colors.context_hint)
                        .bg(colors.overlay_bg),
                ),
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
                .border_type(colors.overlay_border_type)
                .title(confirm.title.as_str())
                .borders(Borders::ALL)
                .border_style(
                    Style::default()
                        .fg(colors.focus_border)
                        .add_modifier(Modifier::BOLD),
                )
                .style(Style::default().bg(colors.overlay_bg));
            frame.render_widget(block, overlay_rect);
            frame.render_widget(
                Paragraph::new(confirm.message.as_str())
                    .style(Style::default().fg(colors.text).bg(colors.overlay_bg)),
                inner[0],
            );
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    button_span(
                        confirm.selected == ConfirmChoice::Cancel,
                        &confirm.cancel_label,
                        colors,
                    ),
                    Span::raw("  "),
                    button_span(
                        confirm.selected == ConfirmChoice::Confirm,
                        &confirm.confirm_label,
                        colors,
                    ),
                ]))
                .style(Style::default().bg(colors.overlay_bg)),
                inner[2],
            );
        }
        OverlayState::Detail(detail) => {
            let block = Block::default()
                .border_type(colors.overlay_border_type)
                .title(detail.title.as_str())
                .borders(Borders::ALL)
                .border_style(
                    Style::default()
                        .fg(colors.focus_border)
                        .add_modifier(Modifier::BOLD),
                )
                .style(Style::default().bg(colors.overlay_bg));
            let inner = block.inner(overlay_rect);
            frame.render_widget(block, overlay_rect);
            let items: Vec<ListItem> = detail
                .lines
                .iter()
                .skip(detail.scroll)
                .map(|line| ListItem::new(line.clone()))
                .collect();
            frame.render_widget(
                List::new(items).style(Style::default().fg(colors.text).bg(colors.overlay_bg)),
                inner,
            );
        }
        OverlayState::Picker(picker) => {
            let block = Block::default()
                .border_type(colors.overlay_border_type)
                .title(picker.title.as_str())
                .borders(Borders::ALL)
                .border_style(
                    Style::default()
                        .fg(colors.focus_border)
                        .add_modifier(Modifier::BOLD),
                )
                .style(Style::default().bg(colors.overlay_bg));
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
                List::new(items).style(Style::default().bg(colors.overlay_bg)),
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
                .border_type(colors.overlay_border_type)
                .title(prompt.title.as_str())
                .borders(Borders::ALL)
                .border_style(
                    Style::default()
                        .fg(colors.focus_border)
                        .add_modifier(Modifier::BOLD),
                )
                .style(Style::default().bg(colors.overlay_bg));
            frame.render_widget(block, overlay_rect);
            frame.render_widget(
                Paragraph::new(prompt.prompt.as_str())
                    .style(Style::default().fg(colors.text).bg(colors.overlay_bg)),
                inner[0],
            );
            frame.render_widget(
                Paragraph::new(Line::from(render_overlay_input(
                    prompt.value.as_str(),
                    prompt.cursor_pos,
                    colors,
                )))
                .style(Style::default().fg(colors.input_text).bg(colors.overlay_bg)),
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

fn button_span(selected: bool, label: &str, colors: &ColorScheme) -> Span<'static> {
    let style = if selected {
        Style::default()
            .fg(colors.overlay_button_selected_fg)
            .bg(colors.overlay_button_selected_bg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(colors.overlay_button_fg)
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
    use omega_session::ResponseSectionState;
    use omega_theme::OmegaTheme;
    use ratatui::{
        backend::TestBackend,
        style::{Modifier, Style},
        Terminal,
    };

    use crate::app::{
        App, MsgKind, Panel, ResponseDisplayLine, SessionRoutingSummary, SessionStatusSummary,
        ThinkingLineKind,
    };

    use super::{
        bottom_status_line, bottom_status_text, input_context_line, input_context_text, render,
        response_line_style, wrap_text,
    };

    #[test]
    fn wraps_unicode_text_by_character_width() {
        assert_eq!(wrap_text("你好世界", 2), vec!["你好", "世界"]);
    }

    #[test]
    fn collapsed_sidebar_hides_sections_and_restores_response_focus() {
        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new();
        let theme = OmegaTheme::dark();
        app.sidebar.shell_collapsed = true;
        app.focused_panel = Panel::SidebarRail;

        terminal
            .draw(|frame| render(frame, &mut app, "test-model", &theme))
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
        let theme = OmegaTheme::dark();
        app.sidebar.diagnostics_expanded = false;
        app.sidebar.todos_expanded = false;
        app.sidebar.logs_expanded = true;

        terminal
            .draw(|frame| render(frame, &mut app, "test-model", &theme))
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
        let theme = OmegaTheme::dark();
        app.focused_panel = Panel::Todo;

        terminal
            .draw(|frame| render(frame, &mut app, "test-model", &theme))
            .unwrap();

        assert_eq!(app.focused_panel, Panel::Response);
        assert_eq!(app.sidebar_rect.width, 0);
    }

    #[test]
    fn input_context_and_bottom_status_bars_have_stable_heights() {
        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new();
        let theme = OmegaTheme::dark();

        terminal
            .draw(|frame| render(frame, &mut app, "test-model", &theme))
            .unwrap();

        assert_eq!(app.response_rect.y, 0);
        assert_eq!(app.input_context_rect.height, 1);
        assert_eq!(app.input_gap_rect.height, 0);
        assert_eq!(app.input_rect.height, 3);
        assert_eq!(app.bottom_status_rect.height, 1);
        assert_eq!(
            app.input_rect.y,
            app.input_context_rect.y + app.input_context_rect.height
        );
        assert!(app.input_context_rect.y < app.bottom_status_rect.y);
    }

    #[test]
    fn thinking_lines_use_stateful_styles() {
        let colors = OmegaTheme::dark().render_palette();

        let header = ResponseDisplayLine {
            kind: MsgKind::Thinking,
            text: "  reasoning  child:chat  Reasoning live  [streaming]".to_string(),
            is_header: true,
            message_id: Some("thinking-1".to_string()),
            action: None,
            is_tool_line: false,
            tool_status: None,
            response_state: Some(ResponseSectionState::Streaming),
            thinking_line_kind: None,
        };
        let summary = ResponseDisplayLine {
            kind: MsgKind::Thinking,
            text: "    = reasoning · 2 lines · outline answer".to_string(),
            is_header: false,
            message_id: Some("thinking-1".to_string()),
            action: None,
            is_tool_line: false,
            tool_status: None,
            response_state: Some(ResponseSectionState::Complete),
            thinking_line_kind: Some(ThinkingLineKind::Summary),
        };
        let failed_body = ResponseDisplayLine {
            kind: MsgKind::Thinking,
            text: "    | tool result mismatched".to_string(),
            is_header: false,
            message_id: Some("thinking-2".to_string()),
            action: None,
            is_tool_line: false,
            tool_status: None,
            response_state: Some(ResponseSectionState::Failed),
            thinking_line_kind: Some(ThinkingLineKind::Body),
        };

        assert_eq!(
            response_line_style(&header, &colors),
            Style::default()
                .fg(colors.focus_border)
                .add_modifier(Modifier::BOLD)
        );
        assert_eq!(
            response_line_style(&summary, &colors),
            Style::default()
                .fg(colors.context_label)
                .add_modifier(Modifier::BOLD)
        );
        assert_eq!(
            response_line_style(&failed_body, &colors),
            Style::default().fg(colors.error_message)
        );
    }

    #[test]
    fn bottom_status_keeps_model_and_runtime_without_old_header_fields() {
        let mut app = App::new();
        app.is_running = true;
        app.spinner_tick = 3;
        app.workflow_summary = Some(crate::app::WorkflowSummary {
            workflow_id: "feature".to_string(),
            workflow_role: omega_session::WorkflowRunRole::Child,
            id: "analysis".to_string(),
            label: "Analyze".to_string(),
            index: 1,
            total: 4,
        });

        let text = bottom_status_text(&app, "test-model", &['⠋', '⠙']);

        assert!(text.contains("test-model"));
        assert!(text.contains("Running…"));
        assert!(text.contains("child:feature Analyze 1/4"));
        assert!(!text.contains("Omega Agent"));
        assert!(!text.contains("Mode:"));
        assert!(!text.contains("Focus:"));
        assert!(!text.contains("KM:"));
    }

    #[test]
    fn leader_and_notice_text_live_in_input_context_bar() {
        let mut app = App::new();

        app.set_status_notice("Context notice");
        assert_eq!(input_context_text(&app, false), "Context notice");

        app.leader_pending_since = Some(std::time::Instant::now());
        assert!(input_context_text(&app, false).contains("Leader pending"));
    }

    #[test]
    fn input_surfaces_use_symmetric_visual_bars() {
        let mut app = App::new();
        app.is_running = true;

        let colors = OmegaTheme::dark().render_palette();
        let context = input_context_line(&app, false, &colors);
        let status = bottom_status_line(&app, "test-model", &['⠋', '⠙'], &colors);

        assert_eq!(context.spans[0].style.fg, Some(colors.context_label));
        assert_eq!(context.spans[0].style.bg, Some(colors.context_bar_bg));
        assert_eq!(context.spans[0].content, " keys ");
        assert_eq!(status.spans[0].style.bg, Some(colors.status_bar_bg));
        assert_eq!(status.spans[0].content, " mode ");
        assert_eq!(status.spans[1].style.fg, Some(colors.mode_normal_fg));
        assert_eq!(status.spans[7].style.fg, Some(colors.status_running_fg));
        assert_eq!(colors.context_bar_bg, colors.status_bar_bg);
        assert_eq!(colors.input_bg, colors.context_bar_bg);
    }

    #[test]
    fn idle_bottom_status_hides_workflow_segment() {
        let mut app = App::new();
        app.workflow_summary = Some(crate::app::WorkflowSummary {
            workflow_id: "feature".to_string(),
            workflow_role: omega_session::WorkflowRunRole::Child,
            id: "report".to_string(),
            label: "Report".to_string(),
            index: 4,
            total: 4,
        });

        let text = bottom_status_text(&app, "test-model", &['⠋', '⠙']);

        assert!(!text.contains("Report 4/4"));
        assert!(text.contains("● Idle"));
    }

    #[test]
    fn bottom_status_renders_session_slot_when_present() {
        let mut app = App::new();
        app.session_status = Some(SessionStatusSummary::Routing(SessionRoutingSummary {
            root_workflow_id: "root".to_string(),
            active_workflow_id: "feature".to_string(),
            active_workflow_role: omega_session::WorkflowRunRole::Child,
            recognized_scene_id: Some("feature".to_string()),
            selected_workflow_id: Some("feature".to_string()),
        }));

        let text = bottom_status_text(&app, "test-model", &['⠋', '⠙']);

        assert!(text.contains("feature -> feature"));
    }
}
