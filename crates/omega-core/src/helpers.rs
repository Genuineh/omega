use omega_client::ContentBlock;
use omega_todo::{TodoItem, TodoStatus};
use omega_tools::ToolResult;
use serde_json::Value;

pub(crate) fn tool_not_visible_error(name: &str) -> String {
    format!("Error: Tool '{name}' is not available in this workflow step")
}

pub(crate) fn tool_result_block(tool_use_id: &str, result: &ToolResult) -> ContentBlock {
    ContentBlock::ToolResult {
        tool_use_id: tool_use_id.to_string(),
        content: if result.is_error() {
            result.as_content_value()
        } else {
            Value::String(success_tool_result_content(result))
        },
        is_error: result.is_error().then_some(true),
    }
}

fn success_tool_result_content(result: &ToolResult) -> String {
    let output = result.output.trim();
    if !output.is_empty() {
        return result.output.clone();
    }

    if let Some(preview) = result.preview.as_deref() {
        if !preview.trim().is_empty() {
            return preview.to_string();
        }
    }

    if result.has_metadata() {
        return serde_json::to_string_pretty(&result.metadata)
            .unwrap_or_else(|_| result.metadata.to_string());
    }

    "Tool completed successfully with no textual output.".to_string()
}

pub(crate) fn todo_input_has_open_items(input: &serde_json::Value) -> Option<bool> {
    let items = input.get("items")?.clone();
    let items: Vec<TodoItem> = serde_json::from_value(items).ok()?;
    Some(
        items
            .iter()
            .any(|item| item.status != TodoStatus::Completed),
    )
}
