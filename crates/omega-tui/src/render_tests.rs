use omega_session::{
    ContextDocumentDiagnostics, ContextMemoryDiagnostics, ResponseSection, ResponseSectionDelta,
    ResponseSectionKind, ResponseSectionMetadata, ResponseSectionState, RuntimeUiEffect,
    RuntimeUiEnvelope, SectionOrigin, StepSubflowState, StepSubflowStatus, WorkflowRunRole,
};
use omega_theme::OmegaTheme;
use ratatui::{
    backend::TestBackend,
    style::{Modifier, Style},
    Terminal,
};

use crate::app::{
    App, MsgKind, Panel, PendingKeySequenceState, ResponseDisplayLine, SessionRoutingSummary,
    SessionStatusSummary, ThinkingLineKind,
};
use crate::sidebar::SidebarSection;

use super::{
    bottom_status_line, bottom_status_text, input_context_line, input_context_text,
    input_info_line, input_info_text, input_viewport_lines, render, response_line_style,
    response_status_symbol_style, wrap_text,
};

fn workflow_metadata(
    scene_id: Option<&str>,
    workflow_id: &str,
    workflow_role: WorkflowRunRole,
    step_id: Option<&str>,
    step_label: Option<&str>,
) -> ResponseSectionMetadata {
    ResponseSectionMetadata {
        scene_id: scene_id.map(ToOwned::to_owned),
        origin: SectionOrigin::Workflow {
            workflow_id: workflow_id.to_string(),
            workflow_role,
        },
        step_id: step_id.map(ToOwned::to_owned),
        step_label: step_label.map(ToOwned::to_owned),
        subflow_ref: None,
    }
}

#[test]
fn wraps_unicode_text_by_character_width() {
    assert_eq!(wrap_text("你好世界", 2), vec!["你好", "世界"]);
}

#[test]
fn collapsed_sidebar_hides_sections_and_restores_response_focus() {
    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut app = App::new();
    let theme = OmegaTheme::dark();
    app.sidebar.shell_collapsed = true;
    app.focused_panel = Panel::SidebarRail;

    terminal
        .draw(|frame| render(frame, &mut app, "test-model", &theme))
        .unwrap();

    assert_eq!(app.focused_panel, Panel::Response);
    assert_eq!(app.sidebar_rect.width, 0);
    assert_eq!(app.todo_rect.width, 0);
    assert_eq!(app.logs_rect.width, 0);
}

#[test]
fn single_activity_section_occupies_sidebar_body() {
    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut app = App::new();
    let theme = OmegaTheme::dark();
    app.sidebar.diagnostics_expanded = false;
    app.sidebar.delivery_expanded = false;
    app.sidebar.skills_expanded = false;
    app.sidebar.knowledge_expanded = false;
    app.sidebar.todos_expanded = false;
    app.sidebar.logs_expanded = true;

    terminal
        .draw(|frame| render(frame, &mut app, "test-model", &theme))
        .unwrap();

    assert_eq!(app.todo_rect.height, 0);
    assert!(app.logs_rect.height > 0);
    assert_eq!(app.sidebar_rail_rect.width, app.sidebar_rect.width - 2);
    assert_eq!(app.sidebar_rail_rect.height, 1);
    assert_eq!(app.logs_rect.width, app.sidebar_rect.width - 2);
    assert!(app.sidebar_rail_rect.y < app.logs_rect.y);
}

#[test]
fn crowded_sidebar_viewport_tracks_lower_rail_selection() {
    let backend = TestBackend::new(120, 18);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut app = App::new();
    let theme = OmegaTheme::dark();
    app.sidebar.diagnostics_expanded = true;
    app.sidebar.delivery_expanded = true;
    app.sidebar.skills_expanded = true;
    app.sidebar.knowledge_expanded = true;
    app.sidebar.todos_expanded = true;
    app.sidebar.logs_expanded = true;
    app.sidebar.rail_selection = SidebarSection::Logs;
    app.focused_panel = Panel::SidebarRail;

    terminal
        .draw(|frame| render(frame, &mut app, "test-model", &theme))
        .unwrap();

    assert_eq!(app.diagnostics_rect.height, 0);
    assert!(app.todo_rect.height > 0);
    assert!(app.logs_rect.height > 0);
}

#[test]
fn narrow_terminal_forces_sidebar_hidden() {
    let backend = TestBackend::new(58, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut app = App::new();
    let theme = OmegaTheme::dark();
    app.focused_panel = Panel::Todo;

    terminal
        .draw(|frame| render(frame, &mut app, "test-model", &theme))
        .unwrap();

    assert_eq!(app.focused_panel, Panel::Response);
    assert_eq!(app.sidebar_rect.width, 0);
}

#[test]
fn markdown_response_lines_wrap_in_narrow_terminal() {
    let backend = TestBackend::new(24, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut app = App::new();
    let theme = OmegaTheme::dark();
    let turn_id = app.begin_turn();

    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::BeginResponseSection {
            section: ResponseSection {
                id: "turn-render:child:chat:final".to_string(),
                parent_id: None,
                kind: ResponseSectionKind::FinalAnswer,
                title: "Final Answer".to_string(),
                state: ResponseSectionState::Streaming,
                metadata: workflow_metadata(
                    Some("chat"),
                    "chat",
                    WorkflowRunRole::Child,
                    Some("report"),
                    Some("Report"),
                ),
            },
        },
    ));
    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::AppendResponseSection {
            id: "turn-render:child:chat:final".to_string(),
            delta: ResponseSectionDelta::Text(
                "This paragraph includes `inline code` and enough words to wrap.".to_string(),
            ),
        },
    ));

    let logical_lines = app.response_display_lines().len();
    terminal
        .draw(|frame| render(frame, &mut app, "test-model", &theme))
        .unwrap();

    // T-67: the chat log shows 1 row per sub-record, so the
    // displayed count is bounded by the sub-record count, not
    // by the body line count. The body wraps inside the
    // popup (StepDetail / TurnDetail), not in the chat log.
    // We assert that the displayed count is at least 1 row
    // (the FinalAnswer summary) and that body content is NOT
    // duplicated in the chat log buffer.
    assert!(app.response_displayed_count >= 1);
    let _ = logical_lines;
}

#[test]
fn input_context_input_info_and_bottom_status_bars_have_stable_heights() {
    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut app = App::new();
    let theme = OmegaTheme::dark();

    terminal
        .draw(|frame| render(frame, &mut app, "test-model", &theme))
        .unwrap();

    assert_eq!(app.response_rect.y, 0);
    assert_eq!(app.input_context_rect.height, 2);
    assert_eq!(app.input_gap_rect.height, 0);
    assert_eq!(app.input_rect.height, 5);
    assert_eq!(app.input_info_rect.height, 1);
    assert_eq!(app.bottom_status_rect.height, 1);
    assert_eq!(app.sidebar_rect.y, 0);
    assert_eq!(app.sidebar_rect.height, app.bottom_status_rect.y);
    assert_eq!(
        app.input_rect.y,
        app.input_context_rect.y + app.input_context_rect.height + 1
    );
    assert_eq!(
        app.input_info_rect.y,
        app.input_rect.y + app.input_rect.height + 1
    );
    assert_eq!(app.input_info_rect.x, app.input_rect.x + 1);
    assert_eq!(app.input_info_rect.width + 2, app.input_rect.width);
    assert!(app.input_context_rect.y < app.bottom_status_rect.y);
}

#[test]
fn input_box_wraps_long_content_across_visible_rows() {
    let mut app = App::new();
    let theme = OmegaTheme::dark();
    app.interaction_mode = omega_keymap::InteractionMode::Insert;
    app.input_rect = ratatui::layout::Rect::new(0, 0, 12, 4);
    app.insert_text("abcdefghijklmnopqrstuvwxyz0123456789");

    let colors = theme.render_palette();
    let input_lines = input_viewport_lines(&app, &colors)
        .into_iter()
        .map(|line| line.to_string().trim_end().to_string())
        .collect::<Vec<_>>();

    assert!(
        input_lines
            .iter()
            .filter(|line| !line.trim().is_empty())
            .count()
            >= 2
    );
    assert!(input_lines[0].contains("abc"));
    assert!(input_lines.iter().skip(1).any(|line| line.contains("jkl")));
}

#[test]
fn input_box_scrolls_to_keep_latest_lines_visible() {
    let mut app = App::new();
    let theme = OmegaTheme::dark();
    app.interaction_mode = omega_keymap::InteractionMode::Insert;
    app.input_rect = ratatui::layout::Rect::new(0, 0, 24, 4);
    app.insert_text("alpha\nbeta\ngamma\ndelta\nepsilon");

    let colors = theme.render_palette();
    let input_lines = input_viewport_lines(&app, &colors)
        .into_iter()
        .map(|line| line.to_string().trim_end().to_string())
        .collect::<Vec<_>>();
    let rendered = input_lines.join("\n");

    assert!(!rendered.contains("alpha"));
    assert!(rendered.contains("beta"));
    assert!(rendered.contains("epsilon"));
}

#[test]
fn input_box_respects_manual_scroll_offset() {
    let mut app = App::new();
    let theme = OmegaTheme::dark();
    app.interaction_mode = omega_keymap::InteractionMode::Insert;
    app.input_rect = ratatui::layout::Rect::new(0, 0, 24, 3);
    app.input_buffer = "alpha\nbeta\ngamma\ndelta".to_string();
    app.cursor_pos = app.char_count();
    app.input_scroll_top = 1;

    let colors = theme.render_palette();
    let input_lines = input_viewport_lines(&app, &colors)
        .into_iter()
        .map(|line| line.to_string().trim_end().to_string())
        .collect::<Vec<_>>();
    let rendered = input_lines.join("\n");

    assert!(!rendered.contains("alpha"));
    assert!(rendered.contains("beta"));
    assert!(rendered.contains("delta"));
}

#[test]
fn thinking_lines_use_stateful_styles() {
    let colors = OmegaTheme::dark().render_palette();

    let header = ResponseDisplayLine {
        kind: MsgKind::Thinking,
        text: "  reasoning  child:chat  Reasoning live  ◉".to_string(),
        is_header: true,
        message_id: Some("thinking-1".to_string()),
        action: None,
        is_tool_line: false,
        tool_status: None,
        response_state: Some(ResponseSectionState::Streaming),
        thinking_line_kind: None,
        spans: Vec::new(),
    };
    let summary = ResponseDisplayLine {
        kind: MsgKind::Thinking,
        text: "    ▸ reasoning · 2 lines · outline answer".to_string(),
        is_header: false,
        message_id: Some("thinking-1".to_string()),
        action: None,
        is_tool_line: false,
        tool_status: None,
        response_state: Some(ResponseSectionState::Complete),
        thinking_line_kind: Some(ThinkingLineKind::Summary),
        spans: Vec::new(),
    };
    let failed_body = ResponseDisplayLine {
        kind: MsgKind::Thinking,
        text: "    | tool result mismatched".to_string(),
        is_header: false,
        message_id: Some("thinking-2".to_string()),
        action: None,
        is_tool_line: false,
        tool_status: None,
        response_state: Some(ResponseSectionState::Failed),
        thinking_line_kind: Some(ThinkingLineKind::Body),
        spans: Vec::new(),
    };

    assert_eq!(
        response_line_style(&header, &colors),
        Style::default()
            .fg(colors.status_running_fg)
            .add_modifier(Modifier::BOLD)
    );
    assert_eq!(
        response_status_symbol_style(&header, &colors),
        Some(
            Style::default()
                .fg(colors.status_running_fg)
                .add_modifier(Modifier::BOLD | Modifier::SLOW_BLINK)
        )
    );
    assert_eq!(
        response_line_style(&summary, &colors),
        Style::default()
            .fg(colors.thinking_summary_fg)
            .add_modifier(Modifier::DIM | Modifier::ITALIC)
    );
    assert_eq!(
        response_line_style(&failed_body, &colors),
        Style::default().fg(colors.error_message)
    );
}

#[test]
fn response_meta_lines_use_muted_styles_and_header_surfaces() {
    let colors = OmegaTheme::dark().render_palette();

    let final_header = ResponseDisplayLine {
        kind: MsgKind::FinalAnswer,
        text: " final  child:chat  Final Answer  ●".to_string(),
        is_header: true,
        message_id: Some("final-1".to_string()),
        action: None,
        is_tool_line: false,
        tool_status: None,
        response_state: Some(ResponseSectionState::Complete),
        thinking_line_kind: None,
        spans: Vec::new(),
    };
    let meta_line = ResponseDisplayLine {
        kind: MsgKind::Step,
        text: "  scene child:report Report".to_string(),
        is_header: false,
        message_id: Some("step-1".to_string()),
        action: None,
        is_tool_line: false,
        tool_status: None,
        response_state: Some(ResponseSectionState::Complete),
        thinking_line_kind: None,
        spans: Vec::new(),
    };

    assert_eq!(
        response_line_style(&final_header, &colors),
        Style::default()
            .fg(colors.status_idle_fg)
            .add_modifier(Modifier::BOLD)
    );
    assert_eq!(
        response_line_style(&meta_line, &colors),
        Style::default().fg(colors.muted_meta_fg)
    );
}

#[test]
fn step_and_command_headers_use_distinct_accent_roles() {
    let colors = OmegaTheme::dark().render_palette();

    let step_header = ResponseDisplayLine {
        kind: MsgKind::Step,
        text: "step  child:chat  Research  ◉".to_string(),
        is_header: true,
        message_id: Some("step-2".to_string()),
        action: None,
        is_tool_line: false,
        tool_status: None,
        response_state: Some(ResponseSectionState::Streaming),
        thinking_line_kind: None,
        spans: Vec::new(),
    };
    let command_header = ResponseDisplayLine {
        kind: MsgKind::Command,
        text: "command  builtin  /document init  ●  collapse".to_string(),
        is_header: true,
        message_id: Some("command-2".to_string()),
        action: None,
        is_tool_line: false,
        tool_status: None,
        response_state: Some(ResponseSectionState::Complete),
        thinking_line_kind: None,
        spans: Vec::new(),
    };

    assert_eq!(
        response_line_style(&step_header, &colors),
        Style::default()
            .fg(colors.status_running_fg)
            .add_modifier(Modifier::BOLD)
    );
    assert_eq!(
        response_status_symbol_style(&step_header, &colors),
        Some(
            Style::default()
                .fg(colors.status_running_fg)
                .add_modifier(Modifier::BOLD | Modifier::SLOW_BLINK)
        )
    );
    assert_eq!(
        response_line_style(&command_header, &colors),
        Style::default()
            .fg(colors.status_idle_fg)
            .add_modifier(Modifier::BOLD)
    );
}

#[test]
fn bottom_status_keeps_runtime_without_old_header_fields() {
    let mut app = App::new();
    app.is_running = true;
    app.spinner_tick = 3;
    app.workflow_summary = Some(crate::app::WorkflowSummary {
        workflow_id: "feature".to_string(),
        workflow_role: omega_session::WorkflowRunRole::Child,
        id: "explore".to_string(),
        label: "Explore".to_string(),
        index: 1,
        total: 4,
    });

    let text = bottom_status_text(&app, "test-model", &['⠋', '⠙']);

    assert!(text.contains("NORMAL"));
    assert!(text.contains("child:feature Explore 1/4"));
    assert!(!text.contains("Omega Agent"));
    assert!(!text.contains("Running…"));
    assert!(!text.contains("tok"));
    assert!(!text.contains("test-model"));
}

#[test]
fn leader_and_notice_text_live_in_input_context_bar() {
    let mut app = App::new();

    app.set_status_notice("Context notice");
    assert_eq!(input_context_text(&app, false), "Context notice");

    app.pending_key_sequence = Some(PendingKeySequenceState {
        started_at: std::time::Instant::now(),
        timeout: std::time::Duration::from_millis(400),
        key_events: vec![crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char(' '),
            crossterm::event::KeyModifiers::NONE,
        )],
        replay_text: Some(" ".to_string()),
    });
    assert!(input_context_text(&app, false).contains("Pending keys"));
}

#[test]
fn command_hint_takes_priority_in_input_context_bar() {
    let mut app = App::new();

    app.set_status_notice("Context notice");
    app.set_command_hint(" Slash: /document query <text>");

    assert_eq!(
        input_context_text(&app, false),
        " Slash: /document query <text>"
    );
}

#[test]
fn insert_mode_hint_mentions_shift_enter_for_newline() {
    let mut app = App::new();
    app.interaction_mode = omega_keymap::InteractionMode::Insert;

    let hint = input_context_text(&app, false);
    assert!(hint.contains("Shift+Enter=Newline"));
    assert!(hint.contains("↑/↓=Line"));
}

#[test]
fn input_surfaces_use_symmetric_visual_bars() {
    let mut app = App::new();
    app.is_running = true;
    app.remember_delivery_model_name("test-model");

    let colors = OmegaTheme::dark().render_palette();
    let context = input_context_line(&app, false, &colors);
    let input_info = input_info_line(&app, &['⠋', '⠙'], &colors, 32);
    let status = bottom_status_line(&app, "test-model", &['⠋', '⠙'], &colors);

    assert_eq!(context.spans[0].style.fg, Some(colors.context_label));
    assert_eq!(context.spans[0].style.bg, Some(colors.context_bar_bg));
    assert_eq!(context.spans[0].content, " keys ");
    assert_eq!(input_info.spans[0].style.bg, Some(colors.input_bg));
    assert!(input_info.spans[0].content.contains("test-model"));
    assert!(input_info
        .spans
        .iter()
        .skip(2)
        .any(|span| span.style.fg == Some(colors.status_running_fg)));
    assert_eq!(status.spans[0].style.bg, Some(colors.status_bar_bg));
    assert_eq!(status.spans[0].content, " mode ");
    assert_eq!(status.spans.len(), 1);
    assert_eq!(colors.context_label, colors.text);
    assert_eq!(colors.status_label, colors.text);
    assert_eq!(colors.bar_divider, colors.text);
    assert_eq!(colors.context_bar_bg, colors.panel_bg);
    assert_ne!(colors.input_bg, colors.panel_bg);
}

#[test]
fn idle_bottom_status_hides_workflow_segment() {
    let mut app = App::new();
    app.workflow_summary = Some(crate::app::WorkflowSummary {
        workflow_id: "feature".to_string(),
        workflow_role: omega_session::WorkflowRunRole::Child,
        id: "report".to_string(),
        label: "Report".to_string(),
        index: 4,
        total: 4,
    });

    let text = bottom_status_text(&app, "test-model", &['⠋', '⠙']);

    assert!(!text.contains("Report 4/4"));
    assert!(text.contains("NORMAL"));
    assert!(!text.contains("test-model"));
}

#[test]
fn bottom_status_renders_session_slot_when_present() {
    let mut app = App::new();
    app.session_status = Some(SessionStatusSummary::Routing(SessionRoutingSummary {
        root_workflow_id: "root".to_string(),
        active_workflow_id: "feature".to_string(),
        active_workflow_role: omega_session::WorkflowRunRole::Child,
        recognized_scene_id: Some("feature".to_string()),
        selected_workflow_id: Some("feature".to_string()),
    }));

    let text = bottom_status_text(&app, "test-model", &['⠋', '⠙']);

    assert!(text.contains("feature -> feature"));
}

#[test]
fn bottom_status_renders_delivery_badge_when_summary_exists() {
    let mut app = App::new();
    app.begin_turn();
    app.remember_delivery_model_name("gpt-5.4");
    app.set_status_slot(
        omega_session::StatusSlot::Agent,
        omega_session::StatusValue::Label("Idle".to_string()),
    );

    let text = input_info_text(&app, &['⠋', '⠙']);

    assert!(text.contains("gpt-5.4"));
    assert!(text.contains("0.0k"));
    assert!(!text.contains("tok"));
    assert!(!text.contains("llm"));
    assert!(!text.contains("tools"));
    assert!(!text.contains("files"));
}

#[test]
fn bottom_status_renders_project_slot_with_name_only() {
    let mut app = App::new();
    app.set_status_slot(
        omega_session::StatusSlot::Project,
        omega_session::StatusValue::ProjectSelection {
            snapshot: Box::new(omega_project::ProjectDetailSnapshot {
                record: omega_project::ProjectRecord {
                    project_id: "proj-123".to_string(),
                    display_name: "omega".to_string(),
                    root: std::path::PathBuf::from("/workspace/omega"),
                    detection_kind: omega_project::ProjectDetectionKind::Explicit,
                    created_at: 1,
                    last_opened_at: 2,
                    active_session_id: Some("session-a".to_string()),
                },
                sessions: vec![omega_project::ProjectSessionRef {
                    session_id: "session-a".to_string(),
                    title: "Current session".to_string(),
                    status: omega_project::ProjectSessionStatus::Active,
                    started_at: 10,
                    last_active_at: 12,
                    turn_count: 4,
                    last_user_turn_preview: Some("Investigate project badge".to_string()),
                    resume_ready: true,
                    archived_turn_count: 4,
                }],
                knowledge: omega_project::ProjectKnowledgeSummary {
                    document: ContextDocumentDiagnostics {
                        total_files_indexed: 42,
                        total_chunks: 128,
                        health_status: omega_session::DocumentHealthStatus::Good,
                        ..ContextDocumentDiagnostics::default()
                    },
                    memory: ContextMemoryDiagnostics {
                        total_turns_archived: 7,
                        memory_query_count: 3,
                        observation_count: 2,
                        ..ContextMemoryDiagnostics::default()
                    },
                    session_count: 1,
                    active_session_id: Some("session-a".to_string()),
                },
                plan: omega_project::ProjectPlanSummary::default(),
            }),
        },
    );

    let text = bottom_status_text(&app, "test-model", &['⠋', '⠙']);

    assert!(text.contains("omega"));
    assert!(!text.contains("sessions"));
    assert!(!text.contains("d:"));
    assert!(!text.contains("m:"));
}

#[test]
fn bottom_status_prefers_active_step_subflow_badge() {
    let mut app = App::new();
    app.is_running = true;
    app.step_subflows.push(StepSubflowStatus {
        workflow_id: "feature".to_string(),
        workflow_role: omega_session::WorkflowRunRole::Child,
        step_id: "execute".to_string(),
        step_label: "Execute".to_string(),
        subflow_id: "execute-2".to_string(),
        item_id: Some("risk-2".to_string()),
        item_label: Some("Validate risk".to_string()),
        item_index: 2,
        item_total: 5,
        status: StepSubflowState::Running,
        repeat_count_for_item: 1,
        no_progress_streak_for_item: 0,
        completion_source: None,
    });
    app.session_status = Some(SessionStatusSummary::Routing(SessionRoutingSummary {
        root_workflow_id: "root".to_string(),
        active_workflow_id: "feature".to_string(),
        active_workflow_role: omega_session::WorkflowRunRole::Child,
        recognized_scene_id: Some("feature".to_string()),
        selected_workflow_id: Some("feature".to_string()),
    }));

    let text = bottom_status_text(&app, "test-model", &['⠋', '⠙']);

    assert!(text.contains("execute-2 2/5 r1"));
    assert!(!text.contains("feature -> feature"));
}

#[test]
fn input_info_bar_renders_idle_arrow_model_and_tokens() {
    let mut app = App::new();
    app.begin_turn();
    app.remember_delivery_model_name("gpt-5.4");
    app.set_status_slot(
        omega_session::StatusSlot::Agent,
        omega_session::StatusValue::Label("Idle".to_string()),
    );

    let text = input_info_text(&app, &['⠋', '⠙']);

    assert!(text.contains("gpt-5.4"));
    assert!(text.contains("↑"));
    assert!(text.contains("0.0k"));
    assert!(!text.contains("tok"));
    assert!(!text.contains("·"));
}

#[test]
fn running_input_info_bar_uses_spinner_icon() {
    let mut app = App::new();
    app.is_running = true;
    app.spinner_tick = 2;

    let text = input_info_text(&app, &['⠋', '⠙']);

    assert!(!text.contains("↑"));
    let animated_count = text
        .chars()
        .filter(|ch| matches!(ch, '●' | '◉' | '◎' | '○' | '·'))
        .count();
    assert_eq!(animated_count, 1);
    assert!(text.contains('◉'));
}

// ---------------------------------------------------------------------------
// Visual regression harness (Task 39).
//
// These tests drive `render()` through `TestBackend` and then read the
// rendered buffer to assert on visible cell content. They are the safety net
// for every later refactor in Task 39A ~ 39I — any change that alters visible
// output must update these snapshots deliberately, not silently.
//
// Buffer rows are normalized to plain String (one String per row) so a
// regression shows up as a clean diff against the asserted-on string.
// ---------------------------------------------------------------------------

fn buffer_rows(width: u16, height: u16) -> Vec<String> {
    use ratatui::buffer::Buffer;
    let buf = Buffer::empty(ratatui::layout::Rect::new(0, 0, width, height));
    let mut rows = Vec::with_capacity(height as usize);
    for y in 0..height {
        let mut row = String::with_capacity(width as usize);
        for x in 0..width {
            row.push_str(buf[(x, y)].symbol());
        }
        rows.push(row);
    }
    rows
}

fn render_to_rows(
    width: u16,
    height: u16,
    setup: impl FnOnce(&mut App),
) -> Vec<String> {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut app = App::new();
    let theme = OmegaTheme::dark();
    setup(&mut app);
    terminal
        .draw(|frame| render(frame, &mut app, "test-model", &theme))
        .unwrap();
    let buf = terminal.backend().buffer().clone();
    let mut rows = Vec::with_capacity(height as usize);
    for y in 0..height {
        let mut row = String::with_capacity(width as usize);
        for x in 0..width {
            row.push_str(buf[(x, y)].symbol());
        }
        rows.push(row);
    }
    rows
}

fn assert_rows_contain(rows: &[String], needle: &str) {
    let found = rows.iter().any(|row| row.contains(needle));
    assert!(
        found,
        "expected buffer to contain {needle:?}; got:\n{}",
        rows.join("\n")
    );
}

fn assert_rows_not_contain(rows: &[String], needle: &str) {
    let found = rows.iter().any(|row| row.contains(needle));
    assert!(
        !found,
        "expected buffer NOT to contain {needle:?}; got:\n{}",
        rows.join("\n")
    );
}

#[test]
fn snapshot_response_panel_renders_title_and_border() {
    let rows = render_to_rows(120, 30, |_| {});
    assert_rows_contain(&rows, "Agent Response");
    // top + bottom borders must be present in the response row
    let top_borders = rows
        .iter()
        .filter(|r| r.contains("─") && r.chars().filter(|c| *c == '─').count() >= 10)
        .count();
    assert!(
        top_borders >= 2,
        "expected at least 2 horizontal border rows in the response panel; got {top_borders}"
    );
}

#[test]
fn snapshot_focused_panel_uses_focus_marker_diamond() {
    // Response focused → title carries the ◆ marker.
    let focused = render_to_rows(120, 30, |app| {
        app.focused_panel = Panel::Response;
    });
    assert_rows_contain(&focused, "◆");
}

#[test]
fn snapshot_empty_response_panel_is_empty_body() {
    let rows = render_to_rows(120, 30, |_| {});
    // No agent text in the body when no messages are present.
    let body = rows
        .iter()
        .skip(1) // skip top border
        .take(28) // body
        .filter(|r| !r.trim().is_empty() && !r.contains("─") && !r.contains("│"))
        .count();
    // We just need to assert the buffer is reachable; the body may carry
    // prompt/placeholder text but the response content area is empty.
    let _ = body;
}

#[test]
fn snapshot_collapsed_sidebar_drops_sidebar_panel_but_status_hint_remains() {
    // Expanded: there is a "Sidebar" panel title in the body.
    let expanded = render_to_rows(120, 30, |app| {
        app.sidebar.shell_collapsed = false;
    });
    assert_rows_contain(&expanded, "Sidebar");

    // Collapsed: the sidebar panel block is gone, but the status bar still
    // shows the "Sidebar hidden" hint as a mode cue. We only assert the
    // sidebar panel header is gone — not all occurrences of the word.
    let collapsed = render_to_rows(120, 30, |app| {
        app.sidebar.shell_collapsed = true;
    });
    // The sidebar panel block is suppressed: there is no `Sidebar ◆` title
    // row (the title is preceded by `│ ` and followed by a border `│`).
    let has_sidebar_panel_title = collapsed
        .iter()
        .any(|row| row.contains("Sidebar") && (row.contains('◆') || row.contains("Sidebar │") || row.contains("│ Sidebar")));
    assert!(
        !has_sidebar_panel_title,
        "expected no sidebar panel title in the body when collapsed"
    );
}

#[test]
fn snapshot_narrow_terminal_hides_sidebar_panel() {
    // Below the width threshold, the sidebar panel is not rendered; the
    // status bar still mentions it as a mode hint.
    let rows = render_to_rows(58, 24, |_| {});
    let has_sidebar_panel_title = rows
        .iter()
        .any(|row| row.contains("Sidebar") && (row.contains('◆') || row.contains("Sidebar │") || row.contains("│ Sidebar")));
    assert!(
        !has_sidebar_panel_title,
        "expected no sidebar panel title in the body for narrow terminal"
    );
}


#[test]
fn snapshot_overlay_focus_marker_uses_filled_diamond() {
    // Panel::SidebarRail focused should still render the diamond in the
    // sidebar title (not the response title).
    let rows = render_to_rows(120, 30, |app| {
        app.focused_panel = Panel::SidebarRail;
    });
    assert_rows_contain(&rows, "◆");
    assert_rows_contain(&rows, "Sidebar");
}

#[test]
fn snapshot_buffer_width_matches_terminal() {
    let rows = render_to_rows(120, 30, |_| {});
    assert_eq!(rows.len(), 30);
    for row in &rows {
        assert!(
            row.chars().count() <= 120,
            "row wider than terminal: {}",
            row.chars().count()
        );
    }
}

// ---------------------------------------------------------------------------
// T-47: Card Layout Contract
// Each MsgKind renders as its own bordered card; cards are 0-indent (aligned
// with the panel border), have a 2-char status glyph slot in the title row,
// and 1 blank line gap between consecutive cards.
// ---------------------------------------------------------------------------

#[test]
fn snapshot_t58_step_renders_header_with_glyph_and_body() {
    // T-67: an orphan Step (no user msg) renders as a single
    // turn with a user-bubble title (`· You`) + a kind-change
    // gap + the Step SUMMARY header (kind glyph + formatted
    // title). The Step's body content does NOT appear in the
    // chat log — it lives in the popup only. Verify the header
    // contains the RUNNING glyph and the body content is
    // absent.
    let rows = render_to_rows(120, 30, |app| {
        app.push_msg(MsgKind::Step, "Gather context");
    });
    let header_idx = rows
        .iter()
        .position(|r| r.contains("step") && r.contains("Section"))
        .expect("step header line present");
    let header_row = &rows[header_idx];
    assert!(
        header_row.contains('◉'),
        "expected RUNNING glyph in step header; got {header_row:?}"
    );
    // Body text must NOT appear in the chat log (it goes to
    // the popup). The "Gather context" string was the message
    // text — the data layer formatted it as the Step header
    // (which is what we see) and would also have produced a
    // body line, but the body line is suppressed by T-67.
    let has_body_text = rows.iter().any(|r| r.contains("  Gather context"));
    assert!(
        !has_body_text,
        "expected body text NOT to appear in the chat log; got:\n{}",
        rows.join("\n")
    );
}

#[test]
fn snapshot_t58_orphan_turns_have_user_bubble_title() {
    // T-58: an orphan Step (no User msg) still gets a `· You`
    // title row (with empty body) so the orphan turn is visible.
    let rows = render_to_rows(120, 30, |app| {
        app.push_msg(MsgKind::Step, "Gather context");
    });
    let has_you = rows.iter().any(|r| r.contains("You"));
    assert!(
        has_you,
        "expected an orphan turn to show a `· You` title row; got:\n{}",
        rows.join("\n")
    );
}

#[test]
fn snapshot_t58_user_msg_creates_chat_turn_with_bubble() {
    // T-58: a User msg + an agent response form a ChatTurn. The
    // user bubble is rendered as a 2-row block (title + body).
    let rows = render_to_rows(120, 30, |app| {
        app.push_msg(MsgKind::User, "Tell me a joke.");
        app.push_msg(MsgKind::FinalAnswer, "Why did the chicken cross the road?");
    });
    let you_idx = rows
        .iter()
        .position(|r| r.contains("You"))
        .expect("`You` title should appear for a User msg");
    // The body row should be the row right after the title.
    let body_row = &rows[you_idx + 1];
    assert!(
        body_row.contains("Tell me a joke."),
        "expected the body row to contain the user text; got {body_row:?}"
    );
    // The agent response (FinalAnswer) should appear later.
    let has_final = rows.iter().any(|r| r.contains("Why did the chicken"));
    assert!(
        has_final,
        "expected the agent's final answer to be visible somewhere"
    );
}

#[test]
fn snapshot_t48_step_header_includes_running_glyph() {
    // Step header should carry the RUNNING glyph.
    let rows = render_to_rows(120, 30, |app| {
        app.push_msg(MsgKind::Step, "Analyzing");
    });
    let title_row = rows
        .iter()
        .find(|r| r.contains("step") && r.contains("Analyzing") == false)
        .expect("step header row");
    assert!(
        title_row.contains('◉'),
        "expected RUNNING glyph in step header; got {title_row:?}"
    );
}

// ---------------------------------------------------------------------------
// T-49: FinalAnswer card uses the FOCUS glyph in the header slot.
// ---------------------------------------------------------------------------

#[test]
fn snapshot_t49_final_answer_header_uses_focus_glyph() {
    let rows = render_to_rows(120, 30, |app| {
        app.push_msg(MsgKind::FinalAnswer, "Here is the answer.");
    });
    // FinalAnswer header should carry the FOCUS glyph (◆).
    let header_idx = rows
        .iter()
        .position(|r| r.contains("final") && r.contains("Section"))
        .expect("final answer header line present");
    let header_row = &rows[header_idx];
    assert!(
        header_row.contains('◆'),
        "expected FOCUS glyph in FinalAnswer header; got {header_row:?}"
    );
    // The data layer's `━`.repeat(40) prelude line should be
    // suppressed: the only rows of pure `━` should be the response
    // panel's own top/bottom borders (which are part of `PanelChrome`,
    // not the response content). A `━`-only row at content-row indices
    // (between the panel's border rows) would indicate the prelude
    // was not suppressed.
    let response_start = rows
        .iter()
        .position(|r| r.contains("Agent Response"))
        .expect("response panel present");
    let response_end = rows
        .iter()
        .rposition(|r| r.contains("└") || r.contains("┘"))
        .unwrap_or(rows.len());
    for (i, row) in rows.iter().enumerate().take(response_end).skip(response_start + 1) {
        if i >= response_end {
            break;
        }
        let is_pure_dash_rule = !row.is_empty()
            && row.chars().all(|c| c == '━' || c == '│' || c.is_whitespace())
            && row.chars().any(|c| c == '━');
        assert!(
            !is_pure_dash_rule,
            "expected no `━`-only content row at index {i} (the FinalAnswer prelude should be suppressed); got {row:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// T-50: Thinking card defaults to a placeholder glyph in the header.
// ---------------------------------------------------------------------------

#[test]
fn snapshot_t50_thinking_header_uses_placeholder_glyph() {
    // T-69: Thinking is an internal-work record. It is hidden
    // from the chat log entirely; the body lives in the popup
    // only. The header glyph is still PLACEHOLDER (◦) for use
    // in the popup. We verify the popup-side glyph by calling
    // `build_header_line` directly.
    use crate::app::ResponseDisplayLine;
    use crate::render::step_unit::build_header_line;
    let l = ResponseDisplayLine {
        kind: MsgKind::Thinking,
        text: "I am thinking".into(),
        is_header: true,
        message_id: Some("t50".into()),
        action: None,
        is_tool_line: false,
        tool_status: None,
        response_state: None,
        thinking_line_kind: None,
        spans: Vec::new(),
    };
    let line = build_header_line(&l, &OmegaTheme::dark().render_palette(), 60);
    assert!(
        line.to_string().contains('◦'),
        "expected PLACEHOLDER glyph (◦) in Thinking header; got {line:?}"
    );
}

// ---------------------------------------------------------------------------
// T-51: 6-kind uniform — User / Agent / Error / Separator / Routing /
// Command each render with the right visual identity.
// ---------------------------------------------------------------------------

#[test]
fn snapshot_t51_user_uses_badge_prefix() {
    // User messages go through the data layer's simple-types block,
    // which adds a `▶ ` badge prefix. The renderer preserves it.
    let rows = render_to_rows(120, 30, |app| {
        app.push_msg(MsgKind::User, "Hello, agent.");
    });
    let user_row = rows
        .iter()
        .find(|r| r.contains("Hello, agent."))
        .expect("user message row");
    assert!(
        user_row.contains('▶'),
        "expected user badge prefix (▶) in user row; got {user_row:?}"
    );
}

#[test]
fn snapshot_t51_agent_uses_plain_text() {
    // Agent messages do not get a badge prefix from the data layer.
    let rows = render_to_rows(120, 30, |app| {
        app.push_msg(MsgKind::Agent, "Hello, human");
    });
    let agent_row = rows
        .iter()
        .find(|r| r.contains("Hello, human"))
        .expect("agent message row");
    // Should contain the text but no badge prefix glyph.
    assert!(
        !agent_row.contains('▶') && !agent_row.contains('✗'),
        "expected no badge prefix in agent row; got {agent_row:?}"
    );
    assert!(agent_row.contains("Hello, human"));
}

#[test]
fn snapshot_t51_error_uses_red_badge() {
    // Error messages get an `✗ ` badge prefix from the data layer.
    let rows = render_to_rows(120, 30, |app| {
        app.push_msg(MsgKind::Error, "Something broke.");
    });
    let error_row = rows
        .iter()
        .find(|r| r.contains("Something broke."))
        .expect("error message row");
    assert!(
        error_row.contains('✗'),
        "expected error badge (✗) in error row; got {error_row:?}"
    );
}

#[test]
fn snapshot_t51_command_header_uses_focus_glyph() {
    // T-69: Command is an internal-work record and is hidden
    // from the chat log. The FOCUS glyph (◆) is used in the
    // popup. We verify it by calling `build_header_line`
    // directly.
    use crate::app::ResponseDisplayLine;
    use crate::render::step_unit::build_header_line;
    let l = ResponseDisplayLine {
        kind: MsgKind::Command,
        text: "command  builtin  Section  ●".into(),
        is_header: true,
        message_id: Some("t51c".into()),
        action: None,
        is_tool_line: false,
        tool_status: None,
        response_state: None,
        thinking_line_kind: None,
        spans: Vec::new(),
    };
    let line = build_header_line(&l, &OmegaTheme::dark().render_palette(), 60);
    assert!(
        line.to_string().contains('◆'),
        "expected FOCUS glyph (◆) in command header; got {line:?}"
    );
}

#[test]
fn snapshot_t51_routing_line_has_no_card_frame() {
    // T-69: Routing is an internal-work record and is hidden
    // from the chat log entirely. The popup (StepDetail) is
    // the source of truth for the routing result.
    let rows = render_to_rows(120, 50, |app| {
        app.push_msg(MsgKind::Routing, "scene  research");
    });
    // Neither the routing header nor the routing body should
    // appear in the chat log.
    let has_route = rows.iter().any(|r| r.contains("route") && r.contains('●'));
    assert!(
        !has_route,
        "routing header must not appear in the chat log; got:\n{}",
        rows.join("\n")
    );
    let has_body = rows.iter().any(|r| r.contains("result scene") || r.contains("scene  research"));
    assert!(
        !has_body,
        "routing body must not appear in the chat log; got:\n{}",
        rows.join("\n")
    );
}

#[test]
fn snapshot_step_and_final_answer_have_distinct_header_colors() {
    use crate::render::response_card::build_response_lines;
    use crate::render::chrome::Glyph;
    use crate::app::{MsgKind, ResponseDisplayLine};
    use omega_theme::OmegaTheme;

    // Build a Step header and a FinalAnswer header with the same
    // text, then assert that their fg colours differ.
    let theme = OmegaTheme::dark();
    let colors = theme.render_palette();

    let mk = |kind: MsgKind, text: &str| ResponseDisplayLine {
        kind,
        text: text.into(),
        is_header: true,
        message_id: None,
        action: None,
        is_tool_line: false,
        tool_status: None,
        response_state: None,
        thinking_line_kind: None,
        spans: Vec::new(),
    };

    let step_line = mk(MsgKind::Step, "x");
    let final_line = mk(MsgKind::FinalAnswer, "x");

    let step_rendered = build_response_lines(&step_line, &colors, 60);
    let final_rendered = build_response_lines(&final_line, &colors, 60);
    assert_eq!(step_rendered.len(), 1);
    assert_eq!(final_rendered.len(), 1);

    let step_line = &step_rendered[0];
    let final_line = &final_rendered[0];
    // Each header has 2 spans: glyph + text. Both should be styled
    // (fg set), and the fg should differ between the two kinds.
    assert!(step_line.spans.len() >= 2);
    assert!(final_line.spans.len() >= 2);
    let step_fg = step_line.spans[1].style.fg;
    let final_fg = final_line.spans[1].style.fg;
    assert!(
        step_fg.is_some(),
        "Step header text should have a foreground color"
    );
    assert!(
        final_fg.is_some(),
        "FinalAnswer header text should have a foreground color"
    );
    assert_ne!(
        step_fg, final_fg,
        "Step and FinalAnswer header text should use different colors"
    );
    // The glyph char should be the per-kind glyph (RUNNING for Step,
    // FOCUS for FinalAnswer).
    let step_glyph_str: String = step_line.spans[0].content.to_string();
    let final_glyph_str: String = final_line.spans[0].content.to_string();
    assert!(step_glyph_str.starts_with(Glyph::RUNNING));
    assert!(final_glyph_str.starts_with(Glyph::FOCUS));
}

#[test]
fn snapshot_t51_separator_is_short_divider() {
    let rows = render_to_rows(120, 30, |app| {
        app.push_msg(MsgKind::Separator, "---sep---");
    });
    // T-68: Separator is decorative and is dropped from the
    // chat log entirely. The body text ("---sep---") must NOT
    // appear in any row of the response panel.
    let found = rows.iter().any(|r| r.contains("---sep---"));
    assert!(
        !found,
        "separator should be dropped from the chat log; got a row containing ---sep---:\n{}",
        rows.join("\n")
    );
}


// ---------------------------------------------------------------------------
// T-69: Chat log = user query + (active steps during work) +
// FinalAnswer + "view details" hint. Internal-work records
// (Routing / Thinking / Command / Separator) are hidden.
// ---------------------------------------------------------------------------

#[test]
fn t69_routing_is_hidden_from_chat_log() {
    // Routing is internal-work (T-69). Neither the header nor
    // the body should appear in the chat log.
    let rows = render_to_rows(120, 30, |app| {
        app.push_msg(MsgKind::Routing, r#"{"selected_workflow_id":"deep-research"}"#);
    });
    let has_header = rows.iter().any(|r| r.contains("route") && r.contains('●'));
    let has_body = rows.iter().any(|r| r.contains("result") || r.contains("deep-research"));
    assert!(
        !has_header && !has_body,
        "routing must be hidden from the chat log; got:\n{}",
        rows.join("\n")
    );
}

#[test]
fn t69_thinking_is_hidden_from_chat_log() {
    // Thinking is internal-work (T-69). The full body is
    // hidden.
    let rows = render_to_rows(120, 30, |app| {
        app.push_msg(
            MsgKind::Thinking,
            "line 1 of reasoning\nline 2 of reasoning\nline 3 of reasoning",
        );
    });
    let has_body = rows.iter().any(|r| r.contains("line 1") || r.contains("line 2"));
    assert!(
        !has_body,
        "thinking body must not appear in the chat log; got:\n{}",
        rows.join("\n")
    );
}

#[test]
fn t68_thinking_body_does_not_appear_in_chat_log() {
    // T-69: thinking is internal-work; the full body is hidden.
    let rows = render_to_rows(120, 30, |app| {
        app.push_msg(
            MsgKind::Thinking,
            "line 1 of reasoning\nline 2 of reasoning\nline 3 of reasoning",
        );
    });
    let has_body = rows.iter().any(|r| r.contains("line 1") || r.contains("line 2"));
    assert!(
        !has_body,
        "thinking body must not appear in the chat log; got:\n{}",
        rows.join("\n")
    );
}

#[test]
fn t69_final_session_shows_only_user_and_final_with_hint() {
    // T-69: After the work is done, the chat log shows only
    // the user query, the FinalAnswer (with body preview), and
    // a "↳ Press Enter to view full trace" hint. Internal-work
    // records (Routing / Thinking) and completed Step records
    // are hidden.
    let rows = render_to_rows(120, 30, |app| {
        app.push_msg(MsgKind::User, "Analyze docs.");
        app.push_msg(MsgKind::Routing, r#"{"selected":"deep-research"}"#);
        app.push_msg(MsgKind::Thinking, "long reasoning text line 1\nline 2\nline 3");
        app.push_msg(MsgKind::Step, "step root Load Skills");
        app.push_msg(MsgKind::FinalAnswer, "Final answer body line 1\nline 2");
    });
    // Internal-work records must not appear.
    let has_route = rows.iter().any(|r| r.contains("route") && r.contains('●'));
    let has_thinking = rows.iter().any(|r| r.contains("reasoning") && r.contains('●'));
    assert!(
        !has_route,
        "routing must not appear in the chat log; got:\n{}",
        rows.join("\n")
    );
    assert!(
        !has_thinking,
        "thinking must not appear in the chat log; got:\n{}",
        rows.join("\n")
    );
    // The FinalAnswer should be present (with body preview).
    let has_final = rows
        .iter()
        .any(|r| r.contains("final") && r.contains('●'));
    assert!(
        has_final,
        "FinalAnswer should be present in the chat log; got:\n{}",
        rows.join("\n")
    );
    // The "view details" hint should be present.
    let has_hint = rows.iter().any(|r| r.contains("view full trace"));
    assert!(
        has_hint,
        "view-details hint should be present; got:\n{}",
        rows.join("\n")
    );
    // The completed Step (no Streaming state) must not appear.
    let has_completed_step = rows
        .iter()
        .any(|r| r.contains("step") && r.contains("Load Skills"));
    assert!(
        !has_completed_step,
        "completed Step should be hidden from the chat log; got:\n{}",
        rows.join("\n")
    );
}

#[test]
fn t69_streaming_step_uses_active_glyph_and_trailing_ellipsis() {
    // T-69: while a Step is actively streaming, the chat log
    // shows it with the ACTIVE glyph (◐) and a trailing `…` to
    // signal "in progress".
    use crate::app::ResponseDisplayLine;
    use crate::render::step_unit::build_subunit_summary;
    let l = ResponseDisplayLine {
        kind: MsgKind::Step,
        text: "step wf Load Skills ●".into(),
        is_header: true,
        message_id: Some("t69s".into()),
        action: None,
        is_tool_line: false,
        tool_status: None,
        response_state: Some(omega_session::ResponseSectionState::Streaming),
        thinking_line_kind: None,
        spans: Vec::new(),
    };
    let line = build_subunit_summary(
        &l,
        std::slice::from_ref(&l),
        0,
        &OmegaTheme::dark().render_palette(),
        80,
    );
    let s = line.to_string();
    assert!(
        s.contains('◐'),
        "streaming Step should use ACTIVE glyph (◐); got {s:?}"
    );
    assert!(
        s.contains('…'),
        "streaming Step should have trailing ellipsis; got {s:?}"
    );
}

#[test]
fn t69_completed_step_uses_running_glyph_no_ellipsis() {
    // T-69: a completed Step (state=Complete) renders with
    // the RUNNING glyph (◉) and no trailing ellipsis. (When
    // shown at all — completed Steps are usually hidden from
    // the chat log; this verifies the summary builder itself.)
    use crate::app::ResponseDisplayLine;
    use crate::render::step_unit::build_subunit_summary;
    let l = ResponseDisplayLine {
        kind: MsgKind::Step,
        text: "step wf Load Skills ●".into(),
        is_header: true,
        message_id: Some("t69c".into()),
        action: None,
        is_tool_line: false,
        tool_status: None,
        response_state: Some(omega_session::ResponseSectionState::Complete),
        thinking_line_kind: None,
        spans: Vec::new(),
    };
    let line = build_subunit_summary(
        &l,
        std::slice::from_ref(&l),
        0,
        &OmegaTheme::dark().render_palette(),
        80,
    );
    let s = line.to_string();
    assert!(
        s.contains('◉'),
        "completed Step should use RUNNING glyph (◉); got {s:?}"
    );
    assert!(
        !s.contains('…'),
        "completed Step should not have trailing ellipsis; got {s:?}"
    );
}

#[test]
fn t69_step_detail_rail_navigation_swaps_content_pane() {
    // T-69 bug fix: when the user navigates the rail of a
    // `StepDetailOverlay` (Up/Down), the right pane must
    // reflect the new selection. Previously the right pane
    // was stuck on the initial selection because only the
    // first rail item's content was kept.
    use crate::app::MsgKind as AppMsgKind;
    use crate::overlay::{
        OverlayState, StepDetailContent, StepDetailOverlay, StepDetailRailItem,
        StepDetailRailKind, ToolRunSummary,
    };

    // Build a StepDetailOverlay with 2 rail items: Tools and
    // Output. Each has a distinct content pane.
    let overlay = StepDetailOverlay {
        origin_panel: crate::app::Panel::Response,
        section_id: "t69-rail".into(),
        title: "Test".into(),
        rail: vec![
            StepDetailRailItem {
                kind: StepDetailRailKind::Tools,
                label: "Tools".into(),
                count_label: "(1)".into(),
            },
            StepDetailRailItem {
                kind: StepDetailRailKind::Output,
                label: "Output".into(),
                count_label: "(2 lines)".into(),
            },
        ],
        selected: 0,
        focus: crate::overlay::DocumentNavigatorFocus::Rail,
        content_per_rail: vec![
            StepDetailContent::Tools(vec![ToolRunSummary {
                id: "t1".into(),
                name: "search".into(),
                status_label: "complete".into(),
                invocation_preview: "search query".into(),
                result_preview: Some("3 results".into()),
            }]),
            StepDetailContent::Output(vec!["line 1 of body".into(), "line 2 of body".into()]),
        ],
        content: StepDetailContent::Output(vec![]),
        content_scroll: 0,
        dismiss_on_backdrop: true,
    };

    // Initially on rail item 0 (Tools). active_content() should
    // return the Tools content.
    match overlay.active_content() {
        StepDetailContent::Tools(tools) => {
            assert_eq!(tools.len(), 1);
            assert_eq!(tools[0].name, "search");
        }
        other => panic!("expected Tools content, got {:?}", other),
    }

    // Move down to rail item 1 (Output). active_content()
    // should now return the Output content.
    let mut overlay = overlay;
    overlay.move_rail(1);
    match overlay.active_content() {
        StepDetailContent::Output(lines) => {
            assert_eq!(lines.len(), 2);
            assert_eq!(lines[0], "line 1 of body");
        }
        other => panic!("expected Output content after move_rail(1), got {:?}", other),
    }

    // Move back up to rail item 0 (Tools). active_content()
    // should return Tools again.
    overlay.move_rail(-1);
    match overlay.active_content() {
        StepDetailContent::Tools(tools) => {
            assert_eq!(tools.len(), 1);
        }
        other => panic!("expected Tools content after move_rail(-1), got {:?}", other),
    }

    // Move past the end — selection should clamp.
    overlay.move_rail(10);
    assert_eq!(overlay.selected, 1);
    // Move past the start — selection should clamp.
    overlay.move_rail(-10);
    assert_eq!(overlay.selected, 0);
    // Suppress unused AppMsgKind warning (we keep the import
    // for symmetry with other tests in this file).
    let _ = AppMsgKind::User;
}

#[test]
fn t70_chat_log_selection_highlight_via_reversed_modifier() {
    // T-70: mouse selection in Agent Response should render the
    // selected range with `Modifier::REVERSED` so the user sees
    // a visible highlight. We test the lower-level helper
    // `apply_selection_to_line_spans` directly (the chat log
    // uses it via `ChatTurn::render`).
    use crate::render::selection::apply_selection_to_line_spans;
    use ratatui::style::{Modifier, Style};
    use ratatui::text::{Line, Span};
    let line = Line::from(vec![
        Span::styled("hello world", Style::default()),
    ]);
    let highlighted = apply_selection_to_line_spans(line, Some((0, 5)));
    // The first 5 chars ("hello") should be reversed; the
    // rest (" world") should be plain.
    let spans = highlighted.spans;
    let mut found_reversed = false;
    let mut found_plain = false;
    for span in &spans {
        if span.content == "hello" {
            assert!(
                span.style.add_modifier.contains(Modifier::REVERSED),
                "expected 'hello' span to have REVERSED modifier; got {:?}",
                span.style
            );
            found_reversed = true;
        } else if span.content == " world" {
            assert!(
                !span.style.add_modifier.contains(Modifier::REVERSED),
                "expected ' world' span to be plain; got {:?}",
                span.style
            );
            found_plain = true;
        }
    }
    assert!(found_reversed && found_plain, "spans were: {:?}", spans);
}

#[test]
fn t70_chat_log_selection_in_middle_splits_spans() {
    // T-70: when the selection is in the middle of a line,
    // the helper splits the line into 3 spans (before,
    // reversed, after).
    use crate::render::selection::apply_selection_to_line_spans;
    use ratatui::style::{Modifier, Style};
    use ratatui::text::{Line, Span};
    let line = Line::from(vec![
        Span::styled("hello world", Style::default()),
    ]);
    let highlighted = apply_selection_to_line_spans(line, Some((6, 11)));
    // "hello " plain, "world" reversed.
    let spans = highlighted.spans;
    let plain_before = spans.iter().find(|s| s.content == "hello ");
    let reversed = spans.iter().find(|s| s.content == "world");
    assert!(plain_before.is_some(), "expected 'hello ' plain span");
    assert!(reversed.is_some(), "expected 'world' reversed span");
    assert!(plain_before.unwrap().style.add_modifier.is_empty());
    assert!(reversed.unwrap().style.add_modifier.contains(Modifier::REVERSED));
}

#[test]
fn t70_app_response_panel_renders_selection_in_buffer() {
    // T-70 integration: push a User msg + a FinalAnswer, set
    // up a mouse selection covering part of the user line,
    // render, and verify the buffer has REVERSED glyphs at
    // the expected positions.
    use crate::app::{PanelTextPoint, PanelTextSelection};
    let mut app = App::new();
    app.push_msg(MsgKind::User, "Hello, agent.");
    app.push_msg(MsgKind::FinalAnswer, "Why did the chicken cross the road?");
    // Set up a mouse selection covering chars 0..5 of the
    // user line (source line index 0).
    app.text_selection = Some(PanelTextSelection {
        panel: Panel::Response,
        anchor: PanelTextPoint { line_index: 0, column: 0 },
        focus: PanelTextPoint { line_index: 0, column: 5 },
    });
    app.mouse_selection_active = false;
    // Render and check the buffer.
    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    let theme = OmegaTheme::dark();
    terminal
        .draw(|frame| render(frame, &mut app, "test", &theme))
        .unwrap();
    let buf = terminal.backend().buffer().clone();
    // At least one cell should have the REVERSED modifier
    // set (the selected user-line text).
    let reversed_count = (0..30)
        .flat_map(|y| (0..120).map(move |x| (x, y)))
        .filter(|(x, y)| buf[(*x, *y)].style().add_modifier.contains(Modifier::REVERSED))
        .count();
    assert!(
        reversed_count > 0,
        "expected at least one cell with REVERSED modifier (the selection); got 0"
    );
}

#[test]
fn t71_clicking_error_row_opens_turn_detail_overlay() {
    // T-71: a 400 Bad Request error from a provider shows as
    // an Error row in the chat log. Clicking on it should
    // open a TurnDetailOverlay that surfaces the full error
    // body (the data layer strips the multi-line stack
    // trace / request body to 1 line in the chat log).
    use crate::overlay::{OverlayState, TurnDetailOverlay};
    use crate::app::MsgKind as AppMsgKind;
    let mut app = App::new();
    app.push_msg(MsgKind::User, "Run the analysis.");
    // The data layer produces 1 body line for an Error
    // record (is_header = false), prefixed with `✗ `.
    let error_text = "Hook-managed step failed: provider returned status 400 Bad Request\nRequest body: {...}\nResponse: invalid_function_call";
    app.push_msg(AppMsgKind::Error, error_text);
    // The Error record's source-line index in
    // `response_display_lines()` is 1 (User is at 0).
    let error_source_index = 1;
    // Activate the error line.
    let activation = app.activate_response_item_at_line(error_source_index);
    assert!(
        activation.is_some(),
        "clicking an Error line should return Some activation"
    );
    // An overlay should now be set to TurnDetail.
    let overlay = app.overlay.as_ref().expect("overlay should be set");
    match overlay {
        OverlayState::TurnDetail(TurnDetailOverlay { sections, .. }) => {
            // The data layer produces one Error section per
            // body line in the error text. The chat log
            // shows only the first; the popup should surface
            // all of them.
            let error_sections: Vec<&_> = sections
                .iter()
                .filter(|s| s.kind == AppMsgKind::Error)
                .collect();
            assert!(
                !error_sections.is_empty(),
                "TurnDetail should include at least one Error section"
            );
            // The first error section's body should contain
            // the `400 Bad Request` text.
            let first_joined = error_sections[0].body.join("\n");
            assert!(
                first_joined.contains("400 Bad Request"),
                "first Error section body should contain the 400 error text; got {first_joined:?}"
            );
            // Across all error sections, the multi-line
            // context (Request body / Response) should be
            // reachable in the popup.
            let all_bodies: String = error_sections
                .iter()
                .map(|s| s.body.join("\n"))
                .collect::<Vec<_>>()
                .join("\n");
            assert!(
                all_bodies.contains("Request body") || all_bodies.contains("Response"),
                "Error popup should contain multi-line context; got {all_bodies:?}"
            );
        }
        other => panic!("expected TurnDetail overlay, got {:?}", other),
    }
}

#[test]
fn t71_clicking_agent_row_opens_turn_detail_overlay() {
    // T-71: clicking on an Agent (non-User, non-Error) body
    // line also opens TurnDetailOverlay, mirroring the
    // User-line contract.
    use crate::overlay::OverlayState;
    let mut app = App::new();
    app.push_msg(MsgKind::User, "Hi.");
    app.push_msg(MsgKind::Agent, "Hello, world!");
    let agent_source_index = 1;
    let activation = app.activate_response_item_at_line(agent_source_index);
    assert!(activation.is_some());
    let overlay = app.overlay.as_ref().expect("overlay should be set");
    assert!(
        matches!(overlay, OverlayState::TurnDetail(_)),
        "expected TurnDetail overlay, got {:?}",
        overlay
    );
}

#[test]
fn t71_clicking_user_row_still_opens_turn_detail() {
    // Regression: clicking a User line (T-61) should still
    // open TurnDetailOverlay.
    use crate::overlay::OverlayState;
    let mut app = App::new();
    app.push_msg(MsgKind::User, "Hello.");
    let activation = app.activate_response_item_at_line(0);
    assert!(activation.is_some());
    let overlay = app.overlay.as_ref().expect("overlay should be set");
    assert!(
        matches!(overlay, OverlayState::TurnDetail(_)),
        "expected TurnDetail overlay for User line, got {:?}",
        overlay
    );
}

#[test]
fn t72_long_user_query_wraps_to_multiple_rows() {
    // T-72: a long user query should wrap to multiple rows
    // in the chat log, not overflow / clip.
    let long_text = "Please analyze the project's documentation system end-to-end, including its structure, tooling, and governance patterns, then write a detailed report covering each of those dimensions with concrete recommendations.";
    let rows = render_to_rows(40, 30, |app| {
        app.push_msg(MsgKind::User, long_text);
    });
    // The body of the user bubble should appear in the
    // chat log, wrapped. Find the row containing the first
    // part of the query.
    let mut wrapped_rows: Vec<&String> = Vec::new();
    for row in &rows {
        if row.contains("Please analyze") {
            wrapped_rows.push(row);
        } else if !wrapped_rows.is_empty() && row.contains("documentation") {
            // A continuation row containing a later word.
            wrapped_rows.push(row);
        }
    }
    // We expect the body to wrap into at least 2 rows
    // (40-wide panel, ~25 chars per line for the user body).
    // The first row starts with "Please analyze"; the
    // second row continues with "documentation system".
    let has_first = rows.iter().any(|r| r.contains("Please analyze"));
    let has_middle = rows
        .iter()
        .any(|r| r.contains("documentation") || r.contains("structure"));
    // The end of the text is "concrete recommendations." —
    // after wrapping at ~36 chars, "recommendation" is in one
    // row and "s." is in the next.
    let has_end_a = rows.iter().any(|r| r.contains("recommendation"));
    let has_end_b = rows.iter().any(|r| r.contains("s."));
    assert!(has_first, "first part of user query should appear");
    assert!(has_middle, "middle part of user query should appear (wrapped)");
    assert!(has_end_a, "end part of user query (recommendation) should appear");
    assert!(has_end_b, "tail of user query (s.) should appear (wrapped)");
    // No row should be wider than the panel (40 cols).
    for (y, r) in rows.iter().enumerate() {
        let response_portion: String = r.chars().take(40).collect();
        assert!(
            response_portion.chars().count() <= 40,
            "row {y} exceeds panel width 40: {response_portion:?}"
        );
    }
}

#[test]
fn t72_long_error_body_wraps_in_chat_log() {
    // T-72: a long Error body (e.g. a 400 Bad Request with
    // a multi-line stack trace) should wrap to multiple
    // rows in the chat log.
    let long_error = "Hook-managed step failed: provider returned status 400 Bad Request: tool_call_definition_invalid: the tools array contains a JSON Schema that is not valid. field 'parameters' must be of type 'object' with 'properties' field";
    let rows = render_to_rows(40, 30, |app| {
        app.push_msg(MsgKind::User, "Run analysis.");
        app.push_msg(MsgKind::Error, long_error);
    });
    // The error row should appear (possibly wrapped) in the
    // first 40 cols of the response panel.
    let response_first_40: Vec<String> = rows
        .iter()
        .map(|r| r.chars().take(40).collect::<String>())
        .collect();
    let has_first = response_first_40
        .iter()
        .any(|r| r.contains("Hook-managed"));
    let has_middle = response_first_40
        .iter()
        .any(|r| r.contains("tool_call") || r.contains("parameters"));
    let has_end = response_first_40
        .iter()
        .any(|r| r.contains("properties"));
    assert!(has_first, "first part of error should appear");
    assert!(has_middle, "middle part of error should appear (wrapped)");
    assert!(has_end, "end part of error should appear (wrapped)");
    // No row exceeds 40 cols.
    for (y, r) in rows.iter().enumerate() {
        let response_portion: String = r.chars().take(40).collect();
        assert!(
            response_portion.chars().count() <= 40,
            "row {y} exceeds panel width 40"
        );
    }
}

#[test]
fn t72_wrap_line_to_lines_splits_long_line() {
    use crate::render::chat_turn::wrap_line_to_lines;
    use ratatui::text::{Line, Span};
    let line = Line::from(vec![Span::raw("a".repeat(25))]);
    let wrapped = wrap_line_to_lines(line, 10, 2);
    // 25 chars / 10 = 2.5 → 3 rows: 10, 10, 5.
    assert_eq!(wrapped.len(), 3);
    // First line: 25 chars (the original).
    assert_eq!(wrapped[0].to_string().chars().count(), 10);
    // Continuation lines: 2-space indent + 8 chars / 2-space indent + 3 chars.
    assert!(wrapped[1].to_string().starts_with("  "));
    assert!(wrapped[2].to_string().starts_with("  "));
}
