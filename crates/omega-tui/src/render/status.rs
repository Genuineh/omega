use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use omega_keymap::InteractionMode;
use omega_theme::RenderPalette as ColorScheme;

use crate::app::{project_badge_text, App, Panel, SessionRoutingSummary, SessionStatusSummary};

use super::chrome::Glyph;
use super::overlay::overlay_hint_text;

const INPUT_INFO_ORBIT_COLUMNS: [usize; 8] = [2, 4, 4, 4, 2, 0, 0, 0];
const INPUT_INFO_ORBIT_GLYPHS: [char; 5] = [
    Glyph::COMPLETE,
    Glyph::RUNNING,
    '◎',
    Glyph::PENDING,
    Glyph::BULLET,
];

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
                " Sidebar hidden below 60 cols. Enter=Send  Shift+Enter=Newline  ↑/↓=Line  Esc=Normal  ←→/Home/End=Cursor  Del/Backspace=Delete"
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
                } else if app.focused_panel == Panel::Project {
                    " Project: Enter/x=Open detail  Space Tab=Focus  Space b=Sidebar  Space /=Search  Space ↑/↓=Scroll"
                        .to_string()
                } else if app.focused_panel == Panel::Document {
                    " Knowledge: Enter/x=Open detail  Space Tab=Focus  Space b=Sidebar  Space /=Search  Space ↑/↓=Scroll"
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
                " Enter=Send  Shift+Enter=Newline  ↑/↓=Line  Esc=Normal  ←→/Home/End=Cursor  Del/Backspace=Delete"
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
pub(super) fn input_info_text(app: &App, spinner_frames: &[char]) -> String {
    let segments = input_info_segments(app, spinner_frames);
    let mut rendered = vec![segments.model.clone()];
    if let Some(delivery_tokens) = segments.delivery_tokens {
        rendered.push(delivery_tokens);
    }
    rendered.push(segments.state_icon);
    format!(" {} ", rendered.join("   "))
}

pub(super) fn input_info_line(
    app: &App,
    spinner_frames: &[char],
    colors: &ColorScheme,
    width: usize,
) -> Line<'static> {
    let segments = input_info_segments(app, spinner_frames);
    let state_color = if app.is_running {
        colors.status_running_fg
    } else {
        colors.status_idle_fg
    };

    let mut left_text = segments.model;
    if let Some(delivery_tokens) = segments.delivery_tokens {
        left_text.push_str("   ");
        left_text.push_str(&delivery_tokens);
    }

    let state_icon = segments.state_icon;
    let left_width = left_text.chars().count();
    let state_width = state_icon.chars().count();
    let filler = width.saturating_sub(left_width.saturating_add(state_width));

    let mut spans = vec![
        Span::styled(
            left_text,
            Style::default()
                .fg(colors.text)
                .bg(colors.input_bg)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " ".repeat(filler.max(1)),
            Style::default().fg(colors.text).bg(colors.input_bg),
        ),
    ];

    if app.is_running {
        spans.extend(running_input_state_spans(app.spinner_tick, colors));
    } else {
        spans.push(Span::styled(
            state_icon,
            Style::default()
                .fg(state_color)
                .bg(colors.input_bg)
                .add_modifier(Modifier::BOLD),
        ));
    }

    Line::from(spans)
}

#[cfg(test)]
pub(super) fn bottom_status_text(app: &App, model_name: &str, spinner_frames: &[char]) -> String {
    let segments = bottom_status_segments(app, model_name, spinner_frames);
    let mut rendered = vec![segments.mode.to_string()];
    if let Some(flow) = segments.flow {
        rendered.push(flow);
    }
    if let Some(aux) = segments.aux {
        rendered.push(aux.value);
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
    let mode_color = match segments.mode {
        "NORMAL" => colors.mode_normal_fg,
        _ => colors.mode_insert_fg,
    };
    let mut spans = vec![
        Span::styled(
            " mode ",
            Style::default()
                .fg(colors.status_label)
                .bg(colors.status_bar_bg),
        ),
        Span::styled(
            segments.mode,
            Style::default()
                .fg(mode_color)
                .bg(colors.status_bar_bg)
                .add_modifier(Modifier::BOLD),
        ),
    ];

    if segments.flow.is_none() && segments.aux.is_none() {
        spans.pop();
    }

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
                .fg(colors.context_hint)
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
    Line::from(spans)
}

struct InputInfoSegments {
    model: String,
    state_icon: String,
    delivery_tokens: Option<String>,
}

struct BottomStatusSegments {
    mode: &'static str,
    flow: Option<String>,
    aux: Option<BottomStatusBadge>,
}

struct BottomStatusBadge {
    label: &'static str,
    value: String,
}

fn input_info_segments(app: &App, _spinner_frames: &[char]) -> InputInfoSegments {
    let state_icon = input_state_icon_text(app);

    InputInfoSegments {
        model: app
            .delivery_model_name
            .clone()
            .unwrap_or_else(|| "model unknown".to_string()),
        state_icon,
        delivery_tokens: app.delivery_token_badge_text(),
    }
}

fn bottom_status_segments(
    app: &App,
    _model_name: &str,
    _spinner_frames: &[char],
) -> BottomStatusSegments {
    let mode = match app.interaction_mode {
        InteractionMode::Normal => "NORMAL",
        InteractionMode::Insert => "INSERT",
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
        match (app.project_status.as_ref(), app.session_status.as_ref()) {
            (Some(project), _) => Some(BottomStatusBadge {
                label: "project",
                value: project_badge_text(project),
            }),
            (None, Some(SessionStatusSummary::Label(label))) => Some(BottomStatusBadge {
                label: "session",
                value: label.clone(),
            }),
            (None, Some(SessionStatusSummary::Routing(routing))) => Some(BottomStatusBadge {
                label: "route",
                value: format_routing_badge(routing),
            }),
            (None, None) => None,
        }
    };

    BottomStatusSegments { mode, flow, aux }
}

fn input_state_icon_text(app: &App) -> String {
    if app.is_running {
        running_input_state_cells(app.spinner_tick)
            .into_iter()
            .collect()
    } else {
        "↑".to_string()
    }
}

fn running_input_state_cells(tick: u8) -> [char; 5] {
    let head = (tick as usize / 2) % INPUT_INFO_ORBIT_COLUMNS.len();
    let mut columns = [' '; 5];
    let column = INPUT_INFO_ORBIT_COLUMNS[head];
    columns[column] = running_input_state_glyph(tick);

    columns
}

fn running_input_state_glyph(tick: u8) -> char {
    let phase = (tick as usize / 2) % INPUT_INFO_ORBIT_GLYPHS.len();
    INPUT_INFO_ORBIT_GLYPHS[phase]
}

fn running_input_state_spans(tick: u8, colors: &ColorScheme) -> Vec<Span<'static>> {
    running_input_state_cells(tick)
        .into_iter()
        .map(|character| {
            Span::styled(
                character.to_string(),
                running_input_state_style(character, colors),
            )
        })
        .collect()
}

fn running_input_state_style(character: char, colors: &ColorScheme) -> Style {
    let base = Style::default().bg(colors.input_bg);
    match character {
        Glyph::COMPLETE | Glyph::RUNNING => base
            .fg(colors.status_running_fg)
            .add_modifier(Modifier::BOLD),
        '◎' => base.fg(colors.status_running_fg),
        '○' => base.fg(colors.context_hint).add_modifier(Modifier::DIM),
        '·' => base.fg(colors.bar_divider).add_modifier(Modifier::DIM),
        _ => base.fg(colors.text),
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
