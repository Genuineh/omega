use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use omega_hpc_paths::{OmegaProjectLayout, KEYMAP_CONFIG_PATH};
use serde::Deserialize;

pub const DEFAULT_KEYMAP_PATH: &str = KEYMAP_CONFIG_PATH;
const DEFAULT_LEADER_TIMEOUT_MS: u64 = 300;
const DEFAULT_KEYMAP_TOML: &str = r#"# Default omega-tui keymap
# Normal-mode commands use the leader prefix to avoid collisions with text input.

[leader]
key = "space"
timeout_ms = 300

[[bindings]]
keys = "esc"
action = "enter_normal_mode"
mode = "insert"

[[bindings]]
keys = "leader j k"
action = "enter_normal_mode"
mode = "insert"
text_fallback = true

[[bindings]]
keys = "leader j k"
action = "enter_insert_mode"
mode = "normal"
input_capable = true

[[bindings]]
keys = "leader tab"
action = "focus_next_panel"
mode = "normal"

[[bindings]]
keys = "leader up"
action = "scroll_panel_up"
mode = "normal"

[[bindings]]
keys = "leader down"
action = "scroll_panel_down"
mode = "normal"

[[bindings]]
keys = "left"
action = "move_cursor_left"
mode = "insert"

[[bindings]]
keys = "right"
action = "move_cursor_right"
mode = "insert"

[[bindings]]
keys = "up"
action = "move_cursor_up"
mode = "insert"

[[bindings]]
keys = "down"
action = "move_cursor_down"
mode = "insert"

[[bindings]]
keys = "home"
action = "move_cursor_home"
mode = "insert"

[[bindings]]
keys = "end"
action = "move_cursor_end"
mode = "insert"

[[bindings]]
keys = "delete"
action = "delete_char_at"
mode = "insert"

[[bindings]]
keys = "backspace"
action = "delete_char_before"
mode = "insert"

[[bindings]]
keys = "shift+enter"
action = "insert_newline"
mode = "insert"
input_capable = true

[[bindings]]
keys = "enter"
action = "submit_input"
mode = "insert"
input_capable = true

[[bindings]]
keys = "leader c"
action = "interrupt_turn"

[[bindings]]
keys = "leader q"
action = "quit"

[[bindings]]
keys = "esc"
action = "cancel_pending_sequence"
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractionMode {
    Normal,
    Insert,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyFocus {
    Response,
    Todo,
    Logs,
    SidebarRail,
    Activity,
    InputField,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyContext {
    pub mode: InteractionMode,
    pub focus: KeyFocus,
    pub input_capable: bool,
    pub leader_pending: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyAction {
    EnterNormalMode,
    EnterInsertMode,
    ToggleInteractionMode,
    FocusNextPanel,
    ScrollPanelUp,
    ScrollPanelDown,
    MoveCursorLeft,
    MoveCursorRight,
    MoveCursorUp,
    MoveCursorDown,
    MoveCursorHome,
    MoveCursorEnd,
    DeleteCharAt,
    DeleteCharBefore,
    InsertNewline,
    SubmitInput,
    InterruptTurn,
    Quit,
    ToggleSidebar,
    PanelSearch,
    HistoryPrevious,
    HistoryNext,
    ResizeSidebarWider,
    ResizeSidebarNarrower,
    CancelPendingSequence,
}

impl KeyAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::EnterNormalMode => "enter_normal_mode",
            Self::EnterInsertMode => "enter_insert_mode",
            Self::ToggleInteractionMode => "toggle_interaction_mode",
            Self::FocusNextPanel => "focus_next_panel",
            Self::ScrollPanelUp => "scroll_panel_up",
            Self::ScrollPanelDown => "scroll_panel_down",
            Self::MoveCursorLeft => "move_cursor_left",
            Self::MoveCursorRight => "move_cursor_right",
            Self::MoveCursorUp => "move_cursor_up",
            Self::MoveCursorDown => "move_cursor_down",
            Self::MoveCursorHome => "move_cursor_home",
            Self::MoveCursorEnd => "move_cursor_end",
            Self::DeleteCharAt => "delete_char_at",
            Self::DeleteCharBefore => "delete_char_before",
            Self::InsertNewline => "insert_newline",
            Self::SubmitInput => "submit_input",
            Self::InterruptTurn => "interrupt_turn",
            Self::Quit => "quit",
            Self::ToggleSidebar => "toggle_sidebar",
            Self::PanelSearch => "panel_search",
            Self::HistoryPrevious => "history_previous",
            Self::HistoryNext => "history_next",
            Self::ResizeSidebarWider => "resize_sidebar_wider",
            Self::ResizeSidebarNarrower => "resize_sidebar_narrower",
            Self::CancelPendingSequence => "cancel_pending_sequence",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyResolution {
    Matched(KeyAction),
    PendingLeader,
    PendingSequence(PendingSequenceState),
    ReplayAsText(String),
    NoMatch,
    InvalidInContext(KeyAction),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingSequenceState {
    pub replay_text: Option<String>,
    pub timeout: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeymapSource {
    BuiltIn,
    File(PathBuf),
}

#[derive(Debug, Clone)]
pub struct KeymapLoadResult {
    pub manager: KeymapManager,
    pub warning: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyBinding {
    pub sequence: KeySequence,
    pub action: KeyAction,
    pub mode: Option<InteractionMode>,
    pub focus: Option<KeyFocus>,
    pub input_capable: Option<bool>,
    pub text_fallback: bool,
    pub timeout: Option<Duration>,
}

impl KeyBinding {
    fn specificity(&self) -> usize {
        usize::from(self.mode.is_some())
            + usize::from(self.focus.is_some())
            + usize::from(self.input_capable.is_some())
    }

    fn matches_context(&self, context: &KeyContext) -> bool {
        self.mode.is_none_or(|mode| mode == context.mode)
            && self.focus.is_none_or(|focus| focus == context.focus)
            && self
                .input_capable
                .is_none_or(|input_capable| input_capable == context.input_capable)
    }

    fn effective_timeout(&self, default_timeout: Duration) -> Duration {
        self.timeout.unwrap_or(default_timeout)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeySequence {
    strokes: Vec<KeyStroke>,
}

impl KeySequence {
    fn new(strokes: Vec<KeyStroke>) -> Result<Self> {
        if strokes.is_empty() {
            bail!("key sequence cannot be empty");
        }

        Ok(Self { strokes })
    }

    fn from_event(event: KeyEvent) -> Result<Self> {
        Self::new(vec![KeyStroke::from_event(event)?])
    }

    fn from_events(events: &[KeyEvent]) -> Result<Self> {
        let strokes = events
            .iter()
            .copied()
            .map(KeyStroke::from_event)
            .collect::<Result<Vec<_>>>()?;
        Self::new(strokes)
    }

    fn parse(text: &str, leader: KeyStroke) -> Result<Self> {
        let mut strokes = Vec::new();
        for token in text.split_whitespace() {
            if token.eq_ignore_ascii_case("leader") {
                strokes.push(leader);
            } else {
                strokes.push(KeyStroke::parse(token)?);
            }
        }

        Self::new(strokes)
    }

    fn with_appended_event(&self, event: KeyEvent) -> Result<Self> {
        let mut strokes = self.strokes.clone();
        strokes.push(KeyStroke::from_event(event)?);
        Self::new(strokes)
    }

    fn starts_with(&self, prefix: &KeySequence) -> bool {
        self.strokes.starts_with(&prefix.strokes)
    }

    fn len(&self) -> usize {
        self.strokes.len()
    }

    fn fallback_text(&self) -> Option<String> {
        self.strokes.iter().map(KeyStroke::fallback_char).collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct KeyStroke {
    code: KeyCodePattern,
    modifiers: KeyModifiers,
}

impl KeyStroke {
    fn parse(text: &str) -> Result<Self> {
        let parts: Vec<_> = text.split('+').collect();
        let (key_part, modifier_parts) = match parts.split_last() {
            Some((key_part, modifier_parts)) => (*key_part, modifier_parts),
            None => bail!("empty key token"),
        };

        let mut modifiers = KeyModifiers::NONE;
        for modifier in modifier_parts {
            match modifier.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => modifiers |= KeyModifiers::CONTROL,
                "alt" => modifiers |= KeyModifiers::ALT,
                "shift" => modifiers |= KeyModifiers::SHIFT,
                other => bail!("unsupported key modifier '{other}'"),
            }
        }

        Ok(Self {
            code: KeyCodePattern::parse(key_part)?,
            modifiers,
        })
    }

    fn from_event(event: KeyEvent) -> Result<Self> {
        Ok(Self {
            code: KeyCodePattern::from_event(event.code)?,
            modifiers: event.modifiers,
        })
    }

    fn fallback_char(&self) -> Option<char> {
        if !(self.modifiers == KeyModifiers::NONE || self.modifiers == KeyModifiers::SHIFT) {
            return None;
        }

        match self.code {
            KeyCodePattern::Char(character) => Some(character),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum KeyCodePattern {
    Char(char),
    Enter,
    Tab,
    Backspace,
    Delete,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    Esc,
}

impl KeyCodePattern {
    fn parse(text: &str) -> Result<Self> {
        let normalized = text.to_ascii_lowercase();
        match normalized.as_str() {
            "enter" => Ok(Self::Enter),
            "tab" => Ok(Self::Tab),
            "backspace" => Ok(Self::Backspace),
            "delete" | "del" => Ok(Self::Delete),
            "left" => Ok(Self::Left),
            "right" => Ok(Self::Right),
            "up" => Ok(Self::Up),
            "down" => Ok(Self::Down),
            "home" => Ok(Self::Home),
            "end" => Ok(Self::End),
            "esc" | "escape" => Ok(Self::Esc),
            "space" => Ok(Self::Char(' ')),
            _ if normalized.chars().count() == 1 => {
                Ok(Self::Char(normalized.chars().next().unwrap_or_default()))
            }
            _ => bail!("unsupported key '{text}'"),
        }
    }

    fn from_event(code: KeyCode) -> Result<Self> {
        match code {
            KeyCode::Char(c) => Ok(Self::Char(c)),
            KeyCode::Enter => Ok(Self::Enter),
            KeyCode::Tab | KeyCode::BackTab => Ok(Self::Tab),
            KeyCode::Backspace => Ok(Self::Backspace),
            KeyCode::Delete => Ok(Self::Delete),
            KeyCode::Left => Ok(Self::Left),
            KeyCode::Right => Ok(Self::Right),
            KeyCode::Up => Ok(Self::Up),
            KeyCode::Down => Ok(Self::Down),
            KeyCode::Home => Ok(Self::Home),
            KeyCode::End => Ok(Self::End),
            KeyCode::Esc => Ok(Self::Esc),
            other => bail!("unsupported runtime key event '{other:?}'"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct KeymapManager {
    leader: KeyStroke,
    leader_timeout: Duration,
    bindings: Vec<KeyBinding>,
    source: KeymapSource,
}

impl KeymapManager {
    pub fn builtin() -> Self {
        Self::parse_keymap_str(DEFAULT_KEYMAP_TOML, KeymapSource::BuiltIn)
            .expect("builtin keymap should be valid")
    }

    pub fn load(root: &Path) -> KeymapLoadResult {
        let path = OmegaProjectLayout::new(root.to_path_buf()).keymap_path();
        if !path.exists() {
            match Self::write_default_file(&path) {
                Ok(()) => {
                    return match Self::load_from_file(&path) {
                        Ok(manager) => KeymapLoadResult {
                            manager,
                            warning: None,
                        },
                        Err(error) => KeymapLoadResult {
                            manager: Self::builtin(),
                            warning: Some(format!(
                                "Default keymap file at {} was created but failed to load: {error}. Falling back to built-in defaults.",
                                path.display()
                            )),
                        },
                    };
                }
                Err(error) => {
                    return KeymapLoadResult {
                        manager: Self::builtin(),
                        warning: Some(format!(
                            "Failed to create default keymap file at {}: {error}. Falling back to built-in defaults.",
                            path.display()
                        )),
                    };
                }
            }
        }

        match Self::load_from_file(&path) {
            Ok(manager) => KeymapLoadResult {
                manager,
                warning: None,
            },
            Err(error) => KeymapLoadResult {
                manager: Self::builtin(),
                warning: Some(format!(
                    "Keymap config at {} is invalid: {error}. Falling back to built-in defaults.",
                    path.display()
                )),
            },
        }
    }

    pub fn load_from_file(path: &Path) -> Result<Self> {
        let raw = fs::read_to_string(path)
            .with_context(|| format!("failed to read keymap file {}", path.display()))?;
        let builtin =
            Self::parse_keymap_file(DEFAULT_KEYMAP_TOML).expect("builtin keymap file should parse");
        let custom = Self::parse_keymap_file(&raw).map_err(|error| {
            anyhow::anyhow!("failed to parse keymap file {}: {error}", path.display())
        })?;
        let merged = merge_keymap_files(builtin, custom);

        Self::compile_keymap(merged, KeymapSource::File(path.to_path_buf())).map_err(|error| {
            anyhow::anyhow!("failed to parse keymap file {}: {error}", path.display())
        })
    }

    pub fn resolve(&self, context: &KeyContext, event: KeyEvent) -> KeyResolution {
        self.resolve_with_pending(context, &[], event)
    }

    pub fn resolve_with_pending(
        &self,
        context: &KeyContext,
        pending: &[KeyEvent],
        event: KeyEvent,
    ) -> KeyResolution {
        if !pending.is_empty() {
            let pending_sequence = match KeySequence::from_events(pending) {
                Ok(sequence) => sequence,
                Err(_) => return KeyResolution::NoMatch,
            };

            let sequence = match pending_sequence.with_appended_event(event) {
                Ok(sequence) => sequence,
                Err(_) => return KeyResolution::NoMatch,
            };

            let exact_resolution = self.resolve_sequence(&sequence, context);
            if !matches!(exact_resolution, KeyResolution::NoMatch) {
                return exact_resolution;
            }

            if let Some(pending_state) = self.pending_state(&sequence, context) {
                return KeyResolution::PendingSequence(pending_state);
            }

            if self
                .prefix_candidates(&pending_sequence, context)
                .iter()
                .any(|binding| binding.text_fallback)
            {
                if let Some(text) = sequence.fallback_text() {
                    return KeyResolution::ReplayAsText(text);
                }
            }

            return KeyResolution::NoMatch;
        }

        if context.leader_pending {
            let sequence = match KeySequence::new(vec![self.leader])
                .and_then(|sequence| sequence.with_appended_event(event))
            {
                Ok(sequence) => sequence,
                Err(_) => return KeyResolution::NoMatch,
            };

            let exact_resolution = self.resolve_sequence(&sequence, context);
            if !matches!(exact_resolution, KeyResolution::NoMatch) {
                return exact_resolution;
            }

            if let Some(pending_state) = self.pending_state(&sequence, context) {
                return KeyResolution::PendingSequence(pending_state);
            }

            return KeyResolution::NoMatch;
        }

        match KeyStroke::from_event(event) {
            Ok(stroke) if stroke == self.leader && context.mode == InteractionMode::Normal => {
                let sequence =
                    KeySequence::new(vec![self.leader]).expect("single leader key should parse");
                if !self.prefix_candidates(&sequence, context).is_empty() {
                    KeyResolution::PendingLeader
                } else {
                    self.resolve_sequence(&sequence, context)
                }
            }
            Ok(_) => match KeySequence::from_event(event) {
                Ok(sequence) => {
                    let exact_resolution = self.resolve_sequence(&sequence, context);
                    if !matches!(exact_resolution, KeyResolution::NoMatch) {
                        exact_resolution
                    } else if let Some(pending_state) = self.pending_state(&sequence, context) {
                        KeyResolution::PendingSequence(pending_state)
                    } else {
                        KeyResolution::NoMatch
                    }
                }
                Err(_) => KeyResolution::NoMatch,
            },
            Err(_) => KeyResolution::NoMatch,
        }
    }

    pub fn leader_timeout(&self) -> Duration {
        self.leader_timeout
    }

    pub fn default_keymap_toml() -> &'static str {
        DEFAULT_KEYMAP_TOML
    }

    pub fn source_label(&self) -> String {
        match &self.source {
            KeymapSource::BuiltIn => "builtin".to_string(),
            KeymapSource::File(path) => path.display().to_string(),
        }
    }

    fn resolve_sequence(&self, sequence: &KeySequence, context: &KeyContext) -> KeyResolution {
        let mut candidates: Vec<&KeyBinding> = self
            .bindings
            .iter()
            .filter(|binding| binding.sequence == *sequence)
            .collect();

        if candidates.is_empty() {
            return KeyResolution::NoMatch;
        }

        candidates.sort_by_key(|binding| std::cmp::Reverse(binding.specificity()));

        if let Some(binding) = candidates
            .iter()
            .copied()
            .find(|binding| binding.matches_context(context))
        {
            return KeyResolution::Matched(binding.action);
        }

        KeyResolution::InvalidInContext(candidates[0].action)
    }

    fn prefix_candidates<'a>(
        &'a self,
        prefix: &KeySequence,
        context: &KeyContext,
    ) -> Vec<&'a KeyBinding> {
        self.bindings
            .iter()
            .filter(|binding| {
                binding.sequence != *prefix
                    && binding.sequence.starts_with(prefix)
                    && binding.mode.is_none_or(|mode| mode == context.mode)
                    && binding.focus.is_none_or(|focus| focus == context.focus)
            })
            .collect()
    }

    fn pending_state(
        &self,
        sequence: &KeySequence,
        context: &KeyContext,
    ) -> Option<PendingSequenceState> {
        let candidates = self.prefix_candidates(sequence, context);
        if candidates.is_empty() {
            return None;
        }

        let replay_text = if candidates.iter().any(|binding| binding.text_fallback) {
            sequence.fallback_text()
        } else {
            None
        };
        let timeout = candidates
            .iter()
            .map(|binding| binding.effective_timeout(self.leader_timeout))
            .min()
            .unwrap_or(self.leader_timeout);

        Some(PendingSequenceState {
            replay_text,
            timeout,
        })
    }

    fn parse_keymap_str(raw: &str, source: KeymapSource) -> Result<Self> {
        let file = Self::parse_keymap_file(raw)?;
        Self::compile_keymap(file, source)
    }

    fn parse_keymap_file(raw: &str) -> Result<KeymapFile> {
        toml::from_str(raw).map_err(Into::into)
    }

    fn compile_keymap(file: KeymapFile, source: KeymapSource) -> Result<Self> {
        let (leader, leader_timeout) = match file.leader {
            Some(config) => {
                let key = KeyStroke::parse(&config.key)?;
                let timeout_ms = config.timeout_ms.unwrap_or(DEFAULT_LEADER_TIMEOUT_MS);
                (key, Duration::from_millis(timeout_ms))
            }
            None => (
                builtin_leader(),
                Duration::from_millis(DEFAULT_LEADER_TIMEOUT_MS),
            ),
        };

        let bindings = file
            .bindings
            .unwrap_or_default()
            .into_iter()
            .map(|binding| {
                Ok::<KeyBinding, anyhow::Error>(KeyBinding {
                    sequence: KeySequence::parse(&binding.keys, leader)?,
                    action: binding.action,
                    mode: binding.mode,
                    focus: binding.focus,
                    input_capable: binding.input_capable,
                    text_fallback: binding.text_fallback.unwrap_or(false),
                    timeout: binding.timeout_ms.map(Duration::from_millis),
                })
            })
            .collect::<Result<Vec<_>>>()?;

        validate_bindings(&bindings)?;

        Ok(Self {
            leader,
            leader_timeout,
            bindings,
            source,
        })
    }

    fn write_default_file(path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create keymap directory {}", parent.display())
            })?;
        }

        fs::write(path, DEFAULT_KEYMAP_TOML)
            .with_context(|| format!("failed to write default keymap file {}", path.display()))
    }
}

impl Default for KeymapManager {
    fn default() -> Self {
        Self::builtin()
    }
}

fn builtin_leader() -> KeyStroke {
    KeyStroke {
        code: KeyCodePattern::Char(' '),
        modifiers: KeyModifiers::NONE,
    }
}

fn validate_bindings(bindings: &[KeyBinding]) -> Result<()> {
    let mut seen = HashSet::new();

    for binding in bindings {
        if binding.action == KeyAction::EnterInsertMode && binding.input_capable != Some(true) {
            bail!(
                "binding '{}' must set input_capable = true for enter_insert_mode",
                binding.action.as_str()
            );
        }

        if binding.mode == Some(InteractionMode::Insert)
            && binding.sequence.len() > 1
            && binding.sequence.fallback_text().is_some()
            && !binding.text_fallback
        {
            bail!(
                "insert-mode multi-key binding '{}' must set text_fallback = true",
                binding.action.as_str()
            );
        }

        if binding.text_fallback && binding.sequence.fallback_text().is_none() {
            bail!(
                "binding '{}' cannot use text_fallback because its sequence contains non-text keys",
                binding.action.as_str()
            );
        }

        let duplicate_key = (
            binding.sequence.clone(),
            binding.mode,
            binding.focus,
            binding.input_capable,
        );

        if !seen.insert(duplicate_key) {
            bail!("duplicate binding for sequence conflict");
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Deserialize)]
struct KeymapFile {
    leader: Option<LeaderConfig>,
    bindings: Option<Vec<KeyBindingConfig>>,
}

#[derive(Debug, Clone, Deserialize)]
struct LeaderConfig {
    key: String,
    timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
struct KeyBindingConfig {
    keys: String,
    action: KeyAction,
    mode: Option<InteractionMode>,
    focus: Option<KeyFocus>,
    input_capable: Option<bool>,
    text_fallback: Option<bool>,
    timeout_ms: Option<u64>,
}

fn merge_keymap_files(mut builtin: KeymapFile, custom: KeymapFile) -> KeymapFile {
    if custom.leader.is_some() {
        builtin.leader = custom.leader;
    }

    let mut bindings = builtin.bindings.take().unwrap_or_default();
    for binding in custom.bindings.unwrap_or_default() {
        bindings.retain(|existing| {
            !(existing.keys == binding.keys
                && existing.mode == binding.mode
                && existing.focus == binding.focus
                && existing.input_capable == binding.input_capable)
        });
        bindings.push(binding);
    }
    builtin.bindings = Some(bindings);
    builtin
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEventKind, KeyEventState};

    fn press(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    fn temp_root(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("omega-keymap-{}-{}", name, std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn missing_file_uses_builtin_defaults() {
        let root = temp_root("missing");

        let loaded = KeymapManager::load(&root);
        let written = fs::read_to_string(root.join(DEFAULT_KEYMAP_PATH)).unwrap();

        assert!(loaded.warning.is_none());
        assert_eq!(
            loaded.manager.source,
            KeymapSource::File(root.join(DEFAULT_KEYMAP_PATH))
        );
        assert_eq!(written, KeymapManager::default_keymap_toml());
    }

    #[test]
    fn invalid_file_falls_back_to_builtin_defaults() {
        let root = temp_root("invalid");
        let omega_dir = root.join(".omega");
        fs::create_dir_all(&omega_dir).unwrap();
        fs::write(
            omega_dir.join("keymap.toml"),
            "[[bindings]]\naction = \"quit\"\n",
        )
        .unwrap();

        let loaded = KeymapManager::load(&root);

        assert!(loaded.warning.is_some());
        assert_eq!(loaded.manager.source, KeymapSource::BuiltIn);
    }

    #[test]
    fn load_from_file_overrides_leader_key() {
        let root = temp_root("leader");
        let omega_dir = root.join(".omega");
        fs::create_dir_all(&omega_dir).unwrap();
        fs::write(
            omega_dir.join("keymap.toml"),
            "[leader]\nkey = \"g\"\ntimeout_ms = 1200\n\n[[bindings]]\nkeys = \"leader j k\"\naction = \"enter_insert_mode\"\nmode = \"normal\"\ninput_capable = true\n",
        )
        .unwrap();

        let manager = KeymapManager::load_from_file(&omega_dir.join("keymap.toml")).unwrap();
        let context = KeyContext {
            mode: InteractionMode::Normal,
            focus: KeyFocus::Response,
            input_capable: true,
            leader_pending: false,
        };

        assert_eq!(manager.leader_timeout(), Duration::from_millis(1200));
        assert_eq!(
            manager.resolve(&context, press(KeyCode::Char('g'), KeyModifiers::NONE)),
            KeyResolution::PendingLeader
        );
    }

    #[test]
    fn enter_insert_mode_requires_input_capable_flag() {
        let root = temp_root("validation");
        let omega_dir = root.join(".omega");
        fs::create_dir_all(&omega_dir).unwrap();
        fs::write(
            omega_dir.join("keymap.toml"),
            "[[bindings]]\nkeys = \"leader k\"\naction = \"enter_insert_mode\"\nmode = \"normal\"\n",
        )
        .unwrap();

        let error = KeymapManager::load_from_file(&omega_dir.join("keymap.toml")).unwrap_err();

        assert!(error.to_string().contains("input_capable = true"));
    }

    #[test]
    fn more_specific_binding_wins() {
        let manager = KeymapManager::parse_keymap_str(
            "[leader]\nkey = \"space\"\n\n[[bindings]]\nkeys = \"leader tab\"\naction = \"focus_next_panel\"\nmode = \"normal\"\n\n[[bindings]]\nkeys = \"leader tab\"\naction = \"toggle_sidebar\"\nmode = \"normal\"\nfocus = \"logs\"\n",
            KeymapSource::BuiltIn,
        )
        .unwrap();

        let resolution = manager.resolve(
            &KeyContext {
                mode: InteractionMode::Normal,
                focus: KeyFocus::Logs,
                input_capable: true,
                leader_pending: true,
            },
            press(KeyCode::Tab, KeyModifiers::NONE),
        );

        assert_eq!(resolution, KeyResolution::Matched(KeyAction::ToggleSidebar));
    }

    #[test]
    fn default_keymap_moves_normal_shortcuts_under_leader() {
        let manager = KeymapManager::default();
        let context = KeyContext {
            mode: InteractionMode::Normal,
            focus: KeyFocus::Response,
            input_capable: true,
            leader_pending: false,
        };

        assert_eq!(
            manager.resolve(&context, press(KeyCode::Tab, KeyModifiers::NONE)),
            KeyResolution::NoMatch
        );
        assert_eq!(
            manager.resolve(&context, press(KeyCode::Char(' '), KeyModifiers::NONE)),
            KeyResolution::PendingLeader
        );
        assert_eq!(
            manager.resolve_with_pending(
                &context,
                &[press(KeyCode::Char(' '), KeyModifiers::NONE)],
                press(KeyCode::Tab, KeyModifiers::NONE)
            ),
            KeyResolution::Matched(KeyAction::FocusNextPanel)
        );
    }

    #[test]
    fn insert_mode_space_starts_replayable_pending_sequence() {
        let manager = KeymapManager::default();
        let context = KeyContext {
            mode: InteractionMode::Insert,
            focus: KeyFocus::InputField,
            input_capable: true,
            leader_pending: false,
        };

        assert_eq!(
            manager.resolve(&context, press(KeyCode::Char(' '), KeyModifiers::NONE)),
            KeyResolution::PendingSequence(PendingSequenceState {
                replay_text: Some(" ".to_string()),
                timeout: Duration::from_millis(DEFAULT_LEADER_TIMEOUT_MS),
            })
        );
        assert_eq!(
            manager.resolve(&context, press(KeyCode::Esc, KeyModifiers::NONE)),
            KeyResolution::Matched(KeyAction::EnterNormalMode)
        );
    }

    #[test]
    fn leader_jk_is_pending_after_j_and_matches_on_k() {
        let manager = KeymapManager::default();
        let context = KeyContext {
            mode: InteractionMode::Normal,
            focus: KeyFocus::Response,
            input_capable: true,
            leader_pending: false,
        };

        assert_eq!(
            manager.resolve_with_pending(
                &context,
                &[press(KeyCode::Char(' '), KeyModifiers::NONE)],
                press(KeyCode::Char('j'), KeyModifiers::NONE)
            ),
            KeyResolution::PendingSequence(PendingSequenceState {
                replay_text: None,
                timeout: Duration::from_millis(DEFAULT_LEADER_TIMEOUT_MS),
            })
        );
        assert_eq!(
            manager.resolve_with_pending(
                &context,
                &[
                    press(KeyCode::Char(' '), KeyModifiers::NONE),
                    press(KeyCode::Char('j'), KeyModifiers::NONE)
                ],
                press(KeyCode::Char('k'), KeyModifiers::NONE)
            ),
            KeyResolution::Matched(KeyAction::EnterInsertMode)
        );
    }

    #[test]
    fn toggle_mode_is_invalid_when_insert_cannot_be_entered() {
        let manager = KeymapManager::default();
        let resolution = manager.resolve_with_pending(
            &KeyContext {
                mode: InteractionMode::Normal,
                focus: KeyFocus::Response,
                input_capable: false,
                leader_pending: false,
            },
            &[
                press(KeyCode::Char(' '), KeyModifiers::NONE),
                press(KeyCode::Char('j'), KeyModifiers::NONE),
            ],
            press(KeyCode::Char('k'), KeyModifiers::NONE),
        );

        assert_eq!(
            resolution,
            KeyResolution::InvalidInContext(KeyAction::EnterInsertMode)
        );
    }

    #[test]
    fn insert_prefix_replays_text_when_sequence_breaks() {
        let manager = KeymapManager::default();
        let context = KeyContext {
            mode: InteractionMode::Insert,
            focus: KeyFocus::InputField,
            input_capable: true,
            leader_pending: false,
        };

        assert_eq!(
            manager.resolve_with_pending(
                &context,
                &[press(KeyCode::Char(' '), KeyModifiers::NONE)],
                press(KeyCode::Char('a'), KeyModifiers::NONE)
            ),
            KeyResolution::ReplayAsText(" a".to_string())
        );
    }

    #[test]
    fn default_keymap_maps_shift_enter_to_insert_newline() {
        let manager = KeymapManager::default();
        let context = KeyContext {
            mode: InteractionMode::Insert,
            focus: KeyFocus::InputField,
            input_capable: true,
            leader_pending: false,
        };

        assert_eq!(
            manager.resolve(&context, press(KeyCode::Enter, KeyModifiers::SHIFT)),
            KeyResolution::Matched(KeyAction::InsertNewline)
        );
    }

    #[test]
    fn default_keymap_maps_arrow_keys_to_vertical_cursor_motion() {
        let manager = KeymapManager::default();
        let context = KeyContext {
            mode: InteractionMode::Insert,
            focus: KeyFocus::InputField,
            input_capable: true,
            leader_pending: false,
        };

        assert_eq!(
            manager.resolve(&context, press(KeyCode::Up, KeyModifiers::NONE)),
            KeyResolution::Matched(KeyAction::MoveCursorUp)
        );
        assert_eq!(
            manager.resolve(&context, press(KeyCode::Down, KeyModifiers::NONE)),
            KeyResolution::Matched(KeyAction::MoveCursorDown)
        );
    }

    #[test]
    fn stale_workspace_keymap_inherits_new_builtin_bindings() {
        let root = temp_root("stale-default-overlay");
        let omega_dir = root.join(".omega");
        fs::create_dir_all(&omega_dir).unwrap();
        fs::write(
            omega_dir.join("keymap.toml"),
            "# Older workspace keymap without newer defaults\n\n[leader]\nkey = \"space\"\ntimeout_ms = 300\n\n[[bindings]]\nkeys = \"esc\"\naction = \"enter_normal_mode\"\nmode = \"insert\"\n\n[[bindings]]\nkeys = \"left\"\naction = \"move_cursor_left\"\nmode = \"insert\"\n\n[[bindings]]\nkeys = \"right\"\naction = \"move_cursor_right\"\nmode = \"insert\"\n\n[[bindings]]\nkeys = \"enter\"\naction = \"submit_input\"\nmode = \"insert\"\ninput_capable = true\n",
        )
        .unwrap();

        let manager = KeymapManager::load_from_file(&omega_dir.join("keymap.toml")).unwrap();
        let context = KeyContext {
            mode: InteractionMode::Insert,
            focus: KeyFocus::InputField,
            input_capable: true,
            leader_pending: false,
        };

        assert_eq!(
            manager.resolve(&context, press(KeyCode::Up, KeyModifiers::NONE)),
            KeyResolution::Matched(KeyAction::MoveCursorUp)
        );
        assert_eq!(
            manager.resolve(&context, press(KeyCode::Down, KeyModifiers::NONE)),
            KeyResolution::Matched(KeyAction::MoveCursorDown)
        );
        assert_eq!(
            manager.resolve(&context, press(KeyCode::Enter, KeyModifiers::SHIFT)),
            KeyResolution::Matched(KeyAction::InsertNewline)
        );
    }

    #[test]
    fn insert_mode_multi_key_bindings_require_text_fallback() {
        let root = temp_root("insert-fallback-validation");
        let omega_dir = root.join(".omega");
        fs::create_dir_all(&omega_dir).unwrap();
        fs::write(
            omega_dir.join("keymap.toml"),
            "[[bindings]]\nkeys = \"j k\"\naction = \"enter_normal_mode\"\nmode = \"insert\"\n",
        )
        .unwrap();

        let error = KeymapManager::load_from_file(&omega_dir.join("keymap.toml")).unwrap_err();

        assert!(error.to_string().contains("text_fallback = true"));
    }

    #[test]
    fn insert_prefix_uses_binding_specific_timeout() {
        let manager = KeymapManager::parse_keymap_str(
            "[leader]\nkey = \"space\"\ntimeout_ms = 900\n\n[[bindings]]\nkeys = \"j k\"\naction = \"enter_normal_mode\"\nmode = \"insert\"\ntext_fallback = true\ntimeout_ms = 120\n",
            KeymapSource::BuiltIn,
        )
        .unwrap();
        let context = KeyContext {
            mode: InteractionMode::Insert,
            focus: KeyFocus::InputField,
            input_capable: true,
            leader_pending: false,
        };

        assert_eq!(
            manager.resolve(&context, press(KeyCode::Char('j'), KeyModifiers::NONE)),
            KeyResolution::PendingSequence(PendingSequenceState {
                replay_text: Some("j".to_string()),
                timeout: Duration::from_millis(120),
            })
        );
    }
}
