use ratatui::style::{Modifier, Style};

use omega_session::{ResponseSectionState, ToolRunStatus};
use omega_theme::RenderPalette as ColorScheme;

use crate::app::{MsgKind, ResponseDisplayLine, ThinkingLineKind};

pub(super) fn response_line_style(line: &ResponseDisplayLine, colors: &ColorScheme) -> Style {
    match line.kind {
        MsgKind::User => Style::default().fg(colors.user_message),
        MsgKind::Agent => Style::default().fg(colors.agent_message),
        MsgKind::Error => Style::default().fg(colors.error_message),
        MsgKind::Separator => Style::default().fg(colors.separator_message),
        MsgKind::Routing => {
            if line.is_header {
                Style::default()
                    .fg(colors.context_label)
                    .add_modifier(Modifier::BOLD)
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
                    Some(ToolRunStatus::Running) => Style::default().fg(colors.focus_border),
                    Some(ToolRunStatus::Failed) => Style::default().fg(colors.error_message),
                    _ => Style::default().fg(colors.context_hint),
                }
            } else if line.is_header {
                Style::default()
                    .fg(colors.focus_border)
                    .add_modifier(Modifier::BOLD)
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
                    Some(ToolRunStatus::Running) => Style::default().fg(colors.focus_border),
                    Some(ToolRunStatus::Failed) => Style::default().fg(colors.error_message),
                    _ => Style::default().fg(colors.context_hint),
                }
            } else if line.is_header {
                Style::default()
                    .fg(colors.mode_insert_fg)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(colors.text)
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

fn thinking_header_style(state: ResponseSectionState, colors: &ColorScheme) -> Style {
    match state {
        ResponseSectionState::Streaming => Style::default()
            .fg(colors.focus_border)
            .add_modifier(Modifier::BOLD),
        ResponseSectionState::Complete => Style::default()
            .fg(colors.context_label)
            .add_modifier(Modifier::BOLD),
        ResponseSectionState::Failed => Style::default()
            .fg(colors.error_message)
            .add_modifier(Modifier::BOLD),
    }
}

fn thinking_summary_style(state: ResponseSectionState, colors: &ColorScheme) -> Style {
    match state {
        ResponseSectionState::Streaming => Style::default()
            .fg(colors.focus_border)
            .add_modifier(Modifier::BOLD),
        ResponseSectionState::Complete => Style::default()
            .fg(colors.context_label)
            .add_modifier(Modifier::BOLD),
        ResponseSectionState::Failed => Style::default()
            .fg(colors.error_message)
            .add_modifier(Modifier::BOLD),
    }
}

fn thinking_body_style(state: ResponseSectionState, colors: &ColorScheme) -> Style {
    match state {
        ResponseSectionState::Failed => Style::default().fg(colors.error_message),
        ResponseSectionState::Streaming | ResponseSectionState::Complete => {
            Style::default().fg(colors.context_hint)
        }
    }
}

fn thinking_placeholder_style(state: ResponseSectionState, colors: &ColorScheme) -> Style {
    thinking_body_style(state, colors).add_modifier(Modifier::ITALIC)
}
