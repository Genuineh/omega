use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    Frame,
};

use omega_theme::RenderPalette as ColorScheme;

#[cfg(test)]
use crate::app::wrap_text_segments;
use crate::app::{App, Panel};
use crate::sidebar::SidebarSection;

pub(super) fn render_sidebar_rail(
    frame: &mut Frame,
    app: &mut App,
    colors: &ColorScheme,
    area: Rect,
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

pub(super) fn render_sidebar_body(
    frame: &mut Frame,
    app: &mut App,
    colors: &ColorScheme,
    area: Rect,
    diagnostics_border: Style,
    todo_border: Style,
    logs_border: Style,
) {
    app.diagnostics_rect = Rect::default();
    app.todo_rect = Rect::default();
    app.logs_rect = Rect::default();

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
            .constraints(vec![
                Constraint::Ratio(1, expanded_sections as u32);
                expanded_sections
            ])
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
                let base_style =
                    if app_ref.highlighted_todo_line_index() == Some(line.source_line_index) {
                        Style::default()
                            .fg(colors.focus_border)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(colors.text)
                    };
                list_item_with_selection(
                    &line.text,
                    base_style,
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
pub(super) fn wrap_text(line: &str, width: usize) -> Vec<String> {
    wrap_text_segments(line, width)
        .into_iter()
        .map(|(_, segment)| segment)
        .collect()
}

pub(super) fn list_item_with_selection(
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
