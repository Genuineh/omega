use omega_client::ContentBlock;
use omega_todo::{TodoItem, TodoStatus};
use omega_tools::ToolResult;

pub(crate) fn tool_not_visible_error(name: &str) -> String {
    format!("Error: Tool '{name}' is not available in this workflow step")
}

pub(crate) fn tool_result_block(tool_use_id: &str, result: &ToolResult) -> ContentBlock {
    ContentBlock::ToolResult {
        tool_use_id: tool_use_id.to_string(),
        content: serde_json::Value::String(result.output.clone()),
        is_error: result.is_error().then_some(true),
    }
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
