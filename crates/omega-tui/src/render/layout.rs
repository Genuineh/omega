use ratatui::text::Span;
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::Line,
    Frame,
};

use omega_keymap::InteractionMode;
use omega_theme::{OmegaTheme, RenderPalette as ColorScheme};

use crate::app::{wrap_text_segments, App, Panel};
use crate::render::markdown::StyledSpan;

use super::overlay::render_overlay;
use super::sidebar::{list_item_with_selection, render_sidebar_body, render_sidebar_rail};
use super::status::{bottom_status_line, input_context_line};
use super::style::response_line_style;

pub(crate) fn render(frame: &mut Frame, app: &mut App, model_name: &str, theme: &OmegaTheme) {
    let colors = theme.render_palette();
    app.cached_palette = Some(colors);

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
    app.input_gap_rect = Rect::default();
    app.input_rect = chunks[2];
    app.sidebar_rect = main_chunks[1];
    app.sidebar_rail_rect = Rect::default();
    app.todo_rect = Rect::default();
    app.logs_rect = Rect::default();
    app.bottom_status_rect = chunks[3];
    app.normalize_focus();
    app.normalize_mode();

    const SPINNER_FRAMES: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
    let status = Paragraph::new(bottom_status_line(app, model_name, SPINNER_FRAMES, &colors))
        .style(Style::default().bg(colors.status_bar_bg));
    frame.render_widget(status, chunks[3]);

    let response_border = panel_border_style(app.focused_panel == Panel::Response, &colors);
    let sidebar_border = panel_border_style(app.focused_panel == Panel::SidebarRail, &colors);
    let todo_border = panel_border_style(app.focused_panel == Panel::Todo, &colors);
    let diagnostics_border = panel_border_style(app.focused_panel == Panel::Diagnostics, &colors);
    let logs_border = panel_border_style(app.focused_panel == Panel::Logs, &colors);

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
            let fallback_style = response_line_style(line, &colors);
            if line.spans.is_empty() {
                // Legacy path: single-style text
                wrap_text_segments(&line.text, resp_inner_w)
                    .into_iter()
                    .map(move |(source_column_start, wrapped)| {
                        list_item_with_selection(
                            &wrapped,
                            fallback_style,
                            app_ref.selection_range_for_segment(
                                Panel::Response,
                                line_index,
                                source_column_start,
                                source_column_start + wrapped.chars().count(),
                            ),
                        )
                    })
                    .collect::<Vec<_>>()
            } else {
                wrap_styled_spans(&line.spans, resp_inner_w)
                    .into_iter()
                    .map(|wrapped_spans| {
                        let ratatui_spans: Vec<Span<'static>> = wrapped_spans
                            .into_iter()
                            .map(|span| Span::styled(span.text, span.style))
                            .collect();
                        ListItem::new(Line::from(ratatui_spans))
                    })
                    .collect::<Vec<_>>()
            }
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
    let mut spans = vec![ratatui::text::Span::styled(
        prefix,
        Style::default().fg(colors.input_text),
    )];
    match app.interaction_mode {
        InteractionMode::Normal => {
            if app.input_buffer.is_empty() {
                spans.push(ratatui::text::Span::styled(
                    "Press Space jk to enter insert mode",
                    Style::default().fg(colors.input_placeholder),
                ));
            } else {
                for ch in chars.iter().skip(scroll_offset).take(avail_w) {
                    spans.push(ratatui::text::Span::styled(
                        ch.to_string(),
                        Style::default().fg(colors.input_placeholder),
                    ));
                }
            }
        }
        InteractionMode::Insert => {
            if app.input_buffer.is_empty() {
                spans.push(ratatui::text::Span::styled(
                    " ",
                    Style::default().bg(colors.input_text),
                ));
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
                    spans.push(ratatui::text::Span::styled(ch.to_string(), style));
                }
                if cursor_pos == char_count && (char_count - scroll_offset) < avail_w {
                    spans.push(ratatui::text::Span::styled(
                        " ",
                        Style::default().bg(colors.input_text),
                    ));
                }
            }
        }
    }

    let input_border_color = match app.interaction_mode {
        InteractionMode::Normal => colors.mode_normal_fg,
        InteractionMode::Insert => colors.mode_insert_fg,
    };

    let input = Paragraph::new(ratatui::text::Line::from(spans))
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

fn wrap_styled_spans(spans: &[StyledSpan], width: usize) -> Vec<Vec<StyledSpan>> {
    if width == 0 {
        return vec![spans.to_vec()];
    }
    if spans.is_empty() {
        return vec![Vec::new()];
    }

    let mut lines: Vec<Vec<StyledSpan>> = Vec::new();
    let mut current_line: Vec<StyledSpan> = Vec::new();
    let mut current_width = 0usize;

    for span in spans {
        if span.text.is_empty() {
            if current_line.is_empty() {
                current_line.push(span.clone());
            }
            continue;
        }

        let chars: Vec<char> = span.text.chars().collect();
        let mut start = 0usize;
        while start < chars.len() {
            if current_width == width {
                lines.push(current_line);
                current_line = Vec::new();
                current_width = 0;
            }

            let take = (width - current_width).min(chars.len() - start);
            let text: String = chars[start..start + take].iter().collect();
            current_line.push(StyledSpan {
                text,
                style: span.style,
            });
            current_width += take;
            start += take;

            if current_width == width {
                lines.push(current_line);
                current_line = Vec::new();
                current_width = 0;
            }
        }
    }

    if current_line.is_empty() {
        if lines.is_empty() {
            lines.push(Vec::new());
        }
    } else {
        lines.push(current_line);
    }

    lines
}

fn panel_border_style(selected: bool, colors: &ColorScheme) -> Style {
    if selected {
        Style::default()
            .fg(colors.focus_border)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(colors.border_dim)
    }
}
