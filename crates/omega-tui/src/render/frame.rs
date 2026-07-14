//! Per-frame `Frame` — the layout rectangles computed during `render()`.
//!
//! `App` exposes a `*_rect` field for every visible panel; the render path
//! writes those fields, and event handlers read them to route clicks and
//! focus. This shared mutable state between the render path and the input
//! path is the implicit coupling Task 39F set out to break.
//!
//! `Frame` is the per-frame replacement: `render()` builds a `Frame`,
//! attaches it to `App` via `App::set_frame`, and event handlers retrieve
//! it via `App::frame()`. The legacy `*_rect` fields on `App` remain for
//! now (to avoid touching ~80 call sites in one pass) but are deprecated
//! in favour of `app.frame().response_rect` etc.
//!
//! See `docs/TODO.md` Task 39F for the plan.

use ratatui::layout::Rect;

/// Per-frame layout rectangles. Constructed once in `render()` and held
/// on `App` until the next render. All `Rect` fields default to
/// `Rect::default()` (zero-area) when no frame has been recorded yet, so
/// event handlers that read them pre-render get a benign "no hit"
/// rectangle.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Frame {
    pub response_rect: Rect,
    pub input_context_rect: Rect,
    pub input_gap_rect: Rect,
    pub input_rect: Rect,
    pub input_info_rect: Rect,
    pub sidebar_rect: Rect,
    pub sidebar_rail_rect: Rect,
    pub diagnostics_rect: Rect,
    pub delivery_rect: Rect,
    pub skills_rect: Rect,
    pub project_rect: Rect,
    pub document_rect: Rect,
    pub memory_rect: Rect,
    pub todo_rect: Rect,
    pub logs_rect: Rect,
    pub bottom_status_rect: Rect,
}

impl Frame {
    /// Replace all rects in this frame with the values from another
    /// `Frame`. Used by render when the per-frame `FrameLayout` is split
    /// into per-panel rects that the rest of the app reads.
    pub fn copy_from(&mut self, other: &Frame) {
        *self = other.clone();
    }

    /// Build a `Frame` from the computed `FrameLayout`. The sub-rects
    /// (input/rail/sidebar sections) are not part of the layout struct
    /// and default to zero-area here; the per-panel render functions
    /// mutate them in place via [`Frame::copy_from`] once the panel
    /// builder computes them.
    pub fn from_layout(layout: &crate::render::layout::FrameLayout) -> Self {
        Frame {
            response_rect: layout.response_rect,
            input_context_rect: layout.input_context_rect,
            input_gap_rect: ratatui::layout::Rect::default(),
            input_rect: ratatui::layout::Rect::default(),
            input_info_rect: ratatui::layout::Rect::default(),
            sidebar_rect: layout.sidebar_rect,
            sidebar_rail_rect: ratatui::layout::Rect::default(),
            diagnostics_rect: ratatui::layout::Rect::default(),
            delivery_rect: ratatui::layout::Rect::default(),
            skills_rect: ratatui::layout::Rect::default(),
            project_rect: ratatui::layout::Rect::default(),
            document_rect: ratatui::layout::Rect::default(),
            memory_rect: ratatui::layout::Rect::default(),
            todo_rect: ratatui::layout::Rect::default(),
            logs_rect: ratatui::layout::Rect::default(),
            bottom_status_rect: layout.status_rect,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_frame_is_all_zero() {
        let frame = Frame::default();
        assert_eq!(frame.response_rect, Rect::default());
        assert_eq!(frame.sidebar_rect, Rect::default());
        assert_eq!(frame.bottom_status_rect, Rect::default());
    }

    #[test]
    fn copy_from_overwrites_all_rects() {
        let mut frame = Frame::default();
        let mut other = Frame::default();
        other.response_rect = Rect::new(0, 0, 80, 24);
        other.sidebar_rect = Rect::new(80, 0, 20, 24);
        frame.copy_from(&other);
        assert_eq!(frame.response_rect, Rect::new(0, 0, 80, 24));
        assert_eq!(frame.sidebar_rect, Rect::new(80, 0, 20, 24));
    }
}
