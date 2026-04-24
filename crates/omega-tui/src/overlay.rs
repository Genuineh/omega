use ratatui::layout::Rect;

use omega_session::{
    DocumentNavigatorEntry, DocumentNavigatorEntryKind, DocumentNavigatorGroup,
    DocumentNavigatorRequest, OperatorPickerAction, OperatorPickerItem, OperatorPickerRequest,
    OperatorPickerShortcut,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentNavigatorFocus {
    Rail,
    Content,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentNavigatorRailItem {
    pub id: String,
    pub group: DocumentNavigatorGroup,
    pub label: String,
    pub subtitle: Option<String>,
    pub kind: DocumentNavigatorEntryKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentNavigatorOverlay {
    pub origin_panel: Panel,
    pub request: DocumentNavigatorRequest,
    pub selected: usize,
    pub focus: DocumentNavigatorFocus,
    pub content_scroll: usize,
    pub history_entry_ids: Vec<String>,
    pub dismiss_on_backdrop: bool,
}

impl DocumentNavigatorOverlay {
    pub fn new(origin_panel: Panel, request: DocumentNavigatorRequest) -> Self {
        let mut overlay = Self {
            origin_panel,
            request,
            selected: 0,
            focus: DocumentNavigatorFocus::Rail,
            content_scroll: 0,
            history_entry_ids: Vec::new(),
            dismiss_on_backdrop: true,
        };
        overlay.sync_selection_to_active();
        overlay
    }

    pub fn replace_request(&mut self, mut request: DocumentNavigatorRequest) {
        let previous_active = self.request.active_entry_id.clone();
        self.history_entry_ids
            .retain(|id| request.entries.iter().any(|entry| entry.id == *id));
        if !request
            .entries
            .iter()
            .any(|entry| entry.id == request.active_entry_id)
        {
            if request.entries.iter().any(|entry| entry.id == previous_active) {
                request.active_entry_id = previous_active.clone();
            } else {
                request.active_entry_id = request
                    .entries
                    .first()
                    .map(|entry| entry.id.clone())
                    .unwrap_or_default();
            }
        }
        if previous_active != request.active_entry_id && !previous_active.is_empty() {
            self.history_entry_ids.retain(|id| id != &previous_active);
            self.history_entry_ids.push(previous_active);
            if self.history_entry_ids.len() > 5 {
                let overflow = self.history_entry_ids.len() - 5;
                self.history_entry_ids.drain(0..overflow);
            }
        }
        self.request = request;
        self.content_scroll = 0;
        self.sync_selection_to_active();
    }

    pub fn visible_items(&self) -> Vec<DocumentNavigatorRailItem> {
        let mut items = self
            .request
            .entries
            .iter()
            .filter(|entry| entry.group != DocumentNavigatorGroup::History)
            .map(rail_item_from_entry)
            .collect::<Vec<_>>();

        for entry_id in self.history_entry_ids.iter().rev() {
            if let Some(entry) = self.entry_by_id(entry_id) {
                items.push(DocumentNavigatorRailItem {
                    id: entry.id.clone(),
                    group: DocumentNavigatorGroup::History,
                    label: entry.label.clone(),
                    subtitle: entry.subtitle.clone(),
                    kind: entry.kind,
                });
            }
        }

        items
    }

    pub fn active_entry(&self) -> Option<&DocumentNavigatorEntry> {
        self.entry_by_id(&self.request.active_entry_id)
    }

    pub fn visible_items_len(&self) -> usize {
        self.visible_items().len()
    }

    pub fn move_selection_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn move_selection_down(&mut self) {
        let len = self.visible_items_len();
        if len > 0 {
            self.selected = (self.selected + 1).min(len - 1);
        }
    }

    pub fn move_selection_to_start(&mut self) {
        self.selected = 0;
    }

    pub fn move_selection_to_end(&mut self) {
        let len = self.visible_items_len();
        if len > 0 {
            self.selected = len - 1;
        }
    }

    pub fn move_selection_by(&mut self, amount: usize, forward: bool) {
        if forward {
            let len = self.visible_items_len();
            if len > 0 {
                self.selected = self.selected.saturating_add(amount).min(len - 1);
            }
        } else {
            self.selected = self.selected.saturating_sub(amount);
        }
    }

    pub fn activate_selected(&mut self) {
        let Some(selected_id) = self
            .visible_items()
            .get(self.selected)
            .map(|item| item.id.clone())
        else {
            return;
        };

        if self.request.active_entry_id != selected_id {
            let previous_active = self.request.active_entry_id.clone();
            if !previous_active.is_empty() {
                self.history_entry_ids.retain(|id| id != &previous_active);
                self.history_entry_ids.push(previous_active);
                if self.history_entry_ids.len() > 5 {
                    let overflow = self.history_entry_ids.len() - 5;
                    self.history_entry_ids.drain(0..overflow);
                }
            }
            self.request.active_entry_id = selected_id;
            self.content_scroll = 0;
        }

        self.sync_selection_to_active();
    }

    pub fn set_focus(&mut self, focus: DocumentNavigatorFocus) {
        self.focus = focus;
    }

    pub fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            DocumentNavigatorFocus::Rail => DocumentNavigatorFocus::Content,
            DocumentNavigatorFocus::Content => DocumentNavigatorFocus::Rail,
        };
    }

    pub fn scroll_content_up(&mut self, amount: usize) {
        self.content_scroll = self.content_scroll.saturating_sub(amount);
    }

    pub fn scroll_content_down(&mut self, amount: usize) {
        self.content_scroll = self.content_scroll.saturating_add(amount);
    }

    pub fn scroll_content_to_start(&mut self) {
        self.content_scroll = 0;
    }

    pub fn scroll_content_to_end(&mut self, viewport_lines: usize) {
        self.content_scroll = self
            .active_entry()
            .map(|entry| entry.body.lines.len().saturating_sub(viewport_lines.max(1)))
            .unwrap_or(0);
    }

    fn sync_selection_to_active(&mut self) {
        let active_id = self.request.active_entry_id.clone();
        self.selected = self
            .visible_items()
            .iter()
            .position(|item| item.id == active_id)
            .unwrap_or(0);
    }

    fn entry_by_id(&self, id: &str) -> Option<&DocumentNavigatorEntry> {
        self.request.entries.iter().find(|entry| entry.id == id)
    }
}

fn rail_item_from_entry(entry: &DocumentNavigatorEntry) -> DocumentNavigatorRailItem {
    DocumentNavigatorRailItem {
        id: entry.id.clone(),
        group: entry.group,
        label: entry.label.clone(),
        subtitle: entry.subtitle.clone(),
        kind: entry.kind,
    }
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
    DocumentNavigator(DocumentNavigatorOverlay),
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
            Self::DocumentNavigator(overlay) => overlay.origin_panel,
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
            Self::DocumentNavigator(_) => OverlaySize::Large,
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
            Self::DocumentNavigator(overlay) => overlay.dismiss_on_backdrop,
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
