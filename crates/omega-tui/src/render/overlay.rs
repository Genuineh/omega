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
    DocumentNavigatorRailItem, OverlayState, StepDetailContent, StepDetailOverlay,
    StepDetailRailKind, StepDetailRailItem, TurnDetailOverlay, TurnDetailSection,
};
use crate::render::chrome::Glyph;
use crate::render::chrome::PanelTitle;

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
                Panel::SidebarRail => PanelTitle::SIDEBAR,
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
            let scroll =
                clamp_overlay_scroll(results.scroll, results.lines.len(), inner.height as usize);
            let show_footer =
                should_show_overlay_footer(scroll, results.lines.len(), inner.height as usize);
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
                overlay_scroll_footer_text(
                    scroll,
                    content_rect.height as usize,
                    results.lines.len(),
                ),
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
            let scroll =
                clamp_overlay_scroll(detail.scroll, detail.lines.len(), inner.height as usize);
            let show_footer =
                should_show_overlay_footer(scroll, detail.lines.len(), inner.height as usize);
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
                overlay_scroll_footer_text(
                    scroll,
                    content_rect.height as usize,
                    detail.lines.len(),
                ),
            );
        }
        OverlayState::StepDetail(step) => {
            render_step_detail(frame, step, overlay_rect, colors);
        }
        OverlayState::TurnDetail(turn) => {
            render_turn_detail(frame, turn, overlay_rect, colors);
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
                .constraints([
                    Constraint::Length(1),
                    Constraint::Min(1),
                    Constraint::Length(1),
                ])
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
                    Paragraph::new("No linked entries.").style(
                        Style::default()
                            .fg(colors.context_hint)
                            .bg(colors.overlay_bg),
                    ),
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
                [
                    Constraint::Length(3),
                    Constraint::Min(1),
                    Constraint::Length(1),
                ]
            } else {
                [
                    Constraint::Min(1),
                    Constraint::Length(1),
                    Constraint::Length(0),
                ]
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
                    Paragraph::new(picker.empty_state_text()).style(
                        Style::default()
                            .fg(colors.context_hint)
                            .bg(colors.overlay_bg),
                    ),
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

fn padded_rect(
    rect: ratatui::layout::Rect,
    horizontal: u16,
    vertical: u16,
) -> ratatui::layout::Rect {
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
        Some(OverlayState::StepDetail(_)) => {
            " Step detail: ↑/↓ rail nav  Enter=open tool  Esc=back"
        }
        Some(OverlayState::InputPrompt(_)) => " Input prompt: type freely  Enter=Submit  Esc=Close",
        Some(OverlayState::TurnDetail(_)) => {
            " Turn detail: ↑/↓ scroll content  Esc=back"
        }
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
    let mut spans = vec![Span::styled(
        prefix.to_string(),
        Style::default().fg(colors.input_text),
    )];

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
    let mut spans = vec![Span::styled(if selected { "› " } else { "  " }, base_style)];
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
    let mut spans = vec![Span::styled(if selected { "> " } else { "  " }, base_style)];
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
    let position =
        overlay_scroll_footer_text(scroll, viewport_height, total_lines).unwrap_or_else(|| {
            format!(
                "lines 1-{}/{}",
                total_lines.min(viewport_height),
                total_lines
            )
        });
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

// ---------------------------------------------------------------------------
// T-55: StepDetailOverlay rendering
// ---------------------------------------------------------------------------

fn render_step_detail(
    frame: &mut Frame,
    step: &crate::overlay::StepDetailOverlay,
    overlay_rect: ratatui::layout::Rect,
    colors: &ColorScheme,
) {
    use crate::overlay::DocumentNavigatorFocus;

    let block = Block::default()
        .border_type(colors.overlay_border_type)
        .title(step.title.as_str())
        .borders(Borders::ALL)
        .border_style(overlay_border_style(colors))
        .style(Style::default().bg(colors.overlay_bg));
    let inner = padded_rect(block.inner(overlay_rect), 1, 1);
    frame.render_widget(block, overlay_rect);

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // subtitle / section id
            Constraint::Min(1),    // rail + content
            Constraint::Length(1), // footer
        ])
        .split(inner);

    // Subtitle.
    frame.render_widget(
        Paragraph::new(format!(" section: {}", step.section_id)).style(
            Style::default()
                .fg(colors.context_hint)
                .bg(colors.overlay_bg),
        ),
        sections[0],
    );

    // Rail + content panes.
    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(24), Constraint::Min(1)])
        .split(sections[1]);

    let rail_block = Block::default()
        .border_type(colors.overlay_border_type)
        .title(if step.focus == DocumentNavigatorFocus::Rail {
            " Detail * "
        } else {
            " Detail "
        })
        .borders(Borders::ALL)
        .border_style(overlay_border_style(colors))
        .style(Style::default().bg(colors.overlay_bg));
    let content_title = step
        .rail
        .get(step.selected)
        .map(|item| format!(" {} ", item.kind.label()))
        .unwrap_or_else(|| " Content ".to_string());
    let content_block = Block::default()
        .border_type(colors.overlay_border_type)
        .title(content_title)
        .borders(Borders::ALL)
        .border_style(overlay_border_style(colors))
        .style(Style::default().bg(colors.overlay_bg));
    let rail_inner = padded_rect(rail_block.inner(panes[0]), 1, 0);
    let content_inner = padded_rect(content_block.inner(panes[1]), 1, 0);
    frame.render_widget(rail_block, panes[0]);
    frame.render_widget(content_block, panes[1]);

    // Rail items.
    let rail_rows: Vec<ListItem> = step
        .rail
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let selected = i == step.selected;
            let marker = if selected { "> " } else { "  " };
            let label_color = if selected {
                colors.focus_border
            } else {
                colors.text
            };
            let style = Style::default().fg(label_color);
            let mut spans = vec![
                Span::styled(marker, style),
                Span::styled(item.kind.label(), style.add_modifier(Modifier::BOLD)),
            ];
            if !item.count_label.is_empty() {
                spans.push(Span::styled(
                    format!(" {}", item.count_label),
                    Style::default().fg(colors.context_hint),
                ));
            }
            ListItem::new(Line::from(spans))
        })
        .collect();
    if rail_rows.is_empty() {
        frame.render_widget(
            Paragraph::new("No detail available.").style(
                Style::default()
                    .fg(colors.context_hint)
                    .bg(colors.overlay_bg),
            ),
            rail_inner,
        );
    } else {
        let list = List::new(rail_rows)
            .style(Style::default().bg(colors.overlay_bg));
        frame.render_widget(list, rail_inner);
    }

    // Content lines. T-69 bug fix: use `active_content()` (i.e.
    // `content_per_rail[selected]`) instead of the legacy
    // `step.content` field. The legacy field was a snapshot of
    // the initial selection; rail navigation never updated it,
    // so the right pane appeared frozen.
    let content_lines = step_detail_content_lines(step.active_content());
    let total = content_lines.len();
    let viewport = content_inner.height as usize;
    let scroll = step.content_scroll.min(total.saturating_sub(viewport));
    let visible: Vec<ListItem> = content_lines
        .into_iter()
        .skip(scroll)
        .take(viewport)
        .map(|s| {
            ListItem::new(Line::from(Span::styled(
                s,
                Style::default().fg(colors.text).bg(colors.overlay_bg),
            )))
        })
        .collect();
    let list = List::new(visible)
        .style(Style::default().bg(colors.overlay_bg));
    frame.render_widget(list, content_inner);

    // Footer.
    let footer = if total > viewport {
        let end = (scroll + viewport).min(total);
        Some(format!(
            "lines {}-{}/{}  ·  Esc=back  ↑/↓=rail  Enter=open tool",
            scroll + 1,
            end,
            total
        ))
    } else {
        Some("Esc=back  ↑/↓=rail  Enter=open tool".to_string())
    };
    render_overlay_footer(frame, sections[2], colors, footer);
}

/// Convert a `StepDetailContent` variant into a flat list of text
/// lines for the content pane. The list is just `String`s — styling
/// is applied at render time.
fn step_detail_content_lines(
    content: &crate::overlay::StepDetailContent,
) -> Vec<String> {
    use crate::overlay::StepDetailContent;
    match content {
        StepDetailContent::Tools(tools) => {
            if tools.is_empty() {
                return vec!["(no tool runs in this section)".to_string()];
            }
            let mut lines = Vec::new();
            for (i, t) in tools.iter().enumerate() {
                lines.push(format!(
                    "# Tool {}: {} ({})",
                    i + 1,
                    t.name,
                    t.status_label
                ));
                lines.push(format!("  invocation: {}", t.invocation_preview));
                if let Some(result) = &t.result_preview {
                    lines.push(format!("  result: {result}"));
                }
                lines.push(String::new());
            }
            lines
        }
        StepDetailContent::Subflows(subs) => {
            if subs.is_empty() {
                return vec!["(no subflows in this section)".to_string()];
            }
            let mut lines = Vec::new();
            for s in subs {
                let progress = match (s.current_index, s.total) {
                    (Some(i), Some(t)) => format!("  ({}/{})", i, t),
                    _ => String::new(),
                };
                lines.push(format!(
                    "- {} ({}){progress}",
                    s.label, s.status_label
                ));
            }
            lines
        }
        StepDetailContent::Scene(scene) => match scene {
            Some(s) => {
                let mut lines = Vec::new();
                if let Some(scene_id) = &s.scene_id {
                    lines.push(format!("scene:    {scene_id}"));
                }
                if let Some(workflow_id) = &s.workflow_id {
                    let role = s.workflow_role.as_deref().unwrap_or("unknown");
                    lines.push(format!("workflow: {role}:{workflow_id}"));
                }
                if let Some(step_id) = &s.step_id {
                    let label = s.step_label.as_deref().unwrap_or("");
                    lines.push(format!("step:     {step_id} {label}"));
                }
                if lines.is_empty() {
                    lines.push("(no scene metadata)".to_string());
                }
                lines
            }
            None => vec!["(no scene context for this section)".to_string()],
        },
        StepDetailContent::Output(lines) => {
            if lines.is_empty() {
                vec!["(no output text)".to_string()]
            } else {
                lines.clone()
            }
        }
        StepDetailContent::Diagnostics(lines) => {
            if lines.is_empty() {
                vec!["(no diagnostics for this section)".to_string()]
            } else {
                lines.clone()
            }
        }
    }
}

// ---------------------------------------------------------------------------
// T-61: TurnDetailOverlay rendering
// ---------------------------------------------------------------------------

fn render_turn_detail(
    frame: &mut Frame,
    turn: &TurnDetailOverlay,
    overlay_rect: ratatui::layout::Rect,
    colors: &ColorScheme,
) {
    let block = Block::default()
        .border_type(colors.overlay_border_type)
        .title(turn.title.as_str())
        .borders(Borders::ALL)
        .border_style(overlay_border_style(colors))
        .style(Style::default().bg(colors.overlay_bg));
    let inner = padded_rect(block.inner(overlay_rect), 1, 1);
    frame.render_widget(block, overlay_rect);

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // user text row
            Constraint::Min(1),    // content
            Constraint::Length(1), // footer
        ])
        .split(inner);

        // User text row (1 line, prefixed with "▶ You").
        let user_text_line = if turn.user_text.is_empty() {
            Line::from("")
        } else {
            Line::from(vec![
                Span::styled(
                    format!("{} You  ", Glyph::BULLET),
                    Style::default()
                        .fg(colors.user_badge_fg)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(turn.user_text.clone(), Style::default().fg(colors.text)),
            ])
        };
        frame.render_widget(
            Paragraph::new(user_text_line).style(Style::default().bg(colors.overlay_bg)),
            sections[0],
        );

    // Content area: each TurnDetailSection becomes a labelled
    // block of body lines. Sections are stacked vertically.
    let mut all_lines: Vec<Line<'static>> = Vec::new();
    for section in &turn.sections {
        all_lines.push(Line::from(Span::styled(
            format!("── {} ", section.label),
            Style::default()
                .fg(colors.context_label)
                .add_modifier(Modifier::BOLD),
        )));
        for body_line in &section.body {
            all_lines.push(Line::from(Span::styled(
                body_line.clone(),
                Style::default().fg(colors.text).bg(colors.overlay_bg),
            )));
        }
        all_lines.push(Line::from(""));
    }
    let total = all_lines.len();
    let viewport = sections[1].height as usize;
    let scroll = turn.scroll.min(total.saturating_sub(viewport));
    let visible: Vec<ListItem> = all_lines
        .into_iter()
        .skip(scroll)
        .take(viewport)
        .map(|l| ListItem::new(l))
        .collect();
    let list = List::new(visible).style(Style::default().bg(colors.overlay_bg));
    frame.render_widget(list, sections[1]);

    // Footer.
    let footer = if total > viewport {
        let end = (scroll + viewport).min(total);
        Some(format!(
            "lines {}-{}/{}  ·  Esc=back  ↑/↓=scroll",
            scroll + 1,
            end,
            total
        ))
    } else {
        Some("Esc=back".to_string())
    };
    render_overlay_footer(frame, sections[2], colors, footer);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overlay::{
        OverlaySize, StepDetailContent, StepDetailOverlay, ToolRunSummary,
    };
    use crate::app::Panel;

    fn empty_step_overlay(content: StepDetailContent) -> StepDetailOverlay {
        StepDetailOverlay {
            origin_panel: Panel::Response,
            section_id: "test-section".into(),
            title: "Test".into(),
            rail: Vec::new(),
            selected: 0,
            focus: crate::overlay::DocumentNavigatorFocus::Rail,
            content_per_rail: vec![content.clone()],
            content,
            content_scroll: 0,
            dismiss_on_backdrop: true,
        }
    }

    #[test]
    fn tools_content_with_one_tool_includes_name_and_status() {
        let content = StepDetailContent::Tools(vec![ToolRunSummary {
            id: "t1".into(),
            name: "search_knowledge".into(),
            status_label: "complete".into(),
            invocation_preview: "query=foo".into(),
            result_preview: Some("3 hits".into()),
        }]);
        let lines = step_detail_content_lines(&content);
        assert!(lines.iter().any(|l| l.contains("search_knowledge")));
        assert!(lines.iter().any(|l| l.contains("complete")));
        assert!(lines.iter().any(|l| l.contains("query=foo")));
        assert!(lines.iter().any(|l| l.contains("3 hits")));
    }

    #[test]
    fn tools_content_with_no_tools_shows_placeholder() {
        let content = StepDetailContent::Tools(Vec::new());
        let lines = step_detail_content_lines(&content);
        assert_eq!(lines, vec!["(no tool runs in this section)"]);
    }

    #[test]
    fn scene_content_with_no_data_shows_placeholder() {
        let content = StepDetailContent::Scene(None);
        let lines = step_detail_content_lines(&content);
        assert_eq!(lines, vec!["(no scene context for this section)"]);
    }

    #[test]
    fn scene_content_with_data_includes_fields() {
        use crate::overlay::SceneContext;
        let content = StepDetailContent::Scene(Some(SceneContext {
            scene_id: Some("chat".into()),
            workflow_id: Some("chat-1".into()),
            workflow_role: Some("child".into()),
            step_id: Some("step-1".into()),
            step_label: Some("Report".into()),
        }));
        let lines = step_detail_content_lines(&content);
        assert!(lines.iter().any(|l| l.contains("chat")));
        assert!(lines.iter().any(|l| l.contains("child:chat-1")));
        assert!(lines.iter().any(|l| l.contains("step-1")));
    }

    #[test]
    fn output_content_passes_through_lines() {
        let content = StepDetailContent::Output(vec!["line a".into(), "line b".into()]);
        let lines = step_detail_content_lines(&content);
        assert_eq!(lines, vec!["line a", "line b"]);
    }

    #[test]
    fn empty_output_shows_placeholder() {
        let content = StepDetailContent::Output(Vec::new());
        let lines = step_detail_content_lines(&content);
        assert_eq!(lines, vec!["(no output text)"]);
    }

    #[test]
    fn step_detail_overlay_size_is_large() {
        let overlay = empty_step_overlay(StepDetailContent::Diagnostics(Vec::new()));
        assert_eq!(OverlayState::StepDetail(overlay).size(), OverlaySize::Large);
    }
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

    Some(format!(
        "lines {start}-{end}/{total_lines}  ·  mouse wheel or PgUp/PgDn"
    ))
}
