use ratatui::text::Span;
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::Line,
    Frame,
};

use omega_keymap::InteractionMode;
use omega_theme::{OmegaTheme, RenderPalette as ColorScheme};

use crate::app::{App, Panel, ResponseDisplayLine};
use crate::render::markdown::StyledSpan;

use super::overlay::render_overlay;
use super::sidebar::{render_sidebar_body, render_sidebar_rail};
use super::status::{bottom_status_line, input_context_line, input_info_line};
use super::style::{response_line_style, response_status_symbol_style};

const INPUT_PROMPT_PREFIX: &str = " > ";
const INPUT_CONTINUATION_PREFIX: &str = "   ";

pub(crate) fn render(frame: &mut Frame, app: &mut App, model_name: &str, theme: &OmegaTheme) {
    let colors = theme.render_palette();
    app.cached_palette = Some(colors);
    app.remember_delivery_model_name(model_name);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(frame.area());

    let term_width = frame.area().width;
    let sidebar_pct: u16 = if term_width < 60 || app.sidebar.shell_collapsed {
        0
    } else if term_width < 100 {
        30
    } else {
        34
    };
    let resp_pct = 100 - sidebar_pct;

    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(resp_pct),
            Constraint::Percentage(sidebar_pct),
        ])
        .split(chunks[0]);

    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(2),
            Constraint::Length(9),
        ])
        .split(main_chunks[0]);

    app.response_rect = left_chunks[0];
    app.input_context_rect = left_chunks[1];
    app.input_gap_rect = Rect::default();
    app.input_rect = Rect::default();
    app.input_info_rect = Rect::default();
    app.sidebar_rect = main_chunks[1];
    app.sidebar_rail_rect = Rect::default();
    app.todo_rect = Rect::default();
    app.delivery_rect = Rect::default();
    app.document_rect = Rect::default();
    app.memory_rect = Rect::default();
    app.logs_rect = Rect::default();
    app.bottom_status_rect = chunks[1];
    app.normalize_mode();

    const SPINNER_FRAMES: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
    let status = Paragraph::new(bottom_status_line(app, model_name, SPINNER_FRAMES, &colors))
        .style(Style::default().fg(colors.text).bg(colors.status_bar_bg));
    frame.render_widget(status, chunks[1]);

    let response_border = panel_border_style(app.focused_panel == Panel::Response, &colors);
    let sidebar_border = panel_border_style(app.focused_panel == Panel::SidebarRail, &colors);

    let response_title = if app.focused_panel == Panel::Response {
        " Agent Response ◆ "
    } else {
        " Agent Response "
    };
    let app_ref: &App = &*app;
    let resp_inner_w = (left_chunks[0].width as usize).saturating_sub(2).max(1);
    let response_lines = app_ref.response_display_lines();
    let output_items: Vec<ListItem> = response_lines
        .iter()
        .enumerate()
        .flat_map(|(line_index, line)| {
            let fallback_style = response_line_style(line, &colors);
            let selection = app_ref.selection_range_for_segment(
                Panel::Response,
                line_index,
                0,
                line.text.chars().count(),
            );
            let source_spans = response_display_spans(line, fallback_style, &colors);
            let selected_spans = apply_selection_to_styled_spans(&source_spans, selection);

            wrap_styled_spans(&selected_spans, resp_inner_w)
                .into_iter()
                .map(|wrapped_spans| {
                    let ratatui_spans: Vec<Span<'static>> = wrapped_spans
                        .into_iter()
                        .map(|span| Span::styled(span.text, span.style))
                        .collect();
                    ListItem::new(Line::from(ratatui_spans))
                })
                .collect::<Vec<_>>()
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
                .title(Line::styled(
                    response_title,
                    panel_title_style(
                        app.focused_panel == Panel::Response,
                        colors.title_fg,
                        colors.context_hint,
                        colors.panel_bg,
                    ),
                ))
                .borders(Borders::ALL)
                .border_style(response_border)
                .style(Style::default().bg(colors.panel_bg)),
        )
        .highlight_style(Style::default())
        .style(panel_content_style(
            app.focused_panel == Panel::Response,
            colors.text,
            colors.panel_bg,
        ));
    frame.render_stateful_widget(output_list, left_chunks[0], &mut app.response_state);

    if app.sidebar_rect.width > 0 && app.sidebar_rect.height > 0 {
        let sidebar_title = if app.focused_panel == Panel::SidebarRail {
            "Sidebar ◆"
        } else {
            "Sidebar"
        };
        let sidebar_block = Block::default()
            .border_type(colors.panel_border_type)
            .title(Line::styled(
                sidebar_title,
                panel_title_style(
                    app.focused_panel == Panel::SidebarRail,
                    colors.title_fg,
                    colors.context_hint,
                    colors.sidebar_bg,
                ),
            ))
            .borders(Borders::ALL)
            .border_style(sidebar_border)
            .style(Style::default().bg(colors.sidebar_bg));
        let sidebar_inner = sidebar_block.inner(app.sidebar_rect);
        frame.render_widget(sidebar_block, app.sidebar_rect);

        let sidebar_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Length(1), Constraint::Min(0)])
            .split(sidebar_inner);
        app.sidebar_rail_rect = sidebar_chunks[0];

        render_sidebar_rail(frame, app, &colors, sidebar_chunks[0]);
        frame.render_widget(
            Paragraph::new("").style(Style::default().bg(colors.sidebar_bg)),
            sidebar_chunks[1],
        );
        render_sidebar_body(frame, app, &colors, sidebar_chunks[2]);
    }
    // normalize_focus after sidebar rects are fully populated (or zeroed if sidebar hidden)
    app.normalize_focus();

    let input_border_color = match app.interaction_mode {
        InteractionMode::Normal => colors.mode_normal_fg,
        InteractionMode::Insert => colors.mode_insert_fg,
    };
    let input_shell = Block::default()
        .border_type(colors.input_border_type)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(input_border_color))
        .style(Style::default().bg(colors.input_bg));
    let input_shell_rect = left_chunks[2];
    let input_inner = input_shell.inner(input_shell_rect);
    frame.render_widget(input_shell, input_shell_rect);

    let input_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(input_inner);
    app.input_rect = input_chunks[0];
    app.input_info_rect = Rect {
        x: input_chunks[2].x.saturating_add(1),
        y: input_chunks[2].y,
        width: input_chunks[2].width.saturating_sub(2),
        height: input_chunks[2].height,
    };

    let input = Paragraph::new(input_viewport_lines(app, &colors))
        .style(Style::default().fg(colors.text).bg(colors.input_bg));
    frame.render_widget(input, app.input_rect);

    let context = Paragraph::new(input_context_line(app, main_chunks[1].width == 0, &colors))
        .wrap(Wrap { trim: false })
        .style(Style::default().fg(colors.text).bg(colors.context_bar_bg));
    frame.render_widget(context, left_chunks[1]);

    let input_info = Paragraph::new(input_info_line(
        app,
        SPINNER_FRAMES,
        &colors,
        app.input_info_rect.width as usize,
    ))
    .style(Style::default().fg(colors.text).bg(colors.input_bg));
    frame.render_widget(input_info, app.input_info_rect);

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

pub(super) fn input_viewport_lines(app: &App, colors: &ColorScheme) -> Vec<Line<'static>> {
    let visible_height = app.input_rect.height as usize;
    if visible_height == 0 {
        return Vec::new();
    }

    let content_width = (app.input_rect.width as usize)
        .saturating_sub(INPUT_PROMPT_PREFIX.chars().count())
        .max(1);
    let lines = build_input_lines(app, colors, content_width);
    let start = app.input_viewport_top(lines.len());

    lines
        .into_iter()
        .skip(start)
        .take(visible_height)
        .collect()
}

fn build_input_lines(
    app: &App,
    colors: &ColorScheme,
    content_width: usize,
) -> Vec<Line<'static>> {
    let prompt_style = Style::default().fg(colors.input_text);
    let text_style = match app.interaction_mode {
        InteractionMode::Normal => Style::default().fg(colors.input_placeholder),
        InteractionMode::Insert => Style::default().fg(colors.input_text),
    };
    let placeholder_style = Style::default().fg(colors.input_placeholder);
    let cursor_style = Style::default()
        .fg(colors.input_bg)
        .bg(colors.input_text)
        .add_modifier(Modifier::BOLD);

    let mut lines = vec![input_line_prefix(true, prompt_style)];
    let mut current_width = 0usize;

    if app.input_buffer.is_empty() {
        match app.interaction_mode {
            InteractionMode::Normal => append_input_text(
                &mut lines,
                "Press Space jk to enter insert mode",
                placeholder_style,
                content_width,
                prompt_style,
                &mut current_width,
            ),
            InteractionMode::Insert => append_input_cell(
                &mut lines,
                " ".to_string(),
                cursor_style,
                content_width,
                prompt_style,
                &mut current_width,
            ),
        }

        return lines.into_iter().map(Line::from).collect();
    }

    let chars: Vec<char> = app.input_buffer.chars().collect();
    for (index, character) in chars.iter().enumerate() {
        if index == app.cursor_pos {
            if app.interaction_mode == InteractionMode::Insert {
                append_input_cell(
                    &mut lines,
                    " ".to_string(),
                    cursor_style,
                    content_width,
                    prompt_style,
                    &mut current_width,
                );
            }
        }

        if *character == '\n' {
            lines.push(input_line_prefix(false, prompt_style));
            current_width = 0;
            continue;
        }

        append_input_cell(
            &mut lines,
            character.to_string(),
            text_style,
            content_width,
            prompt_style,
            &mut current_width,
        );
    }

    if app.cursor_pos == chars.len() {
        if app.interaction_mode == InteractionMode::Insert {
            append_input_cell(
                &mut lines,
                " ".to_string(),
                cursor_style,
                content_width,
                prompt_style,
                &mut current_width,
            );
        }
    }

    lines.into_iter().map(Line::from).collect()
}

fn input_line_prefix(is_first: bool, style: Style) -> Vec<Span<'static>> {
    vec![Span::styled(
        if is_first {
            INPUT_PROMPT_PREFIX.to_string()
        } else {
            INPUT_CONTINUATION_PREFIX.to_string()
        },
        style,
    )]
}

fn append_input_text(
    lines: &mut Vec<Vec<Span<'static>>>,
    text: &str,
    style: Style,
    content_width: usize,
    prompt_style: Style,
    current_width: &mut usize,
) {
    for character in text.chars() {
        append_input_cell(
            lines,
            character.to_string(),
            style,
            content_width,
            prompt_style,
            current_width,
        );
    }
}

fn append_input_cell(
    lines: &mut Vec<Vec<Span<'static>>>,
    text: String,
    style: Style,
    content_width: usize,
    prompt_style: Style,
    current_width: &mut usize,
) {
    if *current_width == content_width {
        lines.push(input_line_prefix(false, prompt_style));
        *current_width = 0;
    }

    if lines.is_empty() {
        lines.push(input_line_prefix(true, prompt_style));
    }

    lines
        .last_mut()
        .expect("input viewport always has at least one line")
        .push(Span::styled(text, style));
    *current_width += 1;
}

fn response_display_spans(
    line: &ResponseDisplayLine,
    fallback_style: Style,
    colors: &ColorScheme,
) -> Vec<StyledSpan> {
    if !line.spans.is_empty() {
        return line.spans.clone();
    }

    let Some((start, end)) = find_status_symbol_range(&line.text) else {
        return vec![StyledSpan {
            text: line.text.clone(),
            style: fallback_style,
        }];
    };
    let Some(symbol_style) = response_status_symbol_style(line, colors) else {
        return vec![StyledSpan {
            text: line.text.clone(),
            style: fallback_style,
        }];
    };

    let mut spans = Vec::new();
    if start > 0 {
        spans.push(StyledSpan {
            text: line.text[..start].to_string(),
            style: fallback_style,
        });
    }
    spans.push(StyledSpan {
        text: line.text[start..end].to_string(),
        style: symbol_style,
    });
    if end < line.text.len() {
        spans.push(StyledSpan {
            text: line.text[end..].to_string(),
            style: fallback_style,
        });
    }
    spans
}

fn find_status_symbol_range(text: &str) -> Option<(usize, usize)> {
    for symbol in ["◉", "●", "✕", "◦"] {
        for (start, _) in text.match_indices(symbol) {
            let end = start + symbol.len();
            let before = &text[..start];
            let after = &text[end..];
            if before.ends_with("  ") && (after.is_empty() || after.starts_with("  ")) {
                return Some((start, end));
            }
        }
    }
    None
}

fn apply_selection_to_styled_spans(
    spans: &[StyledSpan],
    selection: Option<(usize, usize)>,
) -> Vec<StyledSpan> {
    let Some((selection_start, selection_end)) = selection else {
        return spans.to_vec();
    };
    if selection_start >= selection_end {
        return spans.to_vec();
    }

    let mut output = Vec::new();
    let mut current = 0usize;
    for span in spans {
        let span_len = span.text.chars().count();
        let span_start = current;
        let span_end = current + span_len;
        current = span_end;

        if span_len == 0 {
            output.push(span.clone());
            continue;
        }
        if selection_end <= span_start || selection_start >= span_end {
            output.push(span.clone());
            continue;
        }

        let local_start = selection_start.saturating_sub(span_start).min(span_len);
        let local_end = selection_end.saturating_sub(span_start).min(span_len);
        let chars: Vec<char> = span.text.chars().collect();

        if local_start > 0 {
            output.push(StyledSpan {
                text: chars[..local_start].iter().collect(),
                style: span.style,
            });
        }
        if local_start < local_end {
            output.push(StyledSpan {
                text: chars[local_start..local_end].iter().collect(),
                style: span.style.add_modifier(Modifier::REVERSED),
            });
        }
        if local_end < span_len {
            output.push(StyledSpan {
                text: chars[local_end..].iter().collect(),
                style: span.style,
            });
        }
    }
    output
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

fn panel_title_style(
    selected: bool,
    active_fg: ratatui::style::Color,
    inactive_fg: ratatui::style::Color,
    bg: ratatui::style::Color,
) -> Style {
    let style = Style::default()
        .fg(if selected { active_fg } else { inactive_fg })
        .bg(bg);
    if selected {
        style.add_modifier(Modifier::BOLD)
    } else {
        style
    }
}

fn panel_content_style(selected: bool, fg: ratatui::style::Color, bg: ratatui::style::Color) -> Style {
    let style = Style::default().fg(fg).bg(bg);
    if selected {
        style
    } else {
        style.add_modifier(Modifier::DIM)
    }
}
