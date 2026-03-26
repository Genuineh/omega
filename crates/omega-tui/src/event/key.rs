use super::clipboard::{copy_selected_text, is_copy_shortcut};
use super::overlay_handlers::handle_overlay_key_event;
use super::*;

pub(super) fn handle_key_event(
    key: KeyEvent,
    app: &Arc<Mutex<App>>,
    session: &AgentSession,
    tx: &mpsc::Sender<RuntimeMessageEnvelope>,
    keymap: &KeymapManager,
) -> anyhow::Result<bool> {
    if app.lock().unwrap().overlay_active() {
        return handle_overlay_key_event(key, app, session);
    }

    {
        let mut app_guard = app.lock().unwrap();
        if app_guard.interaction_mode == InteractionMode::Normal && is_copy_shortcut(key) {
            if let Some(count) = copy_selected_text(&mut app_guard).map_err(anyhow::Error::msg)? {
                app_guard.set_status_notice(format!("Copied {} chars.", count));
                return Ok(false);
            }
        }
    }

    {
        let mut app_guard = app.lock().unwrap();
        if app_guard.interaction_mode == InteractionMode::Normal
            && app_guard.focused_panel == Panel::SidebarRail
        {
            match key.code {
                KeyCode::Left => {
                    app_guard.cycle_sidebar_rail_previous();
                    return Ok(false);
                }
                KeyCode::Right => {
                    app_guard.cycle_sidebar_rail_next();
                    return Ok(false);
                }
                KeyCode::Enter => {
                    app_guard.activate_sidebar_selection();
                    return Ok(false);
                }
                KeyCode::Char('x') => {
                    app_guard.toggle_selected_sidebar_section();
                    return Ok(false);
                }
                _ => {}
            }
        }
    }

    {
        let mut app_guard = app.lock().unwrap();
        if app_guard.interaction_mode == InteractionMode::Normal
            && app_guard.focused_panel == Panel::Response
        {
            match key.code {
                KeyCode::Enter | KeyCode::Char('x') => {
                    let notice = match app_guard.activate_selected_response_item() {
                        Some(ResponseActivation::ThinkingCollapsed) => "Thinking collapsed.",
                        Some(ResponseActivation::ThinkingExpanded) => "Thinking expanded.",
                        Some(ResponseActivation::ToolDetailOpened(tool_name)) => {
                            app_guard
                                .set_status_notice(format!("Opened {tool_name} detail overlay."));
                            return Ok(false);
                        }
                        None if app_guard.show_thinking => {
                            "Select a thinking block or tool summary before activating it."
                        }
                        None => "Thinking visibility is disabled in .omega/tui.toml.",
                    };
                    app_guard.set_status_notice(notice);
                    return Ok(false);
                }
                _ => {}
            }
        }
    }

    {
        let mut app_guard = app.lock().unwrap();
        if app_guard.interaction_mode == InteractionMode::Normal
            && app_guard.focused_panel == Panel::Diagnostics
        {
            match key.code {
                KeyCode::Enter | KeyCode::Char('x') => {
                    let notice = match app_guard.activate_selected_diagnostics_item() {
                        Some(step_label) => {
                            app_guard.set_status_notice(format!(
                                "Opened diagnostics overlay for {step_label}."
                            ));
                            return Ok(false);
                        }
                        None => "Select a diagnostics entry before activating it.",
                    };
                    app_guard.set_status_notice(notice);
                    return Ok(false);
                }
                _ => {}
            }
        }
    }

    let resolution = {
        let app_guard = app.lock().unwrap();
        let context = KeyContext {
            mode: app_guard.interaction_mode,
            focus: app_guard.key_focus(),
            input_capable: app_guard.input_capable(),
            leader_pending: app_guard.is_leader_pending(),
        };
        keymap.resolve_with_pending(&context, app_guard.pending_key_events(), key)
    };

    match resolution {
        KeyResolution::PendingLeader => {
            let mut app_guard = app.lock().unwrap();
            app_guard.begin_leader_pending(key);
            Ok(false)
        }
        KeyResolution::PendingSequence => {
            let mut app_guard = app.lock().unwrap();
            app_guard.extend_pending_sequence(key);
            Ok(false)
        }
        KeyResolution::Matched(action) => {
            {
                let mut app_guard = app.lock().unwrap();
                app_guard.clear_leader_pending();
                app_guard.clear_status_notice();
            }
            execute_action(action, app, session, tx)
        }
        KeyResolution::InvalidInContext(action) => {
            let mut app_guard = app.lock().unwrap();
            app_guard.clear_leader_pending();
            if action == KeyAction::EnterInsertMode {
                app_guard.set_status_notice("Insert mode is unavailable in the current context.");
            } else {
                app_guard.set_status_notice(format!(
                    "Action '{}' is unavailable in the current mode or focus.",
                    action.as_str()
                ));
            }
            Ok(false)
        }
        KeyResolution::NoMatch => handle_unmatched_key(key, app),
    }
}

pub(super) fn handle_submit(
    app: &Arc<Mutex<App>>,
    session: &AgentSession,
    tx: &mpsc::Sender<RuntimeMessageEnvelope>,
) -> anyhow::Result<bool> {
    let agent_ready = session.is_ready();
    let still_running = app.lock().unwrap().is_running;
    if !agent_ready || still_running {
        app.lock().unwrap().push_msg(
            MsgKind::Error,
            "⚠ Previous turn still finishing — please wait…",
        );
        return Ok(false);
    }

    let input = {
        let mut app_guard = app.lock().unwrap();
        app_guard.take_input()
    };

    if input == "q" || input == "exit" {
        info!("user exit");
        return Ok(true);
    }

    if input.is_empty() {
        return Ok(false);
    }

    session.checkpoint_current_messages();
    let turn_id = {
        let mut app_guard = app.lock().unwrap();
        if !app_guard.output_msgs.is_empty() {
            app_guard.push_msg(MsgKind::Separator, &"─".repeat(40));
        }
        app_guard.push_msg(MsgKind::User, &format!("> {}", input));
        app_guard.begin_turn()
    };

    if let Err(error) = session.spawn_turn(input, turn_id, tx.clone()) {
        let mut app_guard = app.lock().unwrap();
        app_guard.is_running = false;
        app_guard.push_msg(MsgKind::Error, &format!("Error: {error}"));
    }

    Ok(false)
}

fn handle_unmatched_key(key: KeyEvent, app: &Arc<Mutex<App>>) -> anyhow::Result<bool> {
    let mut app_guard = app.lock().unwrap();
    if app_guard.is_leader_pending() {
        if app_guard.interaction_mode == InteractionMode::Normal {
            match key.code {
                KeyCode::Char('/') => {
                    app_guard.clear_leader_pending();
                    app_guard.open_search_overlay();
                    return Ok(false);
                }
                KeyCode::Char('b') => {
                    app_guard.clear_leader_pending();
                    app_guard.toggle_sidebar_shell();
                    let notice = if app_guard.sidebar.shell_collapsed {
                        "Sidebar collapsed."
                    } else {
                        "Sidebar expanded."
                    };
                    app_guard.set_status_notice(notice);
                    return Ok(false);
                }
                _ => {}
            }
        }

        app_guard.clear_leader_pending();
        app_guard.set_status_notice("No mapping for that leader sequence.");
        return Ok(false);
    }

    if app_guard.interaction_mode != InteractionMode::Insert {
        return Ok(false);
    }

    if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT {
        if let KeyCode::Char(character) = key.code {
            app_guard.insert_char(character);
        }
    }

    Ok(false)
}

fn execute_action(
    action: KeyAction,
    app: &Arc<Mutex<App>>,
    session: &AgentSession,
    tx: &mpsc::Sender<RuntimeMessageEnvelope>,
) -> anyhow::Result<bool> {
    match action {
        KeyAction::Quit => {
            info!("user exit via keymap");
            Ok(true)
        }
        KeyAction::InterruptTurn => {
            let mut app_guard = app.lock().unwrap();
            if app_guard.is_running {
                let turn_id = app_guard.active_turn_id;
                app_guard.open_interrupt_confirm_overlay(turn_id);
            } else {
                app_guard.set_status_notice("No running turn to interrupt.");
            }
            Ok(false)
        }
        KeyAction::EnterNormalMode => {
            let mut app_guard = app.lock().unwrap();
            app_guard.enter_normal_mode();
            app_guard.set_status_notice("Mode: Normal");
            Ok(false)
        }
        KeyAction::EnterInsertMode => {
            let mut app_guard = app.lock().unwrap();
            if app_guard.enter_insert_mode() {
                app_guard.set_status_notice("Mode: Insert");
            } else {
                app_guard.set_status_notice("Insert mode is unavailable in the current context.");
            }
            Ok(false)
        }
        KeyAction::ToggleInteractionMode => {
            let mut app_guard = app.lock().unwrap();
            if app_guard.interaction_mode == InteractionMode::Insert {
                app_guard.enter_normal_mode();
                app_guard.set_status_notice("Mode: Normal");
            } else if app_guard.enter_insert_mode() {
                app_guard.set_status_notice("Mode: Insert");
            } else {
                app_guard.set_status_notice("Insert mode is unavailable in the current context.");
            }
            Ok(false)
        }
        KeyAction::FocusNextPanel => {
            let mut app_guard = app.lock().unwrap();
            app_guard.focused_panel = app_guard.next_focus_panel();
            Ok(false)
        }
        KeyAction::ScrollPanelUp => {
            let mut app_guard = app.lock().unwrap();
            let panel = app_guard.focused_panel;
            app_guard.scroll_panel_up(panel, 3);
            Ok(false)
        }
        KeyAction::ScrollPanelDown => {
            let mut app_guard = app.lock().unwrap();
            let panel = app_guard.focused_panel;
            app_guard.scroll_panel_down(panel, 3);
            Ok(false)
        }
        KeyAction::MoveCursorLeft => {
            app.lock().unwrap().move_cursor_left();
            Ok(false)
        }
        KeyAction::MoveCursorRight => {
            app.lock().unwrap().move_cursor_right();
            Ok(false)
        }
        KeyAction::MoveCursorHome => {
            app.lock().unwrap().move_cursor_home();
            Ok(false)
        }
        KeyAction::MoveCursorEnd => {
            app.lock().unwrap().move_cursor_end();
            Ok(false)
        }
        KeyAction::DeleteCharAt => {
            app.lock().unwrap().delete_char_at();
            Ok(false)
        }
        KeyAction::DeleteCharBefore => {
            app.lock().unwrap().delete_char_before();
            Ok(false)
        }
        KeyAction::SubmitInput => handle_submit(app, session, tx),
        KeyAction::CancelPendingSequence => {
            let mut app_guard = app.lock().unwrap();
            app_guard.clear_leader_pending();
            app_guard.set_status_notice("Leader sequence cancelled.");
            Ok(false)
        }
        KeyAction::PanelSearch => {
            let mut app_guard = app.lock().unwrap();
            app_guard.open_search_overlay();
            Ok(false)
        }
        KeyAction::ToggleSidebar => {
            let mut app_guard = app.lock().unwrap();
            app_guard.toggle_sidebar_shell();
            let notice = if app_guard.sidebar.shell_collapsed {
                "Sidebar collapsed."
            } else {
                "Sidebar expanded."
            };
            app_guard.set_status_notice(notice);
            Ok(false)
        }
        KeyAction::HistoryPrevious
        | KeyAction::HistoryNext
        | KeyAction::ResizeSidebarWider
        | KeyAction::ResizeSidebarNarrower => {
            app.lock().unwrap().set_status_notice(format!(
                "Action '{}' is reserved for a later task.",
                action.as_str()
            ));
            Ok(false)
        }
    }
}
