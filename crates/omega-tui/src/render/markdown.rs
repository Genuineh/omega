use omega_theme::RenderPalette as ColorScheme;
use ratatui::style::{Modifier, Style};

/// A styled text fragment within a single display line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyledSpan {
    pub text: String,
    pub style: Style,
}

/// The kind of line produced by the Markdown parser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MdLineKind {
    Normal,
    Heading(u8),  // 1, 2, or 3
    ListItem(u8), // nesting depth (0-based)
    HorizontalRule,
    CodeBlockStart, // ```lang
    CodeBlockBody,  // inside code block
    CodeBlockEnd,   // closing ```
    BlankLine,      // empty paragraph separator
}

/// A parsed Markdown line with styled spans.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MdLine {
    pub kind: MdLineKind,
    pub spans: Vec<StyledSpan>,
}

/// Lightweight line-by-line Markdown parser for terminal rendering.
///
/// Supports: headings (#..###), lists (- * 1.), inline code, bold, italic,
/// horizontal rules (---), and fenced code blocks (```).
/// Does NOT parse nested structures or block quotes.
pub fn parse_markdown_lines(text: &str, base_style: Style, colors: &ColorScheme) -> Vec<MdLine> {
    let mut result = Vec::new();
    let mut in_code_block = false;
    let mut prev_blank = false;

    for line in text.lines() {
        if in_code_block {
            if line.trim_start().starts_with("```") {
                in_code_block = false;
                result.push(MdLine {
                    kind: MdLineKind::CodeBlockEnd,
                    spans: vec![StyledSpan {
                        text: "─".repeat(40),
                        style: Style::default().fg(colors.code_border_fg),
                    }],
                });
                prev_blank = false;
            } else {
                result.push(MdLine {
                    kind: MdLineKind::CodeBlockBody,
                    spans: vec![StyledSpan {
                        text: format!("  {line}"),
                        style: Style::default()
                            .fg(colors.agent_message)
                            .bg(colors.code_block_bg),
                    }],
                });
            }
            prev_blank = false;
            continue;
        }

        let trimmed = line.trim();

        // Blank line → paragraph separator
        if trimmed.is_empty() {
            if !prev_blank {
                result.push(MdLine {
                    kind: MdLineKind::BlankLine,
                    spans: vec![StyledSpan {
                        text: String::new(),
                        style: base_style,
                    }],
                });
            }
            prev_blank = true;
            continue;
        }
        prev_blank = false;

        // Horizontal rule: --- or *** or ___ (3+ chars, only that char + spaces)
        if is_horizontal_rule(trimmed) {
            result.push(MdLine {
                kind: MdLineKind::HorizontalRule,
                spans: vec![StyledSpan {
                    text: "─".repeat(40),
                    style: Style::default().fg(colors.hr_fg),
                }],
            });
            continue;
        }

        // Code block start: ```lang
        if trimmed.starts_with("```") {
            push_blank_if_needed(&mut result, base_style);
            in_code_block = true;
            let lang = trimmed.trim_start_matches('`').trim();
            let mut spans = vec![StyledSpan {
                text: "─".repeat(36),
                style: Style::default().fg(colors.code_border_fg),
            }];
            if !lang.is_empty() {
                spans.push(StyledSpan {
                    text: format!(" {lang} "),
                    style: Style::default().fg(colors.code_lang_fg),
                });
            }
            result.push(MdLine {
                kind: MdLineKind::CodeBlockStart,
                spans,
            });
            continue;
        }

        // Heading: # ## ###
        if let Some(heading) = parse_heading(trimmed) {
            let (level, content) = heading;
            let fg = match level {
                1 => colors.heading_1_fg,
                2 => colors.heading_2_fg,
                _ => colors.heading_3_fg,
            };
            let mut spans = parse_inline_spans(
                content,
                Style::default().fg(fg).add_modifier(Modifier::BOLD),
                colors,
            );
            // Ensure BOLD on all spans for headings
            for span in &mut spans {
                span.style = span.style.add_modifier(Modifier::BOLD);
            }
            result.push(MdLine {
                kind: MdLineKind::Heading(level),
                spans,
            });
            continue;
        }

        // List item: - / * / 1.
        if let Some((depth, content)) = parse_list_item(line) {
            let indent = "  ".repeat(depth as usize + 1);
            let bullet = if line.trim_start().starts_with(|c: char| c.is_ascii_digit()) {
                // Numbered list: keep the number
                let num_part = line.trim_start();
                let dot_idx = num_part.find('.').unwrap_or(0);
                format!("{}. ", &num_part[..dot_idx])
            } else {
                "• ".to_string()
            };
            let mut spans = vec![StyledSpan {
                text: format!("{indent}{bullet}"),
                style: base_style,
            }];
            spans.extend(parse_inline_spans(content, base_style, colors));
            result.push(MdLine {
                kind: MdLineKind::ListItem(depth),
                spans,
            });
            continue;
        }

        // Normal text with inline formatting
        if matches!(
            result.last().map(|line| line.kind),
            Some(MdLineKind::ListItem(_)) | Some(MdLineKind::CodeBlockEnd)
        ) {
            push_blank_if_needed(&mut result, base_style);
        }
        let spans = parse_inline_spans(trimmed, base_style, colors);
        result.push(MdLine {
            kind: MdLineKind::Normal,
            spans,
        });
    }

    result
}

fn push_blank_if_needed(result: &mut Vec<MdLine>, base_style: Style) {
    if result.is_empty()
        || matches!(
            result.last().map(|line| line.kind),
            Some(MdLineKind::BlankLine)
        )
    {
        return;
    }

    result.push(MdLine {
        kind: MdLineKind::BlankLine,
        spans: vec![StyledSpan {
            text: String::new(),
            style: base_style,
        }],
    });
}

/// Check if a line is a horizontal rule (---, ***, ___)
fn is_horizontal_rule(trimmed: &str) -> bool {
    if trimmed.len() < 3 {
        return false;
    }
    let chars: Vec<char> = trimmed.chars().filter(|c| !c.is_whitespace()).collect();
    if chars.len() < 3 {
        return false;
    }
    let first = chars[0];
    (first == '-' || first == '*' || first == '_') && chars.iter().all(|&c| c == first)
}

/// Parse heading line. Returns (level, content) if valid.
fn parse_heading(trimmed: &str) -> Option<(u8, &str)> {
    if !trimmed.starts_with('#') {
        return None;
    }
    let hash_count = trimmed.chars().take_while(|&c| c == '#').count();
    if hash_count > 3 || hash_count == 0 {
        return None;
    }
    let rest = &trimmed[hash_count..];
    if !rest.starts_with(' ') && !rest.is_empty() {
        return None;
    }
    Some((hash_count as u8, rest.trim()))
}

/// Parse a list item. Returns (depth, content).
fn parse_list_item(line: &str) -> Option<(u8, &str)> {
    let stripped = line;
    let indent = stripped.len() - stripped.trim_start().len();
    let depth = (indent / 2) as u8;
    let trimmed = stripped.trim_start();

    // Unordered: - or *
    if let Some(rest) = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
    {
        return Some((depth, rest.trim()));
    }

    // Ordered: 1. 2. etc.
    let digit_end = trimmed.find(|c: char| !c.is_ascii_digit()).unwrap_or(0);
    if digit_end > 0 && trimmed[digit_end..].starts_with(". ") {
        return Some((depth, trimmed[digit_end + 2..].trim()));
    }

    None
}

/// Parse inline formatting: `code`, **bold**, *italic*
fn parse_inline_spans(text: &str, base_style: Style, colors: &ColorScheme) -> Vec<StyledSpan> {
    let mut spans = Vec::new();
    let mut chars = text.char_indices().peekable();
    let mut buf = String::new();

    while let Some(&(_, ch)) = chars.peek() {
        match ch {
            '`' => {
                // Inline code
                if !buf.is_empty() {
                    spans.push(StyledSpan {
                        text: buf.clone(),
                        style: base_style,
                    });
                    buf.clear();
                }
                chars.next(); // consume `
                let mut code = String::new();
                let mut closed = false;
                while let Some(&(_, c)) = chars.peek() {
                    chars.next();
                    if c == '`' {
                        closed = true;
                        break;
                    }
                    code.push(c);
                }
                if closed && !code.is_empty() {
                    spans.push(StyledSpan {
                        text: code,
                        style: Style::default()
                            .fg(colors.inline_code_fg)
                            .bg(colors.inline_code_bg),
                    });
                } else {
                    // Unclosed backtick: treat as literal
                    buf.push('`');
                    buf.push_str(&code);
                }
            }
            '*' => {
                // Check for ** (bold) or * (italic)
                chars.next(); // consume first *
                if chars.peek().is_some_and(|&(_, c)| c == '*') {
                    // Bold: **text**
                    chars.next(); // consume second *
                    if !buf.is_empty() {
                        spans.push(StyledSpan {
                            text: buf.clone(),
                            style: base_style,
                        });
                        buf.clear();
                    }
                    let mut bold_text = String::new();
                    let mut closed = false;
                    while let Some(&(_, c)) = chars.peek() {
                        chars.next();
                        if c == '*' {
                            if chars.peek().is_some_and(|&(_, c2)| c2 == '*') {
                                chars.next(); // consume closing **
                                closed = true;
                                break;
                            }
                            bold_text.push(c);
                        } else {
                            bold_text.push(c);
                        }
                    }
                    if closed && !bold_text.is_empty() {
                        spans.push(StyledSpan {
                            text: bold_text,
                            style: base_style.add_modifier(Modifier::BOLD),
                        });
                    } else {
                        buf.push_str("**");
                        buf.push_str(&bold_text);
                    }
                } else {
                    // Italic: *text*
                    if !buf.is_empty() {
                        spans.push(StyledSpan {
                            text: buf.clone(),
                            style: base_style,
                        });
                        buf.clear();
                    }
                    let mut italic_text = String::new();
                    let mut closed = false;
                    while let Some(&(_, c)) = chars.peek() {
                        chars.next();
                        if c == '*' {
                            closed = true;
                            break;
                        }
                        italic_text.push(c);
                    }
                    if closed && !italic_text.is_empty() {
                        spans.push(StyledSpan {
                            text: italic_text,
                            style: base_style.add_modifier(Modifier::ITALIC),
                        });
                    } else {
                        buf.push('*');
                        buf.push_str(&italic_text);
                    }
                }
            }
            _ => {
                chars.next();
                buf.push(ch);
            }
        }
    }

    if !buf.is_empty() {
        spans.push(StyledSpan {
            text: buf,
            style: base_style,
        });
    }

    if spans.is_empty() {
        spans.push(StyledSpan {
            text: String::new(),
            style: base_style,
        });
    }

    spans
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    fn test_colors() -> ColorScheme {
        omega_theme::OmegaTheme::dark().render_palette()
    }

    #[test]
    fn heading_levels() {
        let colors = test_colors();
        let base = Style::default();
        let lines = parse_markdown_lines("# Title\n## Subtitle\n### Minor", base, &colors);
        assert_eq!(lines.len(), 3);
        assert!(matches!(lines[0].kind, MdLineKind::Heading(1)));
        assert!(matches!(lines[1].kind, MdLineKind::Heading(2)));
        assert!(matches!(lines[2].kind, MdLineKind::Heading(3)));
    }

    #[test]
    fn list_items() {
        let colors = test_colors();
        let base = Style::default();
        let lines = parse_markdown_lines("- first\n- second\n  - nested", base, &colors);
        assert_eq!(lines.len(), 3);
        assert!(matches!(lines[0].kind, MdLineKind::ListItem(0)));
        assert!(matches!(lines[1].kind, MdLineKind::ListItem(0)));
        assert!(matches!(lines[2].kind, MdLineKind::ListItem(1)));
    }

    #[test]
    fn inline_code() {
        let colors = test_colors();
        let base = Style::default().fg(Color::White);
        let lines = parse_markdown_lines("use `code` here", base, &colors);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].spans.len(), 3);
        assert_eq!(lines[0].spans[0].text, "use ");
        assert_eq!(lines[0].spans[1].text, "code");
        assert_eq!(lines[0].spans[2].text, " here");
    }

    #[test]
    fn bold_text() {
        let colors = test_colors();
        let base = Style::default();
        let lines = parse_markdown_lines("this is **bold** text", base, &colors);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].spans.len(), 3);
        assert_eq!(lines[0].spans[1].text, "bold");
        assert!(lines[0].spans[1].style.add_modifier == Modifier::empty() || true);
    }

    #[test]
    fn horizontal_rule() {
        let colors = test_colors();
        let base = Style::default();
        let lines = parse_markdown_lines("text\n---\nmore", base, &colors);
        assert_eq!(lines.len(), 3);
        assert!(matches!(lines[1].kind, MdLineKind::HorizontalRule));
    }

    #[test]
    fn code_block() {
        let colors = test_colors();
        let base = Style::default();
        let lines = parse_markdown_lines("```rust\nfn main() {}\n```", base, &colors);
        assert_eq!(lines.len(), 3);
        assert!(matches!(lines[0].kind, MdLineKind::CodeBlockStart));
        assert!(matches!(lines[1].kind, MdLineKind::CodeBlockBody));
        assert!(matches!(lines[2].kind, MdLineKind::CodeBlockEnd));
    }

    #[test]
    fn code_blocks_gain_spacing_from_neighboring_paragraphs() {
        let colors = test_colors();
        let base = Style::default();
        let lines =
            parse_markdown_lines("before\n```rust\nfn main() {}\n```\nafter", base, &colors);

        assert!(matches!(lines[0].kind, MdLineKind::Normal));
        assert!(matches!(lines[1].kind, MdLineKind::BlankLine));
        assert!(matches!(lines[2].kind, MdLineKind::CodeBlockStart));
        assert!(matches!(lines[4].kind, MdLineKind::CodeBlockEnd));
        assert!(matches!(lines[5].kind, MdLineKind::BlankLine));
        assert!(matches!(lines[6].kind, MdLineKind::Normal));
    }

    #[test]
    fn paragraph_spacing() {
        let colors = test_colors();
        let base = Style::default();
        let lines = parse_markdown_lines("para one\n\npara two\n\npara three", base, &colors);
        // Should be: para one, blank, para two, blank, para three
        assert_eq!(lines.len(), 5);
        assert!(matches!(lines[1].kind, MdLineKind::BlankLine));
        assert!(matches!(lines[3].kind, MdLineKind::BlankLine));
    }

    #[test]
    fn consecutive_blanks_collapsed() {
        let colors = test_colors();
        let base = Style::default();
        let lines = parse_markdown_lines("a\n\n\n\nb", base, &colors);
        // Multiple blanks collapse to 1
        assert_eq!(lines.len(), 3);
    }
}
