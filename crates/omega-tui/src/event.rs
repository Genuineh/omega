use std::sync::{mpsc, Arc, Mutex};

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use omega_keymap::{InteractionMode, KeyAction, KeyContext, KeyResolution, KeymapManager};
use omega_session::{AgentSession, RuntimeUiEnvelope};
use tracing::info;

use crate::app::{App, MsgKind, Panel, ResponseActivation};
use crate::overlay::{ConfirmChoice, ConfirmIntent, OverlayState};

mod clipboard;
mod key;
mod mouse;
mod overlay_handlers;

#[cfg(test)]
use clipboard::{copy_selected_text_with_backend, write_text_with_backend, ClipboardBackend};
use key::handle_key_event;
#[cfg(test)]
use key::handle_submit;
use mouse::handle_mouse_event;

pub fn handle_event(
    event: Event,
    app: &Arc<Mutex<App>>,
    session: &AgentSession,
    tx: &mpsc::Sender<RuntimeUiEnvelope>,
    keymap: &KeymapManager,
) -> anyhow::Result<bool> {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Press => {
            handle_key_event(key, app, session, tx, keymap)
        }
        Event::Mouse(mouse) => {
            handle_mouse_event(mouse, app);
            Ok(false)
        }
        _ => Ok(false),
    }
}

#[cfg(test)]
#[path = "event_tests.rs"]
mod tests;
