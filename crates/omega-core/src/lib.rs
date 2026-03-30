mod agent;
mod helpers;
mod tool_factory;

// Re-export construction types for downstream crates (e.g. omega-tui).
pub use agent::Agent;
pub use omega_client::{ChatEvent, ChatResponseBuilder};
pub use omega_client::{
    ChatEventStream, ChatRequest, ClientError, ContentBlock, DynLlmClient, LlmClient, Message,
    MessageContent, MinimaxClient, MinimaxConfig, PromptCacheControl, Role, SystemBlock,
    ToolDefinition, Usage, STOP_REASON_END_TURN, STOP_REASON_TOOL_USE,
};
pub use omega_todo::{
    SharedTodoManager as CoreSharedTodoManager, TodoItem, TodoManager, TodoStatus,
    TodoToolHandler as CoreTodoToolHandler,
};
pub use omega_tools::{ToolErrorKind as CoreToolErrorKind, ToolResult as CoreToolResult};
pub use tool_factory::{
    create_default_tools, create_default_tools_with_todo_manager,
    create_default_tools_with_todo_manager_and_bash_allowlist,
    create_default_tools_with_todo_manager_and_tool_limits, default_bash_allowed_commands,
    default_batch_max_requests,
};

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
