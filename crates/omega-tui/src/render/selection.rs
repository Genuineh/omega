//! Response line selection and wrap.
//!
//! Three concerns live here, all centred on a single response line's
//! visual journey from "the model wrote this text" to "a styled cell on
//! the screen":
//!
//! 1. [`response_display_spans`] — split a raw `ResponseDisplayLine` text
//!    into `StyledSpan`s, picking out a status-symbol segment
//!    (e.g. `◉`/`●`/`✕`/`◦`) and giving it a distinct style.
//! 2. [`apply_selection_to_styled_spans`] — fold a `(start, end)` char
//!    selection over the styled spans, reversing the selected range so
//!    the user sees a visible highlight.
//! 3. [`wrap_styled_spans`] — word-wrap a styled-span list to a target
//!    width, preserving style across the wrap boundary.
//!
//! See `docs/TODO.md` Task 39D.

use omega_theme::RenderPalette as ColorScheme;
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;

use crate::app::ResponseDisplayLine;
use crate::render::chrome::Glyph;
use crate::render::markdown::StyledSpan;

use super::style::response_status_symbol_style;

const STATUS_SYMBOL_PADDING: &str = "  ";

/// Split a `ResponseDisplayLine` into the styled spans that will be
/// rendered on screen. If the line already has explicit spans, those
/// win. Otherwise the text is scanned for a status symbol (◉/●/✕/◦)
/// flanked by two spaces, and that segment receives the symbol's style.
pub(super) fn response_display_spans(
    line: &ResponseDisplayLine,
    fallback_style: Style,
    colors: &ColorScheme,
) -> Vec<StyledSpan> {
    if !line.spans.is_empty() {
        return line.spans.clone();
    }

    let Some((start, end)) = find_status_symbol_range(&line.text) else {
        return vec![StyledSpan {
            text: line.text.clone(),
            style: fallback_style,
        }];
    };
    let Some(symbol_style) = response_status_symbol_style(line, colors) else {
        return vec![StyledSpan {
            text: line.text.clone(),
            style: fallback_style,
        }];
    };

    let mut spans = Vec::new();
    if start > 0 {
        spans.push(StyledSpan {
            text: line.text[..start].to_string(),
            style: fallback_style,
        });
    }
    spans.push(StyledSpan {
        text: line.text[start..end].to_string(),
        style: symbol_style,
    });
    if end < line.text.len() {
        spans.push(StyledSpan {
            text: line.text[end..].to_string(),
            style: fallback_style,
        });
    }
    spans
}

fn find_status_symbol_range(text: &str) -> Option<(usize, usize)> {
    let symbols: [String; 4] = [
        Glyph::RUNNING.to_string(),
        Glyph::COMPLETE.to_string(),
        Glyph::FAILED.to_string(),
        Glyph::PLACEHOLDER.to_string(),
    ];
    for symbol in &symbols {
        for (start, _) in text.match_indices(symbol) {
            let end = start + symbol.len();
            let before = &text[..start];
            let after = &text[end..];
            if before.ends_with(STATUS_SYMBOL_PADDING)
                && (after.is_empty() || after.starts_with(STATUS_SYMBOL_PADDING))
            {
                return Some((start, end));
            }
        }
    }
    None
}

/// Apply a `(start, end)` char-level selection to a list of styled spans.
/// The selected range is rendered with `Modifier::REVERSED` so the user
/// sees a visible highlight. Spans outside the selection are passed
/// through unchanged.
pub(super) fn apply_selection_to_styled_spans(
    spans: &[StyledSpan],
    selection: Option<(usize, usize)>,
) -> Vec<StyledSpan> {
    let Some((selection_start, selection_end)) = selection else {
        return spans.to_vec();
    };
    if selection_start >= selection_end {
        return spans.to_vec();
    }

    let mut output = Vec::new();
    let mut current = 0usize;
    for span in spans {
        let span_len = span.text.chars().count();
        let span_start = current;
        let span_end = current + span_len;
        current = span_end;

        if span_len == 0 {
            output.push(span.clone());
            continue;
        }
        if selection_end <= span_start || selection_start >= span_end {
            output.push(span.clone());
            continue;
        }

        let local_start = selection_start.saturating_sub(span_start).min(span_len);
        let local_end = selection_end.saturating_sub(span_start).min(span_len);
        let chars: Vec<char> = span.text.chars().collect();

        if local_start > 0 {
            output.push(StyledSpan {
                text: chars[..local_start].iter().collect(),
                style: span.style,
            });
        }
        if local_start < local_end {
            output.push(StyledSpan {
                text: chars[local_start..local_end].iter().collect(),
                style: span.style.add_modifier(Modifier::REVERSED),
            });
        }
        if local_end < span_len {
            output.push(StyledSpan {
                text: chars[local_end..].iter().collect(),
                style: span.style,
            });
        }
    }
    output
}

/// T-70: apply a `(start, end)` char-level selection to a `Line`'s
/// `Span`s (i.e. ratatui's `Line`, not our internal `StyledSpan`).
/// Used by the new chat log render path (`ChatTurn::render` +
/// `render_response_panel`) to highlight mouse-selected text
/// inline. The selected range is rendered with `Modifier::REVERSED`
/// so the user sees a visible highlight. Spans outside the
/// selection are passed through unchanged.
pub(super) fn apply_selection_to_line_spans(
    line: Line<'static>,
    selection: Option<(usize, usize)>,
) -> Line<'static> {
    let Some((selection_start, selection_end)) = selection else {
        return line;
    };
    if selection_start >= selection_end {
        return line;
    }

    let mut output: Vec<ratatui::text::Span<'static>> = Vec::new();
    let mut current = 0usize;
    for span in line.spans {
        let span_len = span.content.chars().count();
        let span_start = current;
        let span_end = current + span_len;
        current = span_end;

        if span_len == 0 {
            output.push(span);
            continue;
        }
        if selection_end <= span_start || selection_start >= span_end {
            output.push(span);
            continue;
        }

        let local_start = selection_start.saturating_sub(span_start).min(span_len);
        let local_end = selection_end.saturating_sub(span_start).min(span_len);
        let chars: Vec<char> = span.content.chars().collect();

        if local_start > 0 {
            output.push(ratatui::text::Span::styled(
                chars[..local_start].iter().collect::<String>(),
                span.style,
            ));
        }
        if local_start < local_end {
            output.push(ratatui::text::Span::styled(
                chars[local_start..local_end].iter().collect::<String>(),
                span.style.add_modifier(Modifier::REVERSED),
            ));
        }
        if local_end < span_len {
            output.push(ratatui::text::Span::styled(
                chars[local_end..].iter().collect::<String>(),
                span.style,
            ));
        }
    }
    Line::from(output)
}

/// Word-wrap a list of `StyledSpan`s to a target character width. Style
/// propagates across the wrap boundary so a span with `BOLD` modifier
/// keeps that modifier on the wrapped continuation.
pub(super) fn wrap_styled_spans(spans: &[StyledSpan], width: usize) -> Vec<Vec<StyledSpan>> {
    if width == 0 {
        return vec![spans.to_vec()];
    }
    if spans.is_empty() {
        return vec![Vec::new()];
    }

    let mut lines: Vec<Vec<StyledSpan>> = Vec::new();
    let mut current_line: Vec<StyledSpan> = Vec::new();
    let mut current_width = 0usize;

    for span in spans {
        if span.text.is_empty() {
            if current_line.is_empty() {
                current_line.push(span.clone());
            }
            continue;
        }

        let chars: Vec<char> = span.text.chars().collect();
        let mut start = 0usize;
        while start < chars.len() {
            if current_width == width {
                lines.push(current_line);
                current_line = Vec::new();
                current_width = 0;
            }

            let take = (width - current_width).min(chars.len() - start);
            let text: String = chars[start..start + take].iter().collect();
            current_line.push(StyledSpan {
                text,
                style: span.style,
            });
            current_width += take;
            start += take;

            if current_width == width && start < chars.len() {
                lines.push(current_line);
                current_line = Vec::new();
                current_width = 0;
            }
        }
    }

    if current_line.is_empty() {
        if lines.is_empty() {
            lines.push(Vec::new());
        }
    } else {
        lines.push(current_line);
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(text: &str) -> StyledSpan {
        StyledSpan {
            text: text.to_string(),
            style: Style::default(),
        }
    }

    #[test]
    fn apply_selection_passthrough_when_none() {
        let input = vec![span("hello")];
        let out = apply_selection_to_styled_spans(&input, None);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].text, "hello");
    }

    #[test]
    fn apply_selection_marks_substring_reversed() {
        let input = vec![span("hello world")];
        let out = apply_selection_to_styled_spans(&input, Some((0, 5)));
        assert!(out.iter().any(|s| s.style.add_modifier == Modifier::REVERSED));
        assert!(out.iter().any(|s| s.text == "hello"));
        assert!(out.iter().any(|s| s.text == " world"));
    }

    #[test]
    fn wrap_respects_width_boundary() {
        let input = vec![span("abcdefghij")];
        let lines = wrap_styled_spans(&input, 4);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].iter().map(|s| s.text.as_str()).collect::<String>(), "abcd");
        assert_eq!(lines[1].iter().map(|s| s.text.as_str()).collect::<String>(), "efgh");
        assert_eq!(lines[2].iter().map(|s| s.text.as_str()).collect::<String>(), "ij");
    }

    #[test]
    fn wrap_zero_width_returns_single_line() {
        let input = vec![span("hello")];
        let lines = wrap_styled_spans(&input, 0);
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn find_status_symbol_finds_padded_symbol() {
        let range = find_status_symbol_range("scene  research  ◉  research");
        assert!(range.is_some());
        let (start, end) = range.unwrap();
        assert_eq!(&"scene  research  ◉  research"[start..end], "◉");
    }

    #[test]
    fn find_status_symbol_ignores_unpadded() {
        assert!(find_status_symbol_range("◉alone").is_none());
        assert!(find_status_symbol_range("alone◉").is_none());
    }
}
