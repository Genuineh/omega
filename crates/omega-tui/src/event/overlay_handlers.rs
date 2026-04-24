use super::key::submit_input_text;
use super::*;

pub(super) fn handle_overlay_key_event(
    key: KeyEvent,
    app: &Arc<Mutex<App>>,
    session: &AgentSession,
    tx: &mpsc::Sender<RuntimeMessageEnvelope>,
) -> anyhow::Result<bool> {
    let mut app_guard = app.lock().unwrap();
    let page_step = app_guard.overlay_page_step();
    let viewport_lines = app_guard.overlay_viewport_lines();
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
            KeyCode::PageUp => {
                overlay.scroll = overlay.scroll.saturating_sub(page_step);
            }
            KeyCode::PageDown => {
                overlay.scroll = overlay.scroll.saturating_add(page_step);
            }
            KeyCode::Home => {
                overlay.scroll = 0;
            }
            KeyCode::End => {
                overlay.scroll = overlay.lines.len().saturating_sub(viewport_lines);
            }
            _ => {}
        },
        OverlayState::Confirm(overlay) => match key.code {
            KeyCode::Esc => {
                app_guard.close_overlay();
                app_guard.set_status_notice("Confirm action cancelled.");
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
                    return confirm_overlay_intent(intent, app, session, tx);
                }

                app_guard.close_overlay();
                app_guard.set_status_notice("Interrupt request cancelled.");
            }
            _ => {}
        },
        OverlayState::DocumentNavigator(overlay) => match key.code {
            KeyCode::Esc => {
                app_guard.close_overlay();
            }
            KeyCode::Tab => {
                overlay.toggle_focus();
            }
            KeyCode::Left if overlay.focus != crate::overlay::DocumentNavigatorFocus::Rail => {
                overlay.set_focus(crate::overlay::DocumentNavigatorFocus::Rail);
            }
            KeyCode::Right if overlay.focus != crate::overlay::DocumentNavigatorFocus::Content => {
                overlay.set_focus(crate::overlay::DocumentNavigatorFocus::Content);
            }
            KeyCode::Up | KeyCode::Char('k') if key.modifiers == KeyModifiers::NONE => {
                if overlay.focus == crate::overlay::DocumentNavigatorFocus::Rail {
                    overlay.move_selection_up();
                } else {
                    overlay.scroll_content_up(1);
                }
            }
            KeyCode::Down | KeyCode::Char('j') if key.modifiers == KeyModifiers::NONE => {
                if overlay.focus == crate::overlay::DocumentNavigatorFocus::Rail {
                    overlay.move_selection_down();
                } else {
                    overlay.scroll_content_down(1);
                }
            }
            KeyCode::PageUp => {
                if overlay.focus == crate::overlay::DocumentNavigatorFocus::Rail {
                    overlay.move_selection_by(page_step, false);
                } else {
                    overlay.scroll_content_up(page_step);
                }
            }
            KeyCode::PageDown => {
                if overlay.focus == crate::overlay::DocumentNavigatorFocus::Rail {
                    overlay.move_selection_by(page_step, true);
                } else {
                    overlay.scroll_content_down(page_step);
                }
            }
            KeyCode::Home => {
                if overlay.focus == crate::overlay::DocumentNavigatorFocus::Rail {
                    overlay.move_selection_to_start();
                } else {
                    overlay.scroll_content_to_start();
                }
            }
            KeyCode::End => {
                if overlay.focus == crate::overlay::DocumentNavigatorFocus::Rail {
                    overlay.move_selection_to_end();
                } else {
                    overlay.scroll_content_to_end(viewport_lines);
                }
            }
            KeyCode::Enter if overlay.focus == crate::overlay::DocumentNavigatorFocus::Rail => {
                overlay.activate_selected();
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
            KeyCode::PageUp => {
                overlay.scroll = overlay.scroll.saturating_sub(page_step);
            }
            KeyCode::PageDown => {
                overlay.scroll = overlay.scroll.saturating_add(page_step);
            }
            KeyCode::Home => {
                overlay.scroll = 0;
            }
            KeyCode::End => {
                overlay.scroll = overlay.lines.len().saturating_sub(viewport_lines);
            }
            _ => {}
        },
        OverlayState::Picker(overlay) => match key.code {
            KeyCode::Esc => {
                app_guard.close_overlay();
            }
            KeyCode::Up | KeyCode::Char('k') if key.modifiers == KeyModifiers::NONE => {
                overlay.move_selection_up();
            }
            KeyCode::Down | KeyCode::Tab | KeyCode::Char('j')
                if key.modifiers == KeyModifiers::NONE =>
            {
                overlay.move_selection_down();
            }
            KeyCode::Char('/') if key.modifiers == KeyModifiers::NONE => {
                overlay.enter_filter_mode();
            }
            KeyCode::Backspace if overlay.filter_mode => {
                delete_char_before(&mut overlay.filter_query, &mut overlay.filter_cursor_pos);
                overlay.apply_filter_query();
            }
            KeyCode::Delete if overlay.filter_mode => {
                delete_char_at(&mut overlay.filter_query, overlay.filter_cursor_pos);
                overlay.apply_filter_query();
            }
            KeyCode::Left if overlay.filter_mode => {
                overlay.filter_cursor_pos = overlay.filter_cursor_pos.saturating_sub(1);
            }
            KeyCode::Right if overlay.filter_mode => {
                let count = overlay.filter_query.chars().count();
                if overlay.filter_cursor_pos < count {
                    overlay.filter_cursor_pos += 1;
                }
            }
            KeyCode::Home if overlay.filter_mode => {
                overlay.filter_cursor_pos = 0;
            }
            KeyCode::End if overlay.filter_mode => {
                overlay.filter_cursor_pos = overlay.filter_query.chars().count();
            }
            KeyCode::Char(character)
                if overlay.filter_mode
                    && (key.modifiers == KeyModifiers::NONE
                        || key.modifiers == KeyModifiers::SHIFT) =>
            {
                insert_char(
                    &mut overlay.filter_query,
                    &mut overlay.filter_cursor_pos,
                    character,
                );
                overlay.apply_filter_query();
            }
            _ => {
                if let Some(action) = picker_action_for_key(key, overlay) {
                    let action = action.clone();
                    let selected_item = overlay.selected_item().cloned();
                    drop(app_guard);
                    return execute_picker_action(action, selected_item, app, session, tx);
                }
            }
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

fn picker_action_for_key(
    key: KeyEvent,
    overlay: &crate::overlay::PickerOverlay,
) -> Option<&omega_session::OperatorPickerAction> {
    match (key.code, key.modifiers) {
        (KeyCode::Enter, KeyModifiers::NONE) => overlay
            .action_for_shortcut(omega_session::OperatorPickerShortcut::Enter),
        (KeyCode::Char(character), KeyModifiers::CONTROL) => overlay
            .action_for_shortcut(omega_session::OperatorPickerShortcut::Ctrl(
                character.to_ascii_lowercase(),
            )),
        _ => None,
    }
}

fn execute_picker_action(
    action: omega_session::OperatorPickerAction,
    selected_item: Option<omega_session::OperatorPickerItem>,
    app: &Arc<Mutex<App>>,
    session: &AgentSession,
    tx: &mpsc::Sender<RuntimeMessageEnvelope>,
) -> anyhow::Result<bool> {
    if action.requires_selection && selected_item.is_none() {
        app.lock()
            .unwrap()
            .set_status_notice("Select an item before running that action.");
        return Ok(false);
    }

    if let Some(disabled_reason) = selected_item
        .as_ref()
        .and_then(|item| item.disabled_reason.as_deref())
        .filter(|_| action.requires_selection)
    {
        app.lock()
            .unwrap()
            .set_status_notice(disabled_reason.to_string());
        return Ok(false);
    }

    match action.intent {
        omega_session::OperatorPickerIntent::OpenDetail => {
            let Some(item) = selected_item else {
                return Ok(false);
            };
            let mut lines = vec![format!("id: {}", item.id)];
            if let Some(subtitle) = item.subtitle.as_deref() {
                lines.push(format!("summary: {subtitle}"));
            }
            if !item.badges.is_empty() {
                lines.push(format!("badges: {}", item.badges.join(", ")));
            }
            if let Some(disabled_reason) = item.disabled_reason.as_deref() {
                lines.push(format!("disabled: {disabled_reason}"));
            }
            if let Some(preview) = item.preview.as_deref() {
                if !preview.trim().is_empty() {
                    lines.push(String::new());
                    lines.extend(preview.lines().map(ToOwned::to_owned));
                }
            }
            let mut app_guard = app.lock().unwrap();
            app_guard.open_detail_overlay(format!(" {} ", item.title), lines);
            app_guard.set_status_notice(format!("Opened {} detail.", item.title));
            Ok(false)
        }
        omega_session::OperatorPickerIntent::SubmitSlashCommand { command_template } => {
            let command = command_from_template(&command_template, selected_item.as_ref());
            {
                let mut app_guard = app.lock().unwrap();
                if action.overlay_behavior
                    == omega_session::OperatorPickerOverlayBehavior::CloseOverlay
                {
                    app_guard.close_overlay();
                }
                app_guard.set_status_notice(format!("Running operator action: {command}"));
            }
            submit_input_text(command, app, session, tx)
        }
        omega_session::OperatorPickerIntent::RequestConfirmSlashCommand {
            title_template,
            message_template,
            confirm_label,
            command_template,
        } => {
            let Some(item) = selected_item else {
                return Ok(false);
            };
            let command = command_template_value(&command_template, &item);
            let title = command_template_value(&title_template, &item);
            let message = command_template_value(&message_template, &item);
            let mut app_guard = app.lock().unwrap();
            let origin_panel = app_guard.focused_panel;
            app_guard.overlay = Some(OverlayState::Confirm(crate::overlay::ConfirmOverlay {
                origin_panel,
                title,
                message,
                confirm_label,
                cancel_label: "Cancel".to_string(),
                selected: ConfirmChoice::Cancel,
                intent: ConfirmIntent::SubmitSlashCommand { command },
                dismiss_on_backdrop: true,
            }));
            app_guard.clear_leader_pending();
            app_guard.set_status_notice("Confirm the session action in the overlay.");
            Ok(false)
        }
        omega_session::OperatorPickerIntent::RefreshPicker => {
            app.lock()
                .unwrap()
                .set_status_notice("Picker refresh is waiting on a runtime-driven update.");
            Ok(false)
        }
        omega_session::OperatorPickerIntent::ClosePicker => {
            app.lock().unwrap().close_overlay();
            Ok(false)
        }
    }
}

fn command_from_template(
    template: &str,
    selected_item: Option<&omega_session::OperatorPickerItem>,
) -> String {
    match selected_item {
        Some(item) => command_template_value(template, item),
        None => template.to_string(),
    }
}

fn command_template_value(template: &str, item: &omega_session::OperatorPickerItem) -> String {
    template
        .replace("{id}", item.id.as_str())
        .replace("{title}", item.title.as_str())
}

fn confirm_overlay_intent(
    intent: ConfirmIntent,
    app: &Arc<Mutex<App>>,
    session: &AgentSession,
    tx: &mpsc::Sender<RuntimeMessageEnvelope>,
) -> anyhow::Result<bool> {
    match intent {
        ConfirmIntent::Dismiss => {
            let mut app_guard = app.lock().unwrap();
            app_guard.close_overlay();
            app_guard.set_status_notice("Approval overlay dismissed.");
            Ok(false)
        }
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
        ConfirmIntent::SubmitSlashCommand { command } => {
            {
                let mut app_guard = app.lock().unwrap();
                app_guard.close_overlay();
                app_guard.set_status_notice(format!("Running operator action: {command}"));
            }
            submit_input_text(command, app, session, tx)
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
