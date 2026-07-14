use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    Frame,
};

use omega_theme::RenderPalette as ColorScheme;

use crate::app::{wrap_text_segments, App, Panel};
use crate::render::component::{FocusState, Panel as PanelChrome};
use crate::sidebar::SidebarSection;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SidebarLineKind {
    EmptyState,
    Hint,
    SectionLabel,
    Summary,
    Metric,
    StatusOk,
    StatusWarn,
    StatusError,
    Meta,
    Preview,
    Codeish,
    TodoDone,
    TodoActive,
    TodoPending,
    LogTool,
    LogError,
    LogText,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SidebarDisplayLine {
    source_line_index: Option<usize>,
    text: String,
    kind: SidebarLineKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WrappedSidebarLine {
    source_line_index: Option<usize>,
    source_column_start: usize,
    text: String,
    kind: SidebarLineKind,
}

pub(super) fn render_sidebar_rail(
    frame: &mut Frame,
    app: &mut App,
    colors: &ColorScheme,
    area: Rect,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let sections = [
        SidebarSection::Diagnostics,
        SidebarSection::Delivery,
        SidebarSection::Skills,
        SidebarSection::Project,
        SidebarSection::Knowledge,
        SidebarSection::Todos,
        SidebarSection::Logs,
    ];
    let mut spans = Vec::new();
    let mut item_widths = Vec::new();

    for &section in &sections {
        let selected = app.sidebar.rail_selection == section;
        let expanded = app.sidebar.is_expanded(section);
        let rail_focused = app.focused_panel == Panel::SidebarRail;
        let count = sidebar_rail_count(app, section);
        let style = if selected && rail_focused {
            Style::default()
                .fg(colors.focus_border)
                .bg(colors.sidebar_rail_bg)
                .add_modifier(Modifier::BOLD)
        } else if selected {
            Style::default()
                .fg(colors.title_fg)
                .bg(colors.sidebar_rail_bg)
                .add_modifier(Modifier::BOLD)
        } else if expanded {
            Style::default()
                .fg(colors.context_hint)
                .bg(colors.sidebar_rail_bg)
        } else {
            Style::default()
                .fg(colors.context_label)
                .bg(colors.sidebar_rail_bg)
        };
        let text = sidebar_rail_item_text(section, expanded, &count);
        item_widths.push(sidebar_rail_item_width(section, &count));
        spans.push(Span::styled(text, style));
    }

    let scroll_x = sidebar_rail_scroll_offset(
        &sections,
        &item_widths,
        app.sidebar.rail_selection,
        area.width as usize,
    );
    frame.render_widget(
        Paragraph::new(Line::from(spans))
            .style(Style::default().bg(colors.sidebar_rail_bg))
            .scroll((0, scroll_x as u16)),
        area,
    );
}

fn sidebar_rail_scroll_offset(
    sections: &[SidebarSection],
    item_widths: &[usize],
    selected: SidebarSection,
    viewport_width: usize,
) -> usize {
    if viewport_width == 0 {
        return 0;
    }

    let total_width: usize = item_widths.iter().sum();
    if total_width <= viewport_width {
        return 0;
    }

    let Some(selected_index) = sections.iter().position(|section| *section == selected) else {
        return 0;
    };
    let selected_start: usize = item_widths.iter().take(selected_index).sum();
    let selected_width = item_widths[selected_index];
    let selected_end = selected_start + selected_width;

    selected_end
        .saturating_sub(viewport_width)
        .min(selected_start)
}

fn sidebar_rail_item_width(section: SidebarSection, count: &str) -> usize {
    let label_width = section.label().chars().count();
    if count.is_empty() {
        label_width + 4
    } else {
        label_width + count.chars().count() + 5
    }
}

fn sidebar_rail_item_text(section: SidebarSection, expanded: bool, count: &str) -> String {
    let marker = if expanded { "▾" } else { "▸" };
    let label = section.label();
    if count.is_empty() {
        format!(" {marker} {label} ")
    } else {
        format!(" {marker} {label} {count} ")
    }
}

fn sidebar_rail_count(app: &App, section: SidebarSection) -> String {
    let badge = app.rail_badge(section);
    let raw = badge
        .split_once(' ')
        .map(|(_, rest)| rest.trim().to_string())
        .unwrap_or(badge);

    match raw.as_str() {
        "ready/ready" | "enabled/enabled" => "ok".to_string(),
        "uninitialized/uninitialized" => "new".to_string(),
        "off" => "off".to_string(),
        other => other.to_string(),
    }
}

pub(super) fn render_sidebar_body(
    frame: &mut Frame,
    app: &mut App,
    colors: &ColorScheme,
    area: Rect,
) {
    app.diagnostics_rect = Rect::default();
    app.delivery_rect = Rect::default();
    app.skills_rect = Rect::default();
    app.project_rect = Rect::default();
    app.document_rect = Rect::default();
    app.memory_rect = Rect::default();
    app.todo_rect = Rect::default();
    app.logs_rect = Rect::default();

    let visible_sections = visible_sidebar_sections(app, area.height);
    let sections = if visible_sections.is_empty() {
        Vec::new()
    } else {
        let total_weight: u32 = visible_sections
            .iter()
            .map(|section| sidebar_section_weight(app, *section))
            .sum();
        Layout::default()
            .direction(Direction::Vertical)
            .constraints(
                visible_sections
                    .iter()
                    .map(|section| {
                        Constraint::Ratio(sidebar_section_weight(app, *section), total_weight)
                    })
                    .collect::<Vec<_>>(),
            )
            .split(area)
            .iter()
            .copied()
            .collect::<Vec<_>>()
    };

    let section_rect = |section: SidebarSection| {
        visible_sections
            .iter()
            .position(|candidate| *candidate == section)
            .and_then(|index| sections.get(index).copied())
            .unwrap_or_default()
    };

    if app.sidebar.diagnostics_expanded {
        let rect = section_rect(SidebarSection::Diagnostics);
        app.diagnostics_rect = rect;
        if rect.height > 0 {
            let focused = app.focused_panel == Panel::Diagnostics;
            let diagnostics_title = app.diagnostics_panel_title();
            let diagnostics_inner_w = (rect.width as usize).saturating_sub(2).max(1);
            let app_ref: &App = &*app;
            let diagnostics_items = render_sidebar_items(
                app_ref,
                Panel::Diagnostics,
                diagnostics_inner_w,
                rect.height,
                colors,
            );
            let diagnostics_total = diagnostics_items.len();
            app.diagnostics_displayed_count = diagnostics_total;
            if !app.diagnostics_pinned && diagnostics_total > 0 {
                app.diagnostics_state.select(Some(diagnostics_total - 1));
            }
            let diagnostics_list = List::new(diagnostics_items)
                .block(
                    PanelChrome::new(styled_title(&diagnostics_title, focused, colors))
                        .focus(FocusState::new(focused))
                        .with_bg(colors.sidebar_bg)
                        .with_border_colors(colors.focus_border, colors.section_outline)
                        .with_title_colors(colors.title_fg, colors.section_header_fg)
                        .block(),
                )
                .highlight_style(Style::default())
                .style(section_body_style(focused, colors));
            frame.render_stateful_widget(diagnostics_list, rect, &mut app.diagnostics_state);
        } else {
            app.diagnostics_displayed_count = 0;
        }
    } else {
        app.diagnostics_displayed_count = 0;
    }

    if app.sidebar.delivery_expanded {
        let rect = section_rect(SidebarSection::Delivery);
        app.delivery_rect = rect;
        if rect.height > 0 {
            let focused = app.focused_panel == Panel::Delivery;
            let title = app.delivery_panel_title();
            let inner_w = (rect.width as usize).saturating_sub(2).max(1);
            let app_ref: &App = &*app;
            let items =
                render_sidebar_items(app_ref, Panel::Delivery, inner_w, rect.height, colors);
            let total = items.len();
            app.delivery_displayed_count = total;
            if !app.delivery_pinned && total > 0 {
                app.delivery_state.select(Some(total - 1));
            }
            let list = List::new(items)
                .block(
                    PanelChrome::new(styled_title(&title, focused, colors))
                        .focus(FocusState::new(focused))
                        .with_bg(colors.sidebar_bg)
                        .with_border_colors(colors.focus_border, colors.section_outline)
                        .with_title_colors(colors.title_fg, colors.section_header_fg)
                        .block(),
                )
                .highlight_style(Style::default())
                .style(section_body_style(focused, colors));
            frame.render_stateful_widget(list, rect, &mut app.delivery_state);
        } else {
            app.delivery_displayed_count = 0;
        }
    } else {
        app.delivery_displayed_count = 0;
    }

    if app.sidebar.skills_expanded {
        let rect = section_rect(SidebarSection::Skills);
        app.skills_rect = rect;
        if rect.height > 0 {
            let focused = app.focused_panel == Panel::Skills;
            let title = app.skills_panel_title();
            let inner_w = (rect.width as usize).saturating_sub(2).max(1);
            let app_ref: &App = &*app;
            let items = render_sidebar_items(app_ref, Panel::Skills, inner_w, rect.height, colors);
            let total = items.len();
            app.skills_displayed_count = total;
            if !app.skills_pinned && total > 0 {
                app.skills_state.select(Some(total - 1));
            }
            let list = List::new(items)
                .block(
                    PanelChrome::new(styled_title(&title, focused, colors))
                        .focus(FocusState::new(focused))
                        .with_bg(colors.sidebar_bg)
                        .with_border_colors(colors.focus_border, colors.section_outline)
                        .with_title_colors(colors.title_fg, colors.section_header_fg)
                        .block(),
                )
                .highlight_style(Style::default())
                .style(section_body_style(focused, colors));
            frame.render_stateful_widget(list, rect, &mut app.skills_state);
        } else {
            app.skills_displayed_count = 0;
        }
    } else {
        app.skills_displayed_count = 0;
    }

    if app.sidebar.project_expanded {
        let rect = section_rect(SidebarSection::Project);
        app.project_rect = rect;
        if rect.height > 0 {
            let focused = app.focused_panel == Panel::Project;
            let title = app.project_panel_title();
            let inner_w = (rect.width as usize).saturating_sub(2).max(1);
            let app_ref: &App = &*app;
            let items = render_sidebar_items(app_ref, Panel::Project, inner_w, rect.height, colors);
            let total = items.len();
            app.project_displayed_count = total;
            if !app.project_pinned && total > 0 {
                app.project_state.select(Some(total - 1));
            }
            let list = List::new(items)
                .block(
                    Block::default()
                        .border_type(colors.panel_border_type)
                        .title(styled_title(&title, focused, colors))
                        .borders(Borders::ALL)
                        .border_style(sidebar_section_border_style(focused, colors))
                        .style(Style::default().bg(colors.sidebar_bg)),
                )
                .highlight_style(Style::default())
                .style(section_body_style(focused, colors));
            frame.render_stateful_widget(list, rect, &mut app.project_state);
        } else {
            app.project_displayed_count = 0;
        }
    } else {
        app.project_displayed_count = 0;
    }

    if app.sidebar.knowledge_expanded {
        let rect = section_rect(SidebarSection::Knowledge);
        app.document_rect = rect;
        if rect.height > 0 {
            let focused = app.focused_panel == Panel::Document;
            let title = app.knowledge_panel_title();
            let inner_w = (rect.width as usize).saturating_sub(2).max(1);
            let app_ref: &App = &*app;
            let items =
                render_sidebar_items(app_ref, Panel::Document, inner_w, rect.height, colors);
            let total = items.len();
            app.document_displayed_count = total;
            if !app.document_pinned && total > 0 {
                app.document_state.select(Some(total - 1));
            }
            let list = List::new(items)
                .block(
                    Block::default()
                        .border_type(colors.panel_border_type)
                        .title(styled_title(&title, focused, colors))
                        .borders(Borders::ALL)
                        .border_style(sidebar_section_border_style(focused, colors))
                        .style(Style::default().bg(colors.sidebar_bg)),
                )
                .highlight_style(Style::default())
                .style(section_body_style(focused, colors));
            frame.render_stateful_widget(list, rect, &mut app.document_state);
        } else {
            app.document_displayed_count = 0;
        }
    } else {
        app.document_displayed_count = 0;
    }
    app.memory_displayed_count = 0;

    if app.sidebar.todos_expanded {
        let rect = section_rect(SidebarSection::Todos);
        app.todo_rect = rect;
        if rect.height > 0 {
            let focused = app.focused_panel == Panel::Todo;
            let todo_title = app.todo_panel_title();
            let todo_inner_w = (rect.width as usize).saturating_sub(2).max(1);
            let app_ref: &App = &*app;
            let todo_items =
                render_sidebar_items(app_ref, Panel::Todo, todo_inner_w, rect.height, colors);
            let todo_total = todo_items.len();
            app.todo_displayed_count = todo_total;
            if !app.todo_pinned && todo_total > 0 {
                app.todo_state.select(Some(todo_total - 1));
            }
            let todo_list = List::new(todo_items)
                .block(
                    Block::default()
                        .border_type(colors.panel_border_type)
                        .title(styled_title(&todo_title, focused, colors))
                        .borders(Borders::ALL)
                        .border_style(sidebar_section_border_style(focused, colors))
                        .style(Style::default().bg(colors.sidebar_bg)),
                )
                .highlight_style(Style::default())
                .style(section_body_style(focused, colors));
            frame.render_stateful_widget(todo_list, rect, &mut app.todo_state);
        } else {
            app.todo_displayed_count = 0;
        }
    } else {
        app.todo_displayed_count = 0;
    }

    if app.sidebar.logs_expanded {
        let rect = section_rect(SidebarSection::Logs);
        app.logs_rect = rect;
        if rect.height > 0 {
            let focused = app.focused_panel == Panel::Logs;
            let logs_title = app.logs_panel_title();
            let logs_inner_w = (rect.width as usize).saturating_sub(2).max(1);
            let app_ref: &App = &*app;
            let log_items =
                render_sidebar_items(app_ref, Panel::Logs, logs_inner_w, rect.height, colors);
            let logs_total = log_items.len();
            app.logs_displayed_count = logs_total;
            if !app.logs_pinned && logs_total > 0 {
                app.logs_state.select(Some(logs_total - 1));
            }
            let logs_list = List::new(log_items)
                .block(
                    Block::default()
                        .border_type(colors.panel_border_type)
                        .title(styled_title(&logs_title, focused, colors))
                        .borders(Borders::ALL)
                        .border_style(sidebar_section_border_style(focused, colors))
                        .style(Style::default().bg(colors.sidebar_bg)),
                )
                .highlight_style(Style::default())
                .style(section_body_style(focused, colors));
            frame.render_stateful_widget(logs_list, rect, &mut app.logs_state);
        } else {
            app.logs_displayed_count = 0;
        }
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

fn styled_title(title: &str, focused: bool, colors: &ColorScheme) -> Line<'static> {
    let title_style = if focused {
        Style::default()
            .fg(colors.title_fg)
            .bg(colors.sidebar_bg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(colors.section_header_fg)
            .bg(colors.sidebar_bg)
    };
    Line::from(vec![Span::styled(title.to_string(), title_style)])
}

fn sidebar_section_border_style(focused: bool, colors: &ColorScheme) -> Style {
    if focused {
        Style::default()
            .fg(colors.focus_border)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(colors.section_outline)
    }
}

fn section_body_style(focused: bool, colors: &ColorScheme) -> Style {
    let _ = focused;
    Style::default().fg(colors.text).bg(colors.sidebar_bg)
}

fn expanded_sidebar_sections(app: &App) -> Vec<SidebarSection> {
    [
        SidebarSection::Diagnostics,
        SidebarSection::Delivery,
        SidebarSection::Skills,
        SidebarSection::Project,
        SidebarSection::Knowledge,
        SidebarSection::Todos,
        SidebarSection::Logs,
    ]
    .into_iter()
    .filter(|section| app.sidebar.is_expanded(*section))
    .collect()
}

fn visible_sidebar_sections(app: &App, available_height: u16) -> Vec<SidebarSection> {
    const MIN_SECTION_HEIGHT: u16 = 5;

    let expanded = expanded_sidebar_sections(app);
    if expanded.is_empty() {
        return expanded;
    }

    let max_sections = usize::from((available_height / MIN_SECTION_HEIGHT).max(1));
    if expanded.len() <= max_sections {
        return expanded;
    }

    let anchor = sidebar_view_anchor(app, &expanded).unwrap_or(expanded[0]);
    let anchor_index = expanded
        .iter()
        .position(|section| *section == anchor)
        .unwrap_or(0);
    let mut start = anchor_index.saturating_add(1).saturating_sub(max_sections);
    let max_start = expanded.len().saturating_sub(max_sections);
    if start > max_start {
        start = max_start;
    }
    expanded[start..start + max_sections].to_vec()
}

fn sidebar_view_anchor(app: &App, expanded: &[SidebarSection]) -> Option<SidebarSection> {
    let focused = match app.focused_panel {
        Panel::Diagnostics => Some(SidebarSection::Diagnostics),
        Panel::Delivery => Some(SidebarSection::Delivery),
        Panel::Skills => Some(SidebarSection::Skills),
        Panel::Project => Some(SidebarSection::Project),
        Panel::Document => Some(SidebarSection::Knowledge),
        Panel::Todo => Some(SidebarSection::Todos),
        Panel::Logs => Some(SidebarSection::Logs),
        _ => None,
    };

    focused
        .filter(|section| expanded.contains(section))
        .or_else(|| {
            expanded
                .contains(&app.sidebar.rail_selection)
                .then_some(app.sidebar.rail_selection)
        })
}

fn sidebar_section_weight(app: &App, section: SidebarSection) -> u32 {
    let panel = panel_for_sidebar_section(section);
    let base = match section {
        SidebarSection::Diagnostics => 8,
        SidebarSection::Delivery => 14,
        SidebarSection::Skills => 10,
        SidebarSection::Project => 11,
        SidebarSection::Knowledge => 14,
        SidebarSection::Todos => 13,
        SidebarSection::Logs => 9,
    };
    let content_bonus = (app.panel_lines(panel).len().min(8) as u32) / 2;
    let focus_bonus = u32::from(app.focused_panel == panel) * 4;
    let running_bonus = u32::from(section == SidebarSection::Delivery && app.is_running) * 2;
    base + content_bonus + focus_bonus + running_bonus
}

fn panel_for_sidebar_section(section: SidebarSection) -> Panel {
    match section {
        SidebarSection::Diagnostics => Panel::Diagnostics,
        SidebarSection::Delivery => Panel::Delivery,
        SidebarSection::Skills => Panel::Skills,
        SidebarSection::Project => Panel::Project,
        SidebarSection::Knowledge => Panel::Document,
        SidebarSection::Todos => Panel::Todo,
        SidebarSection::Logs => Panel::Logs,
    }
}

fn render_sidebar_items(
    app: &App,
    panel: Panel,
    width: usize,
    height: u16,
    colors: &ColorScheme,
) -> Vec<ListItem<'static>> {
    let focused = app.focused_panel == panel;
    let selected_index = selected_sidebar_item_index(app, panel);
    wrapped_sidebar_display_lines(app, panel, width, height)
        .into_iter()
        .enumerate()
        .map(|(display_index, line)| {
            let mut base_style = sidebar_line_style(
                panel,
                line.kind,
                colors,
                focused,
                line.source_line_index,
                app.highlighted_todo_line_index(),
            );
            if focused && selected_index == Some(display_index) {
                base_style = base_style
                    .bg(colors.sidebar_rail_bg)
                    .add_modifier(Modifier::BOLD);
            }
            match line.source_line_index {
                Some(source_line_index) => {
                    let line_len = line.text.chars().count();
                    list_item_with_selection(
                        &line.text,
                        base_style,
                        app.selection_range_for_segment(
                            panel,
                            source_line_index,
                            line.source_column_start,
                            line.source_column_start + line_len,
                        ),
                    )
                }
                None => ListItem::new(Span::styled(line.text, base_style)),
            }
        })
        .collect()
}

fn selected_sidebar_item_index(app: &App, panel: Panel) -> Option<usize> {
    match panel {
        Panel::Diagnostics => app.diagnostics_state.selected(),
        Panel::Delivery => app.delivery_state.selected(),
        Panel::Skills => app.skills_state.selected(),
        Panel::Project => app.project_state.selected(),
        Panel::Document => app.document_state.selected(),
        Panel::Memory => app.memory_state.selected(),
        Panel::Todo => app.todo_state.selected(),
        Panel::Logs => app.logs_state.selected(),
        Panel::Response | Panel::SidebarRail => None,
    }
}

fn wrapped_sidebar_display_lines(
    app: &App,
    panel: Panel,
    width: usize,
    height: u16,
) -> Vec<WrappedSidebarLine> {
    sidebar_display_lines(app, panel, height)
        .into_iter()
        .flat_map(|line| {
            wrap_text_segments(&line.text, width).into_iter().map(
                move |(source_column_start, segment)| WrappedSidebarLine {
                    source_line_index: line.source_line_index,
                    source_column_start,
                    text: segment,
                    kind: line.kind,
                },
            )
        })
        .collect()
}

fn sidebar_display_lines(
    app: &App,
    panel: Panel,
    available_height: u16,
) -> Vec<SidebarDisplayLine> {
    let raw_lines = app.panel_lines(panel);
    let focused = app.focused_panel == panel;
    let preview_limit = sidebar_preview_limit(panel, focused);
    // Suppress overflow hint when the panel is tall enough to show all lines without truncation.
    let inner_h = available_height.saturating_sub(2) as usize;
    let fits_in_height = inner_h > 0 && raw_lines.len() <= inner_h;
    let effective_limit = if fits_in_height {
        usize::MAX
    } else {
        preview_limit
    };
    let mut visible = Vec::new();

    for (index, text) in raw_lines.iter().enumerate() {
        if index >= effective_limit {
            break;
        }
        let kind = classify_sidebar_line(panel, text);
        visible.push(SidebarDisplayLine {
            source_line_index: Some(index),
            kind,
            text: sidebar_preview_text(text, kind, focused),
        });
    }

    if !focused && raw_lines.len() > effective_limit {
        visible.push(SidebarDisplayLine {
            source_line_index: None,
            kind: SidebarLineKind::Hint,
            text: sidebar_overflow_hint(panel, raw_lines.len() - effective_limit),
        });
    }

    visible
}

fn sidebar_preview_limit(panel: Panel, focused: bool) -> usize {
    if focused {
        return usize::MAX;
    }

    match panel {
        Panel::Diagnostics => 6,
        Panel::Delivery => 6,
        Panel::Skills => 5,
        Panel::Project => 6,
        Panel::Document => 6,
        Panel::Memory => 6,
        Panel::Todo => 6,
        Panel::Logs => 6,
        _ => usize::MAX,
    }
}

fn sidebar_preview_text(text: &str, kind: SidebarLineKind, focused: bool) -> String {
    if focused || matches!(kind, SidebarLineKind::SectionLabel | SidebarLineKind::Hint) {
        return text.to_string();
    }

    let max_chars = match kind {
        SidebarLineKind::Hint | SidebarLineKind::SectionLabel => return text.to_string(),
        SidebarLineKind::Metric | SidebarLineKind::StatusOk | SidebarLineKind::StatusWarn => 34,
        SidebarLineKind::StatusError => 30,
        SidebarLineKind::Codeish | SidebarLineKind::LogTool => 38,
        SidebarLineKind::TodoActive | SidebarLineKind::TodoPending | SidebarLineKind::TodoDone => {
            34
        }
        SidebarLineKind::EmptyState
        | SidebarLineKind::Summary
        | SidebarLineKind::Meta
        | SidebarLineKind::Preview
        | SidebarLineKind::LogError
        | SidebarLineKind::LogText => 28,
    };

    truncate_sidebar_preview(text, max_chars)
}

fn truncate_sidebar_preview(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let preview: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{}...", preview.trim_end())
    } else {
        preview
    }
}

fn sidebar_overflow_hint(panel: Panel, hidden_lines: usize) -> String {
    let action = match panel {
        Panel::Diagnostics
        | Panel::Delivery
        | Panel::Skills
        | Panel::Project
        | Panel::Document
        | Panel::Memory => "focus panel for detail",
        Panel::Todo | Panel::Logs => "focus panel to scroll",
        _ => "open for more",
    };
    format!("… {hidden_lines} more lines · {action}")
}

fn classify_sidebar_line(panel: Panel, text: &str) -> SidebarLineKind {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return SidebarLineKind::Meta;
    }
    if trimmed.starts_with("No ") || trimmed.contains(" no ") || trimmed.ends_with("yet.") {
        return SidebarLineKind::EmptyState;
    }

    match panel {
        Panel::Todo => classify_todo_line(trimmed),
        Panel::Logs => classify_log_line(trimmed),
        _ => classify_summary_like_line(trimmed),
    }
}

fn classify_todo_line(trimmed: &str) -> SidebarLineKind {
    if trimmed.starts_with("[x]") || trimmed.starts_with("✓ ") {
        SidebarLineKind::TodoDone
    } else if trimmed.starts_with("[>]") || trimmed.starts_with("→ ") {
        SidebarLineKind::TodoActive
    } else if trimmed.starts_with("[ ]") || trimmed.starts_with("○ ") {
        SidebarLineKind::TodoPending
    } else if trimmed.starts_with('(') && trimmed.ends_with("completed)") {
        SidebarLineKind::Metric
    } else {
        classify_summary_like_line(trimmed)
    }
}

fn classify_log_line(trimmed: &str) -> SidebarLineKind {
    let lower = trimmed.to_ascii_lowercase();
    if trimmed.starts_with("[tool]") || trimmed.starts_with('$') {
        SidebarLineKind::LogTool
    } else if lower.contains("error") || lower.contains("failed") || lower.contains("panic") {
        SidebarLineKind::LogError
    } else {
        SidebarLineKind::LogText
    }
}

fn classify_summary_like_line(trimmed: &str) -> SidebarLineKind {
    if trimmed.ends_with(':') {
        return SidebarLineKind::SectionLabel;
    }
    if trimmed.starts_with("- ") {
        return SidebarLineKind::Summary;
    }
    if trimmed.starts_with("… ") {
        return SidebarLineKind::Hint;
    }
    if trimmed.starts_with("planned ")
        || trimmed.starts_with("rewrite ")
        || trimmed.starts_with("reason:")
    {
        return SidebarLineKind::Meta;
    }
    if trimmed.starts_with("hits:")
        || trimmed.starts_with("activity:")
        || trimmed.starts_with("usage:")
    {
        return SidebarLineKind::SectionLabel;
    }
    if let Some((label, value)) = trimmed.split_once(':') {
        let lower_label = label.to_ascii_lowercase();
        let lower_value = value.trim().to_ascii_lowercase();
        if lower_label.contains("status")
            || lower_label.contains("health")
            || lower_label.contains("freshness")
        {
            return classify_status_value(&lower_value);
        }
        if lower_label.contains("totals")
            || lower_label.contains("store")
            || lower_label.contains("selection")
            || lower_label.contains("archive")
            || lower_label.contains("retention")
            || lower_label.contains("queries")
            || lower_label.contains("observations")
            || lower_label.contains("recognized")
            || lower_label.contains("loaded")
            || lower_label.contains("ignored")
        {
            return SidebarLineKind::Metric;
        }
        if lower_label.contains("active")
            || lower_label.contains("pending")
            || lower_label.contains("query")
            || lower_label.contains("source step")
        {
            return if looks_codeish(value.trim()) {
                SidebarLineKind::Codeish
            } else {
                SidebarLineKind::Summary
            };
        }
        if lower_label.contains("reason")
            || lower_label.contains("rewrite")
            || lower_label.contains("recovery")
        {
            return SidebarLineKind::Meta;
        }
        if lower_value.chars().any(|ch| ch.is_ascii_digit()) {
            return SidebarLineKind::Metric;
        }
        return SidebarLineKind::Summary;
    }
    if trimmed.starts_with("  ") {
        return if looks_codeish(trimmed) {
            SidebarLineKind::Codeish
        } else {
            SidebarLineKind::Preview
        };
    }
    if looks_codeish(trimmed) {
        SidebarLineKind::Codeish
    } else {
        SidebarLineKind::Summary
    }
}

fn classify_status_value(value: &str) -> SidebarLineKind {
    if value.contains("failed") || value.contains("error") || value.contains("disabled") {
        SidebarLineKind::StatusError
    } else if value.contains("running") || value.contains("pending") || value.contains("stale") {
        SidebarLineKind::StatusWarn
    } else {
        SidebarLineKind::StatusOk
    }
}

fn looks_codeish(text: &str) -> bool {
    let trimmed = text.trim();
    trimmed.contains('/')
        || trimmed.contains("::")
        || trimmed.contains('_')
        || trimmed.contains('.')
        || trimmed.starts_with('#')
        || trimmed.starts_with('`')
}

fn sidebar_line_style(
    panel: Panel,
    kind: SidebarLineKind,
    colors: &ColorScheme,
    focused: bool,
    source_line_index: Option<usize>,
    highlighted_todo_line_index: Option<usize>,
) -> Style {
    let style = match kind {
        SidebarLineKind::EmptyState => Style::default()
            .fg(colors.muted_meta_fg)
            .add_modifier(Modifier::ITALIC),
        SidebarLineKind::Hint => Style::default()
            .fg(colors.context_label)
            .add_modifier(Modifier::ITALIC),
        SidebarLineKind::SectionLabel => Style::default()
            .fg(colors.section_header_fg)
            .add_modifier(Modifier::BOLD),
        SidebarLineKind::Summary => Style::default().fg(colors.text),
        SidebarLineKind::Metric => Style::default()
            .fg(colors.metric_emphasis_fg)
            .add_modifier(Modifier::BOLD),
        SidebarLineKind::StatusOk => Style::default()
            .fg(colors.status_idle_fg)
            .add_modifier(Modifier::BOLD),
        SidebarLineKind::StatusWarn => Style::default()
            .fg(colors.status_running_fg)
            .add_modifier(Modifier::BOLD),
        SidebarLineKind::StatusError => Style::default()
            .fg(colors.error_message)
            .add_modifier(Modifier::BOLD),
        SidebarLineKind::Meta => Style::default().fg(colors.muted_meta_fg),
        SidebarLineKind::Preview => Style::default().fg(colors.context_hint),
        SidebarLineKind::Codeish => Style::default().fg(colors.code_fg),
        SidebarLineKind::TodoDone => Style::default()
            .fg(colors.muted_meta_fg)
            .add_modifier(Modifier::DIM),
        SidebarLineKind::TodoActive => Style::default()
            .fg(colors.focus_border)
            .add_modifier(Modifier::BOLD),
        SidebarLineKind::TodoPending => Style::default().fg(colors.text),
        SidebarLineKind::LogTool => Style::default().fg(colors.code_fg),
        SidebarLineKind::LogError => Style::default().fg(colors.error_message),
        SidebarLineKind::LogText => Style::default().fg(colors.context_hint),
    };

    let style = if panel == Panel::Todo && highlighted_todo_line_index == source_line_index {
        style.fg(colors.focus_border).add_modifier(Modifier::BOLD)
    } else {
        style
    };

    if focused || preserves_sidebar_emphasis(kind) {
        style
    } else {
        style.add_modifier(Modifier::DIM)
    }
}

fn preserves_sidebar_emphasis(kind: SidebarLineKind) -> bool {
    matches!(
        kind,
        SidebarLineKind::SectionLabel
            | SidebarLineKind::Metric
            | SidebarLineKind::StatusOk
            | SidebarLineKind::StatusWarn
            | SidebarLineKind::StatusError
            | SidebarLineKind::TodoActive
            | SidebarLineKind::TodoPending
            | SidebarLineKind::LogTool
            | SidebarLineKind::LogError
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;

    #[test]
    fn sidebar_weights_prioritize_delivery_and_todo_over_logs() {
        let mut app = App::new();
        app.delivery_lines = vec!["status: running".to_string(); 8];
        app.todo_lines = vec!["○ item".to_string(); 8];
        app.log_lines = vec!["[tool] echo hi".to_string(); 8];

        assert!(
            sidebar_section_weight(&app, SidebarSection::Delivery)
                > sidebar_section_weight(&app, SidebarSection::Logs)
        );
        assert!(
            sidebar_section_weight(&app, SidebarSection::Todos)
                > sidebar_section_weight(&app, SidebarSection::Logs)
        );
    }

    #[test]
    fn expanded_sidebar_sections_include_project_panel() {
        let app = App::new();

        let sections = expanded_sidebar_sections(&app);

        assert!(sections.contains(&SidebarSection::Project));
    }

    #[test]
    fn sidebar_preview_lines_require_focus_before_detail_hint() {
        let mut app = App::new();
        app.focused_panel = Panel::Response;
        app.delivery_lines = (0..12).map(|index| format!("line {index}")).collect();

        let lines = sidebar_display_lines(&app, Panel::Delivery, 0);

        assert_eq!(lines.len(), 7);
        assert!(lines
            .last()
            .is_some_and(|line| line.kind == SidebarLineKind::Hint));
        assert!(lines
            .last()
            .is_some_and(|line| line.text.contains("focus panel for detail")));
    }

    #[test]
    fn sidebar_unfocused_preview_truncates_long_summary_lines() {
        let mut app = App::new();
        app.focused_panel = Panel::Response;
        app.delivery_lines = vec![
            "active task: prepare a very long implementation summary for the first scan"
                .to_string(),
        ];

        let lines = sidebar_display_lines(&app, Panel::Delivery, 0);

        assert_eq!(lines[0].kind, SidebarLineKind::Summary);
        assert_eq!(lines[0].text, "active task: prepare a very...".to_string());
    }

    #[test]
    fn sidebar_line_taxonomy_distinguishes_status_metric_and_empty_lines() {
        assert_eq!(
            classify_sidebar_line(Panel::Delivery, "status: enabled (ready)"),
            SidebarLineKind::StatusOk
        );
        assert_eq!(
            classify_sidebar_line(Panel::Skills, "loaded: 3"),
            SidebarLineKind::Metric
        );
        assert_eq!(
            classify_sidebar_line(Panel::Document, "No knowledge supervision snapshot yet."),
            SidebarLineKind::EmptyState
        );
        assert_eq!(
            classify_sidebar_line(Panel::Logs, "[tool] cargo test"),
            SidebarLineKind::LogTool
        );
    }

    #[test]
    fn unfocused_sidebar_keeps_metric_and_status_emphasis() {
        let colors = omega_theme::OmegaTheme::dark().render_palette();

        let metric_style = sidebar_line_style(
            Panel::Delivery,
            SidebarLineKind::Metric,
            &colors,
            false,
            None,
            None,
        );
        let summary_style = sidebar_line_style(
            Panel::Delivery,
            SidebarLineKind::Summary,
            &colors,
            false,
            None,
            None,
        );

        assert!(!metric_style.add_modifier.contains(Modifier::DIM));
        assert!(summary_style.add_modifier.contains(Modifier::DIM));
    }

    #[test]
    fn sidebar_rail_scroll_offset_keeps_selected_tab_visible() {
        let sections = [
            SidebarSection::Diagnostics,
            SidebarSection::Delivery,
            SidebarSection::Skills,
            SidebarSection::Knowledge,
            SidebarSection::Todos,
            SidebarSection::Logs,
        ];
        let widths = vec![7, 8, 8, 8, 7, 7];

        let offset = sidebar_rail_scroll_offset(&sections, &widths, SidebarSection::Knowledge, 16);

        assert_eq!(offset, 15);
    }

    #[test]
    fn sidebar_rail_item_text_uses_section_labels() {
        assert_eq!(
            sidebar_rail_item_text(SidebarSection::Project, true, "2/8"),
            " ▾ Project 2/8 "
        );
        assert_eq!(
            sidebar_rail_item_text(SidebarSection::Logs, false, ""),
            " ▸ Logs "
        );
    }
}
