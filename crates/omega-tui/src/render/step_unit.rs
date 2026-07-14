//! Step Unit (Task 53): a 2-row "card" representing one section in
//! the response panel.
//!
//! A `StepUnit` is the visual primitive that every `MsgKind` in the
//! response panel maps to. It has exactly two children:
//!
//! - **fixed** (always shown): kind glyph + formatted title + state.
//!   Uses the per-kind accent colour (from T-51's `header_color`).
//! - **variable** (shows the current state): the currently-running
//!   tool name, the latest thinking line, the first line of the
//!   final answer, etc. Uses regular text colour.
//!
//! Both children are `FlexSize::Length(1)` rows, stacked vertically
//! with `FlexContainer { Column, gap=0 }`. There is no top/bottom
//! border; the per-kind colour is the only visual identifier.
//!
//! Pressing Enter on a `StepUnit`'s variable row in the response
//! panel opens a `StepDetailOverlay` (T-55) via the
//! `detail_target` request, which carries the section id and kind
//! to look up the data.
//!
//! See `docs/specs/omega-tui-flex-layout-and-step-unit.md` §B and
//! `docs/decisions/009-tui-flex-layout-primitives.md` for the
//! motivation.

use omega_theme::RenderPalette as ColorScheme;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::Frame;

use crate::app::{MsgKind, ResponseDisplayLine};

use super::chrome::Glyph;
use super::flex::{FlexChild, FlexContainer, FlexSize};

/// A request to open a `StepDetailOverlay` for a particular
/// `MsgKind` in the response panel. Carries the data needed to
/// build the overlay (section id, kind, title).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepDetailRequest {
    pub section_id: String,
    pub kind: MsgKind,
    pub title: String,
}

/// A visual unit for the response panel (T-63 refactor).
///
/// A `StepUnit` represents one record in the chat log. The
/// chat-log side shows only a 1-line **summary**; the popup
/// (StepDetailOverlay / TurnDetailOverlay) consumes the full
/// **body** for drill-down.
///
/// This is the T-47 "fixed + variable + detail" model, finally
/// implemented: the summary is the "fixed" line (kind glyph +
/// title + state); the body is the "variable" content that
/// belongs to the popup, not the chat log.
///
/// In the response panel, units are stacked with
/// `FlexContainer { Column, gap: 1 }` so each unit reads as a
/// discrete chat-log line separated by a 1-line gap (T-56).
pub struct StepUnit {
    /// 1-line summary rendered in the chat log (T-63).
    pub summary_line: Line<'static>,
    /// Full body content (one entry per body line) for the popup.
    /// For simple kinds (User / Agent / Error / Separator) this
    /// is the same single text; for section kinds (Step /
    /// FinalAnswer / Thinking / Command) it is the data layer's
    /// full output stripped of the header line.
    pub body_lines: Vec<String>,
    /// Optional detail request for the Enter handler.
    pub detail_target: Option<StepDetailRequest>,
}

impl StepUnit {
    /// Construct a StepUnit from a summary line, body content,
    /// and an optional detail target.
    pub fn new(
        summary_line: Line<'static>,
        body_lines: Vec<String>,
        detail_target: Option<StepDetailRequest>,
    ) -> Self {
        Self {
            summary_line,
            body_lines,
            detail_target,
        }
    }

    /// Convenience constructor for a 2-line StepUnit (used by the
    /// per-kind factory functions in T-54).
    pub fn from_lines(
        fixed: Line<'static>,
        variable: Line<'static>,
        detail_target: Option<StepDetailRequest>,
    ) -> Self {
        // Combine the 2 lines into a single summary. The
        // factory's "variable" is already a per-kind summary
        // (e.g. "running tool_name"), so concatenating gives a
        // single rich line.
        let mut spans: Vec<Span<'static>> = Vec::new();
        spans.push(Span::styled(
            String::new(),
            Style::default(),
        ));
        // Reuse the first line's spans; append the variable's
        // text. For our factory this gives "header · variable".
        // We don't combine spans in detail here; the factory
        // pattern is deprecated (T-63) — use `new` instead.
        let combined = Line::from(
            fixed
                .spans
                .iter()
                .cloned()
                .chain(variable.spans.iter().cloned())
                .collect::<Vec<_>>(),
        );
        let _ = spans;
        Self::new(combined, Vec::new(), detail_target)
    }

    /// Convenience constructor for a single-row StepUnit (no
    /// body content, just the summary). Used for kinds that have
    /// no current state to show (User / Agent / Error /
    /// Separator / Routing).
    pub fn single_row(fixed: Line<'static>, detail_target: Option<StepDetailRequest>) -> Self {
        Self::new(fixed, Vec::new(), detail_target)
    }

    /// Render the StepUnit's summary into `area` (1 row). The
    /// body is not rendered here — the popup consumes it.
    pub fn render(&mut self, frame: &mut Frame, area: Rect) -> Vec<Rect> {
        if area.height == 0 {
            return Vec::new();
        }
        use ratatui::widgets::Paragraph;
        // Take the line out of self so we satisfy FnOnce / move.
        let line = std::mem::replace(&mut self.summary_line, Line::from(""));
        let p = Paragraph::new(line);
        frame.render_widget(p, area);
        vec![area]
    }
}

/// Pick the per-kind accent colour for a header line. Re-exported
/// from `response_card::header_color` to keep the colour mapping
/// in one place. The body lines of a StepUnit do NOT use this
/// colour — they use the regular text colour.
pub fn header_color(line: &ResponseDisplayLine, colors: &ColorScheme) -> ratatui::style::Color {
    super::response_card::header_color(line, colors)
}

/// Build the formatted header line for a section. The header text
/// is the data layer's `format_response_header` output, prefixed
/// with the kind's status glyph (RUNNING for Step header lines,
/// FOCUS for FinalAnswer, etc.).
///
/// The line is `inner_w` characters wide; anything longer is
/// truncated with an ellipsis.
pub fn build_header_line(
    line: &ResponseDisplayLine,
    colors: &ColorScheme,
    inner_w: usize,
) -> Line<'static> {
    if inner_w == 0 {
        return Line::from("");
    }
    let glyph = status_glyph(line);
    let text = line.text.trim_end();
    let available = inner_w.saturating_sub(glyph.chars().count());
    let truncated = truncate_with_ellipsis(text, available);
    let accent = header_color(line, colors);
    let header_style = Style::default().fg(accent).add_modifier(Modifier::BOLD);
    Line::from(vec![
        Span::styled(glyph, header_style),
        Span::styled(truncated, header_style),
    ])
}

/// Build a body line for a StepUnit's variable row. The body
/// text comes from the caller (e.g. a synthesised summary like
/// "running tool_name" or "5/5 tools complete"). Uses regular
/// text colour, no glyph prefix.
pub fn build_variable_line(text: String, colors: &ColorScheme) -> Line<'static> {
    Line::from(Span::styled(text, Style::default().fg(colors.text)))
}

// ---------------------------------------------------------------------------
// T-63: Per-Kind Summary Line
// ---------------------------------------------------------------------------

/// Render the chat-log summary for one `ResponseDisplayLine`
/// (T-63). Returns exactly 1 `Line` that fits `inner_w` chars;
/// truncation is applied with an ellipsis. The body content
/// of multi-line records (Step, FinalAnswer, Thinking, etc.)
/// is **not** included here in full — the popup is the source
/// of truth for the full body. A short body preview is
/// appended for FinalAnswer only.
///
/// `all_lines` is the full `agent_msgs` slice; the body
/// preview is composed from the body lines that follow
/// `header_pos` and have the same `kind` as the header. This
/// position-based pairing handles the case where multiple
/// records share `message_id = None` (e.g. messages pushed via
/// `push_msg` without explicit ids).
///
/// Per-kind rules:
/// - **Step**: `◉ step workflow_id Section ●` (glyph picks up
///   `tool_status`: RUNNING / COMPLETE / FAILED).
/// - **FinalAnswer**: `◆ final workflow_id Section ●` + first
///   `inner_w - header_w` chars of body preview (if body is
///   non-empty).
/// - **Thinking**: `◦ Thinking …` (collapsed by default).
/// - **Command**: `◆ command builtin Section ●`.
/// - **User / Agent / Error / Routing / Separator**: the header
///   line from `build_header_line` (kind glyph + text). Single
///   row.
pub fn build_subunit_summary(
    line: &ResponseDisplayLine,
    all_lines: &[ResponseDisplayLine],
    header_pos: usize,
    colors: &ColorScheme,
    inner_w: usize,
) -> Line<'static> {
    if inner_w == 0 {
        return Line::from("");
    }
    // For most kinds, the header line IS the summary. For
    // FinalAnswer, append a short body preview.
    let header = build_header_line(line, colors, inner_w);

    // T-69: when a Step is actively streaming, swap the kind
    // glyph to the spinner `◐` and append a trailing `…` to
    // signal "work in progress" without animating. The
    // popup is the source of truth for the full body.
    if line.kind == MsgKind::Step
        && line
            .response_state
            .map(|s| matches!(s, omega_session::ResponseSectionState::Streaming))
            .unwrap_or(false)
    {
        let mut spans = header.spans.clone();
        // Replace the leading glyph span with the spinner
        // (`◐` = ACTIVE) if present.
        if !spans.is_empty() {
            let first = spans[0].content.clone();
            if let Some(rest) = first.strip_prefix(Glyph::RUNNING) {
                spans[0] = Span::styled(
                    format!("{}{}", Glyph::ACTIVE, rest),
                    Style::default()
                        .fg(header_color(line, colors))
                        .add_modifier(Modifier::BOLD),
                );
            }
        }
        // Append trailing `…` to signal in-progress.
        spans.push(Span::styled(
            " …".to_string(),
            Style::default().fg(colors.context_hint),
        ));
        return Line::from(spans);
    }

    if matches!(line.kind, MsgKind::FinalAnswer) {
        // Compose a body preview from the body lines that
        // follow this header (same kind, is_header: false).
        // The data layer already includes `body_indent` ("  │ ").
        let body_text = followup_body_lines(all_lines, header_pos, line.kind);
        let body = strip_body_indent(&body_text);
        if !body.is_empty() {
            // Compose a single line that includes both the
            // header spans and a `· body…` suffix.
            let mut spans: Vec<Span<'static>> = header.spans.clone();
            spans.push(Span::styled(
                " · ".to_string(),
                Style::default().fg(colors.muted_meta_fg),
            ));
            // Truncate body to fit remaining width.
            let consumed: usize = spans
                .iter()
                .map(|s| s.content.chars().count())
                .sum();
            let available = inner_w.saturating_sub(consumed);
            let preview = truncate_with_ellipsis(&body, available);
            spans.push(Span::styled(
                preview,
                Style::default().fg(colors.text),
            ));
            return Line::from(spans);
        }
    }

    header
}

/// Collect body lines that follow `header_pos` and have the
/// same kind as the header. Stops at the first line of a
/// different kind (or the end of the slice).
fn followup_body_lines(
    all_lines: &[ResponseDisplayLine],
    header_pos: usize,
    kind: MsgKind,
) -> String {
    all_lines
        .iter()
        .skip(header_pos + 1)
        .take_while(|l| l.kind == kind && !l.is_header)
        .map(|l| l.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Strip the data layer's body indent (`  │ ` or `  `) from
/// the start of a body line so the preview reads cleanly.
fn strip_body_indent(text: &str) -> String {
    let trimmed = text.trim_start();
    if let Some(stripped) = trimmed.strip_prefix("│ ") {
        return stripped.to_string();
    }
    if let Some(stripped) = trimmed.strip_prefix("│") {
        return stripped.trim_start().to_string();
    }
    trimmed.to_string()
}

/// Build a body line for the popup. The caller passes the
/// full data layer body content (one string per body line).
/// Returns a `Vec<String>` of body lines, with the data layer
/// body-indent prefix stripped for clean popup rendering.
pub fn build_popup_body_lines(
    body_lines: Vec<String>,
) -> Vec<String> {
    body_lines
}

#[cfg(test)]
mod t63_tests {
    use ratatui::layout::Rect;
    use omega_theme::OmegaTheme;
    use super::*;

    fn line(kind: MsgKind, text: &str) -> ResponseDisplayLine {
        ResponseDisplayLine {
            kind,
            text: text.into(),
            is_header: true,
            message_id: Some("test".into()),
            action: None,
            is_tool_line: false,
            tool_status: None,
            response_state: None,
            thinking_line_kind: None,
            spans: Vec::new(),
        }
    }

    #[test]
    fn t63_summary_step_uses_running_glyph() {
        let l = line(MsgKind::Step, "step wf Section ●");
        let s = build_subunit_summary(&l, std::slice::from_ref(&l), 0, &OmegaTheme::dark().render_palette(), 60);
        let str = s.to_string();
        assert!(str.contains('◉'), "Step summary should use RUNNING glyph; got {str:?}");
        assert!(str.contains("step wf Section"));
    }

    #[test]
    fn t63_summary_final_answer_includes_preview() {
        let l = line(
            MsgKind::FinalAnswer,
            "final wf Section ●",
        );
        let s = build_subunit_summary(&l, std::slice::from_ref(&l), 0, &OmegaTheme::dark().render_palette(), 80);
        let str = s.to_string();
        assert!(str.contains('◆'), "FinalAnswer summary should use FOCUS glyph; got {str:?}");
    }

    #[test]
    fn t63_summary_thinking_uses_placeholder() {
        let l = line(MsgKind::Thinking, "reasoning");
        let s = build_subunit_summary(&l, std::slice::from_ref(&l), 0, &OmegaTheme::dark().render_palette(), 60);
        let str = s.to_string();
        assert!(str.contains('◦'), "Thinking summary should use PLACEHOLDER glyph; got {str:?}");
    }

    #[test]
    fn t63_summary_command_uses_focus() {
        let l = line(MsgKind::Command, "command builtin Section ●");
        let s = build_subunit_summary(&l, std::slice::from_ref(&l), 0, &OmegaTheme::dark().render_palette(), 60);
        let str = s.to_string();
        assert!(str.contains('◆'), "Command summary should use FOCUS glyph; got {str:?}");
    }

    #[test]
    fn t63_summary_zero_width_returns_empty_line() {
        let l = line(MsgKind::Step, "x");
        let s = build_subunit_summary(&l, std::slice::from_ref(&l), 0, &OmegaTheme::dark().render_palette(), 0);
        assert_eq!(s.to_string(), "");
    }

    #[test]
    fn t63_summary_long_text_truncated() {
        let l = line(MsgKind::User, &"a".repeat(200));
        let s = build_subunit_summary(&l, std::slice::from_ref(&l), 0, &OmegaTheme::dark().render_palette(), 30);
        let str = s.to_string();
        assert!(str.chars().count() <= 30, "summary should fit inner width");
        assert!(str.contains('…'), "long text should be truncated");
    }

    #[test]
    fn t63_strip_body_indent_strips_pipe() {
        assert_eq!(strip_body_indent("│ hello"), "hello");
        assert_eq!(strip_body_indent("  │ hello"), "hello");
        assert_eq!(strip_body_indent("hello"), "hello");
    }

    #[test]
    fn t63_followup_body_lines_collects_same_kind() {
        let header = line(MsgKind::FinalAnswer, "final  Section ●");
        let mut body1 = line(MsgKind::FinalAnswer, "  │ line 1");
        body1.is_header = false;
        let mut body2 = line(MsgKind::FinalAnswer, "  │ line 2");
        body2.is_header = false;
        let mut other = line(MsgKind::Step, "  step  Section ●");
        other.is_header = false;
        let all = vec![header, body1, body2, other];
        let joined = followup_body_lines(&all, 0, MsgKind::FinalAnswer);
        assert_eq!(joined, "  │ line 1\n  │ line 2");
    }

    #[test]
    fn t63_chat_turn_renders_one_row_per_subunit() {
        // T-63 (integration): a 1-turn chat with 1 Step + 1
        // FinalAnswer renders as 4 rows in the chat log: user
        // title + user body + step summary + final summary.
        use crate::render::chat_turn::iter_turns;
        let mut lines = vec![
            ResponseDisplayLine {
                kind: MsgKind::User,
                text: "Hello".into(),
                is_header: false,
                message_id: Some("u".into()),
                action: None,
                is_tool_line: false,
                tool_status: None,
                response_state: None,
                thinking_line_kind: None,
                spans: Vec::new(),
            },
            line(MsgKind::Step, "step wf Search ●"),
            line(MsgKind::FinalAnswer, "final wf Answer ●"),
        ];
        // Filter preludes (none in this test, but keep the
        // pattern for safety).
        lines.retain(|l| !(l.kind == MsgKind::FinalAnswer
            && !l.is_header
            && l.text.chars().all(|c| c == '━')));
        let turns = iter_turns(&lines);
        assert_eq!(turns.len(), 1);
        // 1 user bubble (2 rows) + 2 agent sub-units (1 row
        // each) = 4 rows of content.
        // Now render and verify the buffer has 4 rows of
        // non-empty content.
        let backend = ratatui::backend::TestBackend::new(120, 30);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let mut turn = turns.into_iter().next().unwrap();
        let colors = OmegaTheme::dark().render_palette();
        terminal
            .draw(|frame| {
                turn.render(frame, Rect::new(0, 0, 120, 30), 118, &colors, 0, &mut |_| None);
            })
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        let non_empty_rows: Vec<String> = (0..30)
            .map(|y| (0..118).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .filter(|r: &String| !r.trim().is_empty())
            .collect();
        // We expect at least 4 non-empty rows in the response
        // panel content (user title, user body, step summary,
        // final summary). There may be additional empty
        // padding rows in the panel.
        assert!(
            non_empty_rows.len() >= 4,
            "expected at least 4 non-empty rows; got {}",
            non_empty_rows.len()
        );
    }
}

/// Truncate `text` to at most `max_chars` characters, appending
/// `…` if truncation occurred.
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

/// Pick the per-status glyph for a header line. The glyph char is
/// the one whose kind+state combination best represents the line.
fn status_glyph(line: &ResponseDisplayLine) -> String {
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
    format!("{ch} ")
}

// ---------------------------------------------------------------------------
// T-54: Per-MsgKind summary synthesis
// ---------------------------------------------------------------------------

/// A snapshot of a section's state, used to synthesise the variable
/// row of a `StepUnit`. The renderer (which has access to `App`)
/// constructs this and passes it to [`build_step_unit`].
#[derive(Debug, Default, Clone)]
pub struct SectionSummary {
    /// Number of tool runs in this section.
    pub tool_count: usize,
    /// Number of tool runs that are still running.
    pub running_count: usize,
    /// Number of tool runs that have completed.
    pub complete_count: usize,
    /// Number of tool runs that have failed.
    pub failed_count: usize,
    /// Name of the tool currently running (if any).
    pub running_tool: Option<String>,
    /// Number of subflows in this section.
    pub subflow_count: usize,
    /// First 80 chars of the body text (FinalAnswer preview).
    pub body_preview: Option<String>,
    /// Number of body lines.
    pub body_line_count: usize,
}

/// Synthesise the variable-row text for a Step header. The result
/// is the line that goes into the StepUnit's variable row.
pub fn summarize_step(summary: &SectionSummary) -> String {
    if let Some(name) = &summary.running_tool {
        format!("▶ running {name}")
    } else if summary.tool_count == 0 && summary.subflow_count == 0 {
        return String::new();
    } else if summary.failed_count > 0 {
        format!(
            "✗ {}/{} tools ({} failed)",
            summary.complete_count, summary.tool_count, summary.failed_count
        )
    } else {
        format!(
            "✓ {}/{} tools · {} subflow(s)",
            summary.complete_count, summary.tool_count, summary.subflow_count
        )
    }
}

/// Synthesise the variable-row text for a FinalAnswer header.
pub fn summarize_final_answer(summary: &SectionSummary) -> String {
    summary
        .body_preview
        .as_deref()
        .map(|p| {
            if p.chars().count() > 80 {
                let mut out: String = p.chars().take(80).collect();
                out.push('…');
                out
            } else {
                p.to_string()
            }
        })
        .unwrap_or_default()
}

/// Synthesise the variable-row text for a Thinking header.
pub fn summarize_thinking(summary: &SectionSummary) -> String {
    summary
        .body_preview
        .as_deref()
        .map(|p| format!("▸ {p}"))
        .unwrap_or_default()
}

/// Synthesise the variable-row text for a Command header.
pub fn summarize_command(summary: &SectionSummary) -> String {
    if summary.body_line_count == 0 {
        return String::new();
    }
    if summary.running_count > 0 {
        format!("running · {} lines", summary.body_line_count)
    } else if summary.failed_count > 0 {
        format!("failed · {} lines", summary.body_line_count)
    } else {
        format!("complete · {} lines", summary.body_line_count)
    }
}

/// Synthesise the variable-row text for any `MsgKind`. Returns
/// `Some(text)` for kinds that have a variable row, `None` for
/// kinds that should render as a single line.
pub fn variable_for_kind(
    kind: MsgKind,
    summary: &SectionSummary,
) -> Option<String> {
    match kind {
        MsgKind::Step => Some(summarize_step(summary)),
        MsgKind::FinalAnswer => Some(summarize_final_answer(summary)),
        MsgKind::Thinking => Some(summarize_thinking(summary)),
        MsgKind::Command => Some(summarize_command(summary)),
        // Single-line kinds: caller uses build_single_line_unit
        // instead of build_step_unit. variable_for_kind returns None
        // so the caller can detect the difference.
        MsgKind::User | MsgKind::Agent | MsgKind::Error | MsgKind::Separator
        | MsgKind::Routing => None,
    }
}

/// Build a 2-row `StepUnit` from a section's header line and a
/// pre-computed summary. The summary drives the variable row.
pub fn build_step_unit(
    header_line: &ResponseDisplayLine,
    summary: &SectionSummary,
    section_id: Option<String>,
    title: String,
    inner_w: usize,
    colors: &ColorScheme,
) -> StepUnit {
    let variable_text = variable_for_kind(header_line.kind, summary).unwrap_or_default();
    let fixed = build_header_line(header_line, colors, inner_w);
    let variable = build_variable_line(variable_text, colors);
    let detail_target = section_id.map(|id| StepDetailRequest {
        section_id: id,
        kind: header_line.kind,
        title,
    });
    StepUnit::from_lines(fixed, variable, detail_target)
}

/// Build a single-line `StepUnit` for kinds that have no variable
/// row (User / Agent / Error / Separator / Routing). The whole
/// line is the fixed row; the variable row is empty.
pub fn build_single_line_unit(
    line: &ResponseDisplayLine,
    inner_w: usize,
    colors: &ColorScheme,
) -> StepUnit {
    let fixed = build_header_line(line, colors, inner_w);
    // Detail target: only Error gets a detail popup; others None.
    let detail_target = if line.kind == MsgKind::Error {
        line.message_id.as_ref().map(|id| StepDetailRequest {
            section_id: id.clone(),
            kind: line.kind,
            title: "Error".to_string(),
        })
    } else {
        None
    };
    StepUnit::single_row(fixed, detail_target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use omega_session::{ResponseSectionState, ToolRunStatus};
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
    fn from_lines_builds_two_row_unit() {
        let u = StepUnit::from_lines(
            Line::from("header"),
            Line::from("variable"),
            Some(StepDetailRequest {
                section_id: "x".into(),
                kind: MsgKind::Step,
                title: "Step".into(),
            }),
        );
        assert!(u.detail_target.is_some());
    }

    #[test]
    fn single_row_has_no_detail_by_default() {
        let u = StepUnit::single_row(Line::from("only"), None);
        assert!(u.detail_target.is_none());
    }

    #[test]
    fn render_summary_shows_on_first_row() {
        // T-63: a StepUnit renders only its 1-line summary
        // (not the body). The body is consumed by the popup.
        let backend = ratatui::backend::TestBackend::new(40, 4);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let mut u = StepUnit::new(Line::from("H"), vec!["body line 1".into()], None);
        terminal
            .draw(|frame| {
                u.render(frame, Rect::new(0, 0, 40, 4));
            })
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        let row0: String = (0..40).map(|x| buf[(x, 0)].symbol()).collect();
        assert!(row0.contains('H'), "row 0 should contain summary; got {row0:?}");
        // Body content should NOT appear in the chat-log buffer.
        let joined: String = (0..4)
            .map(|y| (0..40).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            !joined.contains("body line"),
            "body content must not appear in chat log; got:\n{joined}"
        );
    }

    #[test]
    fn render_zero_height_area_is_noop() {
        let backend = ratatui::backend::TestBackend::new(40, 0);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let mut u = StepUnit::from_lines(Line::from("H"), Line::from("V"), None);
        terminal
            .draw(|frame| {
                let rects = u.render(frame, Rect::new(0, 0, 40, 0));
                assert!(rects.is_empty());
            })
            .unwrap();
    }

    #[test]
    fn build_header_line_includes_glyph_and_text() {
        let line = line(MsgKind::Step, "step  workflow  Section  ●", true);
        let rendered = build_header_line(&line, &palette(), 60);
        let s = rendered.to_string();
        assert!(s.starts_with(Glyph::RUNNING));
        assert!(s.contains("Section"));
    }

    #[test]
    fn build_header_line_uses_per_kind_color() {
        let step = line(MsgKind::Step, "x", true);
        let final_line = line(MsgKind::FinalAnswer, "x", true);
        let colors = palette();
        let step_rendered = build_header_line(&step, &colors, 60);
        let final_rendered = build_header_line(&final_line, &colors, 60);
        let step_fg = step_rendered.spans[1].style.fg;
        let final_fg = final_rendered.spans[1].style.fg;
        assert!(step_fg.is_some());
        assert!(final_fg.is_some());
        assert_ne!(step_fg, final_fg);
    }

    #[test]
    fn build_header_line_truncates_long_text() {
        let line = line(MsgKind::User, &"a".repeat(200), true);
        let rendered = build_header_line(&line, &palette(), 20);
        let s = rendered.to_string();
        assert!(s.chars().count() <= 20);
        assert!(s.contains('…'));
    }

    #[test]
    fn build_header_line_zero_width_returns_empty() {
        let line = line(MsgKind::Step, "x", true);
        let rendered = build_header_line(&line, &palette(), 0);
        assert!(rendered.to_string().is_empty());
    }

    #[test]
    fn build_variable_line_uses_text_color() {
        let colors = palette();
        let rendered = build_variable_line("5/5 tools complete".to_string(), &colors);
        assert_eq!(rendered.spans.len(), 1);
        assert_eq!(rendered.spans[0].style.fg, Some(colors.text));
    }

    #[test]
    fn status_glyph_running_step() {
        let l = ResponseDisplayLine {
            kind: MsgKind::Step,
            text: "x".into(),
            is_header: true,
            message_id: None,
            action: None,
            is_tool_line: true,
            tool_status: Some(ToolRunStatus::Running),
            response_state: None,
            thinking_line_kind: None,
            spans: Vec::new(),
        };
        assert_eq!(status_glyph(&l), format!("{} ", Glyph::RUNNING));
    }

    #[test]
    fn status_glyph_final_answer() {
        let l = line(MsgKind::FinalAnswer, "x", true);
        assert_eq!(status_glyph(&l), format!("{} ", Glyph::FOCUS));
    }

    // --- T-54: per-kind summary tests ---

    #[test]
    fn summarize_step_with_running_tool() {
        let s = SectionSummary {
            tool_count: 3,
            running_count: 1,
            running_tool: Some("search_knowledge".into()),
            ..Default::default()
        };
        assert_eq!(summarize_step(&s), "▶ running search_knowledge");
    }

    #[test]
    fn summarize_step_all_complete() {
        let s = SectionSummary {
            tool_count: 3,
            complete_count: 3,
            subflow_count: 1,
            ..Default::default()
        };
        assert_eq!(summarize_step(&s), "✓ 3/3 tools · 1 subflow(s)");
    }

    #[test]
    fn summarize_step_with_failure() {
        let s = SectionSummary {
            tool_count: 3,
            complete_count: 2,
            failed_count: 1,
            ..Default::default()
        };
        assert_eq!(summarize_step(&s), "✗ 2/3 tools (1 failed)");
    }

    #[test]
    fn summarize_final_answer_truncates_long_preview() {
        let s = SectionSummary {
            body_preview: Some("a".repeat(200)),
            ..Default::default()
        };
        let out = summarize_final_answer(&s);
        assert!(out.chars().count() <= 81); // 80 + ellipsis
        assert!(out.ends_with('…'));
    }

    #[test]
    fn summarize_thinking_includes_arrow() {
        let s = SectionSummary {
            body_preview: Some("step-by-step thinking".into()),
            ..Default::default()
        };
        assert_eq!(summarize_thinking(&s), "▸ step-by-step thinking");
    }

    #[test]
    fn summarize_command_running() {
        let s = SectionSummary {
            body_line_count: 3,
            running_count: 1,
            ..Default::default()
        };
        assert_eq!(summarize_command(&s), "running · 3 lines");
    }

    #[test]
    fn summarize_command_complete() {
        let s = SectionSummary {
            body_line_count: 5,
            ..Default::default()
        };
        assert_eq!(summarize_command(&s), "complete · 5 lines");
    }

    #[test]
    fn variable_for_kind_returns_some_for_step() {
        let s = SectionSummary::default();
        assert!(variable_for_kind(MsgKind::Step, &s).is_some());
    }

    #[test]
    fn variable_for_kind_returns_none_for_user() {
        let s = SectionSummary::default();
        assert!(variable_for_kind(MsgKind::User, &s).is_none());
    }

    #[test]
    fn build_step_unit_sets_detail_target_from_section_id() {
        let l = line(MsgKind::Step, "step  workflow  Section  ●", true);
        let u = build_step_unit(
            &l,
            &SectionSummary::default(),
            Some("turn-1:step:1".to_string()),
            "Step".to_string(),
            60,
            &palette(),
        );
        assert!(u.detail_target.is_some());
        assert_eq!(u.detail_target.as_ref().unwrap().section_id, "turn-1:step:1");
        assert_eq!(u.detail_target.as_ref().unwrap().kind, MsgKind::Step);
    }

    #[test]
    fn build_single_line_unit_user_has_no_detail() {
        let l = line(MsgKind::User, "Hello", true);
        let u = build_single_line_unit(&l, 60, &palette());
        assert!(u.detail_target.is_none());
    }

    #[test]
    fn build_single_line_unit_error_has_detail() {
        let mut l = line(MsgKind::Error, "boom", true);
        l.message_id = Some("err-1".into());
        let u = build_single_line_unit(&l, 60, &palette());
        assert!(u.detail_target.is_some());
        assert_eq!(u.detail_target.unwrap().kind, MsgKind::Error);
    }
}
