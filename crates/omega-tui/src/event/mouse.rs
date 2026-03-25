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
            if let Some(text) = app_guard.finish_mouse_selection(mouse.column, mouse.row) {
                app_guard.set_status_notice(format!(
                    "Selected {} chars. Press y or Ctrl+C to copy.",
                    text.chars().count()
                ));
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
