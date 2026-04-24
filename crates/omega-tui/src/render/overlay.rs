use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::{Line, Span},
    Frame,
};

use omega_theme::RenderPalette as ColorScheme;

use crate::app::{App, Panel};
use crate::overlay::{
    overlay_area, ConfirmChoice, DocumentNavigatorFocus, DocumentNavigatorOverlay,
    DocumentNavigatorRailItem, OverlayState,
};

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
                " > ",
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
        OverlayState::DocumentNavigator(navigator) => {
            let block = Block::default()
                .border_type(colors.overlay_border_type)
                .title(navigator.request.title.as_str())
                .borders(Borders::ALL)
                .border_style(overlay_border_style(colors))
                .style(Style::default().bg(colors.overlay_bg));
            let inner = padded_rect(block.inner(overlay_rect), 1, 1);
            frame.render_widget(block, overlay_rect);

            let sections = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(1), Constraint::Min(1), Constraint::Length(1)])
                .split(inner);
            frame.render_widget(
                Paragraph::new(navigator.request.origin_label.as_str()).style(
                    Style::default()
                        .fg(colors.context_hint)
                        .bg(colors.overlay_bg),
                ),
                sections[0],
            );

            let panes = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Length(34), Constraint::Min(1)])
                .split(sections[1]);

            let rail_block = Block::default()
                .border_type(colors.overlay_border_type)
                .title(if navigator.focus == DocumentNavigatorFocus::Rail {
                    " Links * "
                } else {
                    " Links "
                })
                .borders(Borders::ALL)
                .border_style(overlay_border_style(colors))
                .style(Style::default().bg(colors.overlay_bg));
            let active_title = navigator
                .active_entry()
                .map(|entry| format!(" {} [{}] ", entry.label, entry.kind.label()))
                .unwrap_or_else(|| " Entry ".to_string());
            let content_block = Block::default()
                .border_type(colors.overlay_border_type)
                .title(active_title)
                .borders(Borders::ALL)
                .border_style(overlay_border_style(colors))
                .style(Style::default().bg(colors.overlay_bg));
            let rail_inner = padded_rect(rail_block.inner(panes[0]), 1, 0);
            let content_inner = padded_rect(content_block.inner(panes[1]), 1, 0);
            frame.render_widget(rail_block, panes[0]);
            frame.render_widget(content_block, panes[1]);

            let rail_rows = build_document_navigator_rows(navigator, colors);
            if rail_rows.is_empty() {
                frame.render_widget(
                    Paragraph::new("No linked entries.")
                        .style(Style::default().fg(colors.context_hint).bg(colors.overlay_bg)),
                    rail_inner,
                );
            } else {
                let selected_row = rail_rows
                    .iter()
                    .position(|(entry_index, _)| *entry_index == Some(navigator.selected))
                    .unwrap_or(0);
                let viewport = rail_inner.height as usize;
                let scroll = selected_row
                    .saturating_sub(viewport.saturating_sub(1))
                    .min(rail_rows.len().saturating_sub(viewport));
                let items = rail_rows
                    .into_iter()
                    .skip(scroll)
                    .take(viewport)
                    .map(|(_, item)| item)
                    .collect::<Vec<_>>();
                frame.render_widget(
                    List::new(items).style(Style::default().bg(colors.overlay_bg)),
                    rail_inner,
                );
            }

            let content_lines = navigator
                .active_entry()
                .map(document_navigator_content_lines)
                .unwrap_or_else(|| vec!["No entry selected.".to_string()]);
            let scroll = clamp_overlay_scroll(
                navigator.content_scroll,
                content_lines.len(),
                content_inner.height as usize,
            );
            let items = content_lines
                .iter()
                .skip(scroll)
                .take(content_inner.height as usize)
                .map(|line| ListItem::new(line.clone()))
                .collect::<Vec<_>>();
            frame.render_widget(
                List::new(items).style(Style::default().fg(colors.text).bg(colors.overlay_bg)),
                content_inner,
            );
            frame.render_widget(
                Paragraph::new(document_navigator_footer_text(
                    navigator,
                    scroll,
                    content_inner.height as usize,
                    content_lines.len(),
                ))
                .style(
                    Style::default()
                        .fg(colors.context_hint)
                        .bg(colors.overlay_bg),
                ),
                sections[2],
            );
        }
        OverlayState::Picker(picker) => {
            let block = Block::default()
                .border_type(colors.overlay_border_type)
                .title(picker.title())
                .borders(Borders::ALL)
                .border_style(overlay_border_style(colors))
                .style(Style::default().bg(colors.overlay_bg));
            let inner = padded_rect(block.inner(overlay_rect), 1, 1);
            frame.render_widget(block, overlay_rect);
            let constraints = if picker.filter_enabled() {
                [Constraint::Length(3), Constraint::Min(1), Constraint::Length(1)]
            } else {
                [Constraint::Min(1), Constraint::Length(1), Constraint::Length(0)]
            };
            let sections = Layout::default()
                .direction(Direction::Vertical)
                .constraints(constraints)
                .split(inner);

            let (list_rect, footer_rect) = if picker.filter_enabled() {
                frame.render_widget(
                    Paragraph::new(Line::from(render_overlay_input(
                        " / ",
                        picker.filter_query.as_str(),
                        picker.filter_cursor_pos,
                        colors,
                    )))
                    .style(Style::default().fg(colors.input_text).bg(colors.overlay_bg)),
                    sections[0],
                );
                (sections[1], sections[2])
            } else {
                (sections[0], sections[1])
            };

            if picker.visible_items_len() == 0 {
                frame.render_widget(
                    Paragraph::new(picker.empty_state_text())
                        .style(Style::default().fg(colors.context_hint).bg(colors.overlay_bg)),
                    list_rect,
                );
            } else {
                let viewport = list_rect.height as usize;
                let scroll = picker
                    .selected
                    .saturating_sub(viewport.saturating_sub(1))
                    .min(picker.visible_items_len().saturating_sub(viewport));
                let items: Vec<ListItem> = (scroll..picker.visible_items_len())
                    .take(viewport)
                    .filter_map(|index| picker.visible_item(index).map(|item| (index, item)))
                    .map(|(index, item)| render_picker_item(index == picker.selected, item, colors))
                    .collect();
                frame.render_widget(
                    List::new(items).style(Style::default().bg(colors.overlay_bg)),
                    list_rect,
                );
            }

            frame.render_widget(
                Paragraph::new(picker.footer_hints().join("  ")).style(
                    Style::default()
                        .fg(colors.context_hint)
                        .bg(colors.overlay_bg),
                ),
                footer_rect,
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
                    " > ",
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
        Some(OverlayState::DocumentNavigator(_)) => {
            " Navigator: Tab switches focus  ↑/↓ move or scroll  Enter opens rail item  Esc=Close"
        }
        Some(OverlayState::Picker(_)) => {
            " Picker popup: ↑/↓/j/k move  Enter=Primary action  Ctrl-*=Actions  /=Filter  Esc=Close"
        }
        Some(OverlayState::InputPrompt(_)) => " Input prompt: type freely  Enter=Submit  Esc=Close",
        None => "",
    }
}

fn render_overlay_input(
    prefix: &str,
    value: &str,
    cursor_pos: usize,
    colors: &ColorScheme,
) -> Vec<Span<'static>> {
    let chars: Vec<char> = value.chars().collect();
    let mut spans = vec![Span::styled(prefix.to_string(), Style::default().fg(colors.input_text))];

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

fn render_picker_item(
    selected: bool,
    item: &omega_session::OperatorPickerItem,
    colors: &ColorScheme,
) -> ListItem<'static> {
    let base_style = if selected {
        Style::default()
            .fg(colors.focus_border)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(colors.text)
    };
    let mut spans = vec![Span::styled(
        if selected { "› " } else { "  " },
        base_style,
    )];
    spans.push(Span::styled(item.title.clone(), base_style));

    if let Some(subtitle) = item.subtitle.as_deref() {
        spans.push(Span::styled(
            format!(" — {subtitle}"),
            Style::default().fg(colors.context_hint),
        ));
    }
    for badge in &item.badges {
        spans.push(Span::styled(
            format!(" [{badge}]"),
            Style::default().fg(colors.context_hint),
        ));
    }
    if let Some(disabled_reason) = item.disabled_reason.as_deref() {
        spans.push(Span::styled(
            format!(" ({disabled_reason})"),
            Style::default().fg(colors.context_hint),
        ));
    }

    ListItem::new(Line::from(spans))
}

fn build_document_navigator_rows(
    navigator: &DocumentNavigatorOverlay,
    colors: &ColorScheme,
) -> Vec<(Option<usize>, ListItem<'static>)> {
    let items = navigator.visible_items();
    let mut rows = Vec::new();
    let mut last_group = None;
    for (index, item) in items.iter().enumerate() {
        if last_group != Some(item.group) {
            rows.push((
                None,
                ListItem::new(Line::from(Span::styled(
                    item.group.label().to_string(),
                    Style::default()
                        .fg(colors.context_label)
                        .add_modifier(Modifier::BOLD),
                ))),
            ));
            last_group = Some(item.group);
        }
        rows.push((
            Some(index),
            render_document_navigator_rail_item(
                index == navigator.selected,
                navigator.request.active_entry_id == item.id,
                item,
                colors,
            ),
        ));
    }
    rows
}

fn render_document_navigator_rail_item(
    selected: bool,
    active: bool,
    item: &DocumentNavigatorRailItem,
    colors: &ColorScheme,
) -> ListItem<'static> {
    let base_style = if selected {
        Style::default()
            .fg(colors.focus_border)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(colors.text)
    };
    let mut spans = vec![Span::styled(
        if selected { "> " } else { "  " },
        base_style,
    )];
    spans.push(Span::styled(item.label.clone(), base_style));
    spans.push(Span::styled(
        format!(" [{}]", item.kind.label()),
        Style::default().fg(colors.context_hint),
    ));
    if active {
        spans.push(Span::styled(
            " [open]".to_string(),
            Style::default().fg(colors.context_label),
        ));
    }
    if let Some(subtitle) = item.subtitle.as_deref() {
        spans.push(Span::styled(
            format!(" - {subtitle}"),
            Style::default().fg(colors.context_hint),
        ));
    }

    ListItem::new(Line::from(spans))
}

fn document_navigator_content_lines(entry: &omega_session::DocumentNavigatorEntry) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(subtitle) = entry.body.subtitle.as_deref() {
        lines.push(subtitle.to_string());
    }
    if !entry.body.breadcrumbs.is_empty() {
        lines.push(format!("Path: {}", entry.body.breadcrumbs.join(" > ")));
    }
    lines.push(format!("Format: {}", entry.body.kind.label()));
    if !entry.body.lines.is_empty() {
        lines.push(String::new());
        lines.extend(entry.body.lines.iter().cloned());
    }
    lines
}

fn document_navigator_footer_text(
    navigator: &DocumentNavigatorOverlay,
    scroll: usize,
    viewport_height: usize,
    total_lines: usize,
) -> String {
    let focus = match navigator.focus {
        DocumentNavigatorFocus::Rail => "rail",
        DocumentNavigatorFocus::Content => "content",
    };
    let position = overlay_scroll_footer_text(scroll, viewport_height, total_lines)
        .unwrap_or_else(|| format!("lines 1-{}/{}", total_lines.min(viewport_height), total_lines));
    format!("Tab=Focus ({focus})  Enter=Open  Esc=Close  {position}")
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
