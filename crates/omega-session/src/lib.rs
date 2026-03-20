use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

use omega_core::{Agent, DynLlmClient, Message};
use omega_skills::SkillLoader;
use omega_workflow::{
    StepLoopMode, WorkflowDefinition, WorkflowPrompts, WorkflowStep, WorkflowStepState,
};
use tokio::runtime::Handle;
use tracing::error;

mod skill_catalog;
mod tool_catalog;

pub use omega_workflow::{StepSkillRequest, StepToolRequest};
pub use skill_catalog::{ResolvedSkillSet, SessionSkillCatalog};
pub use tool_catalog::{ResolvedToolSet, SessionToolCatalog};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionUpdate {
    ToolCallPreview {
        turn_id: u64,
        command: Option<String>,
        preview: String,
    },
    TodoSnapshot {
        turn_id: u64,
        rendered: String,
    },
    WorkflowStepChanged {
        turn_id: u64,
        step_id: String,
        step_label: String,
        index: usize,
        total: usize,
    },
    StepText {
        turn_id: u64,
        step_id: String,
        step_label: String,
        text: String,
    },
    AssistantText {
        turn_id: u64,
        text: String,
    },
    TurnFinished {
        turn_id: u64,
    },
}

pub struct AgentSessionConfig {
    pub client: DynLlmClient,
    pub system: String,
    pub cwd: PathBuf,
    pub runtime_handle: Handle,
    pub workflow_definition: WorkflowDefinition,
    pub workflow_prompts: WorkflowPrompts,
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
    workflow_definition: WorkflowDefinition,
    workflow_prompts: WorkflowPrompts,
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
            workflow_definition: config.workflow_definition,
            workflow_prompts: config.workflow_prompts,
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
        tx: mpsc::Sender<SessionUpdate>,
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
        let workflow_definition = self.workflow_definition.clone();
        let workflow_prompts = self.workflow_prompts.clone();
        thread::spawn(move || {
            agent.add_user_message(&input);
            let runner = WorkflowTurnRunner {
                handle: &handle,
                skill_catalog: &skill_catalog,
                tool_catalog: &tool_catalog,
                base_system: &base_system,
                input: &input,
                workflow_definition: &workflow_definition,
                workflow_prompts: &workflow_prompts,
                turn_id,
                tx_callback: &tx_callback,
                tx_result: &tx_result,
            };
            let result = runner.run(&mut agent);

            match result {
                Ok(text) if !text.is_empty() => {
                    let _ = tx_result.send(SessionUpdate::AssistantText { turn_id, text });
                }
                Ok(_) => {}
                Err(e) => {
                    error!(error = %e, "agent loop error");
                    let _ = tx_result.send(SessionUpdate::AssistantText {
                        turn_id,
                        text: format!("Error: {e}"),
                    });
                }
            }

            let mut slot = agent_slot.lock().unwrap();
            if slot.turn_id == turn_id {
                slot.agent = Some(agent);
            }

            let _ = tx_result.send(SessionUpdate::TurnFinished { turn_id });
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
    workflow_definition: &'a WorkflowDefinition,
    workflow_prompts: &'a WorkflowPrompts,
    turn_id: u64,
    tx_callback: &'a mpsc::Sender<SessionUpdate>,
    tx_result: &'a mpsc::Sender<SessionUpdate>,
}

impl WorkflowTurnRunner<'_> {
    fn run(&self, agent: &mut Agent) -> anyhow::Result<String> {
        let mut last_text = String::new();
        let mut run = self.workflow_definition.start_run();

        loop {
            let Some(step_state) = run.current_step() else {
                break;
            };
            let Some(step) = run.current_step_definition().cloned() else {
                break;
            };
            let is_final_step = step_state.index == step_state.total;

            send_workflow_step(self.tx_result, self.turn_id, Some(step_state));

            let system = build_step_system_prompt(
                self.skill_catalog,
                self.base_system,
                self.input,
                &step,
                self.workflow_prompts
                    .prompt_for(&step.id)
                    .unwrap_or_default(),
            );
            agent.set_system(system);

            let stage_text = match step.loop_mode {
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

                            let _ = tx_callback.send(SessionUpdate::ToolCallPreview {
                                turn_id,
                                command,
                                preview: preview_text(output, 100),
                            });

                            if name == "todo" && !output.starts_with("Error:") {
                                let _ = tx_callback.send(SessionUpdate::TodoSnapshot {
                                    turn_id,
                                    rendered: output.to_string(),
                                });
                            }
                        }
                    }))?
                }
                StepLoopMode::SingleResponse => {
                    agent.set_visible_tools(Some(&[]));
                    self.handle.block_on(agent.run_single_response())?
                }
            };

            if !stage_text.is_empty() {
                if !is_final_step {
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
    tx: &mpsc::Sender<SessionUpdate>,
    turn_id: u64,
    step: Option<WorkflowStepState>,
) {
    let Some(step) = step else {
        return;
    };

    let _ = tx.send(SessionUpdate::WorkflowStepChanged {
        turn_id,
        step_id: step.id,
        step_label: step.label,
        index: step.index,
        total: step.total,
    });
}

fn send_step_text(tx: &mpsc::Sender<SessionUpdate>, turn_id: u64, step: &WorkflowStep, text: &str) {
    let _ = tx.send(SessionUpdate::StepText {
        turn_id,
        step_id: step.id.clone(),
        step_label: step.label.clone(),
        text: text.to_string(),
    });
}

fn build_step_system_prompt(
    skill_catalog: &SessionSkillCatalog,
    base_system: &str,
    task: &str,
    step: &WorkflowStep,
    step_prompt: &str,
) -> String {
    let base_prompt = skill_catalog.build_system_prompt(base_system, task, &step.skill_request);
    if step_prompt.trim().is_empty() {
        return base_prompt;
    }

    format!(
        "{base_prompt}\n\nWorkflow phase: {}\n\n<workflow_prompt step_id=\"{}\" prompt_path=\"{}\">\n{}\n</workflow_prompt>",
        step.label,
        step.id,
        step.prompt_path.display(),
        step_prompt.trim_end()
    )
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
    use omega_workflow::{WorkflowDefinition, WorkflowPrompts, ANALYSIS_STEP_ID, EXECUTE_STEP_ID};

    use super::{
        preview_text, AgentSession, AgentSessionConfig, SessionSkillCatalog, SessionToolCatalog,
        SessionUpdate, StepSkillRequest, StepToolRequest,
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
        let _ = std::fs::create_dir_all(&root);
        let skills_dir = root.join(".claude/skills/review");
        let _ = std::fs::create_dir_all(&skills_dir);
        let _ = std::fs::write(
            skills_dir.join("SKILL.md"),
            "---\nname: review\ndescription: Review code\n---\nFind regressions.",
        );
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let session = AgentSession::new(AgentSessionConfig {
            client,
            system: "system".to_string(),
            cwd: root,
            runtime_handle: runtime.handle().clone(),
            workflow_definition: WorkflowDefinition::default_linear(),
            workflow_prompts: WorkflowPrompts::builtin_defaults(),
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
    fn spawn_turn_emits_workflow_steps_in_order_and_uses_phase_prompts() {
        let client: Arc<SequencedClient> = Arc::new(SequencedClient {
            responses: Mutex::new(vec![
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
        let session = AgentSession::new(AgentSessionConfig {
            client: client_dyn,
            system: "system".to_string(),
            cwd: root,
            runtime_handle: runtime.handle().clone(),
            workflow_definition: WorkflowDefinition::default_linear(),
            workflow_prompts: WorkflowPrompts::builtin_defaults(),
        })
        .unwrap();
        let (tx, rx) = mpsc::channel();

        session.spawn_turn("hello".to_string(), 7, tx).unwrap();

        let mut steps = Vec::new();
        let mut step_texts = Vec::new();
        let mut saw_text = false;
        loop {
            match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
                SessionUpdate::WorkflowStepChanged {
                    step_id,
                    step_label,
                    ..
                } => steps.push((step_id, step_label)),
                SessionUpdate::StepText {
                    step_id,
                    step_label,
                    text,
                    ..
                } => step_texts.push((step_id, step_label, text)),
                SessionUpdate::AssistantText { text, .. } => {
                    assert_eq!(text, "done");
                    saw_text = true;
                }
                SessionUpdate::TurnFinished { turn_id } => {
                    assert_eq!(turn_id, 7);
                    break;
                }
                _ => {}
            }
        }

        assert_eq!(
            steps,
            vec![
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
        let systems = client.systems.lock().unwrap();
        assert_eq!(systems.len(), 5);
        assert!(systems[0]
            .as_deref()
            .is_some_and(|system| system.contains("step_id=\"analysis\"")));
        assert!(systems[1]
            .as_deref()
            .is_some_and(|system| system.contains("Workflow phase: Plan")));
        assert!(systems[2]
            .as_deref()
            .is_some_and(|system| system.contains("step_id=\"execute\"")));
        assert!(systems[4]
            .as_deref()
            .is_some_and(|system| system.contains("Workflow phase: Report")));
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
