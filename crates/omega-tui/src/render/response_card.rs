//! Response panel visual layering (Task 47-51, refactored).
//!
//! The response panel is a flat list of `ResponseDisplayLine`s produced
//! by the data layer. Each line is already self-describing: the data
//! layer pre-formats the title (e.g. `"step unknown:workflow Section ●"`),
//! the body (with its own body_indent such as `"  "` for Step or
//! `"  │ "` for FinalAnswer), and the badge prefix (e.g. `▶` for User,
//! `✗` for Error). The renderer's job is to:
//!
//! 1. Inject a 2-char status glyph at the start of header lines so the
//!    user can scan the panel by kind at a glance.
//! 2. Insert a single blank line between different `MsgKind` groups so
//!    the panel reads as a series of discrete cards (logical grouping)
//!    without heavy per-line borders.
//! 3. Skip the prelude line that the data layer adds for `FinalAnswer`
//!    (`━`.repeat(40)): it duplicates the visual emphasis that the
//!    status glyph and the formatted header already carry.
//!
//! The renderer does NOT add a top/bottom border to each line. The
//! status glyph + formatted header text is the "card" identity. A blank
//! row between groups is the only separator.
//!
//! See `docs/TODO.md` Task 47-51, `docs/specs/omega-tui-visual-refresh.md`,
//! and `docs/decisions/008-tui-component-architecture-refactor.md` for the
//! full visual contract.

use omega_theme::RenderPalette as ColorScheme;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use super::chrome::Glyph;
use super::markdown::StyledSpan;
use super::selection::{apply_selection_to_styled_spans, response_display_spans, wrap_styled_spans};
use crate::app::{MsgKind, ResponseDisplayLine};

/// Build a styled status glyph for the given line. Returns a 1-character
/// String (or 2-character with trailing space) ready to be prepended to
/// a header line. Body lines (where `is_header` is false) do not receive
/// a glyph; the data layer's body_indent already provides the visual
/// grouping.
fn status_glyph_for(line: &ResponseDisplayLine) -> Option<String> {
    if !line.is_header {
        return None;
    }
    let ch = match line.kind {
        MsgKind::Step => match line.tool_status {
            Some(omega_session::ToolRunStatus::Running) => Glyph::RUNNING,
            Some(omega_session::ToolRunStatus::Complete) => Glyph::COMPLETE,
            Some(omega_session::ToolRunStatus::Failed) => Glyph::FAILED,
            None => Glyph::RUNNING,
        },
        MsgKind::FinalAnswer => Glyph::FOCUS,
        MsgKind::Thinking => Glyph::PLACEHOLDER,
        MsgKind::Error => Glyph::FAILED,
        MsgKind::Command => Glyph::FOCUS,
        MsgKind::User => Glyph::BULLET,
        MsgKind::Agent => Glyph::RUNNING,
        MsgKind::Routing => Glyph::BULLET,
        MsgKind::Separator => Glyph::COMPLETE,
    };
    Some(format!("{ch} "))
}

/// Truncate a string to a max char count with a trailing ellipsis.
fn truncate_with_ellipsis(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let keep = max_chars.saturating_sub(1);
    let mut out = String::new();
    for (i, c) in text.chars().enumerate() {
        if i >= keep {
            break;
        }
        out.push(c);
    }
    out.push('…');
    out
}

/// Pick the per-kind accent colour for a header line. The header
/// carries the card's visual identity, so each `MsgKind` gets a
/// distinct colour from the palette (with `BOLD` applied). Body
/// lines do not call this — they keep the regular text colour.
pub(super) fn header_color(line: &ResponseDisplayLine, colors: &ColorScheme) -> Color {
    match line.kind {
        MsgKind::Step => match line.tool_status {
            Some(omega_session::ToolRunStatus::Failed) => colors.error_message,
            Some(omega_session::ToolRunStatus::Complete) => colors.status_idle_fg,
            _ => colors.tool_message,
        },
        MsgKind::FinalAnswer => colors.final_answer_accent_fg,
        MsgKind::Thinking => colors.thinking_summary_fg,
        MsgKind::Command => colors.context_label,
        MsgKind::Error => colors.error_message,
        MsgKind::User => colors.user_badge_fg,
        MsgKind::Agent => colors.assistant_badge_fg,
        MsgKind::Routing => colors.muted_meta_fg,
        MsgKind::Separator => colors.separator_message,
    }
}

/// Build the rendered lines for one `ResponseDisplayLine`. Returns a
/// `Vec<Line>`: 1 line for headers, 1+ lines for body content (body
/// lines wrap to `inner_w` so long markdown paragraphs still fit in
/// narrow terminals). Each line preserves the data layer's prefix
/// (badge, body_indent) and applies the per-kind colour scheme.
pub(super) fn build_response_lines(
    line: &ResponseDisplayLine,
    colors: &ColorScheme,
    inner_w: usize,
) -> Vec<Line<'static>> {
    if inner_w == 0 {
        return Vec::new();
    }

    // The data layer's prelude line for FinalAnswer (`━`.repeat(40))
    // is a decorative element that we suppress: the formatted header
    // that follows already carries enough visual emphasis.
    if line.kind == MsgKind::FinalAnswer && !line.is_header && line.text.chars().all(|c| c == '━') {
        return Vec::new();
    }

    let fallback_style = response_line_style(line, colors);
    let text = line.text.trim_end();

    if let Some(glyph) = status_glyph_for(line) {
        // Header line: prefix the status glyph and follow with the
        // formatted text, both in the per-kind accent colour (BOLD).
        // This is what gives each card (Step / FinalAnswer / Thinking
        // / Command / etc.) its visual identity — the colour tells
        // you which kind the card is at a glance, the glyph tells
        // you the runtime state (Running / Complete / Failed).
        let accent = header_color(line, colors);
        let available = inner_w.saturating_sub(glyph.chars().count());
        let truncated = truncate_with_ellipsis(text, available);
        let header_style = Style::default().fg(accent).add_modifier(Modifier::BOLD);
        vec![Line::from(vec![
            Span::styled(glyph, header_style),
            Span::styled(truncated, header_style),
        ])]
    } else {
        // Body line: wrap the line to inner_w. The data layer's
        // body_indent prefix is preserved on the first wrapped row;
        // continuation rows start at the same indent (acceptable
        // simplification: the indent remains in the text, so the wrap
        // just continues from where the text breaks).
        let source_spans = response_display_spans(line, fallback_style, colors);
        let selected_spans = apply_selection_to_styled_spans(&source_spans, None);
        wrap_styled_spans(&selected_spans, inner_w)
            .into_iter()
            .map(|wrapped| {
                let ratatui_spans: Vec<Span<'static>> = wrapped
                    .into_iter()
                    .map(|s: StyledSpan| Span::styled(s.text, s.style))
                    .collect();
                Line::from(ratatui_spans)
            })
            .collect()
    }
}

/// Map a `ResponseDisplayLine` to its body-line colour. Body lines
/// are the text content under a card header (with the data layer's
/// own `body_indent` prefix). We use a slightly muted variant of the
/// kind's accent so the body reads as content rather than another
/// header.
fn response_line_style(line: &ResponseDisplayLine, colors: &ColorScheme) -> Style {
    use MsgKind::*;
    match line.kind {
        User => Style::default().fg(colors.user_message),
        Agent => Style::default().fg(colors.agent_message),
        Error => Style::default().fg(colors.error_message),
        Separator => Style::default().fg(colors.border_dim),
        Routing => Style::default().fg(colors.muted_meta_fg),
        Step => match line.tool_status {
            Some(omega_session::ToolRunStatus::Failed) => Style::default().fg(colors.error_message),
            _ => Style::default().fg(colors.text),
        },
        FinalAnswer => Style::default().fg(colors.text),
        Thinking => Style::default().fg(colors.thinking_body_fg),
        Command => Style::default().fg(colors.text),
    }
}

/// Convert a `ResponseDisplayLine` to a list of `ListItem`s for the
/// response panel's `List` widget. Each input line produces at least
/// one output `ListItem`; body lines may produce more (when wrapping).
pub(super) fn render_response_line(
    line: &ResponseDisplayLine,
    colors: &ColorScheme,
    inner_w: usize,
) -> Vec<ratatui::widgets::ListItem<'static>> {
    build_response_lines(line, colors, inner_w)
        .into_iter()
        .map(ratatui::widgets::ListItem::new)
        .collect()
}

/// Build a blank `ListItem` used to separate one card from the next
/// (when the `MsgKind` changes).
pub(super) fn blank_list_item() -> ratatui::widgets::ListItem<'static> {
    ratatui::widgets::ListItem::new(Line::from(""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use omega_session::ToolRunStatus;
    use omega_theme::OmegaTheme;

    fn palette() -> ColorScheme {
        OmegaTheme::dark().render_palette()
    }

    fn line(kind: MsgKind, text: &str, is_header: bool) -> ResponseDisplayLine {
        ResponseDisplayLine {
            kind,
            text: text.into(),
            is_header,
            message_id: None,
            action: None,
            is_tool_line: false,
            tool_status: None,
            response_state: None,
            thinking_line_kind: None,
            spans: Vec::new(),
        }
    }

    #[test]
    fn status_glyph_for_header_step() {
        let line = line(MsgKind::Step, "step title", true);
        assert_eq!(status_glyph_for(&line), Some(format!("{} ", Glyph::RUNNING)));
    }

    #[test]
    fn status_glyph_for_header_user() {
        let line = line(MsgKind::User, "hi", true);
        assert_eq!(status_glyph_for(&line), Some(format!("{} ", Glyph::BULLET)));
    }

    #[test]
    fn status_glyph_for_header_error() {
        let line = line(MsgKind::Error, "boom", true);
        assert_eq!(status_glyph_for(&line), Some(format!("{} ", Glyph::FAILED)));
    }

    #[test]
    fn status_glyph_for_header_final_answer() {
        let line = line(MsgKind::FinalAnswer, "answer", true);
        assert_eq!(status_glyph_for(&line), Some(format!("{} ", Glyph::FOCUS)));
    }

    #[test]
    fn status_glyph_for_header_thinking() {
        let line = line(MsgKind::Thinking, "thought", true);
        assert_eq!(
            status_glyph_for(&line),
            Some(format!("{} ", Glyph::PLACEHOLDER))
        );
    }

    #[test]
    fn status_glyph_for_body_line_is_none() {
        let line = line(MsgKind::Step, "body", false);
        assert_eq!(status_glyph_for(&line), None);
    }

    #[test]
    fn truncate_with_ellipsis_short_text_unchanged() {
        assert_eq!(truncate_with_ellipsis("hello", 10), "hello");
    }

    #[test]
    fn truncate_with_ellipsis_long_text_truncated() {
        let s = truncate_with_ellipsis("abcdefghij", 5);
        assert_eq!(s, "abcd…");
        assert_eq!(s.chars().count(), 5);
    }

    #[test]
    fn truncate_with_ellipsis_zero_returns_empty() {
        assert_eq!(truncate_with_ellipsis("hello", 0), "");
    }

    #[test]
    fn build_response_lines_header_includes_glyph_and_text() {
        let line = line(MsgKind::Step, "step workflow_id Section ●", true);
        let rendered = build_response_lines(&line, &palette(), 60);
        assert_eq!(rendered.len(), 1);
        let s = rendered[0].to_string();
        assert!(s.contains("step workflow_id Section ●"));
        assert!(s.starts_with(Glyph::RUNNING));
    }

    #[test]
    fn build_response_lines_body_preserves_data_layer_indent() {
        // The data layer adds a 2-char body_indent for Step ("  ").
        // The renderer must not strip or duplicate it.
        let line = line(MsgKind::Step, "  Gather context", false);
        let rendered = build_response_lines(&line, &palette(), 60);
        assert_eq!(rendered.len(), 1);
        let s = rendered[0].to_string();
        // Body line should preserve the 2-char indent.
        assert!(s.starts_with("  Gather context"));
        // No glyph prefix for body lines.
        assert!(!s.starts_with(Glyph::RUNNING));
        assert!(!s.starts_with(Glyph::BULLET));
    }

    #[test]
    fn build_response_lines_body_wraps_in_narrow_width() {
        // A 60-char body line in a 20-wide panel should wrap to 3+ lines.
        let line = line(
            MsgKind::FinalAnswer,
            "This paragraph includes `inline code` and enough words to wrap.",
            false,
        );
        let rendered = build_response_lines(&line, &palette(), 20);
        assert!(
            rendered.len() >= 2,
            "expected at least 2 wrapped lines; got {}",
            rendered.len()
        );
    }

    #[test]
    fn build_response_lines_final_answer_prelude_is_suppressed() {
        // The data layer adds a `━`.repeat(40) prelude line for
        // FinalAnswer; the renderer should suppress it because the
        // formatted header that follows already carries visual weight.
        let prelude = line(MsgKind::FinalAnswer, &"━".repeat(40), false);
        let rendered = build_response_lines(&prelude, &palette(), 60);
        assert!(rendered.is_empty());
    }

    #[test]
    fn build_response_lines_long_text_is_truncated() {
        let line = line(MsgKind::User, &"a".repeat(200), true);
        let rendered = build_response_lines(&line, &palette(), 20);
        assert_eq!(rendered.len(), 1);
        let s = rendered[0].to_string();
        // The line must fit within inner_w (20) — glyph (2) + text.
        assert!(s.chars().count() <= 20);
        assert!(s.contains('…'));
    }

    #[test]
    fn build_response_lines_zero_width_returns_empty() {
        let line = line(MsgKind::Step, "x", true);
        let rendered = build_response_lines(&line, &palette(), 0);
        assert!(rendered.is_empty());
    }

    #[test]
    fn build_response_lines_error_uses_red_glyph_style() {
        let line = line(MsgKind::Error, "boom", true);
        let rendered = build_response_lines(&line, &palette(), 60);
        assert_eq!(rendered.len(), 1);
        let s = rendered[0].to_string();
        assert!(s.starts_with(Glyph::FAILED));
        assert!(s.contains("boom"));
    }

    #[test]
    fn header_color_distinguishes_step_from_final_answer() {
        // Step and FinalAnswer must have visually distinct header
        // colours — that is the whole point of "distinguish the
        // first line by colour" (the user's request).
        let step_line = line(MsgKind::Step, "step", true);
        let final_line = line(MsgKind::FinalAnswer, "final", true);
        let colors = palette();
        let step_color = header_color(&step_line, &colors);
        let final_color = header_color(&final_line, &colors);
        assert_ne!(
            step_color, final_color,
            "Step and FinalAnswer headers should use different colors"
        );
    }

    #[test]
    fn header_color_error_is_red() {
        let line = line(MsgKind::Error, "boom", true);
        let colors = palette();
        let c = header_color(&line, &colors);
        assert_eq!(
            c, colors.error_message,
            "Error header should use the error_message palette color"
        );
    }

    #[test]
    fn header_color_thinking_is_muted() {
        let line = line(MsgKind::Thinking, "thought", true);
        let colors = palette();
        let c = header_color(&line, &colors);
        assert_eq!(
            c, colors.thinking_summary_fg,
            "Thinking header should use the thinking_summary_fg palette color"
        );
    }

    #[test]
    fn header_color_final_answer_is_accent() {
        let line = line(MsgKind::FinalAnswer, "answer", true);
        let colors = palette();
        let c = header_color(&line, &colors);
        assert_eq!(
            c, colors.final_answer_accent_fg,
            "FinalAnswer header should use the final_answer_accent_fg palette color"
        );
    }

    #[test]
    fn header_color_step_is_tool_message() {
        let line = line(MsgKind::Step, "step", true);
        let colors = palette();
        let c = header_color(&line, &colors);
        assert_eq!(
            c, colors.tool_message,
            "Step header (running, no tool_status) should use the tool_message palette color"
        );
    }
}
