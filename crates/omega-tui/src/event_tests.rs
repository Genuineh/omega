use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use omega_client::test_support::IdleLlmClient;
use omega_core::DynLlmClient;
use omega_keymap::{InteractionMode, KeymapManager};
use omega_project::ProjectDetectionKind;
use omega_session::{
    ConversationMessage, DocumentNavigatorBody, DocumentNavigatorBodyKind,
    DocumentNavigatorEntry, DocumentNavigatorEntryKind, DocumentNavigatorGroup,
    DocumentNavigatorRequest, OperatorPickerAction, OperatorPickerIntent, OperatorPickerItem,
    OperatorPickerOverlayBehavior, OperatorPickerRequest, OperatorPickerShortcut,
    ResponseSectionKind, ResponseSectionState, RuntimeMessage, RuntimeUiEffect,
    RuntimeUiEnvelope, StateMessage, WorkflowRunRole,
};
use omega_theme::OmegaTheme;
use omega_test_support::persistent_test_root;
use omega_workflow::LoadedWorkflowCatalog;
use ratatui::{backend::TestBackend, Terminal};
use std::time::Duration;

use crate::app::{Msg, Panel};
use crate::reducer::TuiUpdateReducer;
use crate::render::render;

use super::*;

#[derive(Default)]
struct FakeClipboard {
    writes: Vec<String>,
    fail_next_write: bool,
}

impl ClipboardBackend for FakeClipboard {
    fn set_text(&mut self, text: &str) -> Result<(), String> {
        if self.fail_next_write {
            self.fail_next_write = false;
            return Err("stale clipboard".to_string());
        }

        self.writes.push(text.to_string());
        Ok(())
    }
}

#[allow(non_upper_case_globals)]
const IdleClient: IdleLlmClient = IdleLlmClient::new("chat should not run in wait-message test");

struct EventReplayHarness {
    app: Arc<Mutex<App>>,
    session: AgentSession,
    tx: mpsc::Sender<omega_session::RuntimeMessageEnvelope>,
    _rx: mpsc::Receiver<omega_session::RuntimeMessageEnvelope>,
    keymap: KeymapManager,
}

fn press_key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent {
        code,
        modifiers,
        kind: KeyEventKind::Press,
        state: crossterm::event::KeyEventState::NONE,
    }
}

impl EventReplayHarness {
    fn new() -> Self {
        let client: DynLlmClient = Arc::new(IdleClient);
        let root = persistent_test_root("tui-event");
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let session = test_session(client, root, &runtime);
        let app = Arc::new(Mutex::new(App::new()));
        let (tx, rx) = mpsc::channel();
        Self {
            app,
            session,
            tx,
            _rx: rx,
            keymap: KeymapManager::default(),
        }
    }

    fn replay_keys(&self, keys: &[(KeyCode, KeyModifiers)]) {
        for (code, modifiers) in keys {
            handle_key_event(
                press_key(*code, *modifiers),
                &self.app,
                &self.session,
                &self.tx,
                &self.keymap,
            )
            .unwrap();
        }
    }

    fn inspect<T>(&self, inspect: impl FnOnce(&App) -> T) -> T {
        let guard = self.app.lock().unwrap();
        inspect(&guard)
    }
}

fn event_test_root(name: &str) -> std::path::PathBuf {
    persistent_test_root(&format!("tui-{name}"))
}

fn apply_runtime_overlays_until_turn_finished(
    app: &Arc<Mutex<App>>,
    rx: &mpsc::Receiver<omega_session::RuntimeMessageEnvelope>,
) {
    loop {
        let envelope = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let turn_id = envelope.turn_id;
        match envelope.message {
            RuntimeMessage::State(StateMessage::ShowOverlay { request }) => {
                let mut app_guard = app.lock().unwrap();
                TuiUpdateReducer::apply(
                    &mut app_guard,
                    RuntimeUiEnvelope::effect(turn_id, RuntimeUiEffect::ShowOverlay(request)),
                );
            }
            RuntimeMessage::State(StateMessage::TurnFinished) => break,
            _ => {}
        }
    }
}

fn write_document_fixture(root: &std::path::Path) {
    let _ = std::fs::create_dir_all(root.join("docs/specs"));
    let _ = std::fs::write(root.join("README.md"), "# Omega Test Fixture\n");
    let _ = std::fs::write(root.join("docs/README.md"), "# Docs\n");
}

fn seed_collapsed_reasoning(app: &mut App) {
    app.response_rect = ratatui::layout::Rect::new(0, 0, 80, 8);
    app.output_msgs.push(Msg {
        kind: MsgKind::Thinking,
        text: "outline answer\nline 2".to_string(),
        id: Some("thinking-1".to_string()),
        parent_id: Some("step-1".to_string()),
        title: Some("Reasoning".to_string()),
        state: Some(ResponseSectionState::Complete),
        workflow_id: Some("research".to_string()),
        workflow_role: Some(WorkflowRunRole::Child),
        scene_id: None,
        subflow_ref: None,
        collapsed: true,
        tool_lane_collapsed: true,
    });
}

fn test_session(
    client: DynLlmClient,
    root: std::path::PathBuf,
    runtime: &tokio::runtime::Runtime,
) -> AgentSession {
    let loaded_catalog = LoadedWorkflowCatalog::load(&root);
    AgentSession::new(omega_session::AgentSessionConfig {
        client,
        system: "system".to_string(),
        cwd: root,
        runtime_handle: runtime.handle().clone(),
        scene_catalog: loaded_catalog.scene_catalog,
        workflow_catalog: loaded_catalog.workflow_catalog,
        prompt_catalog: loaded_catalog.prompt_catalog,
        context_window: 200_000,
        max_output_tokens: 32_000,
        bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        batch_max_requests: omega_core::default_batch_max_requests(),
    })
    .unwrap()
}

#[test]
fn submit_while_running_shows_wait_message() {
    let client: DynLlmClient = Arc::new(IdleClient);
    let root = event_test_root("event-test");
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let session = test_session(client, root, &runtime);
    let app = Arc::new(Mutex::new(App::new()));
    let (tx, _rx) = mpsc::channel();
    {
        let mut app_guard = app.lock().unwrap();
        app_guard.is_running = true;
        app_guard.input_buffer = "pending".to_string();
    }

    let should_quit = handle_submit(&app, &session, &tx).unwrap();
    let app_guard = app.lock().unwrap();

    assert!(!should_quit);
    assert_eq!(app_guard.output_msgs.len(), 1);
    assert_eq!(app_guard.output_msgs[0].kind, MsgKind::Error);
    assert!(app_guard.output_msgs[0]
        .text
        .contains("Previous turn still finishing"));
}

#[test]
fn submit_slash_command_emits_command_section() {
    let client: DynLlmClient = Arc::new(IdleClient);
    let root = event_test_root("slash-command");
    write_document_fixture(&root);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let session = test_session(client, root, &runtime);
    let app = Arc::new(Mutex::new(App::new()));
    let (tx, rx) = mpsc::channel();
    {
        let mut app_guard = app.lock().unwrap();
        app_guard.input_buffer = "/document health".to_string();
    }

    let should_quit = handle_submit(&app, &session, &tx).unwrap();

    assert!(!should_quit);
    let mut recorded = Vec::new();
    loop {
        let envelope = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let finished = matches!(
            envelope.message,
            RuntimeMessage::State(StateMessage::TurnFinished)
        );
        recorded.push(envelope);
        if finished {
            break;
        }
    }

    assert!(recorded.iter().any(|envelope| {
        matches!(
            &envelope.message,
            RuntimeMessage::Conversation(ConversationMessage::BeginSection { section })
                if section.kind == ResponseSectionKind::Command
        )
    }));
    let app_guard = app.lock().unwrap();
    assert!(app_guard
        .output_msgs
        .iter()
        .any(|message| message.text.contains("> /document health")));
}

#[test]
fn typing_slash_command_updates_command_hint() {
    let harness = EventReplayHarness::new();
    {
        let mut app_guard = harness.app.lock().unwrap();
        app_guard.interaction_mode = InteractionMode::Insert;
    }

    harness.replay_keys(&[
        (KeyCode::Char('/'), KeyModifiers::NONE),
        (KeyCode::Char('d'), KeyModifiers::NONE),
        (KeyCode::Char('o'), KeyModifiers::NONE),
        (KeyCode::Char('c'), KeyModifiers::NONE),
        (KeyCode::Char('u'), KeyModifiers::NONE),
        (KeyCode::Char('m'), KeyModifiers::NONE),
        (KeyCode::Char('e'), KeyModifiers::NONE),
        (KeyCode::Char('n'), KeyModifiers::NONE),
        (KeyCode::Char('t'), KeyModifiers::NONE),
    ]);

    let hint = harness.inspect(|app| app.command_hint.clone());
    assert!(hint.as_deref().is_some_and(|value| value.contains("/document")));
}

#[test]
fn shift_enter_inserts_newline_without_submitting() {
    let harness = EventReplayHarness::new();
    {
        let mut app_guard = harness.app.lock().unwrap();
        app_guard.interaction_mode = InteractionMode::Insert;
        app_guard.insert_text("alpha");
    }

    harness.replay_keys(&[
        (KeyCode::Enter, KeyModifiers::SHIFT),
        (KeyCode::Char('b'), KeyModifiers::NONE),
    ]);

    let (buffer, messages) =
        harness.inspect(|app| (app.input_buffer.clone(), app.output_msgs.len()));
    assert_eq!(buffer, "alpha\nb");
    assert_eq!(messages, 0);
}

#[test]
fn up_and_down_move_cursor_between_input_lines() {
    let harness = EventReplayHarness::new();
    {
        let mut app_guard = harness.app.lock().unwrap();
        app_guard.interaction_mode = InteractionMode::Insert;
        app_guard.input_rect = ratatui::layout::Rect::new(0, 0, 24, 4);
        app_guard.insert_text("alpha\nbeta\ngamma");
    }

    harness.replay_keys(&[(KeyCode::Up, KeyModifiers::NONE)]);
    assert_eq!(harness.inspect(|app| app.cursor_pos), 10);

    harness.replay_keys(&[(KeyCode::Down, KeyModifiers::NONE)]);
    assert_eq!(harness.inspect(|app| app.cursor_pos), 16);
}

#[test]
fn mouse_wheel_scrolls_input_viewport_when_pointer_is_inside_input() {
    let app = Arc::new(Mutex::new(App::new()));
    {
        let mut app_guard = app.lock().unwrap();
        app_guard.interaction_mode = InteractionMode::Insert;
        app_guard.input_rect = ratatui::layout::Rect::new(2, 10, 20, 4);
        app_guard.insert_text("alpha\nbeta\ngamma\ndelta\nepsilon\nzeta");
    }

    handle_mouse_event(
        MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 3,
            row: 11,
            modifiers: KeyModifiers::NONE,
        },
        &app,
    );
    handle_mouse_event(
        MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 3,
            row: 11,
            modifiers: KeyModifiers::NONE,
        },
        &app,
    );

    assert_eq!(app.lock().unwrap().input_scroll_top, 2);

    handle_mouse_event(
        MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 3,
            row: 11,
            modifiers: KeyModifiers::NONE,
        },
        &app,
    );

    assert_eq!(app.lock().unwrap().input_scroll_top, 0);
}

#[test]
fn clipboard_backend_is_reused_between_writes() {
    let init_count = std::cell::Cell::new(0);
    let mut backend = None;

    write_text_with_backend(&mut backend, "alpha", || {
        init_count.set(init_count.get() + 1);
        Ok(FakeClipboard::default())
    })
    .unwrap();
    write_text_with_backend(&mut backend, "beta", || {
        init_count.set(init_count.get() + 1);
        Ok(FakeClipboard::default())
    })
    .unwrap();

    assert_eq!(init_count.get(), 1);
    assert_eq!(
        backend.unwrap().writes,
        vec!["alpha".to_string(), "beta".to_string()]
    );
}

#[test]
fn clipboard_backend_reinitializes_after_write_failure() {
    let init_count = std::cell::Cell::new(0);
    let mut backend = Some(FakeClipboard {
        writes: Vec::new(),
        fail_next_write: true,
    });

    write_text_with_backend(&mut backend, "recovered", || {
        init_count.set(init_count.get() + 1);
        Ok(FakeClipboard::default())
    })
    .unwrap();

    assert_eq!(init_count.get(), 1);
    assert_eq!(backend.unwrap().writes, vec!["recovered".to_string()]);
}

#[test]
fn mouse_selection_only_marks_text_ready_for_copy() {
    let app = Arc::new(Mutex::new(App::new()));
    {
        let mut app_guard = app.lock().unwrap();
        app_guard.logs_rect = ratatui::layout::Rect::new(0, 0, 7, 5);
        app_guard.log_lines = vec!["abcdefg".to_string()];
    }

    handle_mouse_event(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 2,
            row: 1,
            modifiers: KeyModifiers::NONE,
        },
        &app,
    );
    handle_mouse_event(
        MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 3,
            row: 2,
            modifiers: KeyModifiers::NONE,
        },
        &app,
    );
    handle_mouse_event(
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 3,
            row: 2,
            modifiers: KeyModifiers::NONE,
        },
        &app,
    );

    let app_guard = app.lock().unwrap();
    assert_eq!(app_guard.selected_text().as_deref(), Some("bcdefg"));
    assert_eq!(
        app_guard.status_notice.as_deref(),
        Some("Selected 6 chars. Press y or Ctrl+C to copy.")
    );
}

#[test]
fn mouse_click_on_collapsed_reasoning_toggles_expand() {
    let app = Arc::new(Mutex::new(App::new()));
    {
        let mut app_guard = app.lock().unwrap();
        seed_collapsed_reasoning(&mut app_guard);
    }

    handle_mouse_event(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 4,
            row: 2,
            modifiers: KeyModifiers::NONE,
        },
        &app,
    );
    handle_mouse_event(
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 4,
            row: 2,
            modifiers: KeyModifiers::NONE,
        },
        &app,
    );

    let app_guard = app.lock().unwrap();
    let thinking = app_guard
        .output_msgs
        .iter()
        .find(|message| message.id.as_deref() == Some("thinking-1"))
        .unwrap();
    assert!(!thinking.collapsed);
    assert_eq!(app_guard.status_notice.as_deref(), Some("Thinking expanded."));
}

#[test]
fn mouse_click_on_bottom_status_opens_delivery_detail() {
    let app = Arc::new(Mutex::new(App::new()));
    {
        let mut app_guard = app.lock().unwrap();
        app_guard.begin_turn();
        app_guard.remember_delivery_model_name("gpt-5.4");
        app_guard.set_status_slot(
            omega_session::StatusSlot::Agent,
            omega_session::StatusValue::Label("Idle".to_string()),
        );
        app_guard.bottom_status_rect = ratatui::layout::Rect::new(0, 20, 80, 1);
    }

    handle_mouse_event(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 3,
            row: 20,
            modifiers: KeyModifiers::NONE,
        },
        &app,
    );
    handle_mouse_event(
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 3,
            row: 20,
            modifiers: KeyModifiers::NONE,
        },
        &app,
    );

    let app_guard = app.lock().unwrap();
    assert_eq!(
        app_guard.status_notice.as_deref(),
        Some("Opened task delivery detail overlay.")
    );
    match app_guard.overlay.as_ref() {
        Some(crate::overlay::OverlayState::Detail(detail)) => {
            assert_eq!(detail.title, " Task Delivery ");
            assert!(detail.lines.iter().any(|line| line.contains("model: gpt-5.4")));
        }
        other => panic!("expected delivery detail overlay, got {other:?}"),
    }
}

#[test]
fn mouse_click_on_bottom_status_opens_project_detail_when_project_slot_present() {
    let client: DynLlmClient = Arc::new(IdleClient);
    let root = event_test_root("project-status-overlay");
    write_document_fixture(&root);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let session = test_session(client, root.clone(), &runtime);
    let app = Arc::new(Mutex::new(App::new()));
    {
        let mut app_guard = app.lock().unwrap();
        app_guard
            .set_status_slot(omega_session::StatusSlot::Project, session.project_status_value().unwrap());
        app_guard.bottom_status_rect = ratatui::layout::Rect::new(0, 20, 120, 1);
    }

    handle_mouse_event(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 3,
            row: 20,
            modifiers: KeyModifiers::NONE,
        },
        &app,
    );
    handle_mouse_event(
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 3,
            row: 20,
            modifiers: KeyModifiers::NONE,
        },
        &app,
    );

    let app_guard = app.lock().unwrap();
    assert_eq!(
        app_guard.status_notice.as_deref(),
        Some("Opened project detail overlay.")
    );
    match app_guard.overlay.as_ref() {
        Some(crate::overlay::OverlayState::Detail(detail)) => {
            assert_eq!(detail.title, " Project ");
            assert!(detail.lines.iter().any(|line| line.contains("project_id:")));
            assert!(detail.lines.iter().any(|line| line.contains("sessions:")));
            assert!(detail.lines.iter().any(|line| line.contains(&root.display().to_string())));
        }
        other => panic!("expected project detail overlay, got {other:?}"),
    }
    let snapshot = app_guard.project_status.as_ref().unwrap();
    assert!(matches!(
        snapshot.snapshot.record.detection_kind,
        ProjectDetectionKind::Cwd | ProjectDetectionKind::LooseDirectory
    ));
}

#[test]
fn mouse_click_on_sidebar_child_panel_selects_item() {
    let app = Arc::new(Mutex::new(App::new()));
    {
        let mut app_guard = app.lock().unwrap();
        app_guard.todo_rect = ratatui::layout::Rect::new(60, 1, 20, 8);
        app_guard.todo_lines = vec![
            "○ first item".to_string(),
            "→ active item".to_string(),
        ];
        app_guard.todo_displayed_count = 2;
    }

    handle_mouse_event(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 62,
            row: 3,
            modifiers: KeyModifiers::NONE,
        },
        &app,
    );
    handle_mouse_event(
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 62,
            row: 3,
            modifiers: KeyModifiers::NONE,
        },
        &app,
    );

    let app_guard = app.lock().unwrap();
    assert_eq!(app_guard.focused_panel, Panel::Todo);
    assert_eq!(app_guard.todo_state.selected(), Some(1));
    assert!(app_guard.todo_pinned);
}

#[test]
fn mouse_click_on_sidebar_child_panel_header_focuses_panel() {
    let app = Arc::new(Mutex::new(App::new()));
    {
        let mut app_guard = app.lock().unwrap();
        app_guard.todo_rect = ratatui::layout::Rect::new(60, 1, 20, 8);
        app_guard.todo_lines = vec![
            "○ first item".to_string(),
            "→ active item".to_string(),
        ];
        app_guard.todo_displayed_count = 2;
    }

    handle_mouse_event(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 62,
            row: 1,
            modifiers: KeyModifiers::NONE,
        },
        &app,
    );
    handle_mouse_event(
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 62,
            row: 1,
            modifiers: KeyModifiers::NONE,
        },
        &app,
    );

    let app_guard = app.lock().unwrap();
    assert_eq!(app_guard.focused_panel, Panel::Todo);
    assert_eq!(app_guard.todo_state.selected(), Some(0));
}

#[test]
fn mouse_click_on_project_sidebar_panel_focuses_project_panel() {
    let app = Arc::new(Mutex::new(App::new()));
    {
        let mut app_guard = app.lock().unwrap();
        app_guard.project_rect = ratatui::layout::Rect::new(60, 1, 24, 8);
        app_guard.project_lines = vec![
            "project: omega".to_string(),
            "active session: session-a".to_string(),
        ];
        app_guard.project_displayed_count = 2;
    }

    handle_mouse_event(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 62,
            row: 3,
            modifiers: KeyModifiers::NONE,
        },
        &app,
    );
    handle_mouse_event(
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 62,
            row: 3,
            modifiers: KeyModifiers::NONE,
        },
        &app,
    );

    let app_guard = app.lock().unwrap();
    assert_eq!(app_guard.focused_panel, Panel::Project);
    assert_eq!(app_guard.project_state.selected(), Some(1));
    assert!(app_guard.project_pinned);
}

#[test]
fn mouse_click_on_sidebar_more_lines_hint_focuses_panel() {
    let app = Arc::new(Mutex::new(App::new()));
    {
        let mut app_guard = app.lock().unwrap();
        app_guard.todo_rect = ratatui::layout::Rect::new(60, 1, 20, 10);
        app_guard.todo_lines = (0..10)
            .map(|index| format!("○ item {index}"))
            .collect();
        app_guard.todo_displayed_count = 7;
    }

    handle_mouse_event(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 62,
            row: 8,
            modifiers: KeyModifiers::NONE,
        },
        &app,
    );
    handle_mouse_event(
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 62,
            row: 8,
            modifiers: KeyModifiers::NONE,
        },
        &app,
    );

    let app_guard = app.lock().unwrap();
    assert_eq!(app_guard.focused_panel, Panel::Todo);
    assert!(app_guard.todo_pinned);
    assert_eq!(app_guard.todo_state.selected(), Some(6));
}

#[test]
fn rendered_sidebar_panel_click_focuses_real_layout_panel() {
    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    let theme = OmegaTheme::dark();
    let mut app_state = App::new();
    app_state.sidebar.delivery_expanded = true;
    app_state.delivery_lines = vec!["status: running".to_string(), "llm: 2".to_string()];

    terminal
        .draw(|frame| render(frame, &mut app_state, "test-model", &theme))
        .unwrap();

    let click_col = app_state.delivery_rect.x + 2;
    let click_row = app_state.delivery_rect.y + 2;
    let app = Arc::new(Mutex::new(app_state));

    handle_mouse_event(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: click_col,
            row: click_row,
            modifiers: KeyModifiers::NONE,
        },
        &app,
    );
    handle_mouse_event(
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: click_col,
            row: click_row,
            modifiers: KeyModifiers::NONE,
        },
        &app,
    );

    let app_guard = app.lock().unwrap();
    assert_eq!(app_guard.focused_panel, Panel::Delivery);
    assert_eq!(app_guard.delivery_state.selected(), Some(1));
}

#[test]
fn rendered_sidebar_clicks_can_focus_multiple_visible_panels() {
    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    let theme = OmegaTheme::dark();
    let mut app_state = App::new();
    app_state.sidebar.diagnostics_expanded = false;
    app_state.sidebar.delivery_expanded = true;
    app_state.sidebar.skills_expanded = false;
    app_state.sidebar.knowledge_expanded = true;
    app_state.sidebar.todos_expanded = true;
    app_state.sidebar.logs_expanded = true;
    app_state.delivery_lines = vec!["status: running".to_string()];
    app_state.document_lines = vec!["status: doc on".to_string()];
    app_state.todo_lines = vec!["○ item 0".to_string()];
    app_state.log_lines = vec!["[tool] cargo test".to_string()];

    terminal
        .draw(|frame| render(frame, &mut app_state, "test-model", &theme))
        .unwrap();

    let targets = [
        (Panel::Delivery, app_state.delivery_rect),
        (Panel::Document, app_state.document_rect),
        (Panel::Todo, app_state.todo_rect),
        (Panel::Logs, app_state.logs_rect),
    ];

    let app = Arc::new(Mutex::new(app_state));

    for (panel, rect) in targets {
        let click_col = rect.x + 2;
        let click_row = rect.y + 2.min(rect.height.saturating_sub(1));
        handle_mouse_event(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: click_col,
                row: click_row,
                modifiers: KeyModifiers::NONE,
            },
            &app,
        );
        handle_mouse_event(
            MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                column: click_col,
                row: click_row,
                modifiers: KeyModifiers::NONE,
            },
            &app,
        );

        let app_guard = app.lock().unwrap();
        assert_eq!(app_guard.focused_panel, panel);
        drop(app_guard);
    }
}

#[test]
fn response_panel_jk_and_enter_expand_selected_reasoning() {
    let harness = EventReplayHarness::new();
    {
        let mut app_guard = harness.app.lock().unwrap();
        seed_collapsed_reasoning(&mut app_guard);
        app_guard.focused_panel = Panel::Response;
        app_guard.interaction_mode = InteractionMode::Normal;
    }

    harness.replay_keys(&[
        (KeyCode::Char('j'), KeyModifiers::NONE),
        (KeyCode::Enter, KeyModifiers::NONE),
    ]);

    let app_guard = harness.app.lock().unwrap();
    let thinking = app_guard
        .output_msgs
        .iter()
        .find(|message| message.id.as_deref() == Some("thinking-1"))
        .unwrap();
    assert!(!thinking.collapsed);
    assert_eq!(app_guard.status_notice.as_deref(), Some("Thinking expanded."));
}

#[test]
fn copy_selected_text_uses_current_selection() {
    let mut app = App::new();
    app.logs_rect = ratatui::layout::Rect::new(0, 0, 7, 5);
    app.log_lines = vec!["abcdefg".to_string()];
    app.begin_mouse_selection(Panel::Logs, 2, 1);
    app.update_mouse_selection(3, 2);
    app.finish_mouse_selection(3, 2);

    let mut backend = Some(FakeClipboard::default());
    let copied =
        copy_selected_text_with_backend(&mut app, &mut backend, || Ok(FakeClipboard::default()))
            .unwrap();

    assert_eq!(copied, Some(6));
    assert_eq!(backend.unwrap().writes, vec!["bcdefg".to_string()]);
}

#[test]
fn ctrl_c_without_selection_does_not_trigger_copy_notice() {
    let client: DynLlmClient = Arc::new(IdleClient);
    let root = event_test_root("copy-quit-test");
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let session = test_session(client, root, &runtime);
    let app = Arc::new(Mutex::new(App::new()));
    let (tx, _rx) = mpsc::channel();
    let keymap = KeymapManager::default();

    let handled = handle_key_event(
        press_key(KeyCode::Char('c'), KeyModifiers::CONTROL),
        &app,
        &session,
        &tx,
        &keymap,
    )
    .unwrap();

    assert!(!handled);
    assert!(app.lock().unwrap().status_notice.is_none());
}

#[test]
fn tab_keeps_focus_on_response_when_sidebar_is_hidden() {
    let client: DynLlmClient = Arc::new(IdleClient);
    let root = event_test_root("tab-hidden-test");
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let session = test_session(client, root, &runtime);
    let app = Arc::new(Mutex::new(App::new()));
    let (tx, _rx) = mpsc::channel();
    let keymap = KeymapManager::default();

    let should_quit = handle_key_event(
        press_key(KeyCode::Char(' '), KeyModifiers::NONE),
        &app,
        &session,
        &tx,
        &keymap,
    )
    .unwrap();
    assert!(!should_quit);
    let should_quit = handle_key_event(
        press_key(KeyCode::Tab, KeyModifiers::NONE),
        &app,
        &session,
        &tx,
        &keymap,
    )
    .unwrap();

    assert!(!should_quit);
    assert_eq!(app.lock().unwrap().focused_panel, Panel::Response);
}

#[test]
fn raw_tab_does_not_change_focus_in_normal_mode() {
    let client: DynLlmClient = Arc::new(IdleClient);
    let root = event_test_root("raw-tab-test");
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let session = test_session(client, root, &runtime);
    let app = Arc::new(Mutex::new(App::new()));
    let (tx, _rx) = mpsc::channel();
    let keymap = KeymapManager::default();

    {
        let mut app_guard = app.lock().unwrap();
        app_guard.todo_rect = ratatui::layout::Rect::new(60, 1, 20, 8);
        app_guard.logs_rect = ratatui::layout::Rect::new(60, 9, 20, 8);
    }

    handle_key_event(
        press_key(KeyCode::Tab, KeyModifiers::NONE),
        &app,
        &session,
        &tx,
        &keymap,
    )
    .unwrap();

    assert_eq!(app.lock().unwrap().focused_panel, Panel::Response);
}

#[test]
fn leader_jk_toggles_into_insert_mode_and_allows_typing() {
    let client: DynLlmClient = Arc::new(IdleClient);
    let root = event_test_root("insert-mode-test");
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let session = test_session(client, root, &runtime);
    let app = Arc::new(Mutex::new(App::new()));
    let (tx, _rx) = mpsc::channel();
    let keymap = KeymapManager::default();

    handle_key_event(
        press_key(KeyCode::Char(' '), KeyModifiers::NONE),
        &app,
        &session,
        &tx,
        &keymap,
    )
    .unwrap();
    handle_key_event(
        press_key(KeyCode::Char('j'), KeyModifiers::NONE),
        &app,
        &session,
        &tx,
        &keymap,
    )
    .unwrap();
    handle_key_event(
        press_key(KeyCode::Char('k'), KeyModifiers::NONE),
        &app,
        &session,
        &tx,
        &keymap,
    )
    .unwrap();
    handle_key_event(
        press_key(KeyCode::Char('h'), KeyModifiers::NONE),
        &app,
        &session,
        &tx,
        &keymap,
    )
    .unwrap();

    let app_guard = app.lock().unwrap();
    assert_eq!(app_guard.interaction_mode, InteractionMode::Insert);
    assert_eq!(app_guard.input_buffer, "h");
}

#[test]
fn leader_jk_enters_insert_mode_while_turn_running() {
    let client: DynLlmClient = Arc::new(IdleClient);
    let root = event_test_root("insert-mode-running-test");
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let session = test_session(client, root, &runtime);
    let app = Arc::new(Mutex::new(App::new()));
    let (tx, _rx) = mpsc::channel();
    let keymap = KeymapManager::default();
    app.lock().unwrap().is_running = true;

    handle_key_event(
        press_key(KeyCode::Char(' '), KeyModifiers::NONE),
        &app,
        &session,
        &tx,
        &keymap,
    )
    .unwrap();
    handle_key_event(
        press_key(KeyCode::Char('j'), KeyModifiers::NONE),
        &app,
        &session,
        &tx,
        &keymap,
    )
    .unwrap();
    handle_key_event(
        press_key(KeyCode::Char('k'), KeyModifiers::NONE),
        &app,
        &session,
        &tx,
        &keymap,
    )
    .unwrap();

    let app_guard = app.lock().unwrap();
    assert_eq!(app_guard.interaction_mode, InteractionMode::Insert);
    assert!(app_guard.status_notice.as_deref().is_some_and(|notice| notice.contains("Mode: Insert")));
}

#[test]
fn plain_text_is_ignored_in_normal_mode() {
    let client: DynLlmClient = Arc::new(IdleClient);
    let root = event_test_root("normal-mode-test");
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let session = test_session(client, root, &runtime);
    let app = Arc::new(Mutex::new(App::new()));
    let (tx, _rx) = mpsc::channel();
    let keymap = KeymapManager::default();

    handle_key_event(
        press_key(KeyCode::Char('h'), KeyModifiers::NONE),
        &app,
        &session,
        &tx,
        &keymap,
    )
    .unwrap();

    assert!(app.lock().unwrap().input_buffer.is_empty());
}

#[test]
fn leader_jk_rejects_insert_mode_when_input_is_disabled() {
    let client: DynLlmClient = Arc::new(IdleClient);
    let root = event_test_root("insert-disabled-test");
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let session = test_session(client, root, &runtime);
    let app = Arc::new(Mutex::new(App::new()));
    let (tx, _rx) = mpsc::channel();
    let keymap = KeymapManager::default();
    app.lock().unwrap().input_enabled = false;

    handle_key_event(
        press_key(KeyCode::Char(' '), KeyModifiers::NONE),
        &app,
        &session,
        &tx,
        &keymap,
    )
    .unwrap();
    handle_key_event(
        press_key(KeyCode::Char('j'), KeyModifiers::NONE),
        &app,
        &session,
        &tx,
        &keymap,
    )
    .unwrap();
    handle_key_event(
        press_key(KeyCode::Char('k'), KeyModifiers::NONE),
        &app,
        &session,
        &tx,
        &keymap,
    )
    .unwrap();

    let app_guard = app.lock().unwrap();
    assert_eq!(app_guard.interaction_mode, InteractionMode::Normal);
    assert!(app_guard
        .status_notice
        .as_deref()
        .is_some_and(|notice| notice.contains("Insert mode")));
}

#[test]
fn esc_returns_insert_mode_to_normal() {
    let client: DynLlmClient = Arc::new(IdleClient);
    let root = event_test_root("toggle-normal-test");
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let session = test_session(client, root, &runtime);
    let app = Arc::new(Mutex::new(App::new()));
    let (tx, _rx) = mpsc::channel();
    let keymap = KeymapManager::default();
    app.lock().unwrap().interaction_mode = InteractionMode::Insert;

    handle_key_event(
        press_key(KeyCode::Esc, KeyModifiers::NONE),
        &app,
        &session,
        &tx,
        &keymap,
    )
    .unwrap();

    let app_guard = app.lock().unwrap();
    assert_eq!(app_guard.interaction_mode, InteractionMode::Normal);
    assert!(app_guard
        .status_notice
        .as_deref()
        .is_some_and(|notice| notice.contains("Mode: Normal")));
}

#[test]
fn esc_returns_insert_mode_to_normal_while_turn_running() {
    let client: DynLlmClient = Arc::new(IdleClient);
    let root = event_test_root("toggle-normal-running-test");
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let session = test_session(client, root, &runtime);
    let app = Arc::new(Mutex::new(App::new()));
    let (tx, _rx) = mpsc::channel();
    let keymap = KeymapManager::default();
    {
        let mut app_guard = app.lock().unwrap();
        app_guard.interaction_mode = InteractionMode::Insert;
        app_guard.is_running = true;
    }

    handle_key_event(
        press_key(KeyCode::Esc, KeyModifiers::NONE),
        &app,
        &session,
        &tx,
        &keymap,
    )
    .unwrap();

    let app_guard = app.lock().unwrap();
    assert_eq!(app_guard.interaction_mode, InteractionMode::Normal);
    assert!(app_guard
        .status_notice
        .as_deref()
        .is_some_and(|notice| notice.contains("Mode: Normal")));
}

#[test]
fn insert_mode_space_is_inserted_into_input() {
    let client: DynLlmClient = Arc::new(IdleClient);
    let root = event_test_root("insert-space-test");
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let session = test_session(client, root, &runtime);
    let app = Arc::new(Mutex::new(App::new()));
    let (tx, _rx) = mpsc::channel();
    let keymap = KeymapManager::default();
    app.lock().unwrap().interaction_mode = InteractionMode::Insert;

    handle_key_event(
        press_key(KeyCode::Char(' '), KeyModifiers::NONE),
        &app,
        &session,
        &tx,
        &keymap,
    )
    .unwrap();

    handle_key_event(
        press_key(KeyCode::Char('a'), KeyModifiers::NONE),
        &app,
        &session,
        &tx,
        &keymap,
    )
    .unwrap();

    let app_guard = app.lock().unwrap();
    assert_eq!(app_guard.input_buffer, " a");
    assert!(!app_guard.is_leader_pending());
}

#[test]
fn insert_mode_space_timeout_replays_pending_text() {
    let client: DynLlmClient = Arc::new(IdleClient);
    let root = event_test_root("insert-space-timeout-test");
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let session = test_session(client, root, &runtime);
    let app = Arc::new(Mutex::new(App::new()));
    let (tx, _rx) = mpsc::channel();
    let keymap = KeymapManager::default();
    app.lock().unwrap().interaction_mode = InteractionMode::Insert;

    handle_key_event(
        press_key(KeyCode::Char(' '), KeyModifiers::NONE),
        &app,
        &session,
        &tx,
        &keymap,
    )
    .unwrap();

    {
        let mut app_guard = app.lock().unwrap();
        let pending = app_guard.pending_key_sequence.as_mut().unwrap();
        pending.started_at -= pending.timeout + Duration::from_millis(1);
        let replay_text = app_guard.expire_pending_key_sequence();
        assert_eq!(replay_text.as_deref(), Some(" "));
        app_guard.insert_text(replay_text.as_deref().unwrap());
    }

    let app_guard = app.lock().unwrap();
    assert_eq!(app_guard.input_buffer, " ");
    assert!(!app_guard.is_leader_pending());
}

#[test]
fn esc_cancels_pending_leader_sequence() {
    let client: DynLlmClient = Arc::new(IdleClient);
    let root = event_test_root("leader-cancel-test");
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let session = test_session(client, root, &runtime);
    let app = Arc::new(Mutex::new(App::new()));
    let (tx, _rx) = mpsc::channel();
    let keymap = KeymapManager::default();

    handle_key_event(
        press_key(KeyCode::Char(' '), KeyModifiers::NONE),
        &app,
        &session,
        &tx,
        &keymap,
    )
    .unwrap();
    handle_key_event(
        press_key(KeyCode::Esc, KeyModifiers::NONE),
        &app,
        &session,
        &tx,
        &keymap,
    )
    .unwrap();

    let app_guard = app.lock().unwrap();
    assert!(!app_guard.is_leader_pending());
    assert!(app_guard
        .status_notice
        .as_deref()
        .is_some_and(|notice| notice.contains("cancelled")));
}

#[test]
fn panel_search_overlay_captures_text_without_touching_main_input() {
    let client: DynLlmClient = Arc::new(IdleClient);
    let root = event_test_root("overlay-search-test");
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let session = test_session(client, root, &runtime);
    let app = Arc::new(Mutex::new(App::new()));
    let (tx, _rx) = mpsc::channel();
    let keymap = KeymapManager::default();

    handle_key_event(
        press_key(KeyCode::Char(' '), KeyModifiers::NONE),
        &app,
        &session,
        &tx,
        &keymap,
    )
    .unwrap();
    handle_key_event(
        press_key(KeyCode::Char('/'), KeyModifiers::NONE),
        &app,
        &session,
        &tx,
        &keymap,
    )
    .unwrap();
    handle_key_event(
        press_key(KeyCode::Char('a'), KeyModifiers::NONE),
        &app,
        &session,
        &tx,
        &keymap,
    )
    .unwrap();

    let app_guard = app.lock().unwrap();
    let query = match app_guard.overlay.as_ref() {
        Some(OverlayState::Search(overlay)) => overlay.query.as_str(),
        _ => "",
    };
    assert_eq!(query, "a");
    assert!(app_guard.input_buffer.is_empty());
}

#[test]
fn overlay_esc_restores_previous_focus() {
    let client: DynLlmClient = Arc::new(IdleClient);
    let root = event_test_root("overlay-escape-test");
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let session = test_session(client, root, &runtime);
    let app = Arc::new(Mutex::new(App::new()));
    let (tx, _rx) = mpsc::channel();
    let keymap = KeymapManager::default();
    {
        let mut app_guard = app.lock().unwrap();
        app_guard.focused_panel = Panel::Logs;
        app_guard.logs_rect = ratatui::layout::Rect::new(60, 8, 20, 8);
    }

    handle_key_event(
        press_key(KeyCode::Char(' '), KeyModifiers::NONE),
        &app,
        &session,
        &tx,
        &keymap,
    )
    .unwrap();
    handle_key_event(
        press_key(KeyCode::Char('/'), KeyModifiers::NONE),
        &app,
        &session,
        &tx,
        &keymap,
    )
    .unwrap();
    handle_key_event(
        press_key(KeyCode::Esc, KeyModifiers::NONE),
        &app,
        &session,
        &tx,
        &keymap,
    )
    .unwrap();

    let app_guard = app.lock().unwrap();
    assert!(!app_guard.overlay_active());
    assert_eq!(app_guard.focused_panel, Panel::Logs);
}

#[test]
fn detail_overlay_supports_page_and_edge_navigation() {
    let client: DynLlmClient = Arc::new(IdleClient);
    let root = event_test_root("detail-overlay-scroll-test");
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let session = test_session(client, root, &runtime);
    let app = Arc::new(Mutex::new(App::new()));
    let (tx, _rx) = mpsc::channel();
    let keymap = KeymapManager::default();
    {
        let mut app_guard = app.lock().unwrap();
        app_guard.focused_panel = Panel::Logs;
        app_guard.overlay_rect = ratatui::layout::Rect::new(0, 0, 80, 16);
        app_guard.open_detail_overlay(
            " Detail ",
            (0..30).map(|index| format!("line {index}")).collect(),
        );
    }

    handle_key_event(
        press_key(KeyCode::PageDown, KeyModifiers::NONE),
        &app,
        &session,
        &tx,
        &keymap,
    )
    .unwrap();

    {
        let app_guard = app.lock().unwrap();
        match app_guard.overlay.as_ref() {
            Some(OverlayState::Detail(detail)) => assert_eq!(detail.scroll, 11),
            other => panic!("expected detail overlay, got {other:?}"),
        }
    }

    handle_key_event(
        press_key(KeyCode::End, KeyModifiers::NONE),
        &app,
        &session,
        &tx,
        &keymap,
    )
    .unwrap();
    handle_key_event(
        press_key(KeyCode::Home, KeyModifiers::NONE),
        &app,
        &session,
        &tx,
        &keymap,
    )
    .unwrap();

    let app_guard = app.lock().unwrap();
    match app_guard.overlay.as_ref() {
        Some(OverlayState::Detail(detail)) => assert_eq!(detail.scroll, 0),
        other => panic!("expected detail overlay, got {other:?}"),
    }
}

#[test]
fn mouse_wheel_scrolls_detail_overlay_without_touching_background_panel() {
    let app = Arc::new(Mutex::new(App::new()));
    {
        let mut app_guard = app.lock().unwrap();
        app_guard.focused_panel = Panel::Logs;
        app_guard.logs_rect = ratatui::layout::Rect::new(60, 8, 20, 8);
        app_guard.logs_state.select(Some(4));
        app_guard.overlay_rect = ratatui::layout::Rect::new(10, 4, 60, 16);
        app_guard.open_detail_overlay(
            " Detail ",
            (0..30).map(|index| format!("line {index}")).collect(),
        );
    }

    handle_mouse_event(
        MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 20,
            row: 10,
            modifiers: KeyModifiers::NONE,
        },
        &app,
    );

    let app_guard = app.lock().unwrap();
    match app_guard.overlay.as_ref() {
        Some(OverlayState::Detail(detail)) => assert_eq!(detail.scroll, 3),
        other => panic!("expected detail overlay, got {other:?}"),
    }
    assert_eq!(app_guard.logs_state.selected(), Some(4));
}

#[test]
fn overlay_blocks_background_tab_focus_changes() {
    let client: DynLlmClient = Arc::new(IdleClient);
    let root = event_test_root("overlay-block-focus-test");
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let session = test_session(client, root, &runtime);
    let app = Arc::new(Mutex::new(App::new()));
    let (tx, _rx) = mpsc::channel();
    let keymap = KeymapManager::default();
    {
        let mut app_guard = app.lock().unwrap();
        app_guard.todo_rect = ratatui::layout::Rect::new(60, 1, 20, 8);
        app_guard.logs_rect = ratatui::layout::Rect::new(60, 9, 20, 8);
    }

    handle_key_event(
        press_key(KeyCode::Char(' '), KeyModifiers::NONE),
        &app,
        &session,
        &tx,
        &keymap,
    )
    .unwrap();
    handle_key_event(
        press_key(KeyCode::Char('/'), KeyModifiers::NONE),
        &app,
        &session,
        &tx,
        &keymap,
    )
    .unwrap();
    handle_key_event(
        press_key(KeyCode::Tab, KeyModifiers::NONE),
        &app,
        &session,
        &tx,
        &keymap,
    )
    .unwrap();

    let app_guard = app.lock().unwrap();
    assert_eq!(app_guard.focused_panel, Panel::Response);
    assert!(app_guard.overlay_active());
}

#[test]
fn leader_b_toggles_sidebar_shell() {
    let client: DynLlmClient = Arc::new(IdleClient);
    let root = event_test_root("toggle-sidebar-test");
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let session = test_session(client, root, &runtime);
    let app = Arc::new(Mutex::new(App::new()));
    let (tx, _rx) = mpsc::channel();
    let keymap = KeymapManager::default();

    handle_key_event(
        press_key(KeyCode::Char(' '), KeyModifiers::NONE),
        &app,
        &session,
        &tx,
        &keymap,
    )
    .unwrap();
    handle_key_event(
        press_key(KeyCode::Char('b'), KeyModifiers::NONE),
        &app,
        &session,
        &tx,
        &keymap,
    )
    .unwrap();

    let app_guard = app.lock().unwrap();
    assert!(app_guard.sidebar.shell_collapsed);
    assert_eq!(app_guard.focused_panel, Panel::Response);
}

#[test]
fn sidebar_rail_cycles_and_toggles_selected_section() {
    let client: DynLlmClient = Arc::new(IdleClient);
    let root = event_test_root("sidebar-rail-test");
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let session = test_session(client, root, &runtime);
    let app = Arc::new(Mutex::new(App::new()));
    let (tx, _rx) = mpsc::channel();
    let keymap = KeymapManager::default();
    {
        let mut app_guard = app.lock().unwrap();
        app_guard.sidebar_rect = ratatui::layout::Rect::new(60, 1, 20, 18);
        app_guard.sidebar_rail_rect = ratatui::layout::Rect::new(61, 2, 18, 3);
        app_guard.todo_rect = ratatui::layout::Rect::new(61, 5, 18, 6);
        app_guard.logs_rect = ratatui::layout::Rect::new(61, 11, 18, 7);
        app_guard.focused_panel = Panel::SidebarRail;
        app_guard.sidebar.delivery_expanded = true;
    }

    handle_key_event(
        press_key(KeyCode::Right, KeyModifiers::NONE),
        &app,
        &session,
        &tx,
        &keymap,
    )
    .unwrap();
    handle_key_event(
        press_key(KeyCode::Char('x'), KeyModifiers::NONE),
        &app,
        &session,
        &tx,
        &keymap,
    )
    .unwrap();

    let app_guard = app.lock().unwrap();
    assert_eq!(
        app_guard.sidebar.rail_selection,
        crate::sidebar::SidebarSection::Delivery
    );
    assert!(!app_guard.sidebar.delivery_expanded);
    assert_eq!(app_guard.focused_panel, Panel::SidebarRail);
}

#[test]
fn sidebar_rail_enter_focuses_collapsed_delivery_with_selection() {
    let client: DynLlmClient = Arc::new(IdleClient);
    let root = event_test_root("sidebar-rail-enter-test");
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let session = test_session(client, root, &runtime);
    let app = Arc::new(Mutex::new(App::new()));
    let (tx, _rx) = mpsc::channel();
    let keymap = KeymapManager::default();
    {
        let mut app_guard = app.lock().unwrap();
        app_guard.sidebar_rect = ratatui::layout::Rect::new(60, 1, 20, 18);
        app_guard.sidebar_rail_rect = ratatui::layout::Rect::new(61, 2, 18, 3);
        app_guard.focused_panel = Panel::SidebarRail;
        app_guard.sidebar.rail_selection = crate::sidebar::SidebarSection::Delivery;
        app_guard.sidebar.delivery_expanded = false;
        app_guard.delivery_lines = vec![
            "status: running".to_string(),
            "activity: 1 llm / 2 tools".to_string(),
        ];
    }

    handle_key_event(
        press_key(KeyCode::Enter, KeyModifiers::NONE),
        &app,
        &session,
        &tx,
        &keymap,
    )
    .unwrap();

    let app_guard = app.lock().unwrap();
    assert!(app_guard.sidebar.delivery_expanded);
    assert_eq!(app_guard.focused_panel, Panel::Delivery);
    assert_eq!(app_guard.delivery_state.selected(), Some(0));
}

#[test]
fn replay_harness_drives_overlay_search_sequence() {
    let harness = EventReplayHarness::new();

    harness.replay_keys(&[
        (KeyCode::Char(' '), KeyModifiers::NONE),
        (KeyCode::Char('/'), KeyModifiers::NONE),
        (KeyCode::Char('a'), KeyModifiers::NONE),
        (KeyCode::Esc, KeyModifiers::NONE),
    ]);

    let (overlay_active, focused_panel, input_buffer) = harness.inspect(|app| {
        (
            app.overlay_active(),
            app.focused_panel,
            app.input_buffer.clone(),
        )
    });

    assert!(!overlay_active);
    assert_eq!(focused_panel, Panel::Response);
    assert!(input_buffer.is_empty());
}

#[test]
fn picker_enter_opens_selected_item_detail_overlay() {
    let harness = EventReplayHarness::new();
    {
        let mut app_guard = harness.app.lock().unwrap();
        app_guard.focused_panel = Panel::Logs;
        app_guard.open_picker_overlay(sample_operator_picker_request());
    }

    harness.replay_keys(&[(KeyCode::Enter, KeyModifiers::NONE)]);

    let app_guard = harness.app.lock().unwrap();
    match app_guard.overlay.as_ref() {
        Some(OverlayState::Detail(detail)) => {
            assert!(detail.title.contains("Session Alpha"));
            assert!(detail.lines.iter().any(|line| line.contains("session-alpha")));
        }
        other => panic!("expected detail overlay, got {other:?}"),
    }
}

#[test]
fn picker_filter_reduces_visible_items() {
    let harness = EventReplayHarness::new();
    {
        let mut app_guard = harness.app.lock().unwrap();
        app_guard.open_picker_overlay(sample_operator_picker_request());
    }

    harness.replay_keys(&[
        (KeyCode::Char('/'), KeyModifiers::NONE),
        (KeyCode::Char('b'), KeyModifiers::NONE),
        (KeyCode::Char('e'), KeyModifiers::NONE),
    ]);

    let app_guard = harness.app.lock().unwrap();
    match app_guard.overlay.as_ref() {
        Some(OverlayState::Picker(picker)) => {
            assert!(picker.filter_mode);
            assert_eq!(picker.filter_query, "be");
            assert_eq!(picker.visible_items_len(), 1);
            assert_eq!(picker.selected_item().map(|item| item.id.as_str()), Some("session-beta"));
        }
        other => panic!("expected picker overlay, got {other:?}"),
    }
}

#[test]
fn document_navigator_keys_switch_active_entry_and_scroll_content() {
    let harness = EventReplayHarness::new();
    {
        let mut app_guard = harness.app.lock().unwrap();
        app_guard.open_document_navigator_overlay(sample_document_navigator_request());
    }

    harness.replay_keys(&[
        (KeyCode::Down, KeyModifiers::NONE),
        (KeyCode::Enter, KeyModifiers::NONE),
        (KeyCode::Tab, KeyModifiers::NONE),
        (KeyCode::Down, KeyModifiers::NONE),
    ]);

    let app_guard = harness.app.lock().unwrap();
    match app_guard.overlay.as_ref() {
        Some(OverlayState::DocumentNavigator(overlay)) => {
            assert_eq!(overlay.request.active_entry_id, "src/navigator.rs");
            assert_eq!(overlay.focus, crate::overlay::DocumentNavigatorFocus::Content);
            assert_eq!(overlay.content_scroll, 1);
            assert_eq!(
                overlay.history_entry_ids,
                vec!["docs/specs/navigator.md".to_string()]
            );
        }
        other => panic!("expected document navigator overlay, got {other:?}"),
    }
}

#[test]
fn picker_ctrl_shortcut_submits_slash_command_template() {
    let client: DynLlmClient = Arc::new(IdleClient);
    let root = event_test_root("picker-ctrl-shortcut");
    write_document_fixture(&root);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let session = test_session(client, root, &runtime);
    let app = Arc::new(Mutex::new(App::new()));
    let (tx, rx) = mpsc::channel();
    let keymap = KeymapManager::default();
    {
        let mut app_guard = app.lock().unwrap();
        app_guard.open_picker_overlay(sample_operator_picker_request());
    }

    handle_key_event(
        press_key(KeyCode::Char('r'), KeyModifiers::CONTROL),
        &app,
        &session,
        &tx,
        &keymap,
    )
    .unwrap();

    let mut saw_command_section = false;
    loop {
        let envelope = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        if matches!(
            &envelope.message,
            RuntimeMessage::Conversation(ConversationMessage::BeginSection { section })
                if section.kind == ResponseSectionKind::Command
        ) {
            saw_command_section = true;
        }
        if matches!(
            &envelope.message,
            RuntimeMessage::State(StateMessage::TurnFinished)
        ) {
            break;
        }
    }

    assert!(saw_command_section);
    let app_guard = app.lock().unwrap();
    assert!(app_guard.overlay.is_none());
    assert!(app_guard
        .output_msgs
        .iter()
        .any(|message| message.text.contains("> /document health")));
}

#[test]
fn picker_enter_submits_plan_view_file_and_opens_document_navigator_overlay() {
    let client: DynLlmClient = Arc::new(IdleClient);
    let root = event_test_root("picker-plan-view-file");
    write_document_fixture(&root);
    std::fs::write(
        root.join("docs/specs/navigator.md"),
        "---\nstatus: draft\n---\n\n# Navigator\n\nUseful content.\n",
    )
    .unwrap();

    let runtime = tokio::runtime::Runtime::new().unwrap();
    let session = test_session(client, root, &runtime);
    let app = Arc::new(Mutex::new(App::new()));
    let (tx, rx) = mpsc::channel();
    let keymap = KeymapManager::default();
    {
        let mut app_guard = app.lock().unwrap();
        app_guard.open_picker_overlay(sample_plan_view_file_picker_request());
    }

    handle_key_event(
        press_key(KeyCode::Enter, KeyModifiers::NONE),
        &app,
        &session,
        &tx,
        &keymap,
    )
    .unwrap();

    apply_runtime_overlays_until_turn_finished(&app, &rx);

    let app_guard = app.lock().unwrap();
    match app_guard.overlay.as_ref() {
        Some(OverlayState::DocumentNavigator(overlay)) => {
            assert_eq!(overlay.request.navigator_id, "plan-view-file:docs/specs/navigator.md");
            assert_eq!(overlay.request.active_entry_id, "docs/specs/navigator.md");
            assert!(overlay
                .request
                .entries
                .iter()
                .any(|entry| entry.id == "docs/specs/navigator.md"));
        }
        other => panic!("expected document navigator overlay, got {other:?}"),
    }
}

#[test]
fn picker_confirm_shortcut_opens_confirm_overlay_and_runs_command_after_confirmation() {
    let client: DynLlmClient = Arc::new(IdleClient);
    let root = event_test_root("picker-confirm-shortcut");
    write_document_fixture(&root);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let session = test_session(client, root, &runtime);
    let app = Arc::new(Mutex::new(App::new()));
    let (tx, rx) = mpsc::channel();
    let keymap = KeymapManager::default();
    {
        let mut app_guard = app.lock().unwrap();
        app_guard.open_picker_overlay(sample_confirm_picker_request());
    }

    handle_key_event(
        press_key(KeyCode::Char('d'), KeyModifiers::CONTROL),
        &app,
        &session,
        &tx,
        &keymap,
    )
    .unwrap();

    {
        let app_guard = app.lock().unwrap();
        match app_guard.overlay.as_ref() {
            Some(OverlayState::Confirm(confirm)) => {
                assert!(confirm.message.contains("Session Alpha"));
                assert_eq!(confirm.confirm_label, "Delete");
            }
            other => panic!("expected confirm overlay, got {other:?}"),
        }
    }

    handle_key_event(
        press_key(KeyCode::Char('y'), KeyModifiers::NONE),
        &app,
        &session,
        &tx,
        &keymap,
    )
    .unwrap();
    handle_key_event(
        press_key(KeyCode::Enter, KeyModifiers::NONE),
        &app,
        &session,
        &tx,
        &keymap,
    )
    .unwrap();

    loop {
        let envelope = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        if matches!(&envelope.message, RuntimeMessage::State(StateMessage::TurnFinished)) {
            break;
        }
    }

    let app_guard = app.lock().unwrap();
    assert!(app_guard.overlay.is_none());
    assert!(app_guard
        .output_msgs
        .iter()
        .any(|message| message.text.contains("> /session delete session-alpha --picker")));
}

fn sample_operator_picker_request() -> OperatorPickerRequest {
    OperatorPickerRequest {
        picker_id: "sessions".to_string(),
        title: " Sessions ".to_string(),
        empty_state: "No sessions found.".to_string(),
        filter_enabled: true,
        items: vec![
            OperatorPickerItem {
                id: "session-alpha".to_string(),
                title: "Session Alpha".to_string(),
                subtitle: Some("resume-ready".to_string()),
                badges: vec!["current".to_string()],
                preview: Some("Last turn: inspect runtime contract".to_string()),
                disabled_reason: None,
            },
            OperatorPickerItem {
                id: "session-beta".to_string(),
                title: "Session Beta".to_string(),
                subtitle: Some("archived".to_string()),
                badges: vec!["resume-ready".to_string()],
                preview: Some("Last turn: validate background worker".to_string()),
                disabled_reason: None,
            },
        ],
        primary_action: OperatorPickerAction {
            action_id: "detail".to_string(),
            label: "Detail".to_string(),
            shortcut: OperatorPickerShortcut::Enter,
            requires_selection: true,
            overlay_behavior: OperatorPickerOverlayBehavior::KeepOpen,
            intent: OperatorPickerIntent::OpenDetail,
        },
        secondary_actions: vec![OperatorPickerAction {
            action_id: "resume".to_string(),
            label: "Resume".to_string(),
            shortcut: OperatorPickerShortcut::Ctrl('r'),
            requires_selection: true,
            overlay_behavior: OperatorPickerOverlayBehavior::CloseOverlay,
            intent: OperatorPickerIntent::SubmitSlashCommand {
                command_template: "/document health".to_string(),
            },
        }],
    }
}

fn sample_plan_view_file_picker_request() -> OperatorPickerRequest {
    OperatorPickerRequest {
        picker_id: "plan-view-file".to_string(),
        title: " Plan Files ".to_string(),
        empty_state: "No files found.".to_string(),
        filter_enabled: true,
        items: vec![OperatorPickerItem {
            id: "docs/specs/navigator.md".to_string(),
            title: "navigator.md".to_string(),
            subtitle: Some("docs/specs/navigator.md".to_string()),
            badges: vec!["spec".to_string()],
            preview: Some("Navigator".to_string()),
            disabled_reason: None,
        }],
        primary_action: OperatorPickerAction {
            action_id: "view-file".to_string(),
            label: "Open".to_string(),
            shortcut: OperatorPickerShortcut::Enter,
            requires_selection: true,
            overlay_behavior: OperatorPickerOverlayBehavior::CloseOverlay,
            intent: OperatorPickerIntent::SubmitSlashCommand {
                command_template: "/plan view-file {id}".to_string(),
            },
        },
        secondary_actions: vec![],
    }
}

fn sample_document_navigator_request() -> DocumentNavigatorRequest {
    DocumentNavigatorRequest {
        navigator_id: "plan-links:TASK-0001".to_string(),
        title: " TASK-0001: Build navigator ".to_string(),
        origin_label: "Task TASK-0001 linked artifacts".to_string(),
        active_entry_id: "docs/specs/navigator.md".to_string(),
        entries: vec![
            DocumentNavigatorEntry {
                id: "docs/specs/navigator.md".to_string(),
                label: "Navigator Spec".to_string(),
                subtitle: Some("Design · docs/specs/navigator.md".to_string()),
                preview: Some("Shared overlay structure".to_string()),
                group: DocumentNavigatorGroup::Context,
                kind: DocumentNavigatorEntryKind::Document,
                disabled_reason: None,
                body: DocumentNavigatorBody {
                    title: "Navigator Spec".to_string(),
                    subtitle: Some("docs/specs/navigator.md".to_string()),
                    breadcrumbs: vec!["TASK-0001".to_string(), "Design".to_string()],
                    kind: DocumentNavigatorBodyKind::Markdown,
                    lines: vec![
                        "File: docs/specs/navigator.md".to_string(),
                        String::new(),
                        "Line 1".to_string(),
                        "Line 2".to_string(),
                        "Line 3".to_string(),
                    ],
                },
            },
            DocumentNavigatorEntry {
                id: "src/navigator.rs".to_string(),
                label: "navigator.rs".to_string(),
                subtitle: Some("Implementation · src/navigator.rs".to_string()),
                preview: Some("pub fn navigator_overlay()".to_string()),
                group: DocumentNavigatorGroup::Context,
                kind: DocumentNavigatorEntryKind::File,
                disabled_reason: None,
                body: DocumentNavigatorBody {
                    title: "navigator.rs".to_string(),
                    subtitle: Some("src/navigator.rs".to_string()),
                    breadcrumbs: vec!["TASK-0001".to_string(), "Implementation".to_string()],
                    kind: DocumentNavigatorBodyKind::File,
                    lines: vec![
                        "File: src/navigator.rs".to_string(),
                        String::new(),
                        "pub fn navigator_overlay() {}".to_string(),
                        "pub fn open_related() {}".to_string(),
                    ],
                },
            },
            DocumentNavigatorEntry {
                id: "docs/guide/linked-navigation.md".to_string(),
                label: "Linked Navigation Guide".to_string(),
                subtitle: Some("references · docs/guide/linked-navigation.md".to_string()),
                preview: Some("Related navigation guidance".to_string()),
                group: DocumentNavigatorGroup::Related,
                kind: DocumentNavigatorEntryKind::Document,
                disabled_reason: None,
                body: DocumentNavigatorBody {
                    title: "Linked Navigation Guide".to_string(),
                    subtitle: Some("docs/guide/linked-navigation.md".to_string()),
                    breadcrumbs: vec![
                        "Navigator Spec".to_string(),
                        "Related via references".to_string(),
                    ],
                    kind: DocumentNavigatorBodyKind::Markdown,
                    lines: vec!["Guide line 1".to_string()],
                },
            },
        ],
    }
}

fn sample_confirm_picker_request() -> OperatorPickerRequest {
    let mut request = sample_operator_picker_request();
    request.secondary_actions.push(OperatorPickerAction {
        action_id: "delete".to_string(),
        label: "Delete".to_string(),
        shortcut: OperatorPickerShortcut::Ctrl('d'),
        requires_selection: true,
        overlay_behavior: OperatorPickerOverlayBehavior::KeepOpen,
        intent: OperatorPickerIntent::RequestConfirmSlashCommand {
            title_template: " Confirm session delete ".to_string(),
            message_template: "Delete session {title} ({id})?".to_string(),
            confirm_label: "Delete".to_string(),
            command_template: "/session delete {id} --picker".to_string(),
        },
    });
    request
}
