use std::cell::RefCell;

use arboard::Clipboard;

use super::*;

thread_local! {
    static PERSISTENT_CLIPBOARD: RefCell<Option<SystemClipboard>> = const { RefCell::new(None) };
}

pub(super) trait ClipboardBackend {
    fn set_text(&mut self, text: &str) -> Result<(), String>;
}

struct SystemClipboard {
    inner: Clipboard,
}

impl SystemClipboard {
    fn new() -> Result<Self, String> {
        Clipboard::new()
            .map(|inner| Self { inner })
            .map_err(|error| error.to_string())
    }
}

impl ClipboardBackend for SystemClipboard {
    fn set_text(&mut self, text: &str) -> Result<(), String> {
        self.inner
            .set_text(text.to_string())
            .map_err(|error| error.to_string())
    }
}

pub(super) fn copy_selected_text(app: &mut App) -> Result<Option<usize>, String> {
    PERSISTENT_CLIPBOARD.with(|clipboard| {
        let mut clipboard = clipboard.borrow_mut();
        copy_selected_text_with_backend(app, &mut clipboard, SystemClipboard::new)
    })
}

pub(super) fn copy_selected_text_with_backend<B, F>(
    app: &mut App,
    backend: &mut Option<B>,
    init: F,
) -> Result<Option<usize>, String>
where
    B: ClipboardBackend,
    F: Fn() -> Result<B, String>,
{
    let Some(text) = app.selected_text() else {
        return Ok(None);
    };
    let count = text.chars().count();
    write_text_with_backend(backend, &text, init)?;
    Ok(Some(count))
}

pub(super) fn is_copy_shortcut(key: KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char('y') | KeyCode::Char('Y'))
        && matches!(key.modifiers, KeyModifiers::NONE | KeyModifiers::SHIFT)
        || matches!(key.code, KeyCode::Char('c')) && key.modifiers == KeyModifiers::CONTROL
}

pub(super) fn write_text_with_backend<B, F>(
    backend: &mut Option<B>,
    text: &str,
    init: F,
) -> Result<(), String>
where
    B: ClipboardBackend,
    F: Fn() -> Result<B, String>,
{
    if backend.is_none() {
        *backend = Some(init()?);
    }

    if let Some(clipboard) = backend.as_mut() {
        if clipboard.set_text(text).is_ok() {
            return Ok(());
        }
    }

    *backend = Some(init()?);
    backend
        .as_mut()
        .ok_or_else(|| "clipboard backend is unavailable".to_string())?
        .set_text(text)
}
