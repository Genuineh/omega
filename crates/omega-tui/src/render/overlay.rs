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
        OverlayState::SearchResults(results) => {
            let block = Block::default()
                .border_type(colors.overlay_border_type)
                .title(results.title.as_str())
                .borders(Borders::ALL)
                .border_style(
                    Style::default()
                        .fg(colors.focus_border)
                        .add_modifier(Modifier::BOLD),
                )
                .style(Style::default().bg(colors.overlay_bg));
            let inner = block.inner(overlay_rect);
            frame.render_widget(block, overlay_rect);
            let items: Vec<ListItem> = results
                .lines
                .iter()
                .skip(results.scroll)
                .map(|line| ListItem::new(line.clone()))
                .collect();
            frame.render_widget(
                List::new(items).style(Style::default().fg(colors.text).bg(colors.overlay_bg)),
                inner,
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

pub(super) fn overlay_hint_text(app: &App) -> &'static str {
    match app.overlay.as_ref() {
        Some(OverlayState::Search(_)) => {
            " Search popup: type to filter the focused panel  Enter=keep query  Esc=Close"
        }
        Some(OverlayState::SearchResults(_)) => " Search results: ↑/↓ scroll  Esc=Close",
        Some(OverlayState::Confirm(_)) => {
            " Confirm dialog: ←/→/Tab switch  Enter accepts selected action  Esc=Cancel"
        }
        Some(OverlayState::Detail(_)) => " Detail dialog: ↑/↓ scroll  Esc=Close",
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
