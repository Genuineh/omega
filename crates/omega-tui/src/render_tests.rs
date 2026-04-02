use omega_session::{
    ResponseSection, ResponseSectionDelta, ResponseSectionKind, ResponseSectionMetadata,
    ResponseSectionState, RuntimeUiEffect, RuntimeUiEnvelope, StepSubflowState,
    StepSubflowStatus, WorkflowRunRole,
};
use omega_theme::OmegaTheme;
use ratatui::{
    backend::TestBackend,
    style::{Modifier, Style},
    Terminal,
};

use crate::app::{
    App, MsgKind, Panel, ResponseDisplayLine, SessionRoutingSummary, SessionStatusSummary,
    ThinkingLineKind,
};

use super::{
    bottom_status_line, bottom_status_text, input_context_line, input_context_text, render,
    response_line_style, wrap_text,
};

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
    app.sidebar.document_expanded = false;
    app.sidebar.memory_expanded = false;
    app.sidebar.todos_expanded = false;
    app.sidebar.logs_expanded = true;

    terminal
        .draw(|frame| render(frame, &mut app, "test-model", &theme))
        .unwrap();

    assert_eq!(app.todo_rect.height, 0);
    assert!(app.logs_rect.height > 0);
    assert_eq!(
        app.logs_rect.height + app.sidebar_rail_rect.height,
        app.sidebar_rect.height - 2
    );
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
                metadata: ResponseSectionMetadata {
                    scene_id: Some("chat".to_string()),
                    workflow_id: "chat".to_string(),
                    workflow_role: WorkflowRunRole::Child,
                    step_id: Some("report".to_string()),
                    step_label: Some("Report".to_string()),
                    subflow_ref: None,
                },
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

    assert!(app.response_displayed_count > logical_lines);
}

#[test]
fn input_context_and_bottom_status_bars_have_stable_heights() {
    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut app = App::new();
    let theme = OmegaTheme::dark();

    terminal
        .draw(|frame| render(frame, &mut app, "test-model", &theme))
        .unwrap();

    assert_eq!(app.response_rect.y, 0);
    assert_eq!(app.input_context_rect.height, 1);
    assert_eq!(app.input_gap_rect.height, 0);
    assert_eq!(app.input_rect.height, 3);
    assert_eq!(app.bottom_status_rect.height, 1);
    assert_eq!(
        app.input_rect.y,
        app.input_context_rect.y + app.input_context_rect.height
    );
    assert!(app.input_context_rect.y < app.bottom_status_rect.y);
}

#[test]
fn thinking_lines_use_stateful_styles() {
    let colors = OmegaTheme::dark().render_palette();

    let header = ResponseDisplayLine {
        kind: MsgKind::Thinking,
        text: "  reasoning  child:chat  Reasoning live  [streaming]".to_string(),
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
            .fg(colors.focus_border)
            .add_modifier(Modifier::BOLD)
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
fn bottom_status_keeps_model_and_runtime_without_old_header_fields() {
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

    assert!(text.contains("test-model"));
    assert!(text.contains("Running…"));
    assert!(text.contains("child:feature Explore 1/4"));
    assert!(!text.contains("Omega Agent"));
    assert!(!text.contains("Mode:"));
    assert!(!text.contains("Focus:"));
    assert!(!text.contains("KM:"));
}

#[test]
fn leader_and_notice_text_live_in_input_context_bar() {
    let mut app = App::new();

    app.set_status_notice("Context notice");
    assert_eq!(input_context_text(&app, false), "Context notice");

    app.leader_pending_since = Some(std::time::Instant::now());
    assert!(input_context_text(&app, false).contains("Leader pending"));
}

#[test]
fn input_surfaces_use_symmetric_visual_bars() {
    let mut app = App::new();
    app.is_running = true;

    let colors = OmegaTheme::dark().render_palette();
    let context = input_context_line(&app, false, &colors);
    let status = bottom_status_line(&app, "test-model", &['⠋', '⠙'], &colors);

    assert_eq!(context.spans[0].style.fg, Some(colors.context_label));
    assert_eq!(context.spans[0].style.bg, Some(colors.context_bar_bg));
    assert_eq!(context.spans[0].content, " keys ");
    assert_eq!(status.spans[0].style.bg, Some(colors.status_bar_bg));
    assert_eq!(status.spans[0].content, " mode ");
    assert_eq!(status.spans[1].style.fg, Some(colors.mode_normal_fg));
    assert_eq!(status.spans[7].style.fg, Some(colors.status_running_fg));
    assert_eq!(colors.context_bar_bg, colors.status_bar_bg);
    assert_eq!(colors.input_bg, colors.context_bar_bg);
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
    assert!(text.contains("● Idle"));
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
