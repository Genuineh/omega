use omega_session::{
    ResponseSection, ResponseSectionDelta, ResponseSectionKind, ResponseSectionMetadata,
    ResponseSectionState, RuntimeUiEffect, RuntimeUiEnvelope, SectionOrigin,
    StepSubflowState, StepSubflowStatus, WorkflowRunRole,
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
    bottom_status_line, bottom_status_text, input_context_line, input_context_text, render,
    response_line_style, response_status_symbol_style, wrap_text,
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
    assert_eq!(app.input_context_rect.height, 2);
    assert_eq!(app.input_gap_rect.height, 0);
    assert_eq!(app.input_rect.height, 6);
    assert_eq!(app.bottom_status_rect.height, 1);
    assert_eq!(app.sidebar_rect.y, 0);
    assert_eq!(app.sidebar_rect.height, app.bottom_status_rect.y);
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
    assert_eq!(colors.context_label, colors.text);
    assert_eq!(colors.status_label, colors.text);
    assert_eq!(colors.bar_divider, colors.text);
    assert_eq!(colors.context_bar_bg, colors.status_bar_bg);
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
fn bottom_status_renders_delivery_badge_when_summary_exists() {
    let mut app = App::new();
    app.begin_turn();
    app.remember_delivery_model_name("gpt-5.4");
    app.set_status_slot(
        omega_session::StatusSlot::Agent,
        omega_session::StatusValue::Label("Idle".to_string()),
    );

    let text = bottom_status_text(&app, "gpt-5.4", &['⠋', '⠙']);

    assert!(text.contains("0 tok · 0 llm · 0 tools · 0 files"));
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
