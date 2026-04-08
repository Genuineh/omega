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
        SidebarSection::Delivery,
        SidebarSection::Skills,
        SidebarSection::Document,
        SidebarSection::Memory,
        SidebarSection::Todos,
        SidebarSection::Logs,
    ];
    let mut spans = Vec::new();

    for (index, section) in sections.into_iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled(
                " ",
                Style::default().bg(colors.sidebar_rail_bg),
            ));
        }

        let selected = app.sidebar.rail_selection == section;
        let expanded = app.sidebar.is_expanded(section);
        let marker = if expanded { "▾" } else { "▸" };
        let style = if selected {
            Style::default()
                .fg(colors.title_fg)
                .bg(colors.section_bg)
                .add_modifier(Modifier::BOLD)
        } else if expanded {
            Style::default()
                .fg(colors.focus_border)
                .bg(colors.sidebar_rail_bg)
        } else {
            Style::default()
                .fg(colors.context_label)
                .bg(colors.sidebar_rail_bg)
        };
        spans.push(Span::styled(
            format!(" {marker} {} {} ", section.label(), app.rail_badge(section)),
            style,
        ));
    }

    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().bg(colors.sidebar_rail_bg)),
        area,
    );
}

pub(super) fn render_sidebar_body(
    frame: &mut Frame,
    app: &mut App,
    colors: &ColorScheme,
    area: Rect,
    diagnostics_border: Style,
    delivery_border: Style,
    skills_border: Style,
    document_border: Style,
    memory_border: Style,
    todo_border: Style,
    logs_border: Style,
) {
    app.diagnostics_rect = Rect::default();
    app.delivery_rect = Rect::default();
    app.skills_rect = Rect::default();
    app.document_rect = Rect::default();
    app.memory_rect = Rect::default();
    app.todo_rect = Rect::default();
    app.logs_rect = Rect::default();

    let expanded_sections = [
        app.sidebar.diagnostics_expanded,
        app.sidebar.delivery_expanded,
        app.sidebar.skills_expanded,
        app.sidebar.document_expanded,
        app.sidebar.memory_expanded,
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
                    .title(styled_title(&diagnostics_title, diagnostics_border, colors))
                    .borders(Borders::ALL)
                    .border_style(diagnostics_border)
                    .style(Style::default().bg(colors.section_bg)),
            )
            .highlight_style(Style::default())
            .style(Style::default().fg(colors.text).bg(colors.section_bg));
        frame.render_stateful_widget(diagnostics_list, rect, &mut app.diagnostics_state);
    } else {
        app.diagnostics_displayed_count = 0;
    }

    if app.sidebar.delivery_expanded {
        let rect = sections.get(next_index).copied().unwrap_or_default();
        next_index += 1;
        app.delivery_rect = rect;
        let title = app.delivery_panel_title();
        let inner_w = (rect.width as usize).saturating_sub(2).max(1);
        let app_ref: &App = &*app;
        let items: Vec<ListItem> = app_ref
            .wrapped_panel_lines(Panel::Delivery, inner_w)
            .into_iter()
            .map(|line| {
                let line_len = line.text.chars().count();
                list_item_with_selection(
                    &line.text,
                    Style::default().fg(colors.text),
                    app_ref.selection_range_for_segment(
                        Panel::Delivery,
                        line.source_line_index,
                        line.source_column_start,
                        line.source_column_start + line_len,
                    ),
                )
            })
            .collect();
        let total = items.len();
        app.delivery_displayed_count = total;
        if !app.delivery_pinned && total > 0 {
            app.delivery_state.select(Some(total - 1));
        }
        let list = List::new(items)
            .block(
                Block::default()
                    .border_type(colors.panel_border_type)
                    .title(styled_title(&title, delivery_border, colors))
                    .borders(Borders::ALL)
                    .border_style(delivery_border)
                    .style(Style::default().bg(colors.section_bg)),
            )
            .highlight_style(Style::default())
            .style(Style::default().fg(colors.text).bg(colors.section_bg));
        frame.render_stateful_widget(list, rect, &mut app.delivery_state);
    } else {
        app.delivery_displayed_count = 0;
    }

    if app.sidebar.skills_expanded {
        let rect = sections.get(next_index).copied().unwrap_or_default();
        next_index += 1;
        app.skills_rect = rect;
        let title = app.skills_panel_title();
        let inner_w = (rect.width as usize).saturating_sub(2).max(1);
        let app_ref: &App = &*app;
        let items: Vec<ListItem> = app_ref
            .wrapped_panel_lines(Panel::Skills, inner_w)
            .into_iter()
            .map(|line| {
                let line_len = line.text.chars().count();
                list_item_with_selection(
                    &line.text,
                    Style::default().fg(colors.text),
                    app_ref.selection_range_for_segment(
                        Panel::Skills,
                        line.source_line_index,
                        line.source_column_start,
                        line.source_column_start + line_len,
                    ),
                )
            })
            .collect();
        let total = items.len();
        app.skills_displayed_count = total;
        if !app.skills_pinned && total > 0 {
            app.skills_state.select(Some(total - 1));
        }
        let list = List::new(items)
            .block(
                Block::default()
                    .border_type(colors.panel_border_type)
                    .title(styled_title(&title, skills_border, colors))
                    .borders(Borders::ALL)
                    .border_style(skills_border)
                    .style(Style::default().bg(colors.section_bg)),
            )
            .highlight_style(Style::default())
            .style(Style::default().fg(colors.text).bg(colors.section_bg));
        frame.render_stateful_widget(list, rect, &mut app.skills_state);
    } else {
        app.skills_displayed_count = 0;
    }

    if app.sidebar.document_expanded {
        let rect = sections.get(next_index).copied().unwrap_or_default();
        next_index += 1;
        app.document_rect = rect;
        let title = app.document_panel_title();
        let inner_w = (rect.width as usize).saturating_sub(2).max(1);
        let app_ref: &App = &*app;
        let items: Vec<ListItem> = app_ref
            .wrapped_panel_lines(Panel::Document, inner_w)
            .into_iter()
            .map(|line| {
                let line_len = line.text.chars().count();
                list_item_with_selection(
                    &line.text,
                    Style::default().fg(colors.text),
                    app_ref.selection_range_for_segment(
                        Panel::Document,
                        line.source_line_index,
                        line.source_column_start,
                        line.source_column_start + line_len,
                    ),
                )
            })
            .collect();
        let total = items.len();
        app.document_displayed_count = total;
        if !app.document_pinned && total > 0 {
            app.document_state.select(Some(total - 1));
        }
        let list = List::new(items)
            .block(
                Block::default()
                    .border_type(colors.panel_border_type)
                    .title(styled_title(&title, document_border, colors))
                    .borders(Borders::ALL)
                    .border_style(document_border)
                    .style(Style::default().bg(colors.section_bg)),
            )
            .highlight_style(Style::default())
            .style(Style::default().fg(colors.text).bg(colors.section_bg));
        frame.render_stateful_widget(list, rect, &mut app.document_state);
    } else {
        app.document_displayed_count = 0;
    }

    if app.sidebar.memory_expanded {
        let rect = sections.get(next_index).copied().unwrap_or_default();
        next_index += 1;
        app.memory_rect = rect;
        let title = app.memory_panel_title();
        let inner_w = (rect.width as usize).saturating_sub(2).max(1);
        let app_ref: &App = &*app;
        let items: Vec<ListItem> = app_ref
            .wrapped_panel_lines(Panel::Memory, inner_w)
            .into_iter()
            .map(|line| {
                let line_len = line.text.chars().count();
                list_item_with_selection(
                    &line.text,
                    Style::default().fg(colors.text),
                    app_ref.selection_range_for_segment(
                        Panel::Memory,
                        line.source_line_index,
                        line.source_column_start,
                        line.source_column_start + line_len,
                    ),
                )
            })
            .collect();
        let total = items.len();
        app.memory_displayed_count = total;
        if !app.memory_pinned && total > 0 {
            app.memory_state.select(Some(total - 1));
        }
        let list = List::new(items)
            .block(
                Block::default()
                    .border_type(colors.panel_border_type)
                    .title(styled_title(&title, memory_border, colors))
                    .borders(Borders::ALL)
                    .border_style(memory_border)
                    .style(Style::default().bg(colors.section_bg)),
            )
            .highlight_style(Style::default())
            .style(Style::default().fg(colors.text).bg(colors.section_bg));
        frame.render_stateful_widget(list, rect, &mut app.memory_state);
    } else {
        app.memory_displayed_count = 0;
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
                    .title(styled_title(&todo_title, todo_border, colors))
                    .borders(Borders::ALL)
                    .border_style(todo_border)
                    .style(Style::default().bg(colors.section_bg)),
            )
            .highlight_style(Style::default())
            .style(Style::default().fg(colors.text).bg(colors.section_bg));
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
                    .title(styled_title(&logs_title, logs_border, colors))
                    .borders(Borders::ALL)
                    .border_style(logs_border)
                    .style(Style::default().bg(colors.section_bg)),
            )
            .highlight_style(Style::default())
            .style(Style::default().fg(colors.text).bg(colors.section_bg));
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

fn styled_title(title: &str, border_style: Style, colors: &ColorScheme) -> Line<'static> {
    let title_color = border_style.fg.unwrap_or(colors.title_fg);
    Line::from(vec![Span::styled(
        title.to_string(),
        Style::default()
            .fg(title_color)
            .bg(colors.section_bg)
            .add_modifier(Modifier::BOLD),
    )])
}
