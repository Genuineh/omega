use ratatui::layout::Rect;

use omega_session::{
    OperatorPickerAction, OperatorPickerItem, OperatorPickerRequest, OperatorPickerShortcut,
};

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
    Dismiss,
    InterruptTurn { turn_id: u64 },
    SubmitSlashCommand { command: String },
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
    pub request: OperatorPickerRequest,
    pub visible_item_indices: Vec<usize>,
    pub filter_query: String,
    pub filter_cursor_pos: usize,
    pub filter_mode: bool,
    pub selected: usize,
    pub dismiss_on_backdrop: bool,
}

impl PickerOverlay {
    pub fn new(origin_panel: Panel, request: OperatorPickerRequest) -> Self {
        let mut overlay = Self {
            origin_panel,
            request,
            visible_item_indices: Vec::new(),
            filter_query: String::new(),
            filter_cursor_pos: 0,
            filter_mode: false,
            selected: 0,
            dismiss_on_backdrop: true,
        };
        overlay.refresh_visible_items();
        overlay
    }

    pub fn title(&self) -> &str {
        self.request.title.as_str()
    }

    pub fn filter_enabled(&self) -> bool {
        self.request.filter_enabled
    }

    pub fn visible_item(&self, visible_index: usize) -> Option<&OperatorPickerItem> {
        let item_index = *self.visible_item_indices.get(visible_index)?;
        self.request.items.get(item_index)
    }

    pub fn selected_item(&self) -> Option<&OperatorPickerItem> {
        self.visible_item(self.selected)
    }

    pub fn visible_items_len(&self) -> usize {
        self.visible_item_indices.len()
    }

    pub fn move_selection_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
        self.filter_mode = false;
    }

    pub fn move_selection_down(&mut self) {
        if !self.visible_item_indices.is_empty() {
            self.selected = (self.selected + 1).min(self.visible_item_indices.len() - 1);
        }
        self.filter_mode = false;
    }

    pub fn action_for_shortcut(
        &self,
        shortcut: OperatorPickerShortcut,
    ) -> Option<&OperatorPickerAction> {
        if self.request.primary_action.shortcut == shortcut {
            return Some(&self.request.primary_action);
        }

        self.request
            .secondary_actions
            .iter()
            .find(|action| action.shortcut == shortcut)
    }

    pub fn footer_hints(&self) -> Vec<String> {
        let mut hints = vec![format!(
            "{}={}",
            self.request.primary_action.shortcut.hint_label(),
            self.request.primary_action.label
        )];
        hints.extend(self.request.secondary_actions.iter().map(|action| {
            format!("{}={}", action.shortcut.hint_label(), action.label)
        }));
        if self.request.filter_enabled {
            hints.push("/=Filter".to_string());
        }
        hints.push("Esc=Close".to_string());
        hints
    }

    pub fn empty_state_text(&self) -> String {
        if self.filter_query.trim().is_empty() {
            self.request.empty_state.clone()
        } else {
            format!("No matches for '{}'.", self.filter_query)
        }
    }

    pub fn enter_filter_mode(&mut self) {
        if self.request.filter_enabled {
            self.filter_mode = true;
        }
    }

    pub fn apply_filter_query(&mut self) {
        self.refresh_visible_items();
        self.filter_mode = true;
    }

    fn refresh_visible_items(&mut self) {
        let query = self.filter_query.trim().to_ascii_lowercase();
        self.visible_item_indices = self
            .request
            .items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| {
                if query.is_empty() || picker_item_filter_text(item).contains(&query) {
                    Some(index)
                } else {
                    None
                }
            })
            .collect();

        if self.visible_item_indices.is_empty() {
            self.selected = 0;
        } else {
            self.selected = self.selected.min(self.visible_item_indices.len() - 1);
        }
    }
}

fn picker_item_filter_text(item: &OperatorPickerItem) -> String {
    let mut parts = vec![item.id.clone(), item.title.clone()];
    if let Some(subtitle) = item.subtitle.as_deref() {
        parts.push(subtitle.to_string());
    }
    if let Some(preview) = item.preview.as_deref() {
        parts.push(preview.to_string());
    }
    if let Some(disabled_reason) = item.disabled_reason.as_deref() {
        parts.push(disabled_reason.to_string());
    }
    parts.extend(item.badges.iter().cloned());
    parts.join(" ").to_ascii_lowercase()
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
