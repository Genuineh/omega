use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::{Line, Span},
    Frame,
};

use omega_theme::RenderPalette as ColorScheme;

use crate::app::{App, Panel};
use crate::overlay::{overlay_area, ConfirmChoice, OverlayState};

pub(super) fn render_overlay(frame: &mut Frame, app: &mut App, colors: &ColorScheme) {
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
    render_overlay_shadow(frame, full_area, overlay_rect, colors);
    frame.render_widget(Clear, overlay_rect);

    match overlay {
        OverlayState::Search(search) => {
            let block = Block::default()
                .border_type(colors.overlay_border_type)
                .title(" Search ")
                .borders(Borders::ALL)
                .border_style(overlay_border_style(colors))
                .style(Style::default().bg(colors.overlay_bg));
            let inner = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Length(1),
                    Constraint::Length(1),
                    Constraint::Min(1),
                ])
                .split(padded_rect(block.inner(overlay_rect), 1, 1));
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
                Panel::Delivery => "Delivery",
                Panel::Skills => "Skills",
                Panel::Project => "Project",
                Panel::Document => "Knowledge",
                Panel::Memory => "Memory",
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
        OverlayState::SearchResults(results) => {
            let block = Block::default()
                .border_type(colors.overlay_border_type)
                .title(results.title.as_str())
                .borders(Borders::ALL)
                .border_style(overlay_border_style(colors))
                .style(Style::default().bg(colors.overlay_bg));
            let inner = padded_rect(block.inner(overlay_rect), 1, 1);
            frame.render_widget(block, overlay_rect);
            let scroll = clamp_overlay_scroll(results.scroll, results.lines.len(), inner.height as usize);
            let show_footer = should_show_overlay_footer(scroll, results.lines.len(), inner.height as usize);
            let [content_rect, footer_rect] = overlay_content_and_footer_rects(inner, show_footer);
            let items: Vec<ListItem> = results
                .lines
                .iter()
                .skip(scroll)
                .take(content_rect.height as usize)
                .map(|line| ListItem::new(line.clone()))
                .collect();
            frame.render_widget(
                List::new(items).style(Style::default().fg(colors.text).bg(colors.overlay_bg)),
                content_rect,
            );
            render_overlay_footer(
                frame,
                footer_rect,
                colors,
                overlay_scroll_footer_text(scroll, content_rect.height as usize, results.lines.len()),
            );
        }
        OverlayState::Confirm(confirm) => {
            let block = Block::default()
                .border_type(colors.overlay_border_type)
                .title(confirm.title.as_str())
                .borders(Borders::ALL)
                .border_style(overlay_border_style(colors))
                .style(Style::default().bg(colors.overlay_bg));
            let inner = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(2),
                    Constraint::Length(1),
                    Constraint::Length(1),
                ])
                .split(padded_rect(block.inner(overlay_rect), 1, 1));
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
                .border_style(overlay_border_style(colors))
                .style(Style::default().bg(colors.overlay_bg));
            let inner = padded_rect(block.inner(overlay_rect), 1, 1);
            frame.render_widget(block, overlay_rect);
            let scroll = clamp_overlay_scroll(detail.scroll, detail.lines.len(), inner.height as usize);
            let show_footer = should_show_overlay_footer(scroll, detail.lines.len(), inner.height as usize);
            let [content_rect, footer_rect] = overlay_content_and_footer_rects(inner, show_footer);
            let items: Vec<ListItem> = detail
                .lines
                .iter()
                .skip(scroll)
                .take(content_rect.height as usize)
                .map(|line| ListItem::new(line.clone()))
                .collect();
            frame.render_widget(
                List::new(items).style(Style::default().fg(colors.text).bg(colors.overlay_bg)),
                content_rect,
            );
            render_overlay_footer(
                frame,
                footer_rect,
                colors,
                overlay_scroll_footer_text(scroll, content_rect.height as usize, detail.lines.len()),
            );
        }
        OverlayState::Picker(picker) => {
            let block = Block::default()
                .border_type(colors.overlay_border_type)
                .title(picker.title.as_str())
                .borders(Borders::ALL)
                .border_style(overlay_border_style(colors))
                .style(Style::default().bg(colors.overlay_bg));
            let inner = padded_rect(block.inner(overlay_rect), 1, 1);
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
            let block = Block::default()
                .border_type(colors.overlay_border_type)
                .title(prompt.title.as_str())
                .borders(Borders::ALL)
                .border_style(overlay_border_style(colors))
                .style(Style::default().bg(colors.overlay_bg));
            let inner = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1),
                    Constraint::Length(3),
                    Constraint::Min(1),
                ])
                .split(padded_rect(block.inner(overlay_rect), 1, 1));
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

fn overlay_border_style(colors: &ColorScheme) -> Style {
    Style::default()
        .fg(colors.overlay_edge_fg)
        .add_modifier(Modifier::BOLD)
}

fn padded_rect(rect: ratatui::layout::Rect, horizontal: u16, vertical: u16) -> ratatui::layout::Rect {
    let width_padding = horizontal.saturating_mul(2);
    let height_padding = vertical.saturating_mul(2);
    if rect.width <= width_padding || rect.height <= height_padding {
        return rect;
    }

    ratatui::layout::Rect::new(
        rect.x.saturating_add(horizontal),
        rect.y.saturating_add(vertical),
        rect.width.saturating_sub(width_padding),
        rect.height.saturating_sub(height_padding),
    )
}

fn render_overlay_shadow(
    frame: &mut Frame,
    full_area: ratatui::layout::Rect,
    overlay_rect: ratatui::layout::Rect,
    colors: &ColorScheme,
) {
    let right_x = overlay_rect.x.saturating_add(overlay_rect.width);
    if right_x < full_area.x.saturating_add(full_area.width) && overlay_rect.height > 1 {
        let width = (full_area.x.saturating_add(full_area.width) - right_x).min(2);
        let rect = ratatui::layout::Rect::new(
            right_x,
            overlay_rect.y.saturating_add(1),
            width,
            overlay_rect.height.saturating_sub(1),
        );
        render_shadow_fill(frame, rect, colors);
    }

    let bottom_y = overlay_rect.y.saturating_add(overlay_rect.height);
    if bottom_y < full_area.y.saturating_add(full_area.height) && overlay_rect.width > 1 {
        let height = (full_area.y.saturating_add(full_area.height) - bottom_y).min(1);
        let rect = ratatui::layout::Rect::new(
            overlay_rect.x.saturating_add(1),
            bottom_y,
            overlay_rect.width.saturating_sub(1),
            height,
        );
        render_shadow_fill(frame, rect, colors);
    }
}

fn render_shadow_fill(frame: &mut Frame, rect: ratatui::layout::Rect, colors: &ColorScheme) {
    if rect.width == 0 || rect.height == 0 {
        return;
    }

    let lines = (0..rect.height)
        .map(|_| {
            Line::from(vec![Span::styled(
                "░".repeat(rect.width as usize),
                Style::default()
                    .fg(colors.overlay_shadow_fg)
                    .bg(colors.overlay_mask_bg),
            )])
        })
        .collect::<Vec<_>>();

    frame.render_widget(Paragraph::new(lines), rect);
}

pub(super) fn overlay_hint_text(app: &App) -> &'static str {
    match app.overlay.as_ref() {
        Some(OverlayState::Search(_)) => {
            " Search popup: type to filter the focused panel  Enter=keep query  Esc=Close"
        }
        Some(OverlayState::SearchResults(_)) => {
            " Search results: ↑/↓ PgUp/PgDn scroll  Home/End jump  Esc=Close"
        }
        Some(OverlayState::Confirm(_)) => {
            " Confirm dialog: ←/→/Tab switch  Enter accepts selected action  Esc=Cancel"
        }
        Some(OverlayState::Detail(_)) => {
            " Detail dialog: ↑/↓ PgUp/PgDn scroll  Home/End jump  Esc=Close"
        }
        Some(OverlayState::Picker(_)) => " Picker popup: ↑/↓/Tab move  Enter=Select  Esc=Close",
        Some(OverlayState::InputPrompt(_)) => " Input prompt: type freely  Enter=Submit  Esc=Close",
        None => "",
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

fn clamp_overlay_scroll(scroll: usize, total_lines: usize, viewport_height: usize) -> usize {
    scroll.min(total_lines.saturating_sub(viewport_height.max(1)))
}

fn should_show_overlay_footer(scroll: usize, total_lines: usize, viewport_height: usize) -> bool {
    total_lines > viewport_height.max(1) || scroll > 0
}

fn overlay_content_and_footer_rects(
    inner: ratatui::layout::Rect,
    show_footer: bool,
) -> [ratatui::layout::Rect; 2] {
    if !show_footer || inner.height <= 1 {
        return [inner, ratatui::layout::Rect::default()];
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);
    [chunks[0], chunks[1]]
}

fn render_overlay_footer(
    frame: &mut Frame,
    rect: ratatui::layout::Rect,
    colors: &ColorScheme,
    text: Option<String>,
) {
    let Some(text) = text else {
        return;
    };
    if rect.width == 0 || rect.height == 0 {
        return;
    }

    frame.render_widget(
        Paragraph::new(text).style(
            Style::default()
                .fg(colors.context_label)
                .bg(colors.overlay_bg)
                .add_modifier(Modifier::ITALIC),
        ),
        rect,
    );
}

fn overlay_scroll_footer_text(
    scroll: usize,
    viewport_height: usize,
    total_lines: usize,
) -> Option<String> {
    if total_lines == 0 {
        return None;
    }

    let start = scroll.saturating_add(1).min(total_lines);
    let end = scroll.saturating_add(viewport_height).min(total_lines);
    if start == 1 && end == total_lines {
        return None;
    }

    Some(format!("lines {start}-{end}/{total_lines}  ·  mouse wheel or PgUp/PgDn"))
}
