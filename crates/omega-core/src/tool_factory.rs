use std::path::PathBuf;

use omega_context::{ContextFacadeServices, ContextToolRegistry, OmegaContextFacade};
use omega_skills::LoadSkillHandler;
use omega_todo::{SharedTodoManager, TodoManager, TodoReadHandler, TodoWriteHandler};
use omega_tools::{
    MemoryScopeLevel, ToolContextProfile, ToolDispatcher, ToolFamily, ToolHandler,
    ToolIoProfile, ToolManifest, ToolObservabilityProfile, ToolOutputFormat,
    ToolPermissionProfile, ToolPromptProfile, ToolStability, ToolStorageProfile,
    ToolUiProfile, TruncationStrategy,
};
use omega_tools_builtin::{
    default_bash_allowed_commands as builtin_default_bash_allowed_commands,
    default_batch_max_requests as builtin_default_batch_max_requests, ApplyPatchHandler,
    AskUserQuestionHandler, BashHandler, BatchHandler, CreateFileHandler, EditHandler,
    GlobSearchHandler, GrepSearchHandler, ListDirHandler, ReadHandler, TaskHandler,
    WebFetchHandler, WebSearchHandler, WriteHandler,
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
    let context_facade = std::sync::Arc::new(OmegaContextFacade::from_services(
        ContextFacadeServices::local(root.clone()),
    ));
    create_default_tools_with_context_and_todo_manager_and_tool_limits(
        root,
        context_facade,
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
    let context_facade = std::sync::Arc::new(OmegaContextFacade::from_services(
        ContextFacadeServices::local(root.clone()),
    ));
    create_default_tools_with_context_and_todo_manager_and_tool_limits(
        root,
        context_facade,
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
    let context_facade = std::sync::Arc::new(OmegaContextFacade::from_services(
        ContextFacadeServices::local(root.clone()),
    ));
    create_default_tools_with_context_and_todo_manager_and_tool_limits(
        root,
        context_facade,
        todo_manager,
        bash_allowed_commands,
        batch_max_requests,
    )
}

pub fn create_default_tools_with_context_and_todo_manager_and_tool_limits(
    root: PathBuf,
    context_facade: std::sync::Arc<OmegaContextFacade>,
    todo_manager: SharedTodoManager,
    bash_allowed_commands: Vec<String>,
    batch_max_requests: usize,
) -> ToolDispatcher {
    let mut dispatcher = ToolDispatcher::new();
    let context_registry = ContextToolRegistry::new(context_facade);
    register_default_manifest(
        &mut dispatcher,
        Box::new(BashHandler::with_allowed_commands(
            root.clone(),
            bash_allowed_commands,
        )),
    );
    register_default_manifest(
        &mut dispatcher,
        Box::new(BatchHandler::with_max_requests(
            root.clone(),
            batch_max_requests,
        )),
    );
    register_default_manifest(&mut dispatcher, Box::new(ListDirHandler::new(root.clone())));
    register_default_manifest(
        &mut dispatcher,
        Box::new(GlobSearchHandler::new(root.clone())),
    );
    register_default_manifest(
        &mut dispatcher,
        Box::new(GrepSearchHandler::new(root.clone())),
    );
    register_default_manifest(&mut dispatcher, Box::new(ReadHandler::new(root.clone())));
    register_default_manifest(
        &mut dispatcher,
        Box::new(CreateFileHandler::new(root.clone())),
    );
    register_default_manifest(&mut dispatcher, Box::new(WriteHandler::new(root.clone())));
    register_default_manifest(&mut dispatcher, Box::new(EditHandler::new(root.clone())));
    register_default_manifest(
        &mut dispatcher,
        Box::new(ApplyPatchHandler::new(root.clone())),
    );
    if let Ok(handler) = LoadSkillHandler::from_repo_root(&root) {
        register_default_manifest(&mut dispatcher, Box::new(handler));
    }
    for handler in context_registry.register_tools() {
        register_default_manifest(&mut dispatcher, handler);
    }
    register_default_manifest(&mut dispatcher, Box::new(WebSearchHandler::new()));
    register_default_manifest(&mut dispatcher, Box::new(WebFetchHandler::new()));
    register_default_manifest(&mut dispatcher, Box::new(AskUserQuestionHandler));
    register_default_manifest(&mut dispatcher, Box::new(TaskHandler));
    let todo_manager_for_write = todo_manager.clone();
    register_default_manifest(
        &mut dispatcher,
        Box::new(TodoWriteHandler::with_manager(todo_manager_for_write)),
    );
    register_default_manifest(
        &mut dispatcher,
        Box::new(TodoReadHandler::with_manager(todo_manager)),
    );
    dispatcher
        .register_alias("todo", "todo_write")
        .expect("todo alias should target registered todo_write tool");
    dispatcher
}

fn register_default_manifest(dispatcher: &mut ToolDispatcher, handler: Box<dyn ToolHandler>) {
    let name = handler.name().to_string();
    let manifest = match name.as_str() {
        "apply_patch" => manifest(
            handler,
            "Apply Patch",
            ToolFamily::Editing,
            ToolStability::Stable,
            prompt(
                "Apply a targeted text patch to an existing file.",
                &["you already know the exact file and local text window to change"],
                &["do not use it to create a new file or for shell-driven editing"],
                &["edit_file", "write_file"],
                &["edit_file"],
                &["apply a focused change to one existing file"],
                &["rewriting a whole file when a narrow patch is enough"],
            ),
        ),
        "bash" => manifest(
            handler,
            "Bash",
            ToolFamily::EscapeHatch,
            ToolStability::Stable,
            prompt(
                "Run an allowlisted shell command in the workspace when structured tools cannot express the task.",
                &[
                    "the exact shell command output is the artifact you need",
                    "no structured inspection or editing tool can express the operation",
                ],
                &["do not use it for routine list/search/read/edit flows covered by structured tools"],
                &[],
                &["list_dir", "glob_search", "grep_search", "read_file"],
                &["run a narrow allowlisted command in a specific workdir"],
                &["broad trial-and-error shell chains instead of a narrower structured tool"],
            ),
        ),
        "batch" => manifest(
            handler,
            "Batch",
            ToolFamily::WorkspaceInspection,
            ToolStability::Stable,
            prompt(
                "Run several read-only inspection tools in one batch call.",
                &["you already know the small set of list/glob/grep/read operations to run together"],
                &["do not use it when a single inspection tool call will answer the question"],
                &["bash"],
                &["list_dir", "glob_search", "grep_search", "read_file"],
                &["collect several read-only inspection results in parallel"],
                &["using batch before you know what you need to inspect"],
            ),
        ),
        "create_file" => manifest(
            handler,
            "Create File",
            ToolFamily::Editing,
            ToolStability::Stable,
            prompt(
                "Create a new file without overwriting an existing path.",
                &["the task requires a new file and the path must not already exist"],
                &["do not use it to update an existing file"],
                &["write_file"],
                &["apply_patch", "write_file"],
                &["create a new file with initial content"],
                &["trying to replace an existing file with create_file"],
            ),
        ),
        "edit_file" => manifest(
            handler,
            "Edit File",
            ToolFamily::Editing,
            ToolStability::Stable,
            prompt(
                "Apply a deterministic in-place edit when you know the exact replacement window.",
                &["you need a precise text replacement in an existing file"],
                &["do not use it when apply_patch can express the change more safely"],
                &["write_file"],
                &["apply_patch"],
                &["replace a specific string or region in one file"],
                &["using edit_file for broad full-file rewrites"],
            ),
        ),
        "glob_search" => manifest(
            handler,
            "Glob Search",
            ToolFamily::WorkspaceInspection,
            ToolStability::Stable,
            prompt(
                "Find files by path pattern inside the workspace.",
                &["you know the filename or path shape you want to match"],
                &["do not use it to search file contents"],
                &["bash"],
                &["list_dir", "grep_search"],
                &["find files matching a glob pattern"],
                &["using glob_search when you need line-level content matches"],
            ),
        ),
        "grep_search" => manifest(
            handler,
            "Grep Search",
            ToolFamily::WorkspaceInspection,
            ToolStability::Stable,
            prompt(
                "Run exact or regex content matching when you need literal hits, file paths, and matching lines.",
                &[
                    "you need exact string or regex matches with file paths and matching lines",
                    "you are validating whether a known symbol, string, or pattern appears in the workspace",
                ],
                &[
                    "do not use it when ranked or semantic retrieval matters more than exact matches",
                    "do not use it as the first tool for broad conceptual discovery across an unfamiliar codebase",
                ],
                &["bash", "search_codebase"],
                &["read_file", "search_codebase"],
                &[
                    "find every exact occurrence of a config key or symbol",
                    "check whether a specific error message appears in tests or source",
                ],
                &[
                    "using grep_search when you already know the exact file and only need to read it",
                    "using grep_search for vague architectural discovery where semantic ranking is more suitable",
                ],
            ),
        ),
        "list_dir" => manifest(
            handler,
            "List Directory",
            ToolFamily::WorkspaceInspection,
            ToolStability::Stable,
            prompt(
                "List directory contents in a stable, structured order.",
                &["you need to inspect the immediate children of a workspace directory"],
                &["do not use it for recursive content search or file reads"],
                &["bash"],
                &["glob_search", "read_file"],
                &["list the contents of a workspace directory"],
                &["using list_dir as a substitute for text search"],
            ),
        ),
        "load_skill" => manifest(
            handler,
            "Load Skill",
            ToolFamily::KnowledgeAndGovernance,
            ToolStability::Preview,
            prompt(
                "Load a repository skill file into the current turn when a task-specific skill is required.",
                &[
                    "the repository already defines a relevant skill and you know the skill path or name",
                ],
                &[
                    "do not use it for ordinary code exploration when workflow guidance is already sufficient",
                ],
                &[],
                &[],
                &["load a repo skill before a specialized documentation or review task"],
                &["loading broad unrelated skills speculatively"],
            ),
        ),
        "web_search" => manifest(
            handler,
            "Web Search",
            ToolFamily::WebResearch,
            ToolStability::Preview,
            prompt(
                "Search external public sources when the answer is not in the local workspace and you do not yet know the target URL.",
                &[
                    "you need external documentation, articles, API references, or public background material",
                    "you need candidate URLs before fetching one specific page",
                ],
                &[
                    "do not use it when local workspace inspection is sufficient",
                    "do not use it when you already know the exact URL to read",
                ],
                &["bash", "web_fetch"],
                &["web_fetch"],
                &["find a vendor documentation page before reading it"],
                &["using shell search commands for routine public web lookup"],
            ),
        ),
        "web_fetch" => manifest(
            handler,
            "Web Fetch",
            ToolFamily::WebResearch,
            ToolStability::Preview,
            prompt(
                "Fetch a known URL and summarize its content with structured metadata.",
                &[
                    "the user gave you a URL",
                    "web_search already found a page you want to inspect",
                ],
                &[
                    "do not use it when you still need to discover candidate URLs",
                    "do not use it for local files or workspace reads",
                ],
                &["bash"],
                &["web_search"],
                &["read a known release note or documentation page"],
                &["using bash curl/wget for ordinary single-page reads"],
            ),
        ),
        "ask_user_question" => manifest(
            handler,
            "Ask User Question",
            ToolFamily::Interaction,
            ToolStability::Preview,
            prompt(
                "Pause to request structured user input when the current branch cannot proceed credibly without it.",
                &[
                    "you need a decision, clarification, or confirmation from the user",
                    "the answer cannot be derived from current context",
                ],
                &[
                    "do not use it for questions you can answer yourself from repo or transcript state",
                    "do not hide a normal final-answer question behind a tool call unless the workflow depends on a structured response",
                ],
                &[],
                &[],
                &["request the user's branch choice before editing the wrong file"],
                &["asking the user for information that is already available in visible context"],
            ),
        ),
        "task" => manifest(
            handler,
            "Task",
            ToolFamily::Interaction,
            ToolStability::Experimental,
            prompt(
                "Record a structured fresh-context task request for later child execution or review.",
                &[
                    "you need to capture a bounded child task with explicit prompt and expected output",
                ],
                &[
                    "do not pretend it has already run unless a later runtime actually executes it",
                ],
                &[],
                &[],
                &["record a code-review subtask request with clear success criteria"],
                &["claiming delegated work completed when the runtime has not executed it"],
            ),
        ),
        "todo_write" => manifest(
            handler,
            "Todo Write",
            ToolFamily::Planning,
            ToolStability::Stable,
            prompt(
                "Update tracked todo state explicitly instead of burying task-progress changes in free text.",
                &[
                    "you are starting, advancing, or finishing concrete task items",
                    "the session todo panel should reflect the current execution state",
                ],
                &[
                    "do not use it for tentative thoughts that do not change task state",
                ],
                &[],
                &["todo_read"],
                &["mark the current item in_progress before editing code"],
                &["leaving the todo panel stale while the execution plan changes"],
            ),
        ),
        "todo_read" => manifest(
            handler,
            "Todo Read",
            ToolFamily::Planning,
            ToolStability::Stable,
            prompt(
                "Read the current tracked todo state without mutating it.",
                &[
                    "you need the current session task snapshot",
                ],
                &[
                    "do not use it when the current todo snapshot is already visible in context",
                ],
                &[],
                &["todo_write"],
                &["check the current open items before updating them"],
                &["rewriting todo state when you only need to read it"],
            ),
        ),
        "manage_document" => manifest(
            handler,
            "Manage Document",
            ToolFamily::KnowledgeAndGovernance,
            ToolStability::Preview,
            prompt(
                "Check, plan, or apply documentation governance operations.",
                &["the task is about document governance, health checks, archival, or metadata updates"],
                &["do not use it for arbitrary source-code editing"],
                &["apply_patch"],
                &["read_file", "search_codebase"],
                &["run a documentation health check or governance operation"],
                &["using it when you only need to inspect document text"],
            ),
        ),
        "read_file" => manifest(
            handler,
            "Read File",
            ToolFamily::WorkspaceInspection,
            ToolStability::Stable,
            prompt(
                "Read a specific file slice from the workspace.",
                &["you know the file path and need exact file contents"],
                &["do not use it to search across many files"],
                &["bash"],
                &["grep_search", "search_codebase"],
                &["read a targeted line range from one file"],
                &["reading many files one by one before doing a path or content search"],
            ),
        ),
        "search_codebase" => manifest(
            handler,
            "Search Codebase",
            ToolFamily::KnowledgeAndGovernance,
            ToolStability::Preview,
            prompt(
                "Search the indexed project with ranked keyword, semantic, or hybrid retrieval when you need discovery rather than exact line matches.",
                &[
                    "you need ranked retrieval, semantic matching, or structured filters over project knowledge",
                    "you are still discovering which files or subsystems are relevant",
                ],
                &[
                    "do not use it when exact line matches or exact file contents are the main need",
                    "do not treat it as a replacement for reading the matched files after retrieval",
                ],
                &["grep_search"],
                &["grep_search", "read_file"],
                &[
                    "discover the most relevant modules for a concept or architecture question",
                    "run semantic or hybrid retrieval before narrowing to exact files",
                ],
                &[
                    "treating semantic search as a replacement for reading the matched file",
                    "using search_codebase when you already know the exact token or regex you want to match",
                ],
            ),
        ),
        "todo" => manifest(
            handler,
            "Todo",
            ToolFamily::Planning,
            ToolStability::Stable,
            prompt(
                "Create or update explicit task state that the runtime can track.",
                &["you need to record, update, or complete concrete task items"],
                &["do not use it for casual scratch notes that are not part of task state"],
                &[],
                &[],
                &["update tracked todo items for the current task"],
                &["using todo for narrative progress messages instead of real task-state changes"],
            ),
        ),
        "write_file" => manifest(
            handler,
            "Write File",
            ToolFamily::Editing,
            ToolStability::Stable,
            prompt(
                "Write file content directly when a full-file replacement is the intended operation.",
                &["you intentionally need to replace the full content of one file"],
                &["do not use it when apply_patch or edit_file can make a smaller, safer change"],
                &["bash"],
                &["apply_patch", "edit_file"],
                &["replace one file with complete new content"],
                &["using write_file for a tiny local edit that should stay patch-sized"],
            ),
        ),
        _ => ToolManifest::legacy(handler),
    };

    dispatcher.register_manifest(attach_capability_profiles(manifest));
}

fn attach_capability_profiles(manifest: ToolManifest) -> ToolManifest {
    match manifest.id.as_str() {
        "apply_patch" | "create_file" | "edit_file" | "write_file" => {
            file_edit_profiles(manifest)
        }
        "todo" | "todo_write" => todo_profiles(manifest),
        "todo_read" => todo_read_profiles(manifest),
        "web_search" | "web_fetch" => web_profiles(manifest),
        "ask_user_question" | "task" => interaction_profiles(manifest),
        "bash" => bash_profiles(manifest),
        "search_codebase" => codebase_search_profiles(manifest),
        "manage_document" => manage_document_profiles(manifest),
        _ if manifest.family == ToolFamily::WorkspaceInspection => inspection_profiles(manifest),
        _ => manifest,
    }
}

fn inspection_profiles(manifest: ToolManifest) -> ToolManifest {
    let tool_id = manifest.id.clone();
    manifest
        .with_io(ToolIoProfile {
            max_output_bytes: 24_000,
            truncation_strategy: TruncationStrategy::Tail,
            output_format: ToolOutputFormat::PlainText,
            normalize_input: true,
        })
        .with_ui(ToolUiProfile {
            invocation_preview: true,
            result_preview: true,
            detail_overlay: true,
            action_affordances: strings(&["open_detail_overlay"]),
        })
        .with_context(ToolContextProfile {
            needs_workspace_root: true,
            needs_step_metadata: true,
            needs_selection: false,
            memory_scope: MemoryScopeLevel::Project,
            network_context: false,
        })
        .with_permissions(ToolPermissionProfile {
            permission_class: "workspace_read".to_string(),
            default_policy_mode: "step_visible".to_string(),
            requires_approval: false,
            denial_remediation: Some(
                "Use a visible inspection tool for this step or continue from existing workflow context without new workspace reads."
                    .to_string(),
            ),
        })
        .with_observability(observability(&tool_id, "workspace_inspection"))
}

fn file_edit_profiles(manifest: ToolManifest) -> ToolManifest {
    let tool_id = manifest.id.clone();
    manifest
        .with_io(ToolIoProfile {
            max_output_bytes: 32_000,
            truncation_strategy: TruncationStrategy::Tail,
            output_format: ToolOutputFormat::Diff,
            normalize_input: false,
        })
        .with_ui(ToolUiProfile {
            invocation_preview: true,
            result_preview: true,
            detail_overlay: true,
            action_affordances: strings(&["open_diff_preview", "open_detail_overlay"]),
        })
        .with_context(ToolContextProfile {
            needs_workspace_root: true,
            needs_step_metadata: true,
            needs_selection: false,
            memory_scope: MemoryScopeLevel::Project,
            network_context: false,
        })
        .with_permissions(ToolPermissionProfile {
            permission_class: "workspace_write".to_string(),
            default_policy_mode: "step_visible_then_runtime_approval".to_string(),
            requires_approval: true,
            denial_remediation: Some(
                "This step does not currently allow workspace writes. Prefer a visible read-only tool, or ask for confirmation before retrying a FileEdit action."
                    .to_string(),
            ),
        })
        .with_storage(ToolStorageProfile {
            writes_session_journal: true,
            produces_artifact: true,
            writes_memory: false,
            writes_todo: false,
            replayable: false,
        })
        .with_observability(observability(&tool_id, "file_edit"))
}

fn todo_profiles(manifest: ToolManifest) -> ToolManifest {
    let tool_id = manifest.id.clone();
    manifest
        .with_ui(ToolUiProfile {
            invocation_preview: true,
            result_preview: true,
            detail_overlay: true,
            action_affordances: strings(&["replace_todo_panel", "open_detail_overlay"]),
        })
        .with_context(ToolContextProfile {
            needs_workspace_root: false,
            needs_step_metadata: true,
            needs_selection: false,
            memory_scope: MemoryScopeLevel::Session,
            network_context: false,
        })
        .with_permissions(ToolPermissionProfile {
            permission_class: "task_state_write".to_string(),
            default_policy_mode: "step_visible".to_string(),
            requires_approval: false,
            denial_remediation: Some(
                "Keep task state in the current answer when todo is not visible, or retry in a step that allows tracked task updates."
                    .to_string(),
            ),
        })
        .with_storage(ToolStorageProfile {
            writes_session_journal: true,
            produces_artifact: false,
            writes_memory: false,
            writes_todo: true,
            replayable: true,
        })
        .with_observability(observability(&tool_id, "planning"))
}

fn todo_read_profiles(manifest: ToolManifest) -> ToolManifest {
    let tool_id = manifest.id.clone();
    manifest
        .with_ui(ToolUiProfile {
            invocation_preview: true,
            result_preview: true,
            detail_overlay: true,
            action_affordances: strings(&["open_detail_overlay"]),
        })
        .with_context(ToolContextProfile {
            needs_workspace_root: false,
            needs_step_metadata: true,
            needs_selection: false,
            memory_scope: MemoryScopeLevel::Session,
            network_context: false,
        })
        .with_permissions(ToolPermissionProfile {
            permission_class: "task_state_read".to_string(),
            default_policy_mode: "step_visible".to_string(),
            requires_approval: false,
            denial_remediation: Some(
                "Continue from the visible workflow state when todo_read is not allowed in this step."
                    .to_string(),
            ),
        })
        .with_observability(observability(&tool_id, "planning"))
}

fn bash_profiles(manifest: ToolManifest) -> ToolManifest {
    let tool_id = manifest.id.clone();
    manifest
        .with_ui(ToolUiProfile {
            invocation_preview: true,
            result_preview: true,
            detail_overlay: true,
            action_affordances: strings(&["open_detail_overlay"]),
        })
        .with_context(ToolContextProfile {
            needs_workspace_root: true,
            needs_step_metadata: true,
            needs_selection: false,
            memory_scope: MemoryScopeLevel::Project,
            network_context: false,
        })
        .with_permissions(ToolPermissionProfile {
            permission_class: "shell_exec".to_string(),
            default_policy_mode: "step_visible_then_runtime_approval".to_string(),
            requires_approval: true,
            denial_remediation: Some(
                "Prefer a visible structured inspection or editing tool first. Only retry bash after confirmation or when the workflow explicitly allows shell execution."
                    .to_string(),
            ),
        })
        .with_observability(observability(&tool_id, "escape_hatch"))
}

fn web_profiles(manifest: ToolManifest) -> ToolManifest {
    let tool_id = manifest.id.clone();
    manifest
        .with_io(ToolIoProfile {
            max_output_bytes: 24_000,
            truncation_strategy: TruncationStrategy::Tail,
            output_format: ToolOutputFormat::Json,
            normalize_input: true,
        })
        .with_ui(ToolUiProfile {
            invocation_preview: true,
            result_preview: true,
            detail_overlay: true,
            action_affordances: strings(&["open_web_result_view", "open_detail_overlay"]),
        })
        .with_context(ToolContextProfile {
            needs_workspace_root: false,
            needs_step_metadata: true,
            needs_selection: false,
            memory_scope: MemoryScopeLevel::Project,
            network_context: true,
        })
        .with_permissions(ToolPermissionProfile {
            permission_class: "network_read".to_string(),
            default_policy_mode: "step_visible_then_runtime_approval".to_string(),
            requires_approval: true,
            denial_remediation: Some(
                "Use local workspace or knowledge tools first, or retry web access after explicit confirmation."
                    .to_string(),
            ),
        })
        .with_observability(observability(&tool_id, "web_research"))
}

fn interaction_profiles(manifest: ToolManifest) -> ToolManifest {
    let tool_id = manifest.id.clone();
    manifest
        .with_io(ToolIoProfile {
            max_output_bytes: 12_000,
            truncation_strategy: TruncationStrategy::Tail,
            output_format: ToolOutputFormat::Json,
            normalize_input: true,
        })
        .with_ui(ToolUiProfile {
            invocation_preview: true,
            result_preview: true,
            detail_overlay: true,
            action_affordances: strings(&["open_input_prompt", "open_detail_overlay"]),
        })
        .with_context(ToolContextProfile {
            needs_workspace_root: false,
            needs_step_metadata: true,
            needs_selection: false,
            memory_scope: MemoryScopeLevel::Session,
            network_context: false,
        })
        .with_permissions(ToolPermissionProfile {
            permission_class: "interaction_control".to_string(),
            default_policy_mode: "step_visible".to_string(),
            requires_approval: false,
            denial_remediation: Some(
                "Ask in normal assistant text only when the workflow does not allow structured interaction tools."
                    .to_string(),
            ),
        })
        .with_storage(ToolStorageProfile {
            writes_session_journal: true,
            produces_artifact: false,
            writes_memory: false,
            writes_todo: false,
            replayable: true,
        })
        .with_observability(observability(&tool_id, "interaction"))
}

fn codebase_search_profiles(manifest: ToolManifest) -> ToolManifest {
    let tool_id = manifest.id.clone();
    manifest
        .with_io(ToolIoProfile {
            max_output_bytes: 24_000,
            truncation_strategy: TruncationStrategy::Tail,
            output_format: ToolOutputFormat::Json,
            normalize_input: true,
        })
        .with_ui(ToolUiProfile {
            invocation_preview: true,
            result_preview: true,
            detail_overlay: true,
            action_affordances: strings(&["open_search_results", "open_detail_overlay"]),
        })
        .with_context(ToolContextProfile {
            needs_workspace_root: true,
            needs_step_metadata: true,
            needs_selection: false,
            memory_scope: MemoryScopeLevel::Project,
            network_context: false,
        })
        .with_permissions(ToolPermissionProfile {
            permission_class: "knowledge_read".to_string(),
            default_policy_mode: "step_visible".to_string(),
            requires_approval: false,
            denial_remediation: Some(
                "Use a visible local inspection tool if ranked codebase search is not allowed in this step."
                    .to_string(),
            ),
        })
        .with_observability(observability(&tool_id, "knowledge"))
}

fn manage_document_profiles(manifest: ToolManifest) -> ToolManifest {
    let tool_id = manifest.id.clone();
    manifest
        .with_io(ToolIoProfile {
            max_output_bytes: 24_000,
            truncation_strategy: TruncationStrategy::Tail,
            output_format: ToolOutputFormat::Json,
            normalize_input: true,
        })
        .with_ui(ToolUiProfile {
            invocation_preview: true,
            result_preview: true,
            detail_overlay: true,
            action_affordances: strings(&["open_detail_overlay"]),
        })
        .with_context(ToolContextProfile {
            needs_workspace_root: true,
            needs_step_metadata: true,
            needs_selection: false,
            memory_scope: MemoryScopeLevel::Project,
            network_context: false,
        })
        .with_permissions(ToolPermissionProfile {
            permission_class: "document_governance".to_string(),
            default_policy_mode: "step_visible_then_runtime_approval".to_string(),
            requires_approval: true,
            denial_remediation: Some(
                "Continue with read-only document inspection or ask for confirmation before retrying a governance mutation."
                    .to_string(),
            ),
        })
        .with_storage(ToolStorageProfile {
            writes_session_journal: true,
            produces_artifact: true,
            writes_memory: false,
            writes_todo: false,
            replayable: false,
        })
        .with_observability(observability(&tool_id, "knowledge"))
}

fn observability(tool_name: &str, family: &str) -> ToolObservabilityProfile {
    ToolObservabilityProfile {
        invocation_metric: format!("tool.{family}.{tool_name}.invoke"),
        success_metric: format!("tool.{family}.{tool_name}.success"),
        failure_metric: format!("tool.{family}.{tool_name}.failure"),
    }
}

fn manifest(
    handler: Box<dyn ToolHandler>,
    display_name: &str,
    family: ToolFamily,
    stability: ToolStability,
    prompt: ToolPromptProfile,
) -> ToolManifest {
    ToolManifest::new(handler.name().to_string(), display_name, family, stability, prompt, handler)
}

fn prompt(
    summary: &str,
    when_to_use: &[&str],
    when_not_to_use: &[&str],
    prefer_over: &[&str],
    fallback_to: &[&str],
    examples: &[&str],
    anti_patterns: &[&str],
) -> ToolPromptProfile {
    ToolPromptProfile {
        summary: summary.to_string(),
        when_to_use: strings(when_to_use),
        when_not_to_use: strings(when_not_to_use),
        prefer_over: strings(prefer_over),
        fallback_to: strings(fallback_to),
        examples: strings(examples),
        anti_patterns: strings(anti_patterns),
    }
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_string()).collect()
}
