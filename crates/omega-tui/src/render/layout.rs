use ratatui::text::Span;
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::Line,
    Frame,
};

use omega_keymap::InteractionMode;
use omega_theme::{OmegaTheme, RenderPalette as ColorScheme};

use crate::app::{App, MsgKind, Panel, ResponseDisplayLine};
use crate::render::frame::Frame as RenderedFrame;
use crate::render::markdown::StyledSpan;

use super::chrome::{PanelTitle, panel_title_with_focus, panel_title_with_focus_suffix};
use super::component::{FocusState, Panel as PanelChrome};
use super::overlay::render_overlay;
use super::response_card::{blank_list_item, render_response_line};
use super::sidebar::{render_sidebar_body, render_sidebar_rail};
use super::status::{bottom_status_line, input_context_line, input_info_line};

const INPUT_PROMPT_PREFIX: &str = " > ";
const INPUT_CONTINUATION_PREFIX: &str = "   ";

/// Below this terminal width (in cells), the sidebar is hidden entirely.
/// The status bar still surfaces a "Sidebar hidden" hint so the user
/// knows the panel is reachable via the keymap.
const MIN_TERM_WIDTH_FOR_SIDEBAR: u16 = 60;

/// Below this terminal width, the sidebar takes 30% of horizontal space.
/// Above it, the sidebar takes 34%.
const MIN_TERM_WIDTH_FOR_WIDE_SIDEBAR: u16 = 100;
const SIDEBAR_PCT_NARROW: u16 = 30;
const SIDEBAR_PCT_WIDE: u16 = 34;

/// Vertical layout: bottom status bar is one row; above the response
/// panel sits the input context bar (2 rows) and the input shell
/// (9 rows total including its border).
const STATUS_BAR_HEIGHT: u16 = 1;
const INPUT_CONTEXT_HEIGHT: u16 = 2;
const INPUT_SHELL_HEIGHT: u16 = 9;
const FULL_PERCENTAGE: u16 = 100;

/// Spinner glyphs for the bottom status bar. Picked for legibility at
/// small sizes; the sequence advances once per render tick.
const SPINNER_FRAMES: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

/// Top-level render entry point. Splits the frame into a status bar and
/// the main area, then dispatches each panel to its own `render_*` function.
/// Each helper is responsible for one panel and writes both the visible
/// chrome and the panel's `*_rect` field on `App` so event handlers can
/// route clicks and focus correctly.
pub(crate) fn render(frame: &mut Frame, app: &mut App, model_name: &str, theme: &OmegaTheme) {
    let colors = theme.render_palette();
    app.cached_palette = Some(colors);
    app.remember_delivery_model_name(model_name);

    let frame_layout = compute_frame_layout(frame.area(), app);
    write_panel_rects(app, &frame_layout);
    app.set_frame(RenderedFrame::from_layout(&frame_layout));

    render_status_bar(frame, app, model_name, &colors, frame_layout.status_rect);
    render_response_panel(frame, app, &colors, frame_layout.response_rect);
    render_sidebar_panel(frame, app, &colors, frame_layout.sidebar_rect);
    render_input_area(
        frame,
        app,
        &colors,
        frame_layout.input_shell_rect,
        frame_layout.input_context_rect,
        frame_layout.sidebar_visible,
    );
    app.normalize_focus();
    render_overlay(frame, app, &colors);
}

/// Layout areas computed once per frame. All `*_rect` fields on `App` are
/// derived from this struct; pulling the layout maths into one place
/// keeps the dispatch in `render()` readable.
pub(crate) struct FrameLayout {
    pub(crate) status_rect: Rect,
    pub(crate) response_rect: Rect,
    pub(crate) input_context_rect: Rect,
    pub(crate) input_shell_rect: Rect,
    pub(crate) sidebar_rect: Rect,
    pub(crate) sidebar_visible: bool,
}

fn compute_frame_layout(area: Rect, app: &App) -> FrameLayout {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(STATUS_BAR_HEIGHT)])
        .split(area);

    let term_width = area.width;
    let sidebar_pct: u16 = if term_width < MIN_TERM_WIDTH_FOR_SIDEBAR || app.sidebar.shell_collapsed {
        0
    } else if term_width < MIN_TERM_WIDTH_FOR_WIDE_SIDEBAR {
        SIDEBAR_PCT_NARROW
    } else {
        SIDEBAR_PCT_WIDE
    };
    let resp_pct = FULL_PERCENTAGE - sidebar_pct;

    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(resp_pct),
            Constraint::Percentage(sidebar_pct),
        ])
        .split(chunks[0]);

    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(INPUT_CONTEXT_HEIGHT),
            Constraint::Length(INPUT_SHELL_HEIGHT),
        ])
        .split(main_chunks[0]);

    let sidebar_visible = main_chunks[1].width > 0 && main_chunks[1].height > 0;

    FrameLayout {
        status_rect: chunks[1],
        response_rect: left_chunks[0],
        input_context_rect: left_chunks[1],
        input_shell_rect: left_chunks[2],
        sidebar_rect: main_chunks[1],
        sidebar_visible,
    }
}

fn write_panel_rects(app: &mut App, layout: &FrameLayout) {
    app.response_rect = layout.response_rect;
    app.input_context_rect = layout.input_context_rect;
    app.input_gap_rect = Rect::default();
    app.input_rect = Rect::default();
    app.input_info_rect = Rect::default();
    app.sidebar_rect = layout.sidebar_rect;
    app.sidebar_rail_rect = Rect::default();
    app.todo_rect = Rect::default();
    app.delivery_rect = Rect::default();
    app.document_rect = Rect::default();
    app.memory_rect = Rect::default();
    app.logs_rect = Rect::default();
    app.bottom_status_rect = layout.status_rect;
    app.normalize_mode();
}

fn render_status_bar(
    frame: &mut Frame,
    app: &App,
    model_name: &str,
    colors: &ColorScheme,
    rect: Rect,
) {
    let status = Paragraph::new(bottom_status_line(app, model_name, SPINNER_FRAMES, colors))
        .style(Style::default().fg(colors.text).bg(colors.status_bar_bg));
    frame.render_widget(status, rect);
}

fn render_response_panel(
    frame: &mut Frame,
    app: &mut App,
    colors: &ColorScheme,
    rect: Rect,
) {
    use crate::render::chat_turn::iter_turns;
    use crate::render::flex::{FlexContainer, FlexDirection, FlexSize};

    let response_focused = app.focused_panel == Panel::Response;
    let response_title = panel_title_with_focus(PanelTitle::RESPONSE, response_focused);

    let resp_inner_w = (rect.width as usize).saturating_sub(2).max(1);
    let response_lines = app.response_display_lines();

    // Build ChatTurns: each turn is 1 user message + N agent
    // response records. The outer FlexContainer stacks turns with
    // gap=2 (T-58), so turn boundaries are visually wider than
    // internal kind changes. Each ChatTurn internally uses
    // gap=1 between its sub-units (the per-kind gap).
    let response_panel_chrome = PanelChrome::new(response_title)
        .focus(FocusState::new(response_focused))
        .with_bg(colors.panel_bg)
        .with_border_colors(colors.focus_border, colors.border_dim)
        .with_title_colors(colors.title_fg, colors.context_hint);
    let response_block = response_panel_chrome.block();
    let inner = response_block.inner(rect);
    frame.render_widget(response_block, rect);

    // T-70: use `iter_turns_with_offsets` so each turn knows
    // its source-line offset (the index in `response_lines()`
    // where its `user_msg` lives). This is required by the
    // mouse-selection callback: the new chat log drops many
    // internal-work lines (Routing / Thinking / etc.), so
    // display rows don't map 1:1 to source lines, and the
    // callback needs the source index for each sub-unit.
    use crate::render::chat_turn::iter_turns_with_offsets;
    let turns_with_offsets = iter_turns_with_offsets(&response_lines);
    let turn_count = turns_with_offsets.len();

    // Each ChatTurn renders itself with `FlexSize::Fill` so it
    // takes whatever space the outer container has left. The
    // outer container's gap=2 separates turns with 2 blank rows
    // (T-58). Total visible lines is computed approximately for
    // `response_displayed_count` and the auto-scroll anchor; the
    // actual rendering is driven by the FlexContainer math, not
    // by this estimate.
    let mut total_lines: usize = 0;
    let mut children: Vec<crate::render::flex::FlexChild> = Vec::new();
    let inner_height = (rect.height as usize).saturating_sub(2);
    // T-70: pre-compute the per-source-line char-count vector
    // (used as the upper bound when looking up the selection
    // range for a source line). This is done once outside the
    // turn render closure so the closure doesn't need to
    // borrow `app` immutably.
    let source_line_lens: Vec<usize> = response_lines
        .iter()
        .map(|l| l.text.chars().count())
        .collect();
    for turn_with_offset in turns_with_offsets {
        let mut turn = turn_with_offset.turn;
        let source_offset = turn_with_offset.source_offset;
        // T-69: compute the turn's exact height. The chat log
        // is a compact summary, not a content dump. Rules:
        // - User bubble: 1 or 2 rows.
        // - Internal-work records (Routing / Thinking / Command
        //   / Separator): dropped.
        // - Step records: show only while Streaming; completed
        //   Steps are dropped.
        // - FinalAnswer: 1 row (with body preview).
        // - Agent / Error: 1 row (first body line only).
        // - After FinalAnswer, if the turn produced trace
        //   content (any Step / Thinking / Routing / Command
        //   record), add a 1-row "view details" hint.
        let user_text_nonempty = !turn.user_msg.text.trim().is_empty();
        // T-72: the user bubble body can wrap to multiple
        // rows. Account for the wrapped height.
        let mut turn_height: usize = if user_text_nonempty {
            // 1 row for the title + wrapped rows for the body.
            1 + {
                let body_line_chars: usize = turn
                    .user_msg
                    .text
                    .chars()
                    .count();
                // Body line is `"  {text}"` (2-char indent).
                let body_w = resp_inner_w.saturating_sub(2).max(1);
                let total_chars = body_line_chars + 2;
                total_chars.div_ceil(body_w).max(1)
            }
        } else {
            1
        };
        let mut prev_rendered_kind: Option<MsgKind> = if user_text_nonempty {
            Some(MsgKind::User)
        } else {
            None
        };
        let mut has_trace = false;
        let mut has_active_step = false;
        let mut has_final_answer = false;
        for (line_idx, line) in turn.agent_msgs.iter().enumerate() {
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
            // T-69: drop internal-work records.
            if line.kind.is_internal_work() {
                continue;
            }
            // Drop body lines for card-header kinds.
            if !line.is_header && line.kind.has_card_header() {
                continue;
            }
            // T-69: drop Step records that are explicitly
            // complete or failed. Steps with no state set
            // (test data) or state = Streaming are kept.
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
            // For body-only kinds (Agent / Error), only count
            // the FIRST body line.
            let is_first_body = !line.is_header
                && !line.kind.has_card_header()
                && turn.agent_msgs[..line_idx]
                    .iter()
                    .all(|l| l.kind != line.kind || l.is_header);
            if !line.is_header && !is_first_body {
                continue;
            }
            if let Some(prev) = prev_rendered_kind {
                if prev != line.kind {
                    turn_height += 1; // kind-change gap
                }
            }
            // T-72: compute the wrapped height of this line.
            // Card-header summaries are already truncated to
            // `inner_w`, so they wrap to 1 row. Body-only
            // lines (Agent / Error) may be longer than
            // `inner_w` and wrap to multiple rows.
            let line_chars: usize = if line.is_header || line.kind.has_card_header() {
                // The summary line, after truncation, is
                // at most `resp_inner_w` chars.
                resp_inner_w
            } else {
                line.text.chars().count()
            };
            let wrap_h = line_chars.div_ceil(resp_inner_w.max(1)).max(1);
            turn_height += wrap_h;
            prev_rendered_kind = Some(line.kind);
        }
        // T-69: "view details" hint. +1 gap + 1 hint row.
        if has_trace && has_final_answer && !has_active_step {
            turn_height += 2;
        }
        total_lines += turn_height;
        let colors_value: ColorScheme = *colors;
        // T-70: pre-compute the selection ranges for every
        // source line in this turn (user line at
        // `source_offset`, agent lines at
        // `source_offset + 1 + j` for j in 0..agent_msgs.len()).
        // The selection is consulted inside the render closure
        // so we capture a static slice (no `app` borrow).
        let mut turn_selections: Vec<Option<(usize, usize)>> =
            Vec::with_capacity(1 + turn.agent_msgs.len());
        // User line selection.
        let user_text_len = source_line_lens
            .get(source_offset)
            .copied()
            .unwrap_or(0);
        turn_selections.push(app.selection_range_for_segment(
            Panel::Response,
            source_offset,
            0,
            user_text_len,
        ));
        // Agent line selections.
        for j in 0..turn.agent_msgs.len() {
            let source_line_index = source_offset + 1 + j;
            let text_len = source_line_lens
                .get(source_line_index)
                .copied()
                .unwrap_or(0);
            turn_selections.push(app.selection_range_for_segment(
                Panel::Response,
                source_line_index,
                0,
                text_len,
            ));
        }
        let selections_for_turn = turn_selections;
        children.push(crate::render::flex::FlexChild::length(
            FlexSize::Length(turn_height as u16),
            move |frame, rect| {
                let mut t = turn;
                t.render(
                    frame,
                    rect,
                    resp_inner_w,
                    &colors_value,
                    source_offset,
                    &mut |source_line_index: usize| -> Option<(usize, usize)> {
                        // Map a source-line index (which is
                        // `source_offset` for the user line or
                        // `source_offset + 1 + j` for agent
                        // line j) to the pre-computed
                        // selection range. The closure captures
                        // `selections_for_turn` (a `Vec`) and
                        // `source_offset` (a `usize`).
                        if source_line_index < source_offset {
                            return None;
                        }
                        let idx = source_line_index - source_offset;
                        selections_for_turn.get(idx).copied().flatten()
                    },
                );
            },
        ));
    }

    // Update the turn count for hotkey navigation (T-60 will
    // hook into this). We add a tiny fudge for the gap=2 between
    // turns so the displayed total includes the visible rhythm.
    if turn_count > 0 {
        total_lines += (turn_count - 1) * 2;
    }
    app.response_turn_count = turn_count;
    app.response_displayed_count = total_lines;
    if !app.response_pinned && total_lines > 0 {
        app.response_state.select(Some(total_lines - 1));
    }

    // Build the outer Flex container with gap=2 (turn-level gap)
    // and render it into the inner rect.
    let mut container = FlexContainer::new(FlexDirection::Column)
        .gap(2)
        .children(children);
    container.render(frame, inner);
}

fn render_sidebar_panel(
    frame: &mut Frame,
    app: &mut App,
    colors: &ColorScheme,
    rect: Rect,
) {
    if rect.width == 0 || rect.height == 0 {
        return;
    }
    let sidebar_focused = app.focused_panel == Panel::SidebarRail;
    let sidebar_title = panel_title_with_focus_suffix(PanelTitle::SIDEBAR, sidebar_focused);
    let sidebar_panel = PanelChrome::new(sidebar_title)
        .focus(FocusState::new(sidebar_focused))
        .with_bg(colors.sidebar_bg)
        .with_border_colors(colors.focus_border, colors.border_dim)
        .with_title_colors(colors.title_fg, colors.context_hint);
    let sidebar_block = sidebar_panel.block();
    let sidebar_inner = sidebar_block.inner(rect);
    frame.render_widget(sidebar_block, rect);

    let sidebar_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(sidebar_inner);
    app.sidebar_rail_rect = sidebar_chunks[0];

    render_sidebar_rail(frame, app, colors, sidebar_chunks[0]);
    frame.render_widget(
        Paragraph::new("").style(Style::default().bg(colors.sidebar_bg)),
        sidebar_chunks[1],
    );
    render_sidebar_body(frame, app, colors, sidebar_chunks[2]);
}

fn render_input_area(
    frame: &mut Frame,
    app: &mut App,
    colors: &ColorScheme,
    shell_rect: Rect,
    context_rect: Rect,
    sidebar_visible: bool,
) {
    let input_border_color = match app.interaction_mode {
        InteractionMode::Normal => colors.mode_normal_fg,
        InteractionMode::Insert => colors.mode_insert_fg,
    };
    let input_shell = Block::default()
        .border_type(colors.input_border_type)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(input_border_color))
        .style(Style::default().bg(colors.input_bg));
    let input_inner = input_shell.inner(shell_rect);
    frame.render_widget(input_shell, shell_rect);

    let input_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(input_inner);
    app.input_rect = input_chunks[0];
    app.input_info_rect = Rect {
        x: input_chunks[2].x.saturating_add(1),
        y: input_chunks[2].y,
        width: input_chunks[2].width.saturating_sub(2),
        height: input_chunks[2].height,
    };

    let input = Paragraph::new(input_viewport_lines(app, colors))
        .style(Style::default().fg(colors.text).bg(colors.input_bg));
    frame.render_widget(input, app.input_rect);

    let context = Paragraph::new(input_context_line(app, !sidebar_visible, colors))
        .wrap(Wrap { trim: false })
        .style(Style::default().fg(colors.text).bg(colors.context_bar_bg));
    frame.render_widget(context, context_rect);

    let input_info = Paragraph::new(input_info_line(
        app,
        SPINNER_FRAMES,
        colors,
        app.input_info_rect.width as usize,
    ))
    .style(Style::default().fg(colors.text).bg(colors.input_bg));
    frame.render_widget(input_info, app.input_info_rect);
}

pub(super) fn input_viewport_lines(app: &App, colors: &ColorScheme) -> Vec<Line<'static>> {
    let visible_height = app.input_rect.height as usize;
    if visible_height == 0 {
        return Vec::new();
    }

    let content_width = (app.input_rect.width as usize)
        .saturating_sub(INPUT_PROMPT_PREFIX.chars().count())
        .max(1);
    let lines = build_input_lines(app, colors, content_width);
    let start = app.input_viewport_top(lines.len());

    lines.into_iter().skip(start).take(visible_height).collect()
}

fn build_input_lines(app: &App, colors: &ColorScheme, content_width: usize) -> Vec<Line<'static>> {
    let prompt_style = Style::default().fg(colors.input_text);
    let text_style = match app.interaction_mode {
        InteractionMode::Normal => Style::default().fg(colors.input_placeholder),
        InteractionMode::Insert => Style::default().fg(colors.input_text),
    };
    let placeholder_style = Style::default().fg(colors.input_placeholder);
    let cursor_style = Style::default()
        .fg(colors.input_bg)
        .bg(colors.input_text)
        .add_modifier(Modifier::BOLD);

    let mut lines = vec![input_line_prefix(true, prompt_style)];
    let mut current_width = 0usize;

    if app.input_buffer.is_empty() {
        match app.interaction_mode {
            InteractionMode::Normal => append_input_text(
                &mut lines,
                "Press Space jk to enter insert mode",
                placeholder_style,
                content_width,
                prompt_style,
                &mut current_width,
            ),
            InteractionMode::Insert => append_input_cell(
                &mut lines,
                " ".to_string(),
                cursor_style,
                content_width,
                prompt_style,
                &mut current_width,
            ),
        }

        return lines.into_iter().map(Line::from).collect();
    }

    let chars: Vec<char> = app.input_buffer.chars().collect();
    for (index, character) in chars.iter().enumerate() {
        if index == app.cursor_pos {
            if app.interaction_mode == InteractionMode::Insert {
                append_input_cell(
                    &mut lines,
                    " ".to_string(),
                    cursor_style,
                    content_width,
                    prompt_style,
                    &mut current_width,
                );
            }
        }

        if *character == '\n' {
            lines.push(input_line_prefix(false, prompt_style));
            current_width = 0;
            continue;
        }

        append_input_cell(
            &mut lines,
            character.to_string(),
            text_style,
            content_width,
            prompt_style,
            &mut current_width,
        );
    }

    if app.cursor_pos == chars.len() {
        if app.interaction_mode == InteractionMode::Insert {
            append_input_cell(
                &mut lines,
                " ".to_string(),
                cursor_style,
                content_width,
                prompt_style,
                &mut current_width,
            );
        }
    }

    lines.into_iter().map(Line::from).collect()
}

fn input_line_prefix(is_first: bool, style: Style) -> Vec<Span<'static>> {
    vec![Span::styled(
        if is_first {
            INPUT_PROMPT_PREFIX.to_string()
        } else {
            INPUT_CONTINUATION_PREFIX.to_string()
        },
        style,
    )]
}

fn append_input_text(
    lines: &mut Vec<Vec<Span<'static>>>,
    text: &str,
    style: Style,
    content_width: usize,
    prompt_style: Style,
    current_width: &mut usize,
) {
    for character in text.chars() {
        append_input_cell(
            lines,
            character.to_string(),
            style,
            content_width,
            prompt_style,
            current_width,
        );
    }
}

fn append_input_cell(
    lines: &mut Vec<Vec<Span<'static>>>,
    text: String,
    style: Style,
    content_width: usize,
    prompt_style: Style,
    current_width: &mut usize,
) {
    if *current_width == content_width {
        lines.push(input_line_prefix(false, prompt_style));
        *current_width = 0;
    }

    if lines.is_empty() {
        lines.push(input_line_prefix(true, prompt_style));
    }

    lines
        .last_mut()
        .expect("input viewport always has at least one line")
        .push(Span::styled(text, style));
    *current_width += 1;
}

fn panel_border_style(selected: bool, colors: &ColorScheme) -> Style {
    if selected {
        Style::default()
            .fg(colors.focus_border)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(colors.border_dim)
    }
}

fn panel_title_style(
    selected: bool,
    active_fg: ratatui::style::Color,
    inactive_fg: ratatui::style::Color,
    bg: ratatui::style::Color,
) -> Style {
    let style = Style::default()
        .fg(if selected { active_fg } else { inactive_fg })
        .bg(bg);
    if selected {
        style.add_modifier(Modifier::BOLD)
    } else {
        style
    }
}

fn panel_content_style(
    selected: bool,
    fg: ratatui::style::Color,
    bg: ratatui::style::Color,
) -> Style {
    let style = Style::default().fg(fg).bg(bg);
    if selected {
        style
    } else {
        style.add_modifier(Modifier::DIM)
    }
}
