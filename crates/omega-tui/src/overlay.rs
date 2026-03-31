use ratatui::layout::Rect;

use crate::app::Panel;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlaySize {
    Small,
    Medium,
    Large,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmChoice {
    Cancel,
    Confirm,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmIntent {
    InterruptTurn { turn_id: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchOverlay {
    pub origin_panel: Panel,
    pub target_panel: Panel,
    pub query: String,
    pub cursor_pos: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResultsOverlay {
    pub origin_panel: Panel,
    pub title: String,
    pub lines: Vec<String>,
    pub scroll: usize,
    pub dismiss_on_backdrop: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmOverlay {
    pub origin_panel: Panel,
    pub title: String,
    pub message: String,
    pub confirm_label: String,
    pub cancel_label: String,
    pub selected: ConfirmChoice,
    pub intent: ConfirmIntent,
    pub dismiss_on_backdrop: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetailOverlay {
    pub origin_panel: Panel,
    pub title: String,
    pub lines: Vec<String>,
    pub scroll: usize,
    pub dismiss_on_backdrop: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickerOverlay {
    pub origin_panel: Panel,
    pub title: String,
    pub items: Vec<String>,
    pub selected: usize,
    pub dismiss_on_backdrop: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputPromptOverlay {
    pub origin_panel: Panel,
    pub title: String,
    pub prompt: String,
    pub value: String,
    pub cursor_pos: usize,
    pub dismiss_on_backdrop: bool,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverlayState {
    Search(SearchOverlay),
    SearchResults(SearchResultsOverlay),
    Confirm(ConfirmOverlay),
    Detail(DetailOverlay),
    Picker(PickerOverlay),
    InputPrompt(InputPromptOverlay),
}

impl OverlayState {
    pub fn origin_panel(&self) -> Panel {
        match self {
            Self::Search(overlay) => overlay.origin_panel,
            Self::SearchResults(overlay) => overlay.origin_panel,
            Self::Confirm(overlay) => overlay.origin_panel,
            Self::Detail(overlay) => overlay.origin_panel,
            Self::Picker(overlay) => overlay.origin_panel,
            Self::InputPrompt(overlay) => overlay.origin_panel,
        }
    }

    pub fn size(&self) -> OverlaySize {
        match self {
            Self::Search(_) => OverlaySize::Small,
            Self::SearchResults(_) => OverlaySize::Large,
            Self::Confirm(_) => OverlaySize::Small,
            Self::Detail(_) => OverlaySize::Large,
            Self::Picker(_) => OverlaySize::Medium,
            Self::InputPrompt(_) => OverlaySize::Small,
        }
    }

    pub fn dismiss_on_backdrop(&self) -> bool {
        match self {
            Self::Search(_) => true,
            Self::SearchResults(overlay) => overlay.dismiss_on_backdrop,
            Self::Confirm(overlay) => overlay.dismiss_on_backdrop,
            Self::Detail(overlay) => overlay.dismiss_on_backdrop,
            Self::Picker(overlay) => overlay.dismiss_on_backdrop,
            Self::InputPrompt(overlay) => overlay.dismiss_on_backdrop,
        }
    }
}

pub fn overlay_area(area: Rect, size: OverlaySize) -> Rect {
    let (width_pct, height_pct, min_width, min_height) = match size {
        OverlaySize::Small => (52, 28, 36, 7),
        OverlaySize::Medium => (68, 42, 50, 10),
        OverlaySize::Large => (82, 60, 64, 14),
    };

    let max_width = area.width.saturating_sub(2).max(1);
    let max_height = area.height.saturating_sub(2).max(1);
    let width = ((area.width.saturating_mul(width_pct)) / 100)
        .max(min_width)
        .min(max_width);
    let height = ((area.height.saturating_mul(height_pct)) / 100)
        .max(min_height)
        .min(max_height);
    let x = area.x.saturating_add(area.width.saturating_sub(width) / 2);
    let y = area
        .y
        .saturating_add(area.height.saturating_sub(height) / 2);

    Rect::new(x, y, width, height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_area_shrinks_inside_narrow_terminal() {
        let area = overlay_area(Rect::new(0, 0, 40, 12), OverlaySize::Large);

        assert!(area.width <= 38);
        assert!(area.height <= 10);
        assert!(area.width > 0);
        assert!(area.height > 0);
    }
}
