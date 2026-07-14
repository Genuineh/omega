use ratatui::style::{Modifier, Style};

use omega_session::{ResponseSectionState, ToolRunStatus};
use omega_theme::RenderPalette as ColorScheme;

use crate::app::{MsgKind, ResponseDisplayLine, ThinkingLineKind};

pub(super) fn response_line_style(line: &ResponseDisplayLine, colors: &ColorScheme) -> Style {
    match line.kind {
        MsgKind::User => Style::default().fg(colors.user_badge_fg),
        MsgKind::Agent => Style::default().fg(colors.assistant_badge_fg),
        MsgKind::Error => Style::default().fg(colors.error_badge_fg),
        MsgKind::Separator => Style::default().fg(colors.separator_message),
        MsgKind::Routing => {
            if line.is_header {
                response_header_style(
                    line.response_state
                        .unwrap_or(ResponseSectionState::Complete),
                    colors,
                )
            } else if is_response_meta_line(line) {
                Style::default().fg(colors.muted_meta_fg)
            } else {
                Style::default().fg(colors.context_hint)
            }
        }
        MsgKind::Step => {
            if line.is_tool_line {
                match line.tool_status {
                    None => Style::default()
                        .fg(colors.context_label)
                        .add_modifier(Modifier::BOLD),
                    Some(ToolRunStatus::Running) => Style::default()
                        .fg(colors.status_running_fg)
                        .add_modifier(Modifier::BOLD),
                    Some(ToolRunStatus::Failed) => Style::default()
                        .fg(colors.error_message)
                        .add_modifier(Modifier::BOLD),
                    _ => Style::default()
                        .fg(colors.status_idle_fg)
                        .add_modifier(Modifier::BOLD),
                }
            } else if line.is_header {
                response_header_style(
                    line.response_state
                        .unwrap_or(ResponseSectionState::Complete),
                    colors,
                )
            } else if is_response_subflow_line(line) {
                response_subflow_style(line, colors)
            } else if is_response_meta_line(line) {
                Style::default().fg(colors.muted_meta_fg)
            } else {
                Style::default().fg(colors.agent_message)
            }
        }
        MsgKind::FinalAnswer => {
            if line.is_tool_line {
                match line.tool_status {
                    None => Style::default()
                        .fg(colors.context_label)
                        .add_modifier(Modifier::BOLD),
                    Some(ToolRunStatus::Running) => Style::default()
                        .fg(colors.status_running_fg)
                        .add_modifier(Modifier::BOLD),
                    Some(ToolRunStatus::Failed) => Style::default()
                        .fg(colors.error_message)
                        .add_modifier(Modifier::BOLD),
                    _ => Style::default()
                        .fg(colors.status_idle_fg)
                        .add_modifier(Modifier::BOLD),
                }
            } else if line.is_header {
                response_header_style(
                    line.response_state
                        .unwrap_or(ResponseSectionState::Complete),
                    colors,
                )
            } else if line.text.chars().all(|ch| ch == '━') {
                Style::default().fg(colors.final_answer_border_fg)
            } else if is_response_meta_line(line) {
                Style::default().fg(colors.muted_meta_fg)
            } else {
                Style::default().fg(colors.text)
            }
        }
        MsgKind::Command => {
            if line.is_tool_line {
                match line.tool_status {
                    None => Style::default()
                        .fg(colors.context_label)
                        .add_modifier(Modifier::BOLD),
                    Some(ToolRunStatus::Running) => Style::default()
                        .fg(colors.status_running_fg)
                        .add_modifier(Modifier::BOLD),
                    Some(ToolRunStatus::Failed) => Style::default()
                        .fg(colors.error_message)
                        .add_modifier(Modifier::BOLD),
                    _ => Style::default()
                        .fg(colors.status_idle_fg)
                        .add_modifier(Modifier::BOLD),
                }
            } else if line.is_header {
                response_header_style(
                    line.response_state
                        .unwrap_or(ResponseSectionState::Complete),
                    colors,
                )
            } else if is_response_meta_line(line) {
                Style::default().fg(colors.muted_meta_fg)
            } else {
                Style::default().fg(colors.agent_message)
            }
        }
        MsgKind::Thinking => {
            let state = line
                .response_state
                .unwrap_or(ResponseSectionState::Complete);
            if line.is_header {
                thinking_header_style(state, colors)
            } else {
                match line.thinking_line_kind {
                    Some(ThinkingLineKind::Summary) => thinking_summary_style(state, colors),
                    Some(ThinkingLineKind::Placeholder) => {
                        thinking_placeholder_style(state, colors)
                    }
                    _ => thinking_body_style(state, colors),
                }
            }
        }
    }
}

fn is_response_meta_line(line: &ResponseDisplayLine) -> bool {
    if line.is_header || line.is_tool_line || !line.spans.is_empty() {
        return false;
    }

    let trimmed = line.text.trim_start();
    trimmed.starts_with("scene ")
        || trimmed.starts_with("result ")
        || trimmed.starts_with("items ")
        || trimmed.starts_with("delivery  ")
        || trimmed.starts_with("skills ")
        || trimmed.starts_with("knowledge ")
        || trimmed.starts_with("document ")
        || trimmed.starts_with("memory ")
        || trimmed.starts_with("source ")
        || trimmed.starts_with("selection ")
        || trimmed.starts_with("reason ")
}

fn is_response_subflow_line(line: &ResponseDisplayLine) -> bool {
    !line.is_header && !line.is_tool_line && line.text.trim_start().starts_with("subflow  ")
}

fn response_subflow_style(line: &ResponseDisplayLine, colors: &ColorScheme) -> Style {
    if line.text.contains("  ✕") {
        Style::default()
            .fg(colors.error_message)
            .add_modifier(Modifier::BOLD)
    } else if line.text.contains("  ◉") {
        Style::default()
            .fg(colors.status_running_fg)
            .add_modifier(Modifier::BOLD)
    } else if line.text.contains("  ●") {
        Style::default()
            .fg(colors.status_idle_fg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(colors.muted_meta_fg)
    }
}

fn response_header_style(state: ResponseSectionState, colors: &ColorScheme) -> Style {
    match state {
        ResponseSectionState::Streaming => Style::default()
            .fg(colors.status_running_fg)
            .add_modifier(Modifier::BOLD),
        ResponseSectionState::Complete => Style::default()
            .fg(colors.status_idle_fg)
            .add_modifier(Modifier::BOLD),
        ResponseSectionState::Failed => Style::default()
            .fg(colors.error_message)
            .add_modifier(Modifier::BOLD),
    }
}

fn thinking_header_style(state: ResponseSectionState, colors: &ColorScheme) -> Style {
    response_header_style(state, colors)
}

pub(super) fn response_status_symbol_style(
    line: &ResponseDisplayLine,
    colors: &ColorScheme,
) -> Option<Style> {
    if line.is_tool_line {
        return Some(match line.tool_status {
            None => Style::default()
                .fg(colors.context_label)
                .add_modifier(Modifier::BOLD),
            Some(ToolRunStatus::Running) => Style::default()
                .fg(colors.status_running_fg)
                .add_modifier(Modifier::BOLD | Modifier::SLOW_BLINK),
            Some(ToolRunStatus::Failed) => Style::default()
                .fg(colors.error_message)
                .add_modifier(Modifier::BOLD),
            Some(ToolRunStatus::Complete) => Style::default()
                .fg(colors.status_idle_fg)
                .add_modifier(Modifier::BOLD),
        });
    }

    if line.is_header {
        let state = line
            .response_state
            .unwrap_or(ResponseSectionState::Complete);
        return Some(match state {
            ResponseSectionState::Streaming => Style::default()
                .fg(colors.status_running_fg)
                .add_modifier(Modifier::BOLD | Modifier::SLOW_BLINK),
            ResponseSectionState::Complete => Style::default()
                .fg(colors.status_idle_fg)
                .add_modifier(Modifier::BOLD),
            ResponseSectionState::Failed => Style::default()
                .fg(colors.error_message)
                .add_modifier(Modifier::BOLD),
        });
    }

    if is_response_subflow_line(line) {
        return Some(if line.text.contains("  ✕") {
            Style::default()
                .fg(colors.error_message)
                .add_modifier(Modifier::BOLD)
        } else if line.text.contains("  ◉") {
            Style::default()
                .fg(colors.status_running_fg)
                .add_modifier(Modifier::BOLD | Modifier::SLOW_BLINK)
        } else if line.text.contains("  ●") {
            Style::default()
                .fg(colors.status_idle_fg)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(colors.muted_meta_fg)
        });
    }

    None
}

fn thinking_summary_style(state: ResponseSectionState, colors: &ColorScheme) -> Style {
    match state {
        ResponseSectionState::Streaming => Style::default()
            .fg(colors.focus_border)
            .add_modifier(Modifier::BOLD),
        ResponseSectionState::Complete => Style::default()
            .fg(colors.thinking_summary_fg)
            .add_modifier(Modifier::DIM | Modifier::ITALIC),
        ResponseSectionState::Failed => Style::default()
            .fg(colors.error_message)
            .add_modifier(Modifier::BOLD),
    }
}

fn thinking_body_style(state: ResponseSectionState, colors: &ColorScheme) -> Style {
    match state {
        ResponseSectionState::Failed => Style::default().fg(colors.error_message),
        ResponseSectionState::Streaming | ResponseSectionState::Complete => Style::default()
            .fg(colors.thinking_body_fg)
            .add_modifier(Modifier::DIM),
    }
}

fn thinking_placeholder_style(state: ResponseSectionState, colors: &ColorScheme) -> Style {
    thinking_body_style(state, colors).add_modifier(Modifier::ITALIC)
}
