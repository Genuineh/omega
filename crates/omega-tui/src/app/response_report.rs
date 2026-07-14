//! Report rendering helpers for the response panel.
//!
//! These are the leaf-level text munging functions that turn a
//! `ResponseCard` body into a list of styled display lines. They are
//! pure functions — no `&mut App`, no IO, no global state — so they
//! can live in their own module and be unit-tested in isolation.
//!
//! Extracted from `app/response.rs` as part of `docs/TODO.md` Task 39G
//! to keep the App-lifecycle methods in `response.rs` from being
//! overshadowed by helper code that has nothing to do with App state.

use super::ResponseCardSectionKind;
use super::{Msg, ResponseDisplayLine, ThinkingLineKind};

use crate::render::markdown::{parse_markdown_lines, MdLineKind, StyledSpan};

pub(super) fn push_report_section(
    sections: &mut Vec<(Option<String>, String)>,
    title: Option<String>,
    lines: &mut Vec<String>,
) {
    let body = lines.join("\n");
    lines.clear();
    if title.is_none() && body.trim().is_empty() {
        return;
    }
    if title.is_some() && body.trim().is_empty() {
        return;
    }
    sections.push((title, body));
}

pub(super) fn parse_section_heading(line: &str) -> Option<String> {
    let trimmed = line.trim();
    for prefix in ["## ", "### ", "# "] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            let title = rest.trim();
            if !title.is_empty() {
                return Some(title.to_string());
            }
        }
    }
    None
}

pub(super) fn classify_report_section(title: &str) -> ResponseCardSectionKind {
    let normalized = normalize_section_title(title);
    match normalized.as_str() {
        "results summary" | "result summary" | "summary" => ResponseCardSectionKind::ResultsSummary,
        "changes made" | "changes" | "change summary" => ResponseCardSectionKind::ChangesMade,
        "verification" | "validation" | "tests" | "test results" => {
            ResponseCardSectionKind::Verification
        }
        "usage" | "how to use" => ResponseCardSectionKind::Usage,
        "optional next step" | "optional next steps" | "next step" | "next steps" => {
            ResponseCardSectionKind::OptionalNextStep
        }
        "key points" | "highlights" | "key takeaways" => ResponseCardSectionKind::KeyPoints,
        _ => ResponseCardSectionKind::Custom,
    }
}

pub(super) fn normalize_section_title(title: &str) -> String {
    let mut normalized = String::with_capacity(title.len());
    let mut previous_space = false;
    for ch in title.chars() {
        if ch.is_ascii_alphanumeric() {
            normalized.push(ch.to_ascii_lowercase());
            previous_space = false;
        } else if !previous_space {
            normalized.push(' ');
            previous_space = true;
        }
    }
    normalized.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(super) fn build_report_section_header_line(
    message: &Msg,
    title: &str,
    summary: Option<String>,
    colors: &omega_theme::RenderPalette,
) -> ResponseDisplayLine {
    let title_style = ratatui::style::Style::default()
        .fg(colors.section_header_fg)
        .add_modifier(ratatui::style::Modifier::BOLD);
    let divider_style = ratatui::style::Style::default().fg(colors.table_border_fg);
    let summary_style = ratatui::style::Style::default()
        .fg(colors.muted_meta_fg)
        .bg(colors.summary_badge_bg)
        .add_modifier(ratatui::style::Modifier::BOLD);
    let text = if let Some(summary) = summary.as_deref() {
        format!("  {title}  {summary}")
    } else {
        format!("  {title}")
    };
    let mut spans = vec![
        StyledSpan {
            text: "  ".to_string(),
            style: divider_style,
        },
        StyledSpan {
            text: title.to_string(),
            style: title_style,
        },
    ];
    if let Some(summary) = summary {
        spans.push(StyledSpan {
            text: "  ".to_string(),
            style: ratatui::style::Style::default(),
        });
        spans.push(StyledSpan {
            text: summary,
            style: summary_style,
        });
    }
    ResponseDisplayLine {
        kind: message.kind,
        text,
        is_header: false,
        message_id: message.id.clone(),
        action: None,
        is_tool_line: false,
        tool_status: None,
        response_state: None,
        thinking_line_kind: None,
        spans,
    }
}

pub(super) fn render_markdown_buffer(
    message: &Msg,
    text: &str,
    body_indent: &str,
    body_indent_style: ratatui::style::Style,
    base_style: ratatui::style::Style,
    colors: &omega_theme::RenderPalette,
) -> Vec<ResponseDisplayLine> {
    parse_markdown_lines(text, base_style, colors)
        .into_iter()
        .map(|md_line| {
            let plain: String = md_line
                .spans
                .iter()
                .map(|span| span.text.as_str())
                .collect();
            let prefixed_spans = {
                let mut spans = vec![StyledSpan {
                    text: body_indent.to_string(),
                    style: body_indent_style,
                }];
                spans.extend(md_line.spans);
                spans
            };
            ResponseDisplayLine {
                kind: message.kind,
                text: format!("{body_indent}{plain}"),
                is_header: false,
                message_id: message.id.clone(),
                action: None,
                is_tool_line: false,
                tool_status: None,
                response_state: None,
                thinking_line_kind: md_line
                    .kind
                    .eq(&MdLineKind::BlankLine)
                    .then_some(ThinkingLineKind::Body),
                spans: prefixed_spans,
            }
        })
        .collect()
}

pub(super) fn is_markdown_table_header(lines: &[&str], index: usize) -> bool {
    index + 1 < lines.len()
        && is_markdown_table_row(lines[index])
        && is_markdown_table_separator(lines[index + 1])
}

pub(super) fn is_markdown_table_row(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with('|')
        && trimmed.ends_with('|')
        && trimmed[1..trimmed.len() - 1].contains('|')
}

pub(super) fn is_markdown_table_separator(line: &str) -> bool {
    let trimmed = line.trim();
    if !trimmed.starts_with('|') || !trimmed.ends_with('|') {
        return false;
    }
    trimmed
        .trim_matches('|')
        .split('|')
        .map(str::trim)
        .all(|part| !part.is_empty() && part.chars().all(|ch| matches!(ch, '-' | ':' | ' ')))
}

pub(super) fn render_markdown_table_lines(
    message: &Msg,
    body_indent: &str,
    body_indent_style: ratatui::style::Style,
    colors: &omega_theme::RenderPalette,
    block: &[String],
) -> Vec<ResponseDisplayLine> {
    if block.len() < 2 {
        return Vec::new();
    }

    let rows: Vec<Vec<String>> = block
        .iter()
        .map(|line| parse_markdown_table_row(line))
        .collect();
    let Some(header) = rows.first() else {
        return Vec::new();
    };
    let data_rows = &rows[2..];
    let column_count = rows.iter().map(Vec::len).max().unwrap_or(0);
    let widths = (0..column_count)
        .map(|column| {
            rows.iter()
                .filter_map(|row| row.get(column))
                .map(|cell| cell.chars().count())
                .max()
                .unwrap_or(0)
        })
        .collect::<Vec<_>>();

    let border_style = ratatui::style::Style::default().fg(colors.table_border_fg);
    let header_style = ratatui::style::Style::default()
        .fg(colors.section_header_fg)
        .add_modifier(ratatui::style::Modifier::BOLD);

    let mut lines = vec![table_border_line(
        message,
        body_indent,
        body_indent_style,
        border_style,
        &widths,
        '╭',
        '┬',
        '╮',
    )];
    lines.push(table_content_line(
        message,
        body_indent,
        body_indent_style,
        border_style,
        &widths,
        header,
        header_style,
        colors,
    ));
    lines.push(table_border_line(
        message,
        body_indent,
        body_indent_style,
        border_style,
        &widths,
        '├',
        '┼',
        '┤',
    ));
    for row in data_rows {
        lines.push(table_content_line(
            message,
            body_indent,
            body_indent_style,
            border_style,
            &widths,
            row,
            ratatui::style::Style::default().fg(colors.text),
            colors,
        ));
    }
    lines.push(table_border_line(
        message,
        body_indent,
        body_indent_style,
        border_style,
        &widths,
        '╰',
        '┴',
        '╯',
    ));
    lines
}

pub(super) fn parse_markdown_table_row(line: &str) -> Vec<String> {
    line.trim()
        .trim_matches('|')
        .split('|')
        .map(|cell| cell.trim().to_string())
        .collect()
}

pub(super) fn table_border_line(
    message: &Msg,
    body_indent: &str,
    body_indent_style: ratatui::style::Style,
    border_style: ratatui::style::Style,
    widths: &[usize],
    left: char,
    middle: char,
    right: char,
) -> ResponseDisplayLine {
    let mut text = String::from(body_indent);
    text.push(left);
    for (index, width) in widths.iter().enumerate() {
        text.push_str(&"─".repeat(width + 2));
        if index + 1 < widths.len() {
            text.push(middle);
        }
    }
    text.push(right);
    ResponseDisplayLine {
        kind: message.kind,
        text: text.clone(),
        is_header: false,
        message_id: message.id.clone(),
        action: None,
        is_tool_line: false,
        tool_status: None,
        response_state: None,
        thinking_line_kind: None,
        spans: vec![
            StyledSpan {
                text: body_indent.to_string(),
                style: body_indent_style,
            },
            StyledSpan {
                text: text.trim_start_matches(body_indent).to_string(),
                style: border_style,
            },
        ],
    }
}

pub(super) fn table_content_line(
    message: &Msg,
    body_indent: &str,
    body_indent_style: ratatui::style::Style,
    border_style: ratatui::style::Style,
    widths: &[usize],
    row: &[String],
    default_cell_style: ratatui::style::Style,
    colors: &omega_theme::RenderPalette,
) -> ResponseDisplayLine {
    let mut text = String::from(body_indent);
    let mut spans = vec![StyledSpan {
        text: body_indent.to_string(),
        style: body_indent_style,
    }];
    for (index, width) in widths.iter().enumerate() {
        let cell = row.get(index).cloned().unwrap_or_default();
        let padded = format!(" {:width$} ", cell, width = width);
        text.push('│');
        text.push_str(&padded);
        spans.push(StyledSpan {
            text: "│".to_string(),
            style: border_style,
        });
        spans.push(StyledSpan {
            text: padded,
            style: table_cell_style(&cell, default_cell_style, colors),
        });
    }
    text.push('│');
    spans.push(StyledSpan {
        text: "│".to_string(),
        style: border_style,
    });

    ResponseDisplayLine {
        kind: message.kind,
        text,
        is_header: false,
        message_id: message.id.clone(),
        action: None,
        is_tool_line: false,
        tool_status: None,
        response_state: None,
        thinking_line_kind: None,
        spans,
    }
}

pub(super) fn table_cell_style(
    cell: &str,
    default_style: ratatui::style::Style,
    colors: &omega_theme::RenderPalette,
) -> ratatui::style::Style {
    if looks_like_metric(cell) {
        ratatui::style::Style::default()
            .fg(colors.metric_emphasis_fg)
            .add_modifier(ratatui::style::Modifier::BOLD)
    } else if looks_like_code_token(cell) {
        ratatui::style::Style::default().fg(colors.code_fg)
    } else {
        default_style
    }
}

pub(super) fn summarize_report_section(kind: ResponseCardSectionKind, body: &str) -> Option<String> {
    let non_empty_lines: Vec<&str> = body
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    if non_empty_lines.is_empty() {
        return None;
    }

    let bullet_count = non_empty_lines
        .iter()
        .filter(|line| is_bullet_or_ordered_item(line.trim_start()))
        .count();
    let table_rows = count_markdown_table_rows(body);
    let command_count = non_empty_lines
        .iter()
        .filter(|line| line.trim_start().starts_with('$') || line.contains('`'))
        .count();

    match kind {
        ResponseCardSectionKind::ResultsSummary
        | ResponseCardSectionKind::ChangesMade
        | ResponseCardSectionKind::KeyPoints
        | ResponseCardSectionKind::OptionalNextStep => Some(if bullet_count > 0 {
            format!("{} items", bullet_count)
        } else {
            format!("{} lines", non_empty_lines.len())
        }),
        ResponseCardSectionKind::Verification => Some(if table_rows > 0 {
            format!("{} rows", table_rows)
        } else {
            format!("{} checks", non_empty_lines.len())
        }),
        ResponseCardSectionKind::Usage => Some(if command_count > 0 {
            format!("{} commands", command_count)
        } else {
            format!("{} lines", non_empty_lines.len())
        }),
        ResponseCardSectionKind::Custom => Some(format!("{} lines", non_empty_lines.len())),
        _ => None,
    }
}

pub(super) fn count_markdown_table_rows(body: &str) -> usize {
    let lines: Vec<&str> = body.lines().collect();
    let mut index = 0usize;
    let mut rows = 0usize;
    while index < lines.len() {
        if is_markdown_table_header(lines.as_slice(), index) {
            index += 2;
            while index < lines.len() && is_markdown_table_row(lines[index]) {
                rows += 1;
                index += 1;
            }
            continue;
        }
        index += 1;
    }
    rows
}

pub(super) fn is_bullet_or_ordered_item(line: &str) -> bool {
    line.starts_with("- ")
        || line.starts_with("* ")
        || line
            .find(". ")
            .is_some_and(|index| index > 0 && line[..index].chars().all(|ch| ch.is_ascii_digit()))
}

pub(super) fn looks_like_metric(text: &str) -> bool {
    let trimmed = text.trim();
    trimmed.contains('%')
        || trimmed.chars().any(|ch| ch.is_ascii_digit())
        || matches!(
            trimmed,
            "pass" | "passed" | "failed" | "complete" | "eliminated"
        )
}

pub(super) fn looks_like_code_token(text: &str) -> bool {
    let trimmed = text.trim();
    trimmed.starts_with('/')
        || trimmed.contains("::")
        || trimmed.contains('.')
        || trimmed.contains('_')
        || trimmed.starts_with('$')
}

pub(super) fn split_or_empty(text: &str) -> Vec<String> {
    if text.is_empty() {
        vec![String::new()]
    } else {
        text.lines().map(ToOwned::to_owned).collect()
    }
}

