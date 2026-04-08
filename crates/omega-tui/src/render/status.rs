use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use omega_keymap::InteractionMode;
use omega_theme::RenderPalette as ColorScheme;

use crate::app::{App, Panel, SessionRoutingSummary, SessionStatusSummary};

use super::overlay::overlay_hint_text;

pub(super) fn input_context_text(app: &App, sidebar_hidden: bool) -> String {
    if app.overlay_active() {
        overlay_hint_text(app).to_string()
    } else if let Some(pending_hint) = app.pending_sequence_hint() {
        pending_hint
    } else if let Some(hint) = app.command_hint.as_deref() {
        hint.to_string()
    } else if let Some(notice) = app.status_notice.as_deref() {
        notice.to_string()
    } else if sidebar_hidden {
        match app.interaction_mode {
            InteractionMode::Normal => {
                " Sidebar hidden. Space=Leader  Space jk=Toggle mode  Space Tab=Focus  Space b=Sidebar  Space /=Search  Space ↑/↓=Scroll"
                    .to_string()
            }
            InteractionMode::Insert => {
                " Sidebar hidden below 60 cols. Enter=Send  Esc=Normal  ←→/Home/End=Cursor  Del/Backspace=Delete"
                    .to_string()
            }
        }
    } else {
        match app.interaction_mode {
            InteractionMode::Normal => {
                if app.focused_panel == Panel::SidebarRail {
                    " Sidebar rail: ←/→ cycle  Enter focus  x collapse  Space b=Toggle sidebar  Space Tab=Next focus"
                        .to_string()
                } else if app.focused_panel == Panel::Diagnostics {
                    " Diagnostics: Enter/x=Open detail  Space Tab=Focus  Space b=Sidebar  Space /=Search  Space ↑/↓=Scroll"
                        .to_string()
                } else if app.focused_panel == Panel::Delivery {
                    " Delivery: Enter/x=Open detail  Space Tab=Focus  Space b=Sidebar  Space /=Search  Space ↑/↓=Scroll"
                        .to_string()
                } else if app.focused_panel == Panel::Skills {
                    " Skills: Enter/x=Open detail  Space Tab=Focus  Space b=Sidebar  Space /=Search  Space ↑/↓=Scroll"
                        .to_string()
                } else if app.focused_panel == Panel::Document {
                    " Document supervision: Enter/x=Open detail  Space Tab=Focus  Space b=Sidebar  Space /=Search  Space ↑/↓=Scroll"
                        .to_string()
                } else if app.focused_panel == Panel::Memory {
                    " Memory supervision: Enter/x=Open detail  Space Tab=Focus  Space b=Sidebar  Space /=Search  Space ↑/↓=Scroll"
                        .to_string()
                } else if app.focused_panel == Panel::Response && app.show_thinking {
                    " Response: Enter/x=Toggle thinking or open subflow/tool detail  Space Tab=Focus  Space b=Sidebar  Space /=Search  Space ↑/↓=Scroll"
                        .to_string()
                } else {
                    " Space=Leader  Space jk=Toggle mode  Space Tab=Focus  Space b=Sidebar  Space /=Search  Space ↑/↓=Scroll"
                        .to_string()
                }
            }
            InteractionMode::Insert => {
                " Enter=Send  Esc=Normal  ←→/Home/End=Cursor  Del/Backspace=Delete"
                    .to_string()
            }
        }
    }
}

pub(super) fn input_context_line(
    app: &App,
    sidebar_hidden: bool,
    colors: &ColorScheme,
) -> Line<'static> {
    let hint_val = input_context_text(app, sidebar_hidden);

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
pub(super) fn bottom_status_text(app: &App, model_name: &str, spinner_frames: &[char]) -> String {
    let segments = bottom_status_segments(app, model_name, spinner_frames);
    let mut rendered = vec![segments.model, segments.state];
    if let Some(flow) = segments.flow {
        rendered.push(flow);
    }
    if let Some(aux) = segments.aux {
        rendered.push(aux.value);
    }
    if let Some(delivery) = segments.delivery {
        rendered.push(delivery);
    }

    format!(" {} ", rendered.join(" │ "))
}

pub(super) fn bottom_status_line(
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

    if let Some(delivery) = segments.delivery {
        spans.push(Span::styled(
            "  ·  ",
            Style::default()
                .fg(colors.bar_divider)
                .bg(colors.status_bar_bg),
        ));
        spans.push(Span::styled(
            " delivery ",
            Style::default()
                .fg(colors.status_label)
                .bg(colors.status_bar_bg),
        ));
        spans.push(Span::styled(
            delivery,
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
    delivery: Option<String>,
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

    let aux = if let Some(subflow) = app.active_step_subflow() {
        let repeat_suffix = if subflow.repeat_count_for_item > 0 {
            format!(" r{}", subflow.repeat_count_for_item)
        } else {
            String::new()
        };
        Some(BottomStatusBadge {
            label: "item",
            value: format!(
                "{} {}/{}{}",
                subflow.subflow_id, subflow.item_index, subflow.item_total, repeat_suffix,
            ),
        })
    } else {
        match app.session_status.as_ref() {
            Some(SessionStatusSummary::Label(label)) => Some(BottomStatusBadge {
                label: "session",
                value: label.clone(),
            }),
            Some(SessionStatusSummary::Routing(routing)) => Some(BottomStatusBadge {
                label: "route",
                value: format_routing_badge(routing),
            }),
            None => None,
        }
    };

    BottomStatusSegments {
        model: model_name.to_string(),
        state,
        flow,
        aux,
        delivery: app.delivery_badge_text(),
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
