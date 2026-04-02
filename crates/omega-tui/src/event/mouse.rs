use super::*;

pub(super) fn handle_mouse_event(mouse: MouseEvent, app: &Arc<Mutex<App>>) {
    let mut app_guard = app.lock().unwrap();
    if app_guard.overlay_active() {
        let inside_overlay = mouse.column >= app_guard.overlay_rect.x
            && mouse.column
                < app_guard
                    .overlay_rect
                    .x
                    .saturating_add(app_guard.overlay_rect.width)
            && mouse.row >= app_guard.overlay_rect.y
            && mouse.row
                < app_guard
                    .overlay_rect
                    .y
                    .saturating_add(app_guard.overlay_rect.height);

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) if !inside_overlay => {
                if app_guard
                    .overlay
                    .as_ref()
                    .is_some_and(|overlay| overlay.dismiss_on_backdrop())
                {
                    app_guard.close_overlay();
                    app_guard.set_status_notice("Overlay closed.");
                }
            }
            _ => {}
        }
        return;
    }

    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            let panel = app_guard.panel_at(mouse.column, mouse.row);
            match panel {
                Panel::SidebarRail => {
                    app_guard.clear_text_selection();
                    app_guard.focus_sidebar_rail();
                }
                Panel::Diagnostics if app_guard.diagnostics_visible() => {
                    app_guard.focused_panel = Panel::Diagnostics;
                    app_guard.begin_mouse_selection(Panel::Diagnostics, mouse.column, mouse.row);
                }
                Panel::Document if app_guard.document_visible() => {
                    app_guard.focused_panel = Panel::Document;
                    app_guard.begin_mouse_selection(Panel::Document, mouse.column, mouse.row);
                }
                Panel::Memory if app_guard.memory_visible() => {
                    app_guard.focused_panel = Panel::Memory;
                    app_guard.begin_mouse_selection(Panel::Memory, mouse.column, mouse.row);
                }
                Panel::Todo if app_guard.todo_visible() => {
                    app_guard.focused_panel = Panel::Todo;
                    app_guard.begin_mouse_selection(Panel::Todo, mouse.column, mouse.row);
                }
                Panel::Logs if app_guard.logs_visible() => {
                    app_guard.focused_panel = Panel::Logs;
                    app_guard.begin_mouse_selection(Panel::Logs, mouse.column, mouse.row);
                }
                _ => {
                    app_guard.focused_panel = Panel::Response;
                    app_guard.begin_mouse_selection(Panel::Response, mouse.column, mouse.row);
                }
            }
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            app_guard.update_mouse_selection(mouse.column, mouse.row);
        }
        MouseEventKind::Up(MouseButton::Left) => {
            let click_response_line = app_guard
                .text_selection
                .as_ref()
                .and_then(|selection| {
                    if selection.panel != Panel::Response {
                        return None;
                    }
                    let point =
                        app_guard.panel_text_point_at(Panel::Response, mouse.column, mouse.row)?;
                    (point == selection.anchor).then_some(point.line_index)
                });

            if let Some(text) = app_guard.finish_mouse_selection(mouse.column, mouse.row) {
                app_guard.set_status_notice(format!(
                    "Selected {} chars. Press y or Ctrl+C to copy.",
                    text.chars().count()
                ));
            } else if let Some(line_index) = click_response_line {
                app_guard.focused_panel = Panel::Response;
                app_guard.select_response_line(line_index);

                if let Some(notice) =
                    response_activation_notice(app_guard.activate_response_item_at_line(line_index))
                {
                    app_guard.set_status_notice(notice);
                }
            }
        }
        MouseEventKind::ScrollUp => {
            let panel = app_guard.panel_at(mouse.column, mouse.row);
            app_guard.scroll_panel_up(panel, 3);
        }
        MouseEventKind::ScrollDown => {
            let panel = app_guard.panel_at(mouse.column, mouse.row);
            app_guard.scroll_panel_down(panel, 3);
        }
        _ => {}
    }
}

fn response_activation_notice(activation: Option<ResponseActivation>) -> Option<String> {
    match activation {
        Some(ResponseActivation::ThinkingCollapsed) => Some("Thinking collapsed.".to_string()),
        Some(ResponseActivation::ThinkingExpanded) => Some("Thinking expanded.".to_string()),
        Some(ResponseActivation::ToolLaneCollapsed) => Some("Tool lane collapsed.".to_string()),
        Some(ResponseActivation::ToolLaneExpanded) => Some("Tool lane expanded.".to_string()),
        Some(ResponseActivation::ToolDetailOpened(tool_name)) => {
            Some(format!("Opened {tool_name} detail overlay."))
        }
        Some(ResponseActivation::StepSubflowDetailOpened(label)) => {
            Some(format!("Opened subflow detail overlay for {label}."))
        }
        None => None,
    }
}
