use std::sync::{mpsc, Arc, Mutex};
use std::thread;

use omega_command::{
    CommandHint, CommandHintProvider, CommandHintResolution, OmegaCommandDescriptor,
    OmegaCommandInvocation, OmegaCommandRegistry, OmegaCommandSource, OmegaCommandSubcommand,
};
use omega_context::{
    ArchiveTrigger, DocType, DocumentMutationMode, DocumentOp, FileRecord, FileStatus,
    OmegaContextFacade, SearchMode, SearchQuery,
};
pub use omega_context::{
    ContextBudgetDiagnostics, ContextDiagnostics, ContextDocumentDiagnostics,
    ContextMemoryDiagnostics, ContextStoreDiagnostics, ContextSupervisionSnapshot,
    DocumentActivitySummary, DocumentHealthStatus, DocumentHitItem, DocumentHitSummary,
    DocumentOperatorUsage, DocumentStoreVersion, DocumentSupervisionSnapshot,
    DocumentSupervisionTotals, HealthScore, MemoryHitItem, MemoryHitSummary,
    MemorySupervisionSnapshot, MemorySupervisionTotals, SupervisionReadiness,
};
use omega_core::{Agent, CoreSharedTodoManager, DynLlmClient, Message, TodoManager};
use omega_hooks::HookHost;
use omega_skills::SkillLoader;
use omega_workflow::{SceneCatalog, WorkflowCatalog, WorkflowPromptCatalog};
use tokio::runtime::Handle;
use tokio::sync::watch;
use tracing::{error, info};

const SUMMARY_CHAR_LIMIT: usize = 2_000;
const CONTEXT_SAFETY_MARGIN_TOKENS: u32 = 2_000;
const TOKEN_ESTIMATE_DIVISOR: usize = 4;
const REPAIR_PASS_MAX_ITERATIONS: u32 = 1;

mod hook_adapter;
mod output;
mod routing;
mod runner;
mod runtime_message;
mod runtime_ui;
mod session_state;
mod skill_catalog;
#[cfg(any(test, feature = "test-support"))]
mod test_support;
mod tool_catalog;
mod ui_emit;

pub use omega_workflow::{
    StepSkillRequest, StepToolRequest, DEEP_RESEARCH_SCENE_ID, DEEP_RESEARCH_WORKFLOW_ID,
    EXECUTE_STEP_ID, EXPLORE_STEP_ID, FEATURE_SCENE_ID, FEATURE_WORKFLOW_ID, PLAN_STEP_ID,
    REPORT_STEP_ID, RESEARCH_SCENE_ID, RESEARCH_WORKFLOW_ID, SCENE_RECOGNITION_STEP_ID,
    SELECT_WORKFLOW_STEP_ID,
};
pub use runtime_message::{
    ConversationMessage, LegacyRuntimeUiBridge, RuntimeContentKind, RuntimeMessage,
    RuntimeMessageBridge, RuntimeMessageEnvelope, RuntimePriority, RuntimeSource,
    SessionRoutingStatus, SharedRuntimeMessageBridge, StateMessage, WorkflowStepStatus,
};
pub use runtime_ui::{
    ActivityTarget, CacheDiagnostics, ExecuteProgressDiagnostics, OverlayRequest, OverlayTarget,
    ResponseSection, ResponseSectionDelta, ResponseSectionKind, ResponseSectionMetadata,
    ResponseSectionState, RuntimeUiBridge, RuntimeUiEffect, RuntimeUiEnvelope, RuntimeUiMessage,
    RuntimeUiSink, SectionOrigin, SessionRuntimeContext, StatusSlot, StatusValue, StepContextWrite,
    StepContextWriteKind, StepDiagnostics, StepInputDiagnostics, StepInputStatus,
    StepOutputAttemptKind, StepOutputContractMode, StepOutputDiagnostics,
    StepOutputRecoveryDecision, StepOutputStatus, StepSubflowRef, StepSubflowState,
    StepSubflowStatus, StepSummarySource, TokenCountSource, ToolCapabilityDiagnostics, ToolRun,
    ToolRunDetail, ToolRunStatus,
    UiContent, UiMessageKind, UiPriority, UiSource, UiTarget, WorkflowRunRole,
};
pub use skill_catalog::{ResolvedSkillSet, SessionSkillCatalog};
#[cfg(any(test, feature = "test-support"))]
pub use test_support::RuntimeEnvelopeRecorder;
pub use tool_catalog::{ResolvedToolSet, SessionToolCatalog};

#[cfg(test)]
pub(crate) use omega_context::render_output_contract;
#[cfg(test)]
pub(crate) use output::{parse_json_values, validate_schema_file};
#[cfg(test)]
pub(crate) use routing::{
    latest_user_turn_prefers_research_scene, latest_user_turn_requires_feature_scene,
};
#[cfg(test)]
pub(crate) use runner::resolve_structured_input;
pub(crate) use session_state::SessionContext;
#[cfg(test)]
pub(crate) use ui_emit::{preview_tool_invocation, ProviderMarkupSanitizer};

#[cfg(test)]
pub(crate) use output::validate_structured_output;

pub struct AgentSessionConfig {
    pub client: DynLlmClient,
    pub system: String,
    pub cwd: std::path::PathBuf,
    pub runtime_handle: Handle,
    pub scene_catalog: SceneCatalog,
    pub workflow_catalog: WorkflowCatalog,
    pub prompt_catalog: WorkflowPromptCatalog,
    pub context_window: u32,
    pub max_output_tokens: u32,
    pub bash_allowed_commands: Vec<String>,
    pub batch_max_requests: usize,
}

struct AgentSlot {
    turn_id: u64,
    agent: Option<Agent>,
}

pub struct AgentSession {
    agent_slot: Arc<Mutex<AgentSlot>>,
    turn_checkpoint: Arc<Mutex<Vec<Message>>>,
    active_turn_tx: watch::Sender<u64>,
    session_context: Arc<Mutex<SessionContext>>,
    context_facade: Arc<OmegaContextFacade>,
    client: DynLlmClient,
    base_system: String,
    cwd: std::path::PathBuf,
    todo_manager: CoreSharedTodoManager,
    hook_host: Arc<HookHost>,
    skill_catalog: Arc<SessionSkillCatalog>,
    tool_catalog: Arc<SessionToolCatalog>,
    runtime_handle: Handle,
    scene_catalog: SceneCatalog,
    workflow_catalog: WorkflowCatalog,
    prompt_catalog: WorkflowPromptCatalog,
    context_window: u32,
    max_output_tokens: u32,
    bash_allowed_commands: Vec<String>,
}

impl AgentSession {
    pub fn new(config: AgentSessionConfig) -> anyhow::Result<Self> {
        let skill_loader = SkillLoader::from_repo_root(&config.cwd)?;
        let skill_catalog = Arc::new(SessionSkillCatalog::new(skill_loader));
        let todo_manager = Arc::new(Mutex::new(TodoManager::new()));
        let hook_host = Arc::new(HookHost::load(&config.cwd)?);
        let dispatcher = omega_core::create_default_tools_with_todo_manager_and_tool_limits(
            config.cwd.clone(),
            todo_manager.clone(),
            config.bash_allowed_commands.clone(),
            config.batch_max_requests,
        );
        let available_manifests = dispatcher.manifest_metadata();
        let default_manifests = available_manifests
            .iter()
            .filter(|manifest| manifest.id != "ask_user_question")
            .cloned()
            .collect::<Vec<_>>();
        let tool_catalog = Arc::new(SessionToolCatalog::with_available_manifests(
            default_manifests,
            available_manifests,
        ));
        let initial_system =
            skill_catalog.build_system_prompt(&config.system, "", &StepSkillRequest::MatchTask);
        let context_facade = Arc::new(OmegaContextFacade::local(config.cwd.clone()));
        let mut agent = Agent::new(config.client.clone(), initial_system, dispatcher)?;
        agent.set_max_tokens(config.max_output_tokens);
        let checkpoint = agent.messages().to_vec();
        let (active_turn_tx, _active_turn_rx) = watch::channel(0u64);

        if config
            .workflow_catalog
            .workflow(&config.scene_catalog.root_workflow_id)
            .is_none()
        {
            return Err(anyhow::anyhow!(
                "missing root workflow '{}' in workflow catalog",
                config.scene_catalog.root_workflow_id
            ));
        }
        if config
            .scene_catalog
            .scene(&config.scene_catalog.default_scene_id)
            .is_none()
        {
            return Err(anyhow::anyhow!(
                "missing default scene '{}' in scene catalog",
                config.scene_catalog.default_scene_id
            ));
        }

        Ok(Self {
            agent_slot: Arc::new(Mutex::new(AgentSlot {
                turn_id: 0,
                agent: Some(agent),
            })),
            turn_checkpoint: Arc::new(Mutex::new(checkpoint)),
            active_turn_tx,
            session_context: Arc::new(Mutex::new(SessionContext::new(
                config.scene_catalog.root_workflow_id.clone(),
            ))),
            context_facade,
            client: config.client,
            base_system: config.system,
            cwd: config.cwd,
            todo_manager,
            hook_host,
            skill_catalog,
            tool_catalog,
            runtime_handle: config.runtime_handle,
            scene_catalog: config.scene_catalog,
            workflow_catalog: config.workflow_catalog,
            prompt_catalog: config.prompt_catalog,
            context_window: config.context_window,
            max_output_tokens: config.max_output_tokens,
            bash_allowed_commands: config.bash_allowed_commands,
        })
    }

    pub fn is_ready(&self) -> bool {
        self.agent_slot.lock().unwrap().agent.is_some()
    }

    pub fn command_hint(&self, input: &str) -> Option<String> {
        if !input.trim_start().starts_with('/') {
            return None;
        }

        let registry = command_registry(&self.context_facade);
        Some(render_command_hint(registry.resolve_hint(input)))
    }

    pub fn checkpoint_current_messages(&self) {
        let current_messages = self
            .agent_slot
            .lock()
            .unwrap()
            .agent
            .as_ref()
            .map(|agent| agent.messages().to_vec())
            .unwrap_or_default();
        *self.turn_checkpoint.lock().unwrap() = current_messages;
    }

    pub fn interrupt(&self, replacement_turn_id: u64) -> anyhow::Result<()> {
        let checkpoint = self.turn_checkpoint.lock().unwrap().clone();
        let system = self.skill_catalog.build_system_prompt(
            &self.base_system,
            "",
            &StepSkillRequest::MatchTask,
        );
        let dispatcher = omega_core::create_default_tools_with_todo_manager_and_bash_allowlist(
            self.cwd.clone(),
            self.todo_manager.clone(),
            self.bash_allowed_commands.clone(),
        );
        let mut replacement = Agent::new(self.client.clone(), system, dispatcher)?;
        replacement.set_max_tokens(self.max_output_tokens);
        replacement.set_messages(checkpoint);

        let mut slot = self.agent_slot.lock().unwrap();
        slot.turn_id = replacement_turn_id;
        slot.agent = Some(replacement);
        self.active_turn_tx.send_replace(replacement_turn_id);
        Ok(())
    }

    pub fn spawn_turn(
        &self,
        input: String,
        turn_id: u64,
        tx: mpsc::Sender<RuntimeMessageEnvelope>,
    ) -> anyhow::Result<()> {
        self.spawn_turn_with_bridge(input, turn_id, Arc::new(tx))
    }

    pub fn spawn_turn_ui_compat(
        &self,
        input: String,
        turn_id: u64,
        tx: mpsc::Sender<RuntimeUiEnvelope>,
    ) -> anyhow::Result<()> {
        self.spawn_turn_with_bridge(input, turn_id, Arc::new(LegacyRuntimeUiBridge::new(tx)))
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn spawn_turn_with_test_bridge(
        &self,
        input: String,
        turn_id: u64,
        tx: SharedRuntimeMessageBridge,
    ) -> anyhow::Result<()> {
        self.spawn_turn_with_bridge(input, turn_id, tx)
    }

    fn spawn_turn_with_bridge(
        &self,
        input: String,
        turn_id: u64,
        tx: SharedRuntimeMessageBridge,
    ) -> anyhow::Result<()> {
        let agent_slot = self.agent_slot.clone();
        let mut slot = self.agent_slot.lock().unwrap();
        slot.turn_id = turn_id;
        let mut agent = match slot.agent.take() {
            Some(agent) => agent,
            None => return Err(anyhow::anyhow!("agent turn already in progress")),
        };
        drop(slot);
        self.active_turn_tx.send_replace(turn_id);

        let tx_callback = tx.clone();
        let tx_result = tx;
        let handle = self.runtime_handle.clone();
        let cancel_turn_rx = self.active_turn_tx.subscribe();
        let base_system = self.base_system.clone();
        let cwd = self.cwd.clone();
        let todo_manager = self.todo_manager.clone();
        let hook_host = self.hook_host.clone();
        let client = self.client.clone();
        let context_facade = self.context_facade.clone();
        let skill_catalog = self.skill_catalog.clone();
        let tool_catalog = self.tool_catalog.clone();
        let scene_catalog = self.scene_catalog.clone();
        let workflow_catalog = self.workflow_catalog.clone();
        let prompt_catalog = self.prompt_catalog.clone();
        let session_context = self.session_context.clone();
        let context_window = self.context_window;
        let max_output_tokens = self.max_output_tokens;
        thread::spawn(move || {
            let thread_turn_rx = cancel_turn_rx.clone();
            agent.add_user_message(&input);
            let mut turn_context = {
                let mut shared = session_context.lock().unwrap();
                shared.begin_turn(input.clone(), scene_catalog.root_workflow_id.clone());
                shared.clone()
            };
            info!(
                turn_id,
                root_workflow_id = %scene_catalog.root_workflow_id,
                latest_user_turn = %preview_text(&input, 160),
                carried_step_summaries = turn_context.step_summaries.len(),
                "session turn started"
            );
            let runner = runner::WorkflowTurnRunner::new(
                &handle,
                &client,
                &context_facade,
                &skill_catalog,
                &tool_catalog,
                &base_system,
                &input,
                &cwd,
                &todo_manager,
                &hook_host,
                &scene_catalog,
                &workflow_catalog,
                &prompt_catalog,
                context_window,
                max_output_tokens,
                turn_id,
                cancel_turn_rx,
                tx_callback.clone(),
                tx_result.clone(),
            );
            let result = runner.run(&mut agent, &mut turn_context);
            let turn_still_active = *thread_turn_rx.borrow() == turn_id;

            if turn_still_active {
                let mut shared = session_context.lock().unwrap();
                *shared = turn_context;
                info!(
                    turn_id,
                    recognized_scene_id = %shared.routing.recognized_scene_id.as_deref().unwrap_or("-"),
                    selected_workflow_id = %shared.routing.selected_workflow_id.as_deref().unwrap_or("-"),
                    active_workflow_id = %shared.routing.active_workflow_id,
                    active_workflow_role = %shared.routing.active_workflow_role.as_str(),
                    stored_step_summaries = shared.step_summaries.len(),
                    "session turn context committed"
                );
            } else {
                info!(turn_id, "discarding canceled turn result");
            }

            match result {
                Ok(text) if turn_still_active && !text.is_empty() => {
                    ui_emit::send_assistant_text(&*tx_result, turn_id, &text);
                }
                Ok(_) => {}
                Err(e) => {
                    if turn_still_active {
                        error!(error = %e, "agent loop error");
                        ui_emit::send_error_text(&*tx_result, turn_id, &format!("Error: {e}"));
                    } else {
                        info!(turn_id, error = %e, "canceled turn stopped before completion");
                    }
                }
            }

            let mut slot = agent_slot.lock().unwrap();
            if slot.turn_id == turn_id {
                slot.agent = Some(agent);
            }

            if turn_still_active {
                ui_emit::send_turn_finished(&*tx_result, turn_id);
            }
        });

        Ok(())
    }

    pub fn spawn_command(
        &self,
        input: String,
        turn_id: u64,
        tx: mpsc::Sender<RuntimeMessageEnvelope>,
    ) -> anyhow::Result<()> {
        self.spawn_command_with_bridge(input, turn_id, Arc::new(tx))
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn spawn_command_with_test_bridge(
        &self,
        input: String,
        turn_id: u64,
        tx: SharedRuntimeMessageBridge,
    ) -> anyhow::Result<()> {
        self.spawn_command_with_bridge(input, turn_id, tx)
    }

    fn spawn_command_with_bridge(
        &self,
        input: String,
        turn_id: u64,
        tx: SharedRuntimeMessageBridge,
    ) -> anyhow::Result<()> {
        self.active_turn_tx.send_replace(turn_id);
        let context_facade = self.context_facade.clone();

        thread::spawn(move || {
            let registry = command_registry(&context_facade);
            let parsed = registry.parse(&input);
            let title = command_title_from_input(&input);
            let source = parsed
                .as_ref()
                .map(|invocation| invocation.source)
                .unwrap_or(OmegaCommandSource::Builtin);
            let section_id = begin_command_output(&*tx, turn_id, &title, source);
            let mut progress = |text: &str| append_command_output(&*tx, turn_id, &section_id, text);

            let output = match parsed {
                Ok(invocation) => execute_command(&context_facade, invocation, &mut progress),
                Err(error) => Err(anyhow::anyhow!(error)),
            };

            match output {
                Ok(output) => emit_command_output(&*tx, turn_id, &section_id, output),
                Err(error) => emit_command_output(
                    &*tx,
                    turn_id,
                    &section_id,
                    CommandExecutionOutput {
                        body: format!("Error: {error}"),
                        state: ResponseSectionState::Failed,
                        activity: format!("{} failed", command_title_from_input(&input)),
                    },
                ),
            }

            ui_emit::send_turn_finished(&*tx, turn_id);
        });

        Ok(())
    }
}

#[derive(Debug)]
struct CommandExecutionOutput {
    body: String,
    state: ResponseSectionState,
    activity: String,
}

fn command_registry(context_facade: &Arc<OmegaContextFacade>) -> OmegaCommandRegistry {
    let facade = Arc::clone(context_facade);
    OmegaCommandRegistry::new(vec![OmegaCommandDescriptor::new(
        "document",
        vec!["doc".to_string()],
        None,
        vec![
            OmegaCommandSubcommand::new("init", "Initialize document indexes", None),
            OmegaCommandSubcommand::new("sync", "Refresh indexed workspace documents", None),
            OmegaCommandSubcommand::new("health", "Check repository document health", None),
            OmegaCommandSubcommand::new(
                "query",
                "Search indexed project documents",
                Some("<text>"),
            ),
            OmegaCommandSubcommand::new(
                "create",
                "Create a managed document from a template",
                Some("<path> <doc_type> <title...>"),
            ),
            OmegaCommandSubcommand::new(
                "archive",
                "Archive a managed document",
                Some("<path> [reason] [replaced_by]"),
            ),
            OmegaCommandSubcommand::new(
                "list",
                "List tracked documents",
                Some("[doc_type] [status]"),
            ),
        ],
        "Manage workspace document indexing and query operations.",
        OmegaCommandSource::Builtin,
        Arc::new(move || facade.document_backend_enabled),
    )])
}

fn execute_command(
    context_facade: &Arc<OmegaContextFacade>,
    invocation: OmegaCommandInvocation,
    progress: &mut dyn FnMut(&str),
) -> anyhow::Result<CommandExecutionOutput> {
    match invocation.name.as_str() {
        "document" => execute_document_command(context_facade, invocation, progress),
        _ => Err(anyhow::anyhow!("unsupported command '/{}'", invocation.name)),
    }
}

fn execute_document_command(
    context_facade: &Arc<OmegaContextFacade>,
    invocation: OmegaCommandInvocation,
    progress: &mut dyn FnMut(&str),
) -> anyhow::Result<CommandExecutionOutput> {
    if !context_facade.document_backend_enabled {
        return Err(anyhow::anyhow!(
            "document backend disabled; rebuild with feature 'document-backend' enabled"
        ));
    }

    let Some(subcommand) = invocation.subcommand.as_deref() else {
        return Err(anyhow::anyhow!(
            "missing subcommand for '/document'; expected init, sync, health, query, create, archive, or list"
        ));
    };

    match subcommand {
        "init" | "sync" => {
            context_facade.diagnostics.record_document_usage(
                "/document",
                "builtin_command",
                &format!("subcommand={subcommand}"),
            );
            progress("Phase: load storeignore rules and scan workspace");
            let scan = context_facade.query.scan_workspace()?;
            progress("Phase: scan complete, preparing command summary");
            context_facade.diagnostics.record_document_scan(&scan);
            let ignored_summary = if scan.vector_ignored_paths.is_empty() {
                String::new()
            } else {
                format!(
                    "\nIgnored paths:\n- {}",
                    scan.vector_ignored_paths.join("\n- ")
                )
            };
            let indexed_summary = if scan.indexed_paths.is_empty() {
                String::new()
            } else {
                format!("\nIndexed files:\n- {}", scan.indexed_paths.join("\n- "))
            };
            let embedded_summary = if scan.embedded_paths.is_empty() {
                String::new()
            } else {
                format!(
                    "\nEmbedded files:\n- {}",
                    scan.embedded_paths.join("\n- ")
                )
            };
            Ok(CommandExecutionOutput {
                body: format!(
                    "Phase: scan workspace\nIndexed {} files\nChunks indexed: {}\nDeleted marked: {}\nStoreignored skipped: {}\nManifest: {}\nKeyword index: {}\nActive version: {}\nPending version: {}\nArchived version path: {}{}{}{}",
                    scan.files_indexed,
                    scan.chunks_indexed,
                    scan.deleted_marked,
                    scan.vector_ignored_files,
                    scan.manifest_path,
                    scan.keyword_index_path,
                    format_store_version(scan.active_version.as_ref()),
                    format_store_version(scan.pending_version.as_ref()),
                    scan.archived_version_path.as_deref().unwrap_or("none"),
                    ignored_summary,
                    indexed_summary,
                    embedded_summary,
                ),
                state: ResponseSectionState::Complete,
                activity: format!("/document {subcommand} indexed {} files", scan.files_indexed),
            })
        }
        "health" => {
            context_facade.diagnostics.record_document_usage(
                "/document",
                "builtin_command",
                "subcommand=health",
            );
            let health = context_facade.governance.check_document_health()?;
            context_facade.diagnostics.record_document_health(&health);
            let snapshot = context_facade.diagnostics.context_diagnostics();
            Ok(CommandExecutionOutput {
                body: format!(
                    "Overall health: {}\nHealth status: {}\nTotal docs: {}\nStructure violations: {}\nNaming violations: {}\nBroken crossrefs: {}\nMissing frontmatter: {}\nStale docs: {}\nLast health check: {}\nActive version: {}\nPending version: {}\nPromotion error: {}",
                    health_score_label(health.overall_health),
                    snapshot.document.health_status.as_str(),
                    health.total_docs,
                    health.structure_violations.len(),
                    health.naming_violations.len(),
                    health.broken_crossrefs.len(),
                    health.missing_frontmatter.len(),
                    health.stale_docs.len(),
                    snapshot
                        .document
                        .last_health_check
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "never".to_string()),
                    format_store_version(snapshot.document.active_version.as_ref()),
                    format_store_version(snapshot.document.pending_version.as_ref()),
                    snapshot
                        .document
                        .last_promotion_error
                        .as_deref()
                        .unwrap_or("none"),
                ),
                state: ResponseSectionState::Complete,
                activity: "/document health completed".to_string(),
            })
        }
        "query" => {
            let query_text = invocation.args.join(" ");
            if query_text.trim().is_empty() {
                return Err(anyhow::anyhow!("missing search text for '/document query'"));
            }
            context_facade.diagnostics.record_document_usage(
                "/document",
                "builtin_command",
                &format!("subcommand=query query={query_text}"),
            );
            let scan = context_facade.query.scan_workspace()?;
            context_facade.diagnostics.record_document_scan(&scan);
            let results = context_facade.query.search(SearchQuery {
                text: Some(query_text.clone()),
                mode: SearchMode::Hybrid,
                filters: Vec::new(),
                sort: None,
                max_results: 10,
            })?;
            Ok(CommandExecutionOutput {
                body: render_document_query_results(&query_text, &results),
                state: ResponseSectionState::Complete,
                activity: format!("/document query returned {} results", results.len()),
            })
        }
        "create" => {
            if invocation.args.len() < 3 {
                return Err(anyhow::anyhow!(
                    "usage: /document create <path> <doc_type> <title...>"
                ));
            }
            context_facade.diagnostics.record_document_usage(
                "/document",
                "builtin_command",
                "subcommand=create",
            );

            let path = invocation.args[0].clone();
            let doc_type = parse_doc_type(&invocation.args[1]).ok_or_else(|| {
                anyhow::anyhow!("unknown doc_type '{}' for '/document create'", invocation.args[1])
            })?;
            let title = invocation.args[2..].join(" ");
            let result = context_facade.governance.manage_document(DocumentOp::Create {
                mode: DocumentMutationMode::Apply,
                path: path.clone(),
                doc_type,
                title: title.clone(),
                content: document_template(doc_type, &title),
            })?;

            Ok(CommandExecutionOutput {
                body: render_document_operation_result(&result),
                state: state_from_document_result(&result),
                activity: format!("/document create wrote {}", path),
            })
        }
        "archive" => {
            if invocation.args.is_empty() {
                return Err(anyhow::anyhow!(
                    "usage: /document archive <path> [reason] [replaced_by]"
                ));
            }
            context_facade.diagnostics.record_document_usage(
                "/document",
                "builtin_command",
                "subcommand=archive",
            );

            let path = invocation.args[0].clone();
            let reason = invocation
                .args
                .get(1)
                .and_then(|value| parse_archive_trigger(value))
                .unwrap_or(ArchiveTrigger::HistoryOnly);
            let replaced_by = invocation.args.get(2).cloned();
            let result = context_facade.governance.manage_document(DocumentOp::Archive {
                mode: DocumentMutationMode::Apply,
                path: path.clone(),
                reason,
                replaced_by,
            })?;

            Ok(CommandExecutionOutput {
                body: render_document_operation_result(&result),
                state: state_from_document_result(&result),
                activity: format!("/document archive updated {}", path),
            })
        }
        "list" => {
            context_facade.diagnostics.record_document_usage(
                "/document",
                "builtin_command",
                "subcommand=list",
            );
            let doc_type = invocation.args.first().and_then(|value| parse_doc_type(value));
            let status = invocation.args.get(1).and_then(|value| parse_file_status(value));
            let result = context_facade
                .governance
                .manage_document(DocumentOp::List {
                    doc_type,
                    status: status.clone(),
                })?;

            Ok(CommandExecutionOutput {
                body: render_document_list_result(&result.files, doc_type, status),
                state: state_from_document_result(&result),
                activity: format!("/document list returned {} files", result.files.len()),
            })
        }
        other => Err(anyhow::anyhow!(
            "unsupported '/document' subcommand '{other}'"
        )),
    }
}

fn render_document_query_results(
    query_text: &str,
    results: &[omega_context::SearchResult],
) -> String {
    if results.is_empty() {
        return format!("Query: {query_text}\nNo indexed documents matched.");
    }

    let mut body = format!("Query: {query_text}\nResults: {}", results.len());
    for result in results.iter().take(5) {
        body.push_str(&format!(
            "\n- {} ({}, score {:.2})\n  {}",
            result.path,
            search_mode_label(result.mode_used),
            result.score,
            preview_text(&result.preview, 140),
        ));
    }
    body
}

fn render_document_operation_result(result: &omega_context::DocumentOpResult) -> String {
    let mut body = result.message.clone();

    if let Some(mode) = result.mode {
        body.push_str(&format!("\nMode: {}", mutation_mode_label(mode)));
    }

    if !result.files.is_empty() {
        body.push_str(&format!("\nFiles: {}", result.files.len()));
        for file in result.files.iter().take(5) {
            body.push_str(&format!(
                "\n- {} ({}, {})",
                file.path,
                doc_type_label(file.doc_type),
                file_status_label(&file.status),
            ));
        }
    }

    if !result.warnings.is_empty() {
        body.push_str("\nWarnings:");
        for warning in &result.warnings {
            body.push_str(&format!("\n- {warning}"));
        }
    }

    body
}

fn render_document_list_result(
    files: &[FileRecord],
    doc_type: Option<DocType>,
    status: Option<FileStatus>,
) -> String {
    let mut body = format!(
        "Tracked documents: {}\nFilter: doc_type={} status={}",
        files.len(),
        doc_type_label(doc_type),
        status_label(status.as_ref()),
    );

    if files.is_empty() {
        body.push_str("\nNo tracked documents matched.");
        return body;
    }

    for file in files.iter().take(10) {
        body.push_str(&format!(
            "\n- {} ({}, {})",
            file.path,
            doc_type_label(file.doc_type),
            file_status_label(&file.status),
        ));
    }
    body
}

fn render_command_hint(resolution: CommandHintResolution) -> String {
    match resolution {
        CommandHintResolution::TopLevel(commands) => {
            let summary = commands
                .into_iter()
                .map(|command| format_hint_item(&format!("/{}", command.name), &command))
                .collect::<Vec<_>>()
                .join("  •  ");
            format!(" Slash: {summary}")
        }
        CommandHintResolution::Command {
            command,
            subcommands,
        } => {
            let list = subcommands
                .into_iter()
                .take(4)
                .map(|subcommand| format_hint_item(&subcommand.name, &subcommand))
                .collect::<Vec<_>>()
                .join("  •  ");
            if list.is_empty() {
                format_hint_line(&format!("/{}", command.name), &command)
            } else {
                format!(
                    " {}  •  subcommands: {}",
                    format_hint_line(&format!("/{}", command.name), &command),
                    list,
                )
            }
        }
        CommandHintResolution::Ready {
            command,
            subcommand,
            args,
        } => {
            if let Some(subcommand) = subcommand {
                let pending = if args.is_empty() {
                    String::new()
                } else {
                    format!("  •  args: {}", args.join(" "))
                };
                format!(
                    " Ready: /{} {}{}",
                    command.name,
                    format_hint_item(&subcommand.name, &subcommand),
                    pending,
                )
            } else {
                format!(" Ready: {}", format_hint_line(&format!("/{}", command.name), &command))
            }
        }
        CommandHintResolution::Disabled { command, .. } => {
            format!(
                " Slash unavailable: /{} — {}",
                command.name, command.description
            )
        }
        CommandHintResolution::NoMatch { input } => {
            format!(" Slash: no command matches '/{}'", input)
        }
    }
}

fn format_hint_line(label: &str, hint: &CommandHint) -> String {
    let mut line = format!("{} — {}", label, hint.description);
    if let Some(argument_hint) = hint.argument_hint.as_deref() {
        line.push_str(&format!(" {}", argument_hint));
    }
    line
}

fn format_hint_item(label: &str, hint: &CommandHint) -> String {
    match hint.argument_hint.as_deref() {
        Some(argument_hint) => format!("{} {}", label, argument_hint),
        None => format!("{}", label),
    }
}

fn document_template(doc_type: DocType, title: &str) -> String {
    match doc_type {
        DocType::Readme | DocType::Todo | DocType::Changelog => {
            format!("# {title}\n")
        }
        _ => format!("---\nstatus: draft\ntitle: {title}\n---\n\n# {title}\n"),
    }
}

fn parse_doc_type(value: &str) -> Option<DocType> {
    match value.to_ascii_lowercase().as_str() {
        "spec" | "specs" => Some(DocType::Spec),
        "prd" | "prds" => Some(DocType::Prd),
        "guide" | "guides" => Some(DocType::Guide),
        "adr" | "adrs" | "decision" | "decisions" => Some(DocType::Adr),
        "todo" => Some(DocType::Todo),
        "archive" | "archived" => Some(DocType::Archive),
        "readme" => Some(DocType::Readme),
        "changelog" => Some(DocType::Changelog),
        _ => None,
    }
}

fn parse_file_status(value: &str) -> Option<FileStatus> {
    match value.to_ascii_lowercase().as_str() {
        "active" => Some(FileStatus::Active),
        "archived" | "archive" => Some(FileStatus::Archived),
        "deleted" => Some(FileStatus::Deleted),
        _ => None,
    }
}

fn parse_archive_trigger(value: &str) -> Option<ArchiveTrigger> {
    match value.to_ascii_lowercase().as_str() {
        "superseded" => Some(ArchiveTrigger::Superseded),
        "completed" | "completed_and_inactive" | "inactive" => {
            Some(ArchiveTrigger::CompletedAndInactive)
        }
        "outdated" | "structurally_outdated" => Some(ArchiveTrigger::StructurallyOutdated),
        "history" | "history_only" => Some(ArchiveTrigger::HistoryOnly),
        _ => None,
    }
}

fn state_from_document_result(result: &omega_context::DocumentOpResult) -> ResponseSectionState {
    if result.ok {
        ResponseSectionState::Complete
    } else {
        ResponseSectionState::Failed
    }
}

fn mutation_mode_label(mode: DocumentMutationMode) -> &'static str {
    match mode {
        DocumentMutationMode::Check => "check",
        DocumentMutationMode::Plan => "plan",
        DocumentMutationMode::Apply => "apply",
    }
}

fn doc_type_label(doc_type: Option<DocType>) -> &'static str {
    match doc_type {
        Some(DocType::Spec) => "spec",
        Some(DocType::Prd) => "prd",
        Some(DocType::Guide) => "guide",
        Some(DocType::Adr) => "adr",
        Some(DocType::Todo) => "todo",
        Some(DocType::Archive) => "archive",
        Some(DocType::Readme) => "readme",
        Some(DocType::Changelog) => "changelog",
        None => "any",
    }
}

fn status_label(status: Option<&FileStatus>) -> &'static str {
    match status {
        Some(FileStatus::Active) => "active",
        Some(FileStatus::Deleted) => "deleted",
        Some(FileStatus::Archived) => "archived",
        Some(FileStatus::Moved { .. }) => "moved",
        None => "any",
    }
}

fn file_status_label(status: &FileStatus) -> &'static str {
    status_label(Some(status))
}

fn health_score_label(score: HealthScore) -> &'static str {
    match score {
        HealthScore::Good => "good",
        HealthScore::NeedsAttention => "needs_attention",
        HealthScore::Critical => "critical",
    }
}

fn format_store_version(version: Option<&DocumentStoreVersion>) -> String {
    version
        .map(|version| {
            format!(
                "{} rev={} path={}",
                version.version_id, version.manifest_revision, version.storage_path
            )
        })
        .unwrap_or_else(|| "none".to_string())
}

fn search_mode_label(mode: SearchMode) -> &'static str {
    match mode {
        SearchMode::Keyword => "keyword",
        SearchMode::Semantic => "semantic",
        SearchMode::Hybrid => "hybrid",
    }
}

fn append_command_output(
    tx: &dyn RuntimeMessageBridge,
    turn_id: u64,
    section_id: &str,
    text: &str,
) {
    tx.send(RuntimeMessageEnvelope::conversation(
        turn_id,
        ConversationMessage::AppendSection {
            id: section_id.to_string(),
            delta: ResponseSectionDelta::Text(format!("\n{text}")),
        },
    ));
}

fn emit_command_output(
    tx: &dyn RuntimeMessageBridge,
    turn_id: u64,
    section_id: &str,
    output: CommandExecutionOutput,
) {
    append_command_output(tx, turn_id, section_id, &output.body);
    tx.send(RuntimeMessageEnvelope::conversation(
        turn_id,
        ConversationMessage::CompleteSection {
            id: section_id.to_string(),
            state: output.state,
        },
    ));
    tx.send(RuntimeMessageEnvelope::state(
        turn_id,
        StateMessage::Activity {
            source: RuntimeSource::System,
            kind: if output.state == ResponseSectionState::Failed {
                RuntimeContentKind::Error
            } else {
                RuntimeContentKind::Result
            },
            text: output.activity,
            priority: None,
        },
    ));
}

fn begin_command_output(
    tx: &dyn RuntimeMessageBridge,
    turn_id: u64,
    title: &str,
    source: OmegaCommandSource,
) -> String {
    let section_id = format!("turn-{turn_id}:command");
    tx.send(RuntimeMessageEnvelope::conversation(
        turn_id,
        ConversationMessage::BeginSection {
            section: ResponseSection {
                id: section_id.clone(),
                parent_id: None,
                kind: ResponseSectionKind::Command,
                title: title.to_string(),
                state: ResponseSectionState::Streaming,
                metadata: ResponseSectionMetadata {
                    scene_id: None,
                    origin: SectionOrigin::Command {
                        command_name: title.to_string(),
                        source: source.as_str().to_string(),
                    },
                    step_id: None,
                    step_label: None,
                    subflow_ref: None,
                },
            },
        },
    ));
    tx.send(RuntimeMessageEnvelope::conversation(
        turn_id,
        ConversationMessage::AppendSection {
            id: section_id.clone(),
            delta: ResponseSectionDelta::Text(format!("Running {title}...")),
        },
    ));
    section_id
}

fn command_title_from_input(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return "/command".to_string();
    }
    trimmed
        .split_whitespace()
        .take(2)
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn preview_text(text: &str, limit: usize) -> String {
    let mut chars = text.chars();
    let preview: String = chars.by_ref().take(limit).collect();
    if chars.next().is_some() {
        format!("{}...", preview)
    } else {
        text.to_string()
    }
}

pub(crate) fn preview_json_value(value: &serde_json::Value, limit: usize) -> String {
    preview_text(
        &serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string()),
        limit,
    )
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
