use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use omega_client::test_support::IdleLlmClient;
use omega_core::DynLlmClient;
use omega_keymap::{InteractionMode, KeymapManager};
use omega_session::{ResponseSectionState, WorkflowRunRole};
use omega_test_support::persistent_test_root;
use omega_workflow::LoadedWorkflowCatalog;

use crate::app::{Msg, Panel};

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
fn leader_jk_toggles_back_to_normal_mode() {
    let client: DynLlmClient = Arc::new(IdleClient);
    let root = event_test_root("toggle-normal-test");
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
        .is_some_and(|notice| notice.contains("Mode: Normal")));
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
        crate::sidebar::SidebarSection::Todos
    );
    assert!(!app_guard.sidebar.todos_expanded);
    assert_eq!(app_guard.focused_panel, Panel::SidebarRail);
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
