use super::*;

pub(super) fn handle_overlay_key_event(
    key: KeyEvent,
    app: &Arc<Mutex<App>>,
    session: &AgentSession,
) -> anyhow::Result<bool> {
    let mut app_guard = app.lock().unwrap();
    let Some(overlay) = app_guard.overlay.as_mut() else {
        return Ok(false);
    };

    match overlay {
        OverlayState::Search(overlay) => match key.code {
            KeyCode::Esc => {
                app_guard.close_overlay();
                app_guard.set_status_notice("Search popup closed.");
            }
            KeyCode::Left => {
                overlay.cursor_pos = overlay.cursor_pos.saturating_sub(1);
            }
            KeyCode::Right => {
                let count = overlay.query.chars().count();
                if overlay.cursor_pos < count {
                    overlay.cursor_pos += 1;
                }
            }
            KeyCode::Home => {
                overlay.cursor_pos = 0;
            }
            KeyCode::End => {
                overlay.cursor_pos = overlay.query.chars().count();
            }
            KeyCode::Backspace => {
                delete_char_before(&mut overlay.query, &mut overlay.cursor_pos);
            }
            KeyCode::Delete => {
                delete_char_at(&mut overlay.query, overlay.cursor_pos);
            }
            KeyCode::Enter => {
                let query = overlay.query.clone();
                let target_panel = overlay.target_panel;
                let count = if query.trim().is_empty() {
                    0
                } else {
                    let lowered = query.to_ascii_lowercase();
                    app_guard
                        .panel_lines(target_panel)
                        .into_iter()
                        .map(|line| line.to_ascii_lowercase().matches(&lowered).count())
                        .sum()
                };
                app_guard.set_status_notice(format!(
                    "Search popup captured '{}' with {count} match(es). Jump navigation lands in Task 15B-11.",
                    query
                ));
            }
            KeyCode::Char(character)
                if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT =>
            {
                insert_char(&mut overlay.query, &mut overlay.cursor_pos, character);
            }
            _ => {}
        },
        OverlayState::SearchResults(overlay) => match key.code {
            KeyCode::Esc => {
                app_guard.close_overlay();
                app_guard.set_status_notice("Search results overlay closed.");
            }
            KeyCode::Up => {
                overlay.scroll = overlay.scroll.saturating_sub(1);
            }
            KeyCode::Down => {
                overlay.scroll = overlay.scroll.saturating_add(1);
            }
            _ => {}
        },
        OverlayState::Confirm(overlay) => match key.code {
            KeyCode::Esc => {
                app_guard.close_overlay();
                app_guard.set_status_notice("Interrupt request cancelled.");
            }
            KeyCode::Left => {
                overlay.selected = ConfirmChoice::Cancel;
            }
            KeyCode::Right | KeyCode::Tab => {
                overlay.selected = match overlay.selected {
                    ConfirmChoice::Cancel => ConfirmChoice::Confirm,
                    ConfirmChoice::Confirm => ConfirmChoice::Cancel,
                };
            }
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                overlay.selected = ConfirmChoice::Confirm;
            }
            KeyCode::Char('n') | KeyCode::Char('N') => {
                overlay.selected = ConfirmChoice::Cancel;
            }
            KeyCode::Enter => {
                let selected = overlay.selected;
                let intent = overlay.intent.clone();
                if selected == ConfirmChoice::Confirm {
                    drop(app_guard);
                    return confirm_overlay_intent(intent, app, session);
                }

                app_guard.close_overlay();
                app_guard.set_status_notice("Interrupt request cancelled.");
            }
            _ => {}
        },
        OverlayState::Detail(overlay) => match key.code {
            KeyCode::Esc => {
                app_guard.close_overlay();
            }
            KeyCode::Up => {
                overlay.scroll = overlay.scroll.saturating_sub(1);
            }
            KeyCode::Down => {
                overlay.scroll = overlay.scroll.saturating_add(1);
            }
            _ => {}
        },
        OverlayState::Picker(overlay) => match key.code {
            KeyCode::Esc => {
                app_guard.close_overlay();
            }
            KeyCode::Up => {
                overlay.selected = overlay.selected.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Tab => {
                if !overlay.items.is_empty() {
                    overlay.selected = (overlay.selected + 1).min(overlay.items.len() - 1);
                }
            }
            KeyCode::Enter => {
                let selection = overlay.items.get(overlay.selected).cloned();
                app_guard.close_overlay();
                if let Some(selection) = selection {
                    app_guard.set_status_notice(format!("Picker selected '{selection}'."));
                }
            }
            _ => {}
        },
        OverlayState::InputPrompt(overlay) => match key.code {
            KeyCode::Esc => {
                app_guard.close_overlay();
            }
            KeyCode::Left => {
                overlay.cursor_pos = overlay.cursor_pos.saturating_sub(1);
            }
            KeyCode::Right => {
                let count = overlay.value.chars().count();
                if overlay.cursor_pos < count {
                    overlay.cursor_pos += 1;
                }
            }
            KeyCode::Home => {
                overlay.cursor_pos = 0;
            }
            KeyCode::End => {
                overlay.cursor_pos = overlay.value.chars().count();
            }
            KeyCode::Backspace => {
                delete_char_before(&mut overlay.value, &mut overlay.cursor_pos);
            }
            KeyCode::Delete => {
                delete_char_at(&mut overlay.value, overlay.cursor_pos);
            }
            KeyCode::Enter => {
                let value = overlay.value.clone();
                app_guard.close_overlay();
                app_guard.set_status_notice(format!("Input prompt submitted '{value}'."));
            }
            KeyCode::Char(character)
                if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT =>
            {
                insert_char(&mut overlay.value, &mut overlay.cursor_pos, character);
            }
            _ => {}
        },
    }

    Ok(false)
}

fn confirm_overlay_intent(
    intent: ConfirmIntent,
    app: &Arc<Mutex<App>>,
    session: &AgentSession,
) -> anyhow::Result<bool> {
    match intent {
        ConfirmIntent::InterruptTurn { turn_id } => {
            let mut app_guard = app.lock().unwrap();
            if app_guard.is_running && app_guard.is_current_turn(turn_id) {
                app_guard.interrupt_turn();
                app_guard.push_msg(MsgKind::Error, "⚠ Interrupted");
                app_guard.close_overlay();
                app_guard.set_status_notice("Running turn interrupted.");
                drop(app_guard);
                info!(turn_id, "user interrupted running task via overlay confirm");
                session.interrupt(turn_id)?;
            } else {
                app_guard.close_overlay();
                app_guard.set_status_notice("Running turn already finished.");
            }
            Ok(false)
        }
    }
}

fn insert_char(buffer: &mut String, cursor_pos: &mut usize, character: char) {
    let byte_pos = cursor_byte_pos(buffer, *cursor_pos);
    buffer.insert(byte_pos, character);
    *cursor_pos += 1;
}

fn delete_char_before(buffer: &mut String, cursor_pos: &mut usize) {
    if *cursor_pos > 0 {
        *cursor_pos -= 1;
        let byte_pos = cursor_byte_pos(buffer, *cursor_pos);
        buffer.remove(byte_pos);
    }
}

fn delete_char_at(buffer: &mut String, cursor_pos: usize) {
    if cursor_pos < buffer.chars().count() {
        let byte_pos = cursor_byte_pos(buffer, cursor_pos);
        buffer.remove(byte_pos);
    }
}

fn cursor_byte_pos(buffer: &str, cursor_pos: usize) -> usize {
    buffer
        .char_indices()
        .nth(cursor_pos)
        .map(|(index, _)| index)
        .unwrap_or(buffer.len())
}
