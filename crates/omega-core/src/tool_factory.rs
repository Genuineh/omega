use std::path::PathBuf;

use omega_context::{ContextToolRegistry, OmegaContextFacade};
use omega_skills::LoadSkillHandler;
use omega_todo::{SharedTodoManager, TodoManager, TodoToolHandler};
use omega_tools::ToolDispatcher;
use omega_tools_builtin::{
    default_bash_allowed_commands as builtin_default_bash_allowed_commands,
    default_batch_max_requests as builtin_default_batch_max_requests, ApplyPatchHandler,
    BashHandler, BatchHandler, CreateFileHandler, EditHandler, GlobSearchHandler,
    GrepSearchHandler, ListDirHandler, ReadHandler, WriteHandler,
};

/// Create a ToolDispatcher with all built-in tools.
pub fn create_default_tools(root: PathBuf) -> ToolDispatcher {
    create_default_tools_with_todo_manager(
        root,
        std::sync::Arc::new(std::sync::Mutex::new(TodoManager::new())),
    )
}

pub fn default_bash_allowed_commands() -> Vec<String> {
    builtin_default_bash_allowed_commands()
}

pub fn default_batch_max_requests() -> usize {
    builtin_default_batch_max_requests()
}

pub fn create_default_tools_with_todo_manager(
    root: PathBuf,
    todo_manager: SharedTodoManager,
) -> ToolDispatcher {
    create_default_tools_with_todo_manager_and_tool_limits(
        root,
        todo_manager,
        default_bash_allowed_commands(),
        default_batch_max_requests(),
    )
}

pub fn create_default_tools_with_todo_manager_and_bash_allowlist(
    root: PathBuf,
    todo_manager: SharedTodoManager,
    bash_allowed_commands: Vec<String>,
) -> ToolDispatcher {
    create_default_tools_with_todo_manager_and_tool_limits(
        root,
        todo_manager,
        bash_allowed_commands,
        default_batch_max_requests(),
    )
}

pub fn create_default_tools_with_todo_manager_and_tool_limits(
    root: PathBuf,
    todo_manager: SharedTodoManager,
    bash_allowed_commands: Vec<String>,
    batch_max_requests: usize,
) -> ToolDispatcher {
    let mut dispatcher = ToolDispatcher::new();
    let context_registry =
        ContextToolRegistry::new(std::sync::Arc::new(OmegaContextFacade::local(root.clone())));
    dispatcher.register(Box::new(BashHandler::with_allowed_commands(
        root.clone(),
        bash_allowed_commands,
    )));
    dispatcher.register(Box::new(BatchHandler::with_max_requests(
        root.clone(),
        batch_max_requests,
    )));
    dispatcher.register(Box::new(ListDirHandler::new(root.clone())));
    dispatcher.register(Box::new(GlobSearchHandler::new(root.clone())));
    dispatcher.register(Box::new(GrepSearchHandler::new(root.clone())));
    dispatcher.register(Box::new(ReadHandler::new(root.clone())));
    dispatcher.register(Box::new(CreateFileHandler::new(root.clone())));
    dispatcher.register(Box::new(WriteHandler::new(root.clone())));
    dispatcher.register(Box::new(EditHandler::new(root.clone())));
    dispatcher.register(Box::new(ApplyPatchHandler::new(root.clone())));
    if let Ok(handler) = LoadSkillHandler::from_repo_root(&root) {
        dispatcher.register(Box::new(handler));
    }
    for handler in context_registry.register_tools() {
        dispatcher.register(handler);
    }
    dispatcher.register(Box::new(TodoToolHandler::with_manager(todo_manager)));
    dispatcher
}
