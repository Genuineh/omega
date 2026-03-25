use omega_observability::strip_ansi;

use super::{App, Panel, TodoPanelStatus, TodoSummary, TODO_EMPTY_LINES, TODO_UNSYNCED_LINES};

pub(crate) fn todo_unsynced_lines() -> Vec<String> {
    TODO_UNSYNCED_LINES
        .iter()
        .map(|line| (*line).to_string())
        .collect()
}

pub(crate) fn todo_empty_lines() -> Vec<String> {
    TODO_EMPTY_LINES
        .iter()
        .map(|line| (*line).to_string())
        .collect()
}

impl App {
    pub fn set_todo_snapshot(&mut self, turn_id: u64, rendered: &str) {
        let clean = strip_ansi(rendered);
        self.last_todo_turn_id = Some(turn_id);

        if clean.trim().is_empty() || clean.trim() == "No todos." {
            self.todo_status = TodoPanelStatus::Empty;
            self.todo_summary = Some(TodoSummary {
                completed: 0,
                total: 0,
            });
            self.todo_lines = todo_empty_lines();
            return;
        }

        let mut lines: Vec<String> = clean.lines().map(ToOwned::to_owned).collect();
        let summary = extract_summary(&mut lines).or_else(|| summarize_todo_lines(&lines));
        if lines.last().is_some_and(|line| line.is_empty()) {
            lines.pop();
        }

        self.todo_status = TodoPanelStatus::Ready;
        self.todo_summary = summary;
        self.todo_lines = lines;
    }

    pub fn todo_panel_title(&self) -> String {
        let mut title = match self.todo_status {
            TodoPanelStatus::NeverSynced => " Todos ".to_string(),
            TodoPanelStatus::Empty => " Todos empty ".to_string(),
            TodoPanelStatus::Ready => match self.todo_summary {
                Some(summary) => format!(" Todos {}/{} ", summary.completed, summary.total),
                None => " Todos ".to_string(),
            },
        };

        if self.todo_refresh_pending() {
            title = format!("{}(stale) ", title.trim_end());
        }

        if self.focused_panel == Panel::Todo {
            title.push('◆');
            title.push(' ');
        }

        title
    }
}

fn extract_summary(lines: &mut Vec<String>) -> Option<TodoSummary> {
    let last_line = lines.last()?.trim();
    let inner = last_line.strip_prefix('(')?.strip_suffix(" completed)")?;
    let (completed, total) = inner.split_once('/')?;
    let summary = TodoSummary {
        completed: completed.trim().parse().ok()?,
        total: total.trim().parse().ok()?,
    };

    lines.pop();
    if lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }

    Some(summary)
}

fn summarize_todo_lines(lines: &[String]) -> Option<TodoSummary> {
    let mut total = 0;
    let mut completed = 0;

    for line in lines {
        let trimmed = line.trim_start();
        if trimmed.starts_with("[ ]") || trimmed.starts_with("[>]") || trimmed.starts_with("[x]") {
            total += 1;
            if trimmed.starts_with("[x]") {
                completed += 1;
            }
        }
    }

    if total == 0 {
        None
    } else {
        Some(TodoSummary { completed, total })
    }
}
