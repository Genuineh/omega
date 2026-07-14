//! Chrome — the user-visible panel titles and decoration glyphs.
//!
//! Centralised so changing a panel name or swapping the focus marker is a
//! single-file edit instead of a global find-and-replace. See
//! `docs/TODO.md` Task 39B.
//!
//! Chrome is split into three groups:
//! - [`PanelTitle`]: the visible titles of the four primary panels.
//! - [`Glyph`]: decoration symbols that carry meaning in the UI
//!   (focused, running, complete, failed).
//! - [`Text`]: short text fragments used in chrome contexts (status
//!   hints, mode labels) where the literal matters for layout.
//!
//! Not all glyphs in the renderer live here yet — the per-message symbols
//! in `app/response.rs` are still string literals in their match arms.
//! Those will move here as part of Task 39G (split `app/response.rs`).

/// The four primary panel titles. Callers should always go through these
/// constants rather than inlining the string.
pub struct PanelTitle;

impl PanelTitle {
    pub const RESPONSE: &'static str = "Agent Response";
    pub const SIDEBAR: &'static str = "Sidebar";
}

/// Decoration glyphs with a single semantic role.
pub struct Glyph;

impl Glyph {
    /// Filled diamond used as the focused-panel marker in panel titles.
    /// Also used as the routing/scene focus indicator.
    pub const FOCUS: char = '◆';
    /// Filled circle, "complete" state.
    pub const COMPLETE: char = '●';
    /// Bullseye, "running" / in-progress state.
    pub const RUNNING: char = '◉';
    /// Cross, "failed" / error state.
    pub const FAILED: char = '✕';
    /// Hollow circle, "pending" / not-started state.
    pub const PENDING: char = '○';
    /// Hollow dot, "placeholder" state.
    pub const PLACEHOLDER: char = '◦';
    /// Middle dot, "bullet" used in lists.
    pub const BULLET: char = '·';
    /// Half-filled circle, "active" / spinner state (T-69).
    /// Used for Steps that are currently streaming in the chat
    /// log so the user can see work is happening.
    pub const ACTIVE: char = '◐';

    /// All glyphs as a static slice, useful for the spinner / orbit ring
    /// animation in the status bar.
    pub const ORBIT: [char; 5] = [
        Self::COMPLETE,
        Self::RUNNING,
        '◎',
        Self::PENDING,
        Self::BULLET,
    ];
}

/// Build a panel title that includes the focus marker when the panel is
/// focused. Use this everywhere a panel title is rendered so the marker
/// placement (leading or trailing, padded or not) is consistent.
pub fn panel_title_with_focus(base: &str, focused: bool) -> String {
    if focused {
        format!(" {} {} ", base, Glyph::FOCUS)
    } else {
        format!(" {} ", base)
    }
}

/// Build a panel title where the focus marker leads. Used by the sidebar
/// header style, which renders the marker after the title in the original
/// code.
pub fn panel_title_with_focus_suffix(base: &str, focused: bool) -> String {
    if focused {
        format!("{}{}", base, Glyph::FOCUS)
    } else {
        base.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_marker_in_focused_title() {
        assert!(panel_title_with_focus("Agent Response", true).contains('◆'));
        assert!(panel_title_with_focus("Agent Response", false).contains("Agent Response"));
        assert!(!panel_title_with_focus("Agent Response", false).contains('◆'));
    }

    #[test]
    fn focus_marker_suffix_form() {
        assert_eq!(panel_title_with_focus_suffix("Sidebar", true), "Sidebar◆");
        assert_eq!(panel_title_with_focus_suffix("Sidebar", false), "Sidebar");
    }

    #[test]
    fn glyph_set_is_stable() {
        assert_eq!(Glyph::FOCUS, '◆');
        assert_eq!(Glyph::COMPLETE, '●');
        assert_eq!(Glyph::RUNNING, '◉');
        assert_eq!(Glyph::FAILED, '✕');
        assert_eq!(Glyph::ORBIT.len(), 5);
    }
}
