use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

use omega_core::{Agent, DynLlmClient, Message};
use omega_skills::SkillLoader;
use omega_workflow::{
    SceneCatalog, StepLoopMode, WorkflowCatalog, WorkflowPromptCatalog, WorkflowPrompts,
    WorkflowStep, WorkflowStepState, FEATURE_WORKFLOW_ID, SCENE_RECOGNITION_STEP_ID,
    SELECT_WORKFLOW_STEP_ID,
};
use tokio::runtime::Handle;
use tracing::error;

mod runtime_ui;
mod skill_catalog;
mod tool_catalog;

pub use omega_workflow::{StepSkillRequest, StepToolRequest};
pub use runtime_ui::{
    ActivityTarget, OverlayRequest, OverlayTarget, RuntimeUiBridge, RuntimeUiEffect,
    RuntimeUiEnvelope, RuntimeUiMessage, RuntimeUiSink, SessionRuntimeContext, StatusSlot,
    StatusValue, UiContent, UiMessageKind, UiPriority, UiSource, UiTarget,
};
pub use skill_catalog::{ResolvedSkillSet, SessionSkillCatalog};
pub use tool_catalog::{ResolvedToolSet, SessionToolCatalog};

pub struct AgentSessionConfig {
    pub client: DynLlmClient,
    pub system: String,
    pub cwd: PathBuf,
    pub runtime_handle: Handle,
    pub scene_catalog: SceneCatalog,
    pub workflow_catalog: WorkflowCatalog,
    pub prompt_catalog: WorkflowPromptCatalog,
}

struct AgentSlot {
    turn_id: u64,
    agent: Option<Agent>,
}

pub struct AgentSession {
    agent_slot: Arc<Mutex<AgentSlot>>,
    turn_checkpoint: Arc<Mutex<Vec<Message>>>,
    client: DynLlmClient,
    base_system: String,
    cwd: PathBuf,
    skill_catalog: Arc<SessionSkillCatalog>,
    tool_catalog: Arc<SessionToolCatalog>,
    runtime_handle: Handle,
    scene_catalog: SceneCatalog,
    workflow_catalog: WorkflowCatalog,
    prompt_catalog: WorkflowPromptCatalog,
}

impl AgentSession {
    pub fn new(config: AgentSessionConfig) -> anyhow::Result<Self> {
        let skill_loader = SkillLoader::from_repo_root(&config.cwd)?;
        let skill_catalog = Arc::new(SessionSkillCatalog::new(skill_loader));
        let dispatcher = omega_core::create_default_tools(config.cwd.clone());
        let tool_catalog = Arc::new(SessionToolCatalog::new(
            dispatcher
                .tool_names()
                .into_iter()
                .map(ToOwned::to_owned)
                .collect(),
        ));
        let initial_system =
            skill_catalog.build_system_prompt(&config.system, "", &StepSkillRequest::MatchTask);
        let agent = Agent::new(config.client.clone(), initial_system, dispatcher)?;
        let checkpoint = agent.messages().to_vec();

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
            client: config.client,
            base_system: config.system,
            cwd: config.cwd,
            skill_catalog,
            tool_catalog,
            runtime_handle: config.runtime_handle,
            scene_catalog: config.scene_catalog,
            workflow_catalog: config.workflow_catalog,
            prompt_catalog: config.prompt_catalog,
        })
    }

    pub fn is_ready(&self) -> bool {
        self.agent_slot.lock().unwrap().agent.is_some()
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
        let dispatcher = omega_core::create_default_tools(self.cwd.clone());
        let mut replacement = Agent::new(self.client.clone(), system, dispatcher)?;
        replacement.set_messages(checkpoint);

        let mut slot = self.agent_slot.lock().unwrap();
        slot.turn_id = replacement_turn_id;
        slot.agent = Some(replacement);
        Ok(())
    }

    pub fn spawn_turn(
        &self,
        input: String,
        turn_id: u64,
        tx: mpsc::Sender<RuntimeUiEnvelope>,
    ) -> anyhow::Result<()> {
        self.agent_slot.lock().unwrap().turn_id = turn_id;

        let agent_slot = self.agent_slot.clone();
        let mut agent = match self.agent_slot.lock().unwrap().agent.take() {
            Some(agent) => agent,
            None => return Err(anyhow::anyhow!("agent turn already in progress")),
        };

        let tx_callback = tx.clone();
        let tx_result = tx;
        let handle = self.runtime_handle.clone();
        let base_system = self.base_system.clone();
        let skill_catalog = self.skill_catalog.clone();
        let tool_catalog = self.tool_catalog.clone();
        let scene_catalog = self.scene_catalog.clone();
        let workflow_catalog = self.workflow_catalog.clone();
        let prompt_catalog = self.prompt_catalog.clone();
        thread::spawn(move || {
            agent.add_user_message(&input);
            let runner = WorkflowTurnRunner {
                handle: &handle,
                skill_catalog: &skill_catalog,
                tool_catalog: &tool_catalog,
                base_system: &base_system,
                input: &input,
                scene_catalog: &scene_catalog,
                workflow_catalog: &workflow_catalog,
                prompt_catalog: &prompt_catalog,
                turn_id,
                tx_callback: &tx_callback,
                tx_result: &tx_result,
            };
            let result = runner.run(&mut agent);

            match result {
                Ok(text) if !text.is_empty() => {
                    send_assistant_text(&tx_result, turn_id, &text);
                }
                Ok(_) => {}
                Err(e) => {
                    error!(error = %e, "agent loop error");
                    send_error_text(&tx_result, turn_id, &format!("Error: {e}"));
                }
            }

            let mut slot = agent_slot.lock().unwrap();
            if slot.turn_id == turn_id {
                slot.agent = Some(agent);
            }

            send_turn_finished(&tx_result, turn_id);
        });

        Ok(())
    }
}

struct WorkflowTurnRunner<'a> {
    handle: &'a Handle,
    skill_catalog: &'a Arc<SessionSkillCatalog>,
    tool_catalog: &'a Arc<SessionToolCatalog>,
    base_system: &'a str,
    input: &'a str,
    scene_catalog: &'a SceneCatalog,
    workflow_catalog: &'a WorkflowCatalog,
    prompt_catalog: &'a WorkflowPromptCatalog,
    turn_id: u64,
    tx_callback: &'a mpsc::Sender<RuntimeUiEnvelope>,
    tx_result: &'a mpsc::Sender<RuntimeUiEnvelope>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkflowExecutionRole {
    Root,
    Child,
}

impl WorkflowExecutionRole {
    fn as_str(self) -> &'static str {
        match self {
            Self::Root => "root",
            Self::Child => "child",
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct WorkflowRoutingState {
    recognized_scene_id: Option<String>,
    selected_workflow_id: Option<String>,
}

impl WorkflowTurnRunner<'_> {
    fn run(&self, agent: &mut Agent) -> anyhow::Result<String> {
        let mut routing = WorkflowRoutingState::default();
        self.send_session_status(&routing);
        self.send_runtime_log(format!(
            "Routing turn through root workflow '{}'.",
            self.scene_catalog.root_workflow_id
        ));

        self.run_workflow(
            agent,
            &self.scene_catalog.root_workflow_id,
            WorkflowExecutionRole::Root,
            &mut routing,
        )?;

        let selected_workflow_id = self.ensure_selected_workflow(&mut routing);
        self.send_runtime_log(format!(
            "Delegating to child workflow '{}'.",
            selected_workflow_id
        ));

        self.run_workflow(
            agent,
            &selected_workflow_id,
            WorkflowExecutionRole::Child,
            &mut routing,
        )
    }

    fn run_workflow(
        &self,
        agent: &mut Agent,
        workflow_id: &str,
        role: WorkflowExecutionRole,
        routing: &mut WorkflowRoutingState,
    ) -> anyhow::Result<String> {
        let (definition, prompts) = self.resolve_workflow_bundle(workflow_id)?;
        let mut last_text = String::new();
        let mut run = definition.start_run();

        loop {
            let Some(step_state) = run.current_step() else {
                break;
            };
            let Some(step) = run.current_step_definition().cloned() else {
                break;
            };
            let is_final_step = step_state.index == step_state.total;

            send_workflow_step(
                self.tx_result,
                self.turn_id,
                Some(step_state),
                workflow_id,
                role,
            );

            let routing_context = self.build_routing_context(workflow_id, role, routing);
            let system = build_step_system_prompt(
                self.skill_catalog,
                self.base_system,
                self.input,
                &step,
                prompts.prompt_for(&step.id).unwrap_or_default(),
                routing_context.as_deref(),
            );
            agent.set_system(system);

            let checkpoint = if role == WorkflowExecutionRole::Root {
                Some(agent.messages().to_vec())
            } else {
                None
            };
            let stage_text = self.execute_step(agent, &step)?;
            if let Some(checkpoint) = checkpoint {
                agent.set_messages(checkpoint);
            }

            self.apply_step_transition(workflow_id, role, &step, &stage_text, routing);

            if !stage_text.is_empty() {
                if role == WorkflowExecutionRole::Child && !is_final_step {
                    send_step_text(self.tx_result, self.turn_id, &step, &stage_text);
                }
                last_text = stage_text;
            }

            if run.advance().is_none() {
                break;
            }
        }

        Ok(last_text)
    }

    fn execute_step(&self, agent: &mut Agent, step: &WorkflowStep) -> anyhow::Result<String> {
        match step.loop_mode {
            StepLoopMode::ToolLoop => {
                let resolved_tools = self.tool_catalog.resolve_for_step(&step.tool_request);
                let tool_name_refs = resolved_tools.tool_name_refs();
                agent.set_visible_tools(Some(&tool_name_refs));

                self.handle.block_on(agent.run_loop_with({
                    let tx_callback = self.tx_callback.clone();
                    let turn_id = self.turn_id;
                    move |name, tool_input, output| {
                        let command = if name == "bash" {
                            tool_input
                                .get("command")
                                .and_then(|value| value.as_str())
                                .map(ToOwned::to_owned)
                        } else {
                            None
                        };

                        send_tool_call_preview(
                            &tx_callback,
                            turn_id,
                            name,
                            command,
                            preview_text(output, 100),
                        );

                        if name == "todo" && !output.starts_with("Error:") {
                            send_todo_snapshot(&tx_callback, turn_id, output);
                        }
                    }
                }))
            }
            StepLoopMode::SingleResponse => {
                agent.set_visible_tools(Some(&[]));
                self.handle.block_on(agent.run_single_response())
            }
        }
    }

    fn resolve_workflow_bundle(
        &self,
        workflow_id: &str,
    ) -> anyhow::Result<(&omega_workflow::WorkflowDefinition, &WorkflowPrompts)> {
        let definition = self
            .workflow_catalog
            .workflow(workflow_id)
            .ok_or_else(|| anyhow::anyhow!("missing workflow '{}' in catalog", workflow_id))?;
        let prompts = self.prompt_catalog.prompts_for_workflow(workflow_id).ok_or_else(|| {
            anyhow::anyhow!("missing workflow prompt set for '{}'", workflow_id)
        })?;
        Ok((definition, prompts))
    }

    fn build_routing_context(
        &self,
        workflow_id: &str,
        role: WorkflowExecutionRole,
        routing: &WorkflowRoutingState,
    ) -> Option<String> {
        let mut lines = vec![
            format!("Workflow role: {}", role.as_str()),
            format!("Active workflow: {workflow_id}"),
        ];
        if let Some(scene_id) = routing.recognized_scene_id.as_deref() {
            lines.push(format!("Recognized scene: {scene_id}"));
        }
        if let Some(selected_workflow_id) = routing.selected_workflow_id.as_deref() {
            lines.push(format!("Selected workflow: {selected_workflow_id}"));
        }
        Some(lines.join("\n"))
    }

    fn apply_step_transition(
        &self,
        workflow_id: &str,
        role: WorkflowExecutionRole,
        step: &WorkflowStep,
        stage_text: &str,
        routing: &mut WorkflowRoutingState,
    ) {
        if role != WorkflowExecutionRole::Root {
            return;
        }

        match step.id.as_str() {
            SCENE_RECOGNITION_STEP_ID => {
                let scene_id = self.resolve_scene_from_output(stage_text);
                routing.recognized_scene_id = Some(scene_id.clone());
                routing.selected_workflow_id = None;
                self.send_session_status(routing);
                self.send_runtime_log(format!(
                    "Recognized scene '{}' via workflow '{}'.",
                    scene_id, workflow_id
                ));
            }
            SELECT_WORKFLOW_STEP_ID => {
                let scene_id = routing
                    .recognized_scene_id
                    .clone()
                    .unwrap_or_else(|| self.scene_catalog.default_scene_id.clone());
                let workflow_id = self.resolve_workflow_from_output(stage_text, &scene_id);
                routing.selected_workflow_id = Some(workflow_id.clone());
                self.send_session_status(routing);
                self.send_runtime_log(format!(
                    "Selected workflow '{}' for scene '{}'.",
                    workflow_id, scene_id
                ));
            }
            _ => {}
        }
    }

    fn resolve_scene_from_output(&self, stage_text: &str) -> String {
        match find_catalog_match(stage_text, self.scene_catalog.scenes.iter().map(|scene| scene.id.as_str())) {
            Some(scene_id) => scene_id,
            None => {
                let fallback = self.scene_catalog.default_scene_id.clone();
                send_warning_text(
                    self.tx_result,
                    self.turn_id,
                    &format!(
                        "Scene recognition did not resolve a configured scene; defaulting to '{}'.",
                        fallback
                    ),
                );
                fallback
            }
        }
    }

    fn resolve_workflow_from_output(&self, stage_text: &str, scene_id: &str) -> String {
        let mapped_workflow = self
            .scene_catalog
            .scene(scene_id)
            .map(|scene| scene.workflow_id.clone())
            .unwrap_or_else(|| FEATURE_WORKFLOW_ID.to_string());

        match find_catalog_match(stage_text, self.workflow_catalog.workflow_ids()) {
            Some(workflow_id)
                if workflow_id != self.scene_catalog.root_workflow_id
                    && self.workflow_catalog.workflow(&workflow_id).is_some() =>
            {
                workflow_id
            }
            _ => mapped_workflow,
        }
    }

    fn ensure_selected_workflow(&self, routing: &mut WorkflowRoutingState) -> String {
        if routing.recognized_scene_id.is_none() {
            routing.recognized_scene_id = Some(self.scene_catalog.default_scene_id.clone());
            self.send_session_status(routing);
        }

        if routing.selected_workflow_id.is_none() {
            let scene_id = routing
                .recognized_scene_id
                .clone()
                .unwrap_or_else(|| self.scene_catalog.default_scene_id.clone());
            let workflow_id = self
                .scene_catalog
                .scene(&scene_id)
                .map(|scene| scene.workflow_id.clone())
                .unwrap_or_else(|| FEATURE_WORKFLOW_ID.to_string());
            routing.selected_workflow_id = Some(workflow_id.clone());
            self.send_session_status(routing);
            self.send_runtime_log(format!(
                "Workflow selection fell back to '{}' for scene '{}'.",
                workflow_id, scene_id
            ));
        }

        let selected_workflow_id = routing
            .selected_workflow_id
            .clone()
            .unwrap_or_else(|| FEATURE_WORKFLOW_ID.to_string());
        if selected_workflow_id == self.scene_catalog.root_workflow_id {
            let fallback = self
                .scene_catalog
                .scene(
                    routing
                        .recognized_scene_id
                        .as_deref()
                        .unwrap_or(&self.scene_catalog.default_scene_id),
                )
                .map(|scene| scene.workflow_id.clone())
                .unwrap_or_else(|| FEATURE_WORKFLOW_ID.to_string());
            routing.selected_workflow_id = Some(fallback.clone());
            self.send_session_status(routing);
            self.send_runtime_log(format!(
                "Ignoring root workflow as child target; using '{}' instead.",
                fallback
            ));
            return fallback;
        }

        selected_workflow_id
    }

    fn send_session_status(&self, routing: &WorkflowRoutingState) {
        let label = match (
            routing.recognized_scene_id.as_deref(),
            routing.selected_workflow_id.as_deref(),
        ) {
            (None, None) => format!("Route: {}", self.scene_catalog.root_workflow_id),
            (Some(scene_id), None) => format!("Scene: {} | Workflow: selecting", scene_id),
            (Some(scene_id), Some(workflow_id)) => {
                format!("Scene: {} | Workflow: {}", scene_id, workflow_id)
            }
            (None, Some(workflow_id)) => format!("Workflow: {}", workflow_id),
        };

        let _ = self.tx_result.send(RuntimeUiEnvelope::effect(
            self.turn_id,
            RuntimeUiEffect::SetStatusSlot {
                slot: StatusSlot::Session,
                value: StatusValue::Label(label),
            },
        ));
    }

    fn send_runtime_log(&self, text: String) {
        let _ = self.tx_result.send(RuntimeUiEnvelope::message(
            self.turn_id,
            RuntimeUiMessage {
                target: UiTarget::Activity(ActivityTarget::Log),
                source: UiSource::System,
                kind: UiMessageKind::Summary,
                content: UiContent::Text(text),
                priority: None,
            },
        ));
    }
}

fn find_catalog_match<'a>(text: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let candidates = candidates.into_iter().collect::<Vec<_>>();
    let normalized = text.to_ascii_lowercase();
    normalized
        .split(|character: char| {
            !character.is_ascii_alphanumeric() && character != '-' && character != '_'
        })
        .find_map(|token| {
            if token.is_empty() {
                return None;
            }
            candidates.iter().find_map(|candidate| {
                token.eq_ignore_ascii_case(candidate)
                    .then(|| (*candidate).to_string())
            })
        })
}

fn preview_text(text: &str, limit: usize) -> String {
    let mut chars = text.chars();
    let preview: String = chars.by_ref().take(limit).collect();
    if chars.next().is_some() {
        format!("{}...", preview)
    } else {
        text.to_string()
    }
}

fn send_workflow_step(
    tx: &mpsc::Sender<RuntimeUiEnvelope>,
    turn_id: u64,
    step: Option<WorkflowStepState>,
    workflow_id: &str,
    role: WorkflowExecutionRole,
) {
    let Some(step) = step else {
        return;
    };

    let _ = tx.send(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::SetStatusSlot {
            slot: StatusSlot::Workflow,
            value: StatusValue::WorkflowStep {
                step_id: step.id.clone(),
                step_label: step.label.clone(),
                index: step.index,
                total: step.total,
            },
        },
    ));
    let _ = tx.send(RuntimeUiEnvelope::message(
        turn_id,
        RuntimeUiMessage {
            target: UiTarget::Activity(ActivityTarget::Log),
            source: UiSource::System,
            kind: UiMessageKind::Summary,
            content: UiContent::Text(format!(
                "[{}:{} {}/{}] {}",
                role.as_str(),
                workflow_id,
                step.index,
                step.total,
                step.label
            )),
            priority: None,
        },
    ));
}

fn send_step_text(
    tx: &mpsc::Sender<RuntimeUiEnvelope>,
    turn_id: u64,
    step: &WorkflowStep,
    text: &str,
) {
    let _ = tx.send(RuntimeUiEnvelope::message(
        turn_id,
        RuntimeUiMessage {
            target: UiTarget::Response,
            source: UiSource::WorkflowStep {
                step_id: step.id.clone(),
                step_label: step.label.clone(),
                index: 0,
                total: 0,
            },
            kind: UiMessageKind::Narrative,
            content: UiContent::Text(text.to_string()),
            priority: None,
        },
    ));
}

fn send_assistant_text(tx: &mpsc::Sender<RuntimeUiEnvelope>, turn_id: u64, text: &str) {
    let _ = tx.send(RuntimeUiEnvelope::message(
        turn_id,
        RuntimeUiMessage {
            target: UiTarget::Response,
            source: UiSource::Assistant,
            kind: UiMessageKind::Result,
            content: UiContent::Text(text.to_string()),
            priority: None,
        },
    ));
}

fn send_error_text(tx: &mpsc::Sender<RuntimeUiEnvelope>, turn_id: u64, text: &str) {
    let _ = tx.send(RuntimeUiEnvelope::message(
        turn_id,
        RuntimeUiMessage {
            target: UiTarget::Response,
            source: UiSource::System,
            kind: UiMessageKind::Error,
            content: UiContent::Text(text.to_string()),
            priority: Some(UiPriority::High),
        },
    ));
}

fn send_warning_text(tx: &mpsc::Sender<RuntimeUiEnvelope>, turn_id: u64, text: &str) {
    let _ = tx.send(RuntimeUiEnvelope::message(
        turn_id,
        RuntimeUiMessage {
            target: UiTarget::Activity(ActivityTarget::Log),
            source: UiSource::System,
            kind: UiMessageKind::Warning,
            content: UiContent::Text(text.to_string()),
            priority: Some(UiPriority::Normal),
        },
    ));
}

fn send_tool_call_preview(
    tx: &mpsc::Sender<RuntimeUiEnvelope>,
    turn_id: u64,
    tool_name: &str,
    command: Option<String>,
    preview: String,
) {
    let source = UiSource::Tool {
        tool_name: tool_name.to_string(),
    };

    if let Some(command) = command {
        let _ = tx.send(RuntimeUiEnvelope::message(
            turn_id,
            RuntimeUiMessage {
                target: UiTarget::Activity(ActivityTarget::Log),
                source: source.clone(),
                kind: UiMessageKind::Log,
                content: UiContent::Text(format!("$ {command}")),
                priority: None,
            },
        ));
    }

    let _ = tx.send(RuntimeUiEnvelope::message(
        turn_id,
        RuntimeUiMessage {
            target: UiTarget::Activity(ActivityTarget::Log),
            source,
            kind: UiMessageKind::Log,
            content: UiContent::Text(preview),
            priority: None,
        },
    ));
}

fn send_todo_snapshot(tx: &mpsc::Sender<RuntimeUiEnvelope>, turn_id: u64, rendered: &str) {
    let _ = tx.send(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::ReplacePanel {
            target: UiTarget::Todo,
            content: UiContent::Text(rendered.to_string()),
        },
    ));
}

fn send_turn_finished(tx: &mpsc::Sender<RuntimeUiEnvelope>, turn_id: u64) {
    let _ = tx.send(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::ClearStatusSlot {
            slot: StatusSlot::Workflow,
        },
    ));
    let _ = tx.send(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::SetStatusSlot {
            slot: StatusSlot::Agent,
            value: StatusValue::Label("Idle".to_string()),
        },
    ));
}

fn build_step_system_prompt(
    skill_catalog: &SessionSkillCatalog,
    base_system: &str,
    task: &str,
    step: &WorkflowStep,
    step_prompt: &str,
    routing_context: Option<&str>,
) -> String {
    let base_prompt = skill_catalog.build_system_prompt(base_system, task, &step.skill_request);
    let mut sections = vec![base_prompt, format!("Workflow phase: {}", step.label)];
    if let Some(routing_context) = routing_context {
        if !routing_context.trim().is_empty() {
            sections.push(format!(
                "<workflow_runtime>\n{}\n</workflow_runtime>",
                routing_context.trim_end()
            ));
        }
    }
    if !step_prompt.trim().is_empty() {
        sections.push(format!(
            "<workflow_prompt step_id=\"{}\" prompt_path=\"{}\">\n{}\n</workflow_prompt>",
            step.id,
            step.prompt_path.display(),
            step_prompt.trim_end()
        ));
    }

    sections.join("\n\n")
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use async_trait::async_trait;
    use omega_client::{
        ChatRequest, ChatResponse, ClientError, ContentBlock, STOP_REASON_END_TURN,
        STOP_REASON_TOOL_USE,
    };
    use omega_core::{DynLlmClient, LlmClient};
    use omega_workflow::{
        LoadedWorkflowCatalog, ANALYSIS_STEP_ID, CHAT_STEP_ID, CHAT_WORKFLOW_ID,
        EXECUTE_STEP_ID, SCENE_RECOGNITION_STEP_ID, SELECT_WORKFLOW_STEP_ID,
    };

    use super::{
        preview_text, AgentSession, AgentSessionConfig, RuntimeUiEffect, RuntimeUiEnvelope,
        SessionSkillCatalog, SessionToolCatalog, StatusSlot, StatusValue, StepSkillRequest,
        StepToolRequest, UiMessageKind, UiSource,
    };

    struct IdleClient;

    struct SequencedClient {
        responses: Mutex<Vec<ChatResponse>>,
        systems: Mutex<Vec<Option<String>>>,
    }

    #[async_trait]
    impl LlmClient for IdleClient {
        async fn chat(&self, _request: ChatRequest) -> Result<ChatResponse, ClientError> {
            panic!("chat should not be called in AgentSession unit tests");
        }

        fn provider_name(&self) -> &'static str {
            "idle"
        }
    }

    #[async_trait]
    impl LlmClient for SequencedClient {
        async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, ClientError> {
            self.systems.lock().unwrap().push(request.system.clone());
            let mut responses = self.responses.lock().unwrap();
            Ok(responses.remove(0))
        }

        fn provider_name(&self) -> &'static str {
            "sequenced"
        }
    }

    #[test]
    fn preview_text_preserves_utf8_boundaries() {
        assert_eq!(preview_text("你好世界", 3), "你好世...");
    }

    #[test]
    fn interrupt_restores_checkpoint_messages() {
        let client: DynLlmClient = Arc::new(IdleClient);
        let root = std::env::temp_dir().join("omega-agent-session-test");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::create_dir_all(&root);
        let skills_dir = root.join(".claude/skills/review");
        let _ = std::fs::create_dir_all(&skills_dir);
        let _ = std::fs::write(
            skills_dir.join("SKILL.md"),
            "---\nname: review\ndescription: Review code\n---\nFind regressions.",
        );
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let loaded_catalog = LoadedWorkflowCatalog::load(&root);
        let session = AgentSession::new(AgentSessionConfig {
            client,
            system: "system".to_string(),
            cwd: root,
            runtime_handle: runtime.handle().clone(),
            scene_catalog: loaded_catalog.scene_catalog,
            workflow_catalog: loaded_catalog.workflow_catalog,
            prompt_catalog: loaded_catalog.prompt_catalog,
        })
        .unwrap();

        {
            let mut slot = session.agent_slot.lock().unwrap();
            let agent = slot.agent.as_mut().unwrap();
            agent.add_user_message("checkpoint me");
        }
        session.checkpoint_current_messages();
        session.interrupt(42).unwrap();

        let slot = session.agent_slot.lock().unwrap();
        let restored = slot.agent.as_ref().unwrap().messages();
        assert_eq!(slot.turn_id, 42);
        assert_eq!(restored.len(), 1);
    }

    #[test]
    fn spawn_turn_emits_root_then_child_workflow_steps_and_uses_phase_prompts() {
        let client: Arc<SequencedClient> = Arc::new(SequencedClient {
            responses: Mutex::new(vec![
                ChatResponse {
                    id: "scene-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text("feature")],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "select-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text("feature")],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "analysis-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text("analysis")],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "plan-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text("plan")],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "execute-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::tool_use(
                        "tool-1",
                        "bash",
                        serde_json::json!({"command": "echo hi"}),
                    )],
                    stop_reason: Some(STOP_REASON_TOOL_USE.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "execute-2".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text("execution complete")],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "report-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text("done")],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
            ]),
            systems: Mutex::new(Vec::new()),
        });
        let client_dyn: DynLlmClient = client.clone();
        let root = std::env::temp_dir().join("omega-agent-session-workflow-test");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::create_dir_all(&root);
        let skills_dir = root.join(".claude/skills/review");
        let _ = std::fs::create_dir_all(&skills_dir);
        let _ = std::fs::write(
            skills_dir.join("SKILL.md"),
            "---\nname: review\ndescription: Review code\n---\nFind regressions.",
        );
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let loaded_catalog = LoadedWorkflowCatalog::load(&root);
        let session = AgentSession::new(AgentSessionConfig {
            client: client_dyn,
            system: "system".to_string(),
            cwd: root,
            runtime_handle: runtime.handle().clone(),
            scene_catalog: loaded_catalog.scene_catalog,
            workflow_catalog: loaded_catalog.workflow_catalog,
            prompt_catalog: loaded_catalog.prompt_catalog,
        })
        .unwrap();
        let (tx, rx) = mpsc::channel();

        session.spawn_turn("hello".to_string(), 7, tx).unwrap();

        let mut steps = Vec::new();
        let mut step_texts = Vec::new();
        let mut session_labels = Vec::new();
        let mut logs = Vec::new();
        let mut saw_text = false;
        loop {
            match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
                RuntimeUiEnvelope::Effect {
                    turn_id,
                    effect:
                        RuntimeUiEffect::SetStatusSlot {
                            slot: StatusSlot::Workflow,
                            value:
                                StatusValue::WorkflowStep {
                                    step_id,
                                    step_label,
                                    ..
                                },
                        },
                } => {
                    assert_eq!(turn_id, 7);
                    steps.push((step_id, step_label));
                }
                RuntimeUiEnvelope::Effect {
                    turn_id,
                    effect:
                        RuntimeUiEffect::SetStatusSlot {
                            slot: StatusSlot::Session,
                            value: StatusValue::Label(label),
                        },
                } => {
                    assert_eq!(turn_id, 7);
                    session_labels.push(label);
                }
                RuntimeUiEnvelope::Message { turn_id, message } => {
                    assert_eq!(turn_id, 7);
                    match (message.source, message.kind) {
                        (
                            UiSource::WorkflowStep {
                                step_id,
                                step_label,
                                ..
                            },
                            UiMessageKind::Narrative,
                        ) => step_texts.push((
                            step_id,
                            step_label,
                            message.content.as_text().to_string(),
                        )),
                        (UiSource::Assistant, UiMessageKind::Result) => {
                            assert_eq!(message.content.as_text(), "done");
                            saw_text = true;
                        }
                        (UiSource::System, UiMessageKind::Summary | UiMessageKind::Warning) => {
                            logs.push(message.content.as_text().to_string())
                        }
                        _ => {}
                    }
                }
                RuntimeUiEnvelope::Effect {
                    turn_id,
                    effect:
                        RuntimeUiEffect::SetStatusSlot {
                            slot: StatusSlot::Agent,
                            value: StatusValue::Label(label),
                        },
                } => {
                    assert_eq!(turn_id, 7);
                    assert_eq!(label, "Idle");
                    break;
                }
                _ => {}
            }
        }

        assert_eq!(
            steps,
            vec![
                (
                    SCENE_RECOGNITION_STEP_ID.to_string(),
                    "Scene Recognition".to_string(),
                ),
                (
                    SELECT_WORKFLOW_STEP_ID.to_string(),
                    "Select Workflow".to_string(),
                ),
                (ANALYSIS_STEP_ID.to_string(), "Analyze".to_string()),
                ("plan".to_string(), "Plan".to_string()),
                (EXECUTE_STEP_ID.to_string(), "Execute".to_string()),
                ("report".to_string(), "Report".to_string()),
            ]
        );
        assert_eq!(
            step_texts,
            vec![
                (
                    ANALYSIS_STEP_ID.to_string(),
                    "Analyze".to_string(),
                    "analysis".to_string(),
                ),
                ("plan".to_string(), "Plan".to_string(), "plan".to_string()),
                (
                    EXECUTE_STEP_ID.to_string(),
                    "Execute".to_string(),
                    "execution complete".to_string(),
                ),
            ]
        );
        assert!(saw_text);
        assert!(session_labels.iter().any(|label| label == "Route: root"));
        assert!(session_labels
            .iter()
            .any(|label| label == "Scene: feature | Workflow: selecting"));
        assert!(session_labels
            .iter()
            .any(|label| label == "Scene: feature | Workflow: feature"));
        assert!(logs.iter().any(|line| line.contains("Recognized scene 'feature'")));
        assert!(logs
            .iter()
            .any(|line| line.contains("Selected workflow 'feature'")));
        let systems = client.systems.lock().unwrap();
        assert_eq!(systems.len(), 7);
        assert!(systems[0]
            .as_deref()
            .is_some_and(|system| system.contains("Workflow role: root")));
        assert!(systems[1]
            .as_deref()
            .is_some_and(|system| system.contains("Recognized scene: feature")));
        assert!(systems[2]
            .as_deref()
            .is_some_and(|system| system.contains("Workflow role: child")));
        assert!(systems[2]
            .as_deref()
            .is_some_and(|system| system.contains("Active workflow: feature")));
        assert!(systems[6]
            .as_deref()
            .is_some_and(|system| system.contains("Workflow phase: Report")));
    }

    #[test]
    fn chat_scene_routes_to_chat_workflow_without_showing_root_text() {
        let client: Arc<SequencedClient> = Arc::new(SequencedClient {
            responses: Mutex::new(vec![
                ChatResponse {
                    id: "scene-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text("chat")],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "select-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text(CHAT_WORKFLOW_ID)],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "chat-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text("chat answer")],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
            ]),
            systems: Mutex::new(Vec::new()),
        });
        let client_dyn: DynLlmClient = client.clone();
        let root = std::env::temp_dir().join("omega-agent-session-chat-test");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::create_dir_all(&root);
        let skills_dir = root.join(".claude/skills/review");
        let _ = std::fs::create_dir_all(&skills_dir);
        let _ = std::fs::write(
            skills_dir.join("SKILL.md"),
            "---\nname: review\ndescription: Review code\n---\nFind regressions.",
        );
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let loaded_catalog = LoadedWorkflowCatalog::load(&root);
        let session = AgentSession::new(AgentSessionConfig {
            client: client_dyn,
            system: "system".to_string(),
            cwd: root,
            runtime_handle: runtime.handle().clone(),
            scene_catalog: loaded_catalog.scene_catalog,
            workflow_catalog: loaded_catalog.workflow_catalog,
            prompt_catalog: loaded_catalog.prompt_catalog,
        })
        .unwrap();
        let (tx, rx) = mpsc::channel();

        session.spawn_turn("just chat".to_string(), 9, tx).unwrap();

        let mut steps = Vec::new();
        let mut root_narratives = Vec::new();
        let mut assistant_results = Vec::new();
        let mut session_labels = Vec::new();
        loop {
            match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
                RuntimeUiEnvelope::Effect {
                    turn_id,
                    effect:
                        RuntimeUiEffect::SetStatusSlot {
                            slot: StatusSlot::Workflow,
                            value:
                                StatusValue::WorkflowStep {
                                    step_id,
                                    ..
                                },
                        },
                } => {
                    assert_eq!(turn_id, 9);
                    steps.push(step_id);
                }
                RuntimeUiEnvelope::Effect {
                    turn_id,
                    effect:
                        RuntimeUiEffect::SetStatusSlot {
                            slot: StatusSlot::Session,
                            value: StatusValue::Label(label),
                        },
                } => {
                    assert_eq!(turn_id, 9);
                    session_labels.push(label);
                }
                RuntimeUiEnvelope::Message { turn_id, message } => {
                    assert_eq!(turn_id, 9);
                    match (message.source, message.kind) {
                        (
                            UiSource::WorkflowStep { step_id, .. },
                            UiMessageKind::Narrative,
                        ) => root_narratives.push(step_id),
                        (UiSource::Assistant, UiMessageKind::Result) => {
                            assistant_results.push(message.content.as_text().to_string())
                        }
                        _ => {}
                    }
                }
                RuntimeUiEnvelope::Effect {
                    turn_id,
                    effect:
                        RuntimeUiEffect::SetStatusSlot {
                            slot: StatusSlot::Agent,
                            value: StatusValue::Label(label),
                        },
                } => {
                    assert_eq!(turn_id, 9);
                    assert_eq!(label, "Idle");
                    break;
                }
                _ => {}
            }
        }

        assert_eq!(
            steps,
            vec![
                SCENE_RECOGNITION_STEP_ID.to_string(),
                SELECT_WORKFLOW_STEP_ID.to_string(),
                CHAT_STEP_ID.to_string(),
            ]
        );
        assert!(root_narratives.is_empty());
        assert_eq!(assistant_results, vec!["chat answer".to_string()]);
        assert!(session_labels
            .iter()
            .any(|label| label == "Scene: chat | Workflow: chat"));
        let systems = client.systems.lock().unwrap();
        assert_eq!(systems.len(), 3);
        assert!(systems[2]
            .as_deref()
            .is_some_and(|system| system.contains("Active workflow: chat")));
    }

    #[test]
    fn session_tool_catalog_matches_current_default_tool_set() {
        let dispatcher = omega_core::create_default_tools(std::env::temp_dir());
        let catalog = SessionToolCatalog::new(
            dispatcher
                .tool_names()
                .into_iter()
                .map(ToOwned::to_owned)
                .collect(),
        );

        let inherit = catalog.resolve_for_step(&StepToolRequest::Inherit);
        let blocked = catalog.resolve_for_step(&StepToolRequest::Block(vec![
            "bash".to_string(),
            "read_file".to_string(),
        ]));

        assert_eq!(
            inherit.tool_names(),
            [
                "bash",
                "edit_file",
                "load_skill",
                "read_file",
                "todo",
                "write_file"
            ]
        );
        assert_eq!(
            blocked.tool_names(),
            ["edit_file", "load_skill", "todo", "write_file"]
        );
    }

    #[test]
    fn session_skill_catalog_preserves_existing_prompt_shape() {
        let root = std::env::temp_dir().join("omega-agent-session-skill-catalog-test");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::create_dir_all(&root);
        let review = root.join(".claude/skills/review");
        let docs = root.join(".claude/skills/docs");
        let _ = std::fs::create_dir_all(&review);
        let _ = std::fs::create_dir_all(&docs);
        let _ = std::fs::write(
            review.join("SKILL.md"),
            "---\nname: review\ndescription: Review code\n---\nFind regressions.",
        );
        let _ = std::fs::write(
            docs.join("SKILL.md"),
            "---\nname: docs-specs\ndescription: Technical specs\n---\nBe precise.",
        );

        let loader = omega_skills::SkillLoader::from_repo_root(&root).unwrap();
        let catalog = SessionSkillCatalog::new(loader);
        let prompt = catalog.build_system_prompt(
            "Base prompt",
            "Please review this patch",
            &StepSkillRequest::Append(vec!["docs-specs".to_string()]),
        );

        assert!(prompt.contains("Skills available:"));
        assert!(prompt.contains("review: Review code"));
        assert!(prompt.contains("Preloaded skills for this task:"));
        assert!(prompt.contains("<skill name=\"review\">"));
        assert!(prompt.contains("<skill name=\"docs-specs\">"));
    }
}
