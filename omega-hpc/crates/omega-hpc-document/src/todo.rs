use std::sync::{Arc, Mutex};

use anyhow::Result;
use serde::{Deserialize, Serialize};

pub const MAX_TODO_ITEMS: usize = 20;
pub const TODO_REMINDER: &str = "<reminder>Update your todos.</reminder>";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TodoItem {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, alias = "content")]
    pub text: String,
    pub status: TodoStatus,
    #[serde(default, rename = "activeForm", alias = "active_form")]
    pub active_form: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
}

#[derive(Debug, Clone)]
pub struct TodoManager {
    items: Vec<TodoItem>,
    rounds_without_update: usize,
}

impl TodoManager {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            rounds_without_update: 0,
        }
    }

    pub fn update(&mut self, items: Vec<TodoItem>) -> Result<String> {
        if items.len() > MAX_TODO_ITEMS {
            anyhow::bail!("Max {MAX_TODO_ITEMS} todos allowed");
        }

        let mut in_progress_count = 0usize;
        let mut validated = Vec::with_capacity(items.len());

        for (index, item) in items.into_iter().enumerate() {
            let text = item.text.trim();
            let id = item
                .id
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow::anyhow!("Item {}: id required", index + 1))?;

            if text.is_empty() {
                anyhow::bail!("Item {id}: text required");
            }

            if item.status == TodoStatus::InProgress {
                in_progress_count += 1;
            }

            validated.push(TodoItem {
                id: Some(id),
                text: text.to_string(),
                status: item.status,
                active_form: item
                    .active_form
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty()),
            });
        }

        if in_progress_count > 1 {
            anyhow::bail!("Only one task can be in_progress at a time");
        }

        self.items = validated;
        self.reset_rounds();
        Ok(self.render())
    }

    pub fn render(&self) -> String {
        if self.items.is_empty() {
            return "No todos.".to_string();
        }

        let mut lines: Vec<String> = self
            .items
            .iter()
            .map(|item| {
                let marker = match item.status {
                    TodoStatus::Pending => "[ ]",
                    TodoStatus::InProgress => "[>]",
                    TodoStatus::Completed => "[x]",
                };

                let label = match item.id.as_deref() {
                    Some(id) => format!("#{id}: {}", item.text),
                    None => item.text.clone(),
                };

                let suffix = match (&item.status, item.active_form.as_deref()) {
                    (TodoStatus::InProgress, Some(active_form)) => format!(" <- {active_form}"),
                    _ => String::new(),
                };

                format!("{marker} {label}{suffix}")
            })
            .collect();

        let completed = self
            .items
            .iter()
            .filter(|item| item.status == TodoStatus::Completed)
            .count();
        lines.push(String::new());
        lines.push(format!("({completed}/{} completed)", self.items.len()));

        lines.join("\n")
    }

    pub fn has_open_items(&self) -> bool {
        self.items
            .iter()
            .any(|item| item.status != TodoStatus::Completed)
    }

    pub fn should_nag(&self) -> bool {
        self.has_open_items() && self.rounds_without_update >= 3
    }

    pub fn increment_rounds(&mut self) {
        self.rounds_without_update += 1;
    }

    pub fn reset_rounds(&mut self) {
        self.rounds_without_update = 0;
    }

    pub fn rounds_without_update(&self) -> usize {
        self.rounds_without_update
    }

    pub fn items(&self) -> &[TodoItem] {
        &self.items
    }

    pub fn reminder_text(&self) -> &'static str {
        TODO_REMINDER
    }
}

impl Default for TodoManager {
    fn default() -> Self {
        Self::new()
    }
}

pub type SharedTodoManager = Arc<Mutex<TodoManager>>;

#[cfg(test)]
mod tests {
    use super::*;

    fn pending(id: &str, text: &str) -> TodoItem {
        TodoItem {
            id: Some(id.to_string()),
            text: text.to_string(),
            status: TodoStatus::Pending,
            active_form: None,
        }
    }

    #[test]
    fn update_renders_markers_and_summary() {
        let mut manager = TodoManager::new();
        let rendered = manager
            .update(vec![
                pending("1", "Inspect code"),
                TodoItem {
                    id: Some("2".to_string()),
                    text: "Implement feature".to_string(),
                    status: TodoStatus::InProgress,
                    active_form: Some("implementing feature".to_string()),
                },
                TodoItem {
                    id: Some("3".to_string()),
                    text: "Run tests".to_string(),
                    status: TodoStatus::Completed,
                    active_form: None,
                },
            ])
            .unwrap();

        assert_eq!(
			rendered,
			"[ ] #1: Inspect code\n[>] #2: Implement feature <- implementing feature\n[x] #3: Run tests\n\n(1/3 completed)"
		);
    }

    #[test]
    fn update_rejects_more_than_one_in_progress_item() {
        let mut manager = TodoManager::new();
        let error = manager
            .update(vec![
                TodoItem {
                    id: Some("1".to_string()),
                    text: "One".to_string(),
                    status: TodoStatus::InProgress,
                    active_form: None,
                },
                TodoItem {
                    id: Some("2".to_string()),
                    text: "Two".to_string(),
                    status: TodoStatus::InProgress,
                    active_form: None,
                },
            ])
            .unwrap_err();

        assert!(error
            .to_string()
            .contains("Only one task can be in_progress at a time"));
    }

    #[test]
    fn update_rejects_blank_text() {
        let mut manager = TodoManager::new();
        let error = manager
            .update(vec![TodoItem {
                id: Some("1".to_string()),
                text: "   ".to_string(),
                status: TodoStatus::Pending,
                active_form: None,
            }])
            .unwrap_err();

        assert!(error.to_string().contains("text required"));
    }

    #[test]
    fn has_open_items_and_nag_follow_rounds_counter() {
        let mut manager = TodoManager::new();
        manager
            .update(vec![
                pending("1", "First"),
                TodoItem {
                    id: Some("2".to_string()),
                    text: "Done".to_string(),
                    status: TodoStatus::Completed,
                    active_form: None,
                },
            ])
            .unwrap();

        assert!(manager.has_open_items());
        assert!(!manager.should_nag());

        manager.increment_rounds();
        manager.increment_rounds();
        manager.increment_rounds();

        assert_eq!(manager.rounds_without_update(), 3);
        assert!(manager.should_nag());
        assert_eq!(manager.reminder_text(), TODO_REMINDER);
    }

    #[test]
    fn no_open_items_when_all_completed() {
        let mut manager = TodoManager::new();
        manager
            .update(vec![TodoItem {
                id: Some("1".to_string()),
                text: "Done".to_string(),
                status: TodoStatus::Completed,
                active_form: None,
            }])
            .unwrap();

        manager.increment_rounds();
        manager.increment_rounds();
        manager.increment_rounds();

        assert!(!manager.has_open_items());
        assert!(!manager.should_nag());
    }
}
