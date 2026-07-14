//! Chat Turn (Task 58): a chat-bubble visual unit that groups a
//! user message with the agent's response records.
//!
//! A `ChatTurn` represents one round of conversation:
//!
//! - 1 user message (the "ask")
//! - N agent response records (Step/Thinking/Command/FinalAnswer/...)
//!
//! The turn boundary is detected at the first `MsgKind::User`
//! line and continues until the next `User` line or the end of
//! the slice.
//!
//! `ChatTurn::render` lays out the sub-units vertically using a
//! `FlexContainer { Column, gap=1 }` so the per-kind gap (1 row)
//! is preserved inside a turn. The outer `render_response_panel`
//! stacks `ChatTurn` instances with `gap=2` so turn boundaries
//! read as wider visual gaps than internal kind changes.
//!
//! See `docs/specs/omega-tui-chat-turn-history.md` §A and
//! `docs/decisions/010-tui-chat-turn-history.md` for the design.

use omega_theme::RenderPalette as ColorScheme;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::Frame;

use crate::app::{MsgKind, ResponseDisplayLine};

use super::flex::{FlexChild, FlexContainer, FlexDirection, FlexSize};
use super::selection::apply_selection_to_line_spans;

/// Returns `true` if the line is the suppressed `FinalAnswer`
/// prelude (a row of `━` characters that the data layer emits
/// for visual flair but which the render layer discards).
fn is_suppressed_prelude(line: &ResponseDisplayLine) -> bool {
    line.kind == MsgKind::FinalAnswer
        && !line.is_header
        && !line.text.is_empty()
        && line.text.chars().all(|c| c == '━')
}

/// A "chat turn" = 1 user message + N agent response records.
/// Each `ChatTurn` is rendered as a visual chunk in the response
/// panel, with internal sub-units stacked vertically.
#[derive(Debug, Clone)]
pub struct ChatTurn {
    /// The 1 user message (always present at the top of the
    /// turn).
    pub user_msg: ResponseDisplayLine,
    /// The N agent response messages (Step/Thinking/Command/
    /// FinalAnswer/...); may be empty if the agent hasn't replied
    /// yet. Suppressed preludes are filtered out.
    pub agent_msgs: Vec<ResponseDisplayLine>,
    /// The 1-based turn index (for display in the chat-bubble
    /// header).
    pub turn_index: usize,
    /// Number of input lines consumed by this turn (1 user + N
    /// agent + any filtered preludes). Used by `iter_turns` to
    /// advance the slice pointer.
    _consumed: usize,
}

impl ChatTurn {
    /// Build a `ChatTurn` from a slice of `ResponseDisplayLine`s
    /// starting at the first `MsgKind::User` line. Returns
    /// `None` if the first line is not a `User` line.
    ///
    /// The returned turn consumes lines until the next `User` line
    /// (exclusive) or the end of the slice. Suppressed `FinalAnswer`
    /// preludes are filtered out of `agent_msgs` but still count
    /// toward [`ChatTurn::consumed`].
    pub fn from_lines(lines: &[ResponseDisplayLine], turn_index: usize) -> Option<Self> {
        let first = lines.first()?;
        if first.kind != MsgKind::User {
            return None;
        }
        let user_msg = first.clone();
        let mut agent_msgs = Vec::new();
        let mut consumed = 1; // 1 for the user_msg
        for line in &lines[1..] {
            consumed += 1;
            if line.kind == MsgKind::User {
                // Don't include this User line in the current turn.
                consumed -= 1;
                break;
            }
            if is_suppressed_prelude(line) {
                continue;
            }
            agent_msgs.push(line.clone());
        }
        Some(Self {
            user_msg,
            agent_msgs,
            turn_index,
            _consumed: consumed,
        })
    }

    /// Return the number of `ResponseDisplayLine`s this turn
    /// consumed (1 user + N agent + any filtered preludes).
    pub fn consumed(&self) -> usize {
        self._consumed
    }

    /// Render the turn into `area` using a `FlexContainer`.
    /// Returns the per-sub-unit rects.
    ///
    /// `source_offset` is the index of this turn's `user_msg` in
    /// the caller's `response_lines()` list. `agent_msgs[j]` is
    /// therefore at `source_offset + 1 + j` (offset by 1 to
    /// skip the user line).
    ///
    /// `selection_for_line` is a callback that returns the
    /// char-level selection range `(start, end)` for a given
    /// source line index, or `None` if no selection is active.
    /// Used to highlight the mouse-selected range inside each
    /// rendered sub-unit (T-70).
    pub fn render(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        inner_w: usize,
        colors: &ColorScheme,
        source_offset: usize,
        selection_for_line: &mut dyn FnMut(usize) -> Option<(usize, usize)>,
    ) -> Vec<Rect> {
        if area.height == 0 || area.width == 0 {
            return Vec::new();
        }

        // Build children: [user_bubble, then per-agent sub-units
        // with 1-row gap between different kinds].
        let mut children: Vec<FlexChild> = Vec::new();

        // User bubble: a single sub-unit of 1 or 2 rows
        // (title + body if text is non-empty). T-72: the
        // body line is rendered with `Paragraph::wrap` so
        // long user queries wrap inside the panel rather
        // than overflow / clip.
        let user_bubble = build_user_bubble(&self.user_msg, colors);
        let user_text_nonempty = user_bubble.len() > 1;
        // Compute the bubble's total height by summing the
        // wrapped heights of each line. The title row is
        // always 1 row. The body row may wrap to multiple
        // rows.
        let mut bubble_height: u16 = 0;
        for (idx, l) in user_bubble.iter().enumerate() {
            if idx == 0 || !user_text_nonempty {
                bubble_height += 1;
            } else {
                // Body line: wrap to inner_w - 2 (so the
                // 2-space indent doesn't overflow the first
                // row).
                let body_w = inner_w.saturating_sub(2).max(1);
                bubble_height += wrapped_height(l, body_w);
            }
        }
        // T-70: look up the mouse selection for the user line
        // and apply it to the bubble body. The title row
        // ("▶ You") is always a constant prefix and is NOT
        // subject to selection (selection starts from column
        // 0 of the user msg's actual text content).
        let user_selection = selection_for_line(source_offset);
        children.push(FlexChild::length(
            FlexSize::Length(bubble_height),
            move |frame, rect| {
                // Apply selection only to the body row (the
                // second line, if present). The title row is
                // not part of the user's selectable text.
                let mut bubble = user_bubble;
                if let Some(sel) = user_selection {
                    if bubble.len() > 1 {
                        // The user bubble's body is the
                        // bubble's text content with a 2-space
                        // indent prefix. The selection range
                        // from `App::selection_range_for_segment`
                        // is a char-level range over the raw
                        // user text (without the 2-space
                        // indent). Shift the range by 2 to
                        // account for the indent when applying
                        // it to the rendered body line.
                        let shift = 2usize;
                        let (s, e) = sel;
                        let shifted = (s + shift, e + shift);
                        let last = bubble.len() - 1;
                        let body_line =
                            std::mem::replace(&mut bubble[last], Line::from(""));
                        bubble[last] = apply_selection_to_line_spans(
                            body_line,
                            Some(shifted),
                        );
                    }
                }
                // Build a sub-FlexContainer where the body
                // line wraps to multiple rows if needed. The
                // title is always 1 row. The body is wrapped
                // at `inner_w - 2` (accounting for the 2-space
                // indent).
                let body_w = inner_w.saturating_sub(2).max(1);
                let mut sub_children: Vec<FlexChild> = Vec::new();
                for (idx, l) in bubble.into_iter().enumerate() {
                    if idx == 0 {
                        // Title: 1 row, no wrap.
                        sub_children.push(FlexChild::line(FlexSize::Length(1), l));
                    } else {
                        // Body: wrap, with 2-space continuation
                        // indent.
                        let wrapped = wrap_line_to_lines(l, body_w, 2);
                        for wl in wrapped {
                            sub_children.push(FlexChild::line(FlexSize::Length(1), wl));
                        }
                    }
                }
                let mut sub = FlexContainer::new(FlexDirection::Column)
                    .gap(0)
                    .children(sub_children);
                sub.render(frame, rect);
            },
        ));

        // Agent msgs: T-69 chat log discipline.
        //
        // The chat log is a compact summary, not a content
        // dump. Each non-User record is rendered as 1 line at
        // most, with these rules:
        //
        // - Internal-work kinds (Routing / Thinking / Command /
        //   Separator): DROP entirely from the chat log. The
        //   user only sees them in the popup.
        // - Step records: show ONLY when state = Streaming
        //   (the step is currently in progress). Once a step
        //   completes, it disappears from the chat log — the
        //   FinalAnswer replaces it. While active, the step
        //   shows a spinner glyph (`◐`) and a trailing `…`.
        // - FinalAnswer: always show. Has a body preview
        //   suffix.
        // - Agent / Error: show the first body line as a 1-line
        //   status row (T-68).
        // - Body lines for Step / FinalAnswer / etc.: drop
        //   (body lives in popup only).
        //
        // After FinalAnswer, if any non-User trace content
        // exists for this turn (any Step / Thinking / Routing /
        // Command record, regardless of state), a 1-line
        // "↳ Press Enter to view full trace" hint is rendered
        // so the user knows they can drill into the popup.
        let mut prev_agent_kind: Option<MsgKind> = if user_text_nonempty {
            Some(MsgKind::User)
        } else {
            None
        };
        let mut has_trace = false; // any non-User, non-Final record
        let mut has_active_step = false;
        let mut has_final_answer = false;
        for (idx, line) in self.agent_msgs.iter().enumerate() {
            // Track for the "view details" hint.
            if !matches!(line.kind, MsgKind::User | MsgKind::FinalAnswer)
                && !line.kind.is_internal_work()
                && line.is_header
            {
                has_trace = true;
            }
            if line.kind == MsgKind::Step && line.is_header {
                let is_streaming = line
                    .response_state
                    .map(|s| matches!(s, omega_session::ResponseSectionState::Streaming))
                    .unwrap_or(false);
                if is_streaming {
                    has_active_step = true;
                }
            }
            if line.kind == MsgKind::FinalAnswer && line.is_header {
                has_final_answer = true;
            }
            // T-69: drop internal-work records from the chat log.
            if line.kind.is_internal_work() {
                continue;
            }
            // Drop body lines for card-header kinds (Step /
            // FinalAnswer). Their body content goes to the
            // popup only.
            if !line.is_header && line.kind.has_card_header() {
                continue;
            }
            // T-69: drop Step records that are explicitly
            // complete or failed (state = Complete / Failed).
            // Steps with no state set (default for tests) or
            // state = Streaming are shown. The data layer
            // marks a Step as `Complete` once the work is
            // done, which is when we hide it from the chat
            // log so the FinalAnswer takes over.
            if line.kind == MsgKind::Step
                && line.is_header
                && line
                    .response_state
                    .map(|s| {
                        matches!(
                            s,
                            omega_session::ResponseSectionState::Complete
                                | omega_session::ResponseSectionState::Failed
                        )
                    })
                    .unwrap_or(false)
            {
                continue;
            }
            // For body-only kinds (Agent / Error): only emit
            // the FIRST body line as a 1-line status row.
            let is_first_body = !line.is_header
                && !line.kind.has_card_header()
                && self.agent_msgs[..idx]
                    .iter()
                    .all(|l| l.kind != line.kind || l.is_header);
            if !line.is_header && !is_first_body {
                continue;
            }
            if let Some(prev) = prev_agent_kind {
                if prev != line.kind {
                    children.push(FlexChild::length(
                        FlexSize::Length(1),
                        |frame, rect| {
                            let p = ratatui::widgets::Paragraph::new(Line::from(""));
                            frame.render_widget(p, rect);
                        },
                    ));
                }
            }
            let line_clone = line.clone();
            let siblings = self.agent_msgs.clone();
            let colors_value = *colors;
            let header_pos = idx;
            // T-70: the source-line index for this agent line
            // is `source_offset + 1 + idx` (offset by 1 to skip
            // the user_msg at the start of this turn). The
            // selection callback returns the char-level
            // selection range for that line, which we apply to
            // the rendered Line before pushing it to the
            // Paragraph widget.
            let source_line_index = source_offset + 1 + idx;
            let selection = selection_for_line(source_line_index);
            // T-72: compute the wrapped height of the line
            // BEFORE pushing the FlexChild. The summary line
            // is already truncated to `inner_w` by
            // `build_subunit_summary`, but Agent / Error
            // body-only lines are raw data-layer text and
            // could be longer than `inner_w` (e.g. a long
            // error message from a 400 Bad Request).
            let summary_inner = build_summary_for_height(
                &line_clone,
                &siblings,
                header_pos,
                &colors_value,
                inner_w,
            );
            let wrap_h = wrapped_height(&summary_inner, inner_w);
            children.push(FlexChild::length(
                FlexSize::Length(wrap_h),
                move |frame, rect| {
                    let mut l = summary_inner;
                    // Apply mouse selection to the rendered
                    // line (T-70).
                    l = apply_selection_to_line_spans(l, selection);
                    // T-72: wrap the line to fit the rect's
                    // width. The continuation lines use a
                    // 2-space indent to align with the kind
                    // glyph's prefix.
                    let wrapped = wrap_line_to_lines(l, inner_w, 2);
                    let p =
                        ratatui::widgets::Paragraph::new(ratatui::text::Text::from(wrapped));
                    frame.render_widget(p, rect);
                },
            ));
            prev_agent_kind = Some(line.kind);
        }

        // T-69: "view details" hint. If this turn produced any
        // trace content (Step / Thinking / Routing / Command)
        // and the FinalAnswer is present (work is done), show
        // a 1-line hint inviting the user to open the popup.
        // Suppress the hint while a step is still in progress
        // (the spinner on the active step is the dynamic
        // signal; the hint appears once everything settles).
        let show_hint = has_trace && has_final_answer && !has_active_step;
        if show_hint {
            // Add a 1-row gap between FinalAnswer and hint.
            if let Some(prev) = prev_agent_kind {
                let _ = prev; // gap is between any two rows of diff kind
            }
            children.push(FlexChild::length(
                FlexSize::Length(1),
                move |_frame, _rect| {
                    // No-op placeholder; the next child renders
                    // the actual hint line.
                },
            ));
            // Hint row.
            let hint_colors = *colors;
            children.push(FlexChild::length(
                FlexSize::Length(1),
                move |frame, rect| {
                    let l = Line::from(vec![
                        ratatui::text::Span::styled(
                            "  ↳ ",
                            ratatui::style::Style::default()
                                .fg(hint_colors.muted_meta_fg),
                        ),
                        ratatui::text::Span::styled(
                            "Press Enter to view full trace",
                            ratatui::style::Style::default()
                                .fg(hint_colors.context_hint)
                                .add_modifier(ratatui::style::Modifier::ITALIC),
                        ),
                    ]);
                    let p = ratatui::widgets::Paragraph::new(l);
                    frame.render_widget(p, rect);
                },
            ));
        }

        // T-67: the inner container uses gap=0 because we
        // already push explicit gap children (1 row each) for
        // kind-change boundaries. The outer container in
        // `render_response_panel` allocates `turn_height` rows
        // for this turn, and the children's lengths sum to
        // exactly that (1 row per line + 1 row per kind-change
        // gap + user bubble rows).
        let mut container = FlexContainer::new(FlexDirection::Column)
            .gap(0)
            .children(children);
        container.render(frame, area)
    }
}

/// Wrap a `Line` into a `Vec<Line>` where each line is at most
/// `inner_w` chars wide, breaking at character boundaries. The
/// first line keeps the original content; continuation lines
/// are indented by `cont_indent` spaces (so wrapped user
/// queries are visually aligned with the user bubble body).
pub(super) fn wrap_line_to_lines(
    line: Line<'static>,
    inner_w: usize,
    cont_indent: usize,
) -> Vec<Line<'static>> {
    if inner_w == 0 {
        return vec![line];
    }
    // Combine all spans into a single string for wrapping,
    // then re-apply a flat style. Wrapping that preserves
    // per-span styles across wrap boundaries is more
    // involved; for the chat log, all spans use the same
    // style (the summary's header style) so a flat wrap is
    // sufficient.
    let full: String = line
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect();
    let chars: Vec<char> = full.chars().collect();
    if chars.is_empty() {
        return vec![line];
    }
    if chars.len() <= inner_w {
        return vec![line];
    }
    let indent_str = " ".repeat(cont_indent);
    let mut out: Vec<Line<'static>> = Vec::new();
    let mut start = 0usize;
    while start < chars.len() {
        let end = (start + inner_w).min(chars.len());
        let chunk: String = chars[start..end].iter().collect();
        if start == 0 {
            // First chunk: keep the original style (the line's
            // own style).
            out.push(Line::from(chunk));
        } else {
            // Continuation chunk: indent + plain style.
            out.push(Line::from(format!("{indent_str}{chunk}")));
        }
        start = end;
    }
    out
}

/// Compute the wrapped height of a `Line` at `inner_w` chars.
fn wrapped_height(line: &Line<'static>, inner_w: usize) -> u16 {
    if inner_w == 0 {
        return 1;
    }
    let chars: usize = line.spans.iter().map(|s| s.content.chars().count()).sum();
    ((chars.max(1) + inner_w - 1) / inner_w) as u16
}

/// T-72: build the summary `Line` for a sub-unit, used for
/// the chat log height calculation. Identical to the line
/// rendered inside the FlexChild closure (a card-header
/// kind's `build_subunit_summary` or a body-only kind's
/// raw text).
fn build_summary_for_height(
    line: &ResponseDisplayLine,
    siblings: &[ResponseDisplayLine],
    header_pos: usize,
    colors: &ColorScheme,
    inner_w: usize,
) -> Line<'static> {
    if line.is_header || line.kind.has_card_header() {
        super::step_unit::build_subunit_summary(line, siblings, header_pos, colors, inner_w)
    } else {
        Line::from(line.text.clone())
    }
}

/// Build the user chat-bubble (1 or 2 lines: "▶ You" title +
/// optionally a body row) from a `User` response display line.
fn build_user_bubble(line: &ResponseDisplayLine, colors: &ColorScheme) -> Vec<Line<'static>> {
    use super::chrome::Glyph;
    let mut out = Vec::new();
    // Title row: "▶ You"
    let title_style = ratatui::style::Style::default()
        .fg(colors.user_badge_fg)
        .add_modifier(ratatui::style::Modifier::BOLD);
    out.push(Line::from(vec![
        ratatui::text::Span::styled(format!("{} ", Glyph::BULLET), title_style),
        ratatui::text::Span::styled("You", title_style),
    ]));
    // Body row: 2-char indented content (only if non-empty).
    let body_style = ratatui::style::Style::default().fg(colors.text);
    let text = line.text.trim();
    if !text.is_empty() {
        out.push(Line::from(ratatui::text::Span::styled(
            format!("  {text}"),
            body_style,
        )));
    }
    out
}

/// Iterate over a slice of `ResponseDisplayLine`s, yielding
/// successive `ChatTurn`s. A new turn starts at the first
/// `MsgKind::User` line; consecutive non-User lines after a
/// `User` line belong to that turn until the next `User` or
/// end of slice.
///
/// If the slice starts with non-User lines (orphan agent
/// records, e.g. from tests that push a single kind without a
/// user message), those lines form an "unanchored" turn with
/// `user_msg.text == String::new()` and a 0 turn_index.
pub fn iter_turns(lines: &[ResponseDisplayLine]) -> Vec<ChatTurn> {
    iter_turns_with_offsets(lines)
        .into_iter()
        .map(|t| t.turn)
        .collect()
}

/// T-70: like [`iter_turns`] but also returns the source-line
/// offset of each turn in the input slice. The offset is the
/// index in `response_lines()` (or `response_display_lines()`)
/// where the turn's `user_msg` lives. For an orphan turn, the
/// `user_msg` is a synthetic empty line, so the offset still
/// points at the first orphan agent line in the slice.
///
/// This is needed by the new render path to map each rendered
/// sub-unit back to a source line index, so that mouse-selection
/// ranges (which are tracked per source line) can be applied
/// to the right sub-unit.
pub fn iter_turns_with_offsets(
    lines: &[ResponseDisplayLine],
) -> Vec<TurnWithOffset> {
    let mut turns = Vec::new();
    let mut i = 0;
    let mut turn_index: usize = 1;
    while i < lines.len() {
        let source_offset = i;
        if lines[i].kind == MsgKind::User {
            if let Some(turn) = ChatTurn::from_lines(&lines[i..], turn_index) {
                i += turn.consumed();
                turn_index += 1;
                turns.push(TurnWithOffset {
                    turn,
                    source_offset,
                });
            } else {
                break;
            }
        } else {
            // Orphan agent line(s) at the start or between turns.
            let start = i;
            let mut orphan_consumed = 0;
            while i < lines.len() && lines[i].kind != MsgKind::User {
                i += 1;
                orphan_consumed += 1;
            }
            let orphan_lines: Vec<ResponseDisplayLine> = lines[start..i]
                .iter()
                .filter(|l| !is_suppressed_prelude(l))
                .cloned()
                .collect();
            if !orphan_lines.is_empty() {
                let empty_user = ResponseDisplayLine {
                    kind: MsgKind::User,
                    text: String::new(),
                    is_header: false,
                    message_id: None,
                    action: None,
                    is_tool_line: false,
                    tool_status: None,
                    response_state: None,
                    thinking_line_kind: None,
                    spans: Vec::new(),
                };
                turns.push(TurnWithOffset {
                    turn: ChatTurn {
                        user_msg: empty_user,
                        agent_msgs: orphan_lines,
                        turn_index,
                        _consumed: orphan_consumed,
                    },
                    source_offset,
                });
                turn_index += 1;
            }
        }
    }
    turns
}

/// T-70: a turn plus the source-line offset of its `user_msg` in
/// the input slice. See [`iter_turns_with_offsets`].
#[derive(Debug, Clone)]
pub struct TurnWithOffset {
    pub turn: ChatTurn,
    pub source_offset: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use omega_theme::OmegaTheme;

    fn user_line(text: &str) -> ResponseDisplayLine {
        ResponseDisplayLine {
            kind: MsgKind::User,
            text: text.into(),
            is_header: false,
            message_id: Some("u-1".into()),
            action: None,
            is_tool_line: false,
            tool_status: None,
            response_state: None,
            thinking_line_kind: None,
            spans: Vec::new(),
        }
    }

    fn step_line(text: &str, is_header: bool) -> ResponseDisplayLine {
        ResponseDisplayLine {
            kind: MsgKind::Step,
            text: text.into(),
            is_header,
            message_id: Some("s-1".into()),
            action: None,
            is_tool_line: false,
            tool_status: None,
            response_state: None,
            thinking_line_kind: None,
            spans: Vec::new(),
        }
    }

    fn final_answer_line(text: &str, is_header: bool) -> ResponseDisplayLine {
        ResponseDisplayLine {
            kind: MsgKind::FinalAnswer,
            text: text.into(),
            is_header,
            message_id: Some("f-1".into()),
            action: None,
            is_tool_line: false,
            tool_status: None,
            response_state: None,
            thinking_line_kind: None,
            spans: Vec::new(),
        }
    }

    fn prelude_line() -> ResponseDisplayLine {
        ResponseDisplayLine {
            kind: MsgKind::FinalAnswer,
            text: "━".repeat(40),
            is_header: false,
            message_id: Some("f-1".into()),
            action: None,
            is_tool_line: false,
            tool_status: None,
            response_state: None,
            thinking_line_kind: None,
            spans: Vec::new(),
        }
    }

    #[test]
    fn from_lines_single_user_no_agents() {
        let lines = vec![user_line("Hello?")];
        let turn = ChatTurn::from_lines(&lines, 1).unwrap();
        assert_eq!(turn.user_msg.text, "Hello?");
        assert!(turn.agent_msgs.is_empty());
        assert_eq!(turn.turn_index, 1);
    }

    #[test]
    fn from_lines_user_plus_three_agents() {
        let lines = vec![
            user_line("Ask 1"),
            step_line("step workflow Section ●", true),
            final_answer_line("Answer 1", true),
        ];
        let turn = ChatTurn::from_lines(&lines, 1).unwrap();
        assert_eq!(turn.user_msg.text, "Ask 1");
        assert_eq!(turn.agent_msgs.len(), 2);
        assert_eq!(turn.agent_msgs[0].kind, MsgKind::Step);
        assert_eq!(turn.agent_msgs[1].kind, MsgKind::FinalAnswer);
    }

    #[test]
    fn from_lines_skips_final_answer_prelude() {
        let lines = vec![
            user_line("Ask"),
            prelude_line(),
            final_answer_line("Answer", true),
        ];
        let turn = ChatTurn::from_lines(&lines, 1).unwrap();
        // Prelude is filtered; only header (and body if present)
        // remain. Here we have just the header.
        assert_eq!(turn.agent_msgs.len(), 1);
        assert!(turn.agent_msgs[0].is_header);
    }

    #[test]
    fn from_lines_returns_none_if_first_line_not_user() {
        let lines = vec![step_line("step", true), final_answer_line("a", true)];
        assert!(ChatTurn::from_lines(&lines, 1).is_none());
    }

    #[test]
    fn iter_turns_two_user_lines_yields_two_turns() {
        let lines = vec![
            user_line("Ask 1"),
            final_answer_line("Answer 1", true),
            user_line("Ask 2"),
            final_answer_line("Answer 2", true),
        ];
        let turns = iter_turns(&lines);
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].user_msg.text, "Ask 1");
        assert_eq!(turns[0].agent_msgs.len(), 1);
        assert_eq!(turns[1].user_msg.text, "Ask 2");
        assert_eq!(turns[1].agent_msgs.len(), 1);
        assert_eq!(turns[0].turn_index, 1);
        assert_eq!(turns[1].turn_index, 2);
    }

    #[test]
    fn iter_turns_with_prelude_filtered() {
        let lines = vec![
            user_line("Ask"),
            prelude_line(),
            final_answer_line("Answer", true),
            user_line("Ask 2"),
            final_answer_line("Answer 2", true),
        ];
        let turns = iter_turns(&lines);
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].agent_msgs.len(), 1);
    }

    #[test]
    fn consumed_returns_one_plus_agent_count() {
        let lines = vec![
            user_line("Ask"),
            step_line("step", true),
            final_answer_line("a", true),
        ];
        let turn = ChatTurn::from_lines(&lines, 1).unwrap();
        assert_eq!(turn.consumed(), 3);
    }

    #[test]
    fn render_zero_height_returns_empty() {
        let backend = ratatui::backend::TestBackend::new(40, 0);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let mut turn = ChatTurn::from_lines(&[user_line("hi")], 1).unwrap();
        terminal
            .draw(|frame| {
                let rects = turn.render(
                    frame,
                    Rect::new(0, 0, 40, 0),
                    38,
                    &OmegaTheme::dark().render_palette(),
                    0,
                    &mut |_| None,
                );
                assert!(rects.is_empty());
            })
            .unwrap();
    }

    #[test]
    fn render_user_bubble_contains_you_label() {
        let backend = ratatui::backend::TestBackend::new(60, 10);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let mut turn = ChatTurn::from_lines(&[user_line("Tell me a joke.")], 1).unwrap();
        terminal
            .draw(|frame| {
                turn.render(
                    frame,
                    Rect::new(0, 0, 60, 10),
                    58,
                    &OmegaTheme::dark().render_palette(),
                    0,
                    &mut |_| None,
                );
            })
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        let row0: String = (0..60).map(|x| buf[(x, 0)].symbol()).collect();
        let row1: String = (0..60).map(|x| buf[(x, 1)].symbol()).collect();
        assert!(row0.contains("You"), "row 0 should contain 'You' label; got {row0:?}");
        assert!(
            row1.contains("Tell me a joke."),
            "row 1 should contain body text; got {row1:?}"
        );
    }

    #[test]
    fn render_turn_with_agent_subunit_renders_all() {
        let backend = ratatui::backend::TestBackend::new(60, 20);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let lines = vec![user_line("Ask"), final_answer_line("Answer", true)];
        let mut turn = ChatTurn::from_lines(&lines, 1).unwrap();
        terminal
            .draw(|frame| {
                turn.render(
                    frame,
                    Rect::new(0, 0, 60, 20),
                    58,
                    &OmegaTheme::dark().render_palette(),
                    0,
                    &mut |_| None,
                );
            })
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        let joined: String = (0..20)
            .map(|y| (0..60).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("You"), "should contain 'You' badge");
        assert!(joined.contains("Answer"), "should contain agent answer");
    }
}
