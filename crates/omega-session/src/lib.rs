use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

use omega_core::{Agent, DynLlmClient, Message};
use omega_skills::SkillLoader;
use tokio::runtime::Handle;
use tracing::error;

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
    skill_loader: SkillLoader,
    runtime_handle: Handle,
}

impl AgentSession {
    pub fn new(config: AgentSessionConfig) -> anyhow::Result<Self> {
        let skill_loader = SkillLoader::from_repo_root(&config.cwd)?;
        let initial_system = skill_loader.build_system_prompt(&config.system, "");
        let agent = Agent::new(
            config.client.clone(),
            initial_system,
            omega_core::create_default_tools(config.cwd.clone()),
        )?;
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
            skill_loader,
            runtime_handle: config.runtime_handle,
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
        let system = self.skill_loader.build_system_prompt(&self.base_system, "");
        let mut replacement = Agent::new(
            self.client.clone(),
            system,
            omega_core::create_default_tools(self.cwd.clone()),
        )?;
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
        let skill_loader = self.skill_loader.clone();
        thread::spawn(move || {
            let system = skill_loader.build_system_prompt(&base_system, &input);
            agent.set_system(system);
            agent.add_user_message(&input);

            let result = handle.block_on(agent.run_loop_with(move |name, tool_input, output| {
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
            }));

            match result {
                Ok(text) => {
                    if !text.is_empty() {
                        let _ = tx_result.send(SessionUpdate::AssistantText { turn_id, text });
                    }
                }
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

fn preview_text(text: &str, limit: usize) -> String {
    let mut chars = text.chars();
    let preview: String = chars.by_ref().take(limit).collect();
    if chars.next().is_some() {
        format!("{}...", preview)
    } else {
        text.to_string()
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use async_trait::async_trait;
    use omega_client::{ChatRequest, ChatResponse, ClientError};
    use omega_core::{DynLlmClient, LlmClient};

    use super::{preview_text, AgentSession, AgentSessionConfig};

    struct IdleClient;

    #[async_trait]
    impl LlmClient for IdleClient {
        async fn chat(&self, _request: ChatRequest) -> Result<ChatResponse, ClientError> {
            panic!("chat should not be called in AgentSession unit tests");
        }

        fn provider_name(&self) -> &'static str {
            "idle"
        }
    }

    #[test]
    fn preview_text_preserves_utf8_boundaries() {
        assert_eq!(preview_text("你好世界", 3), "你好世...");
    }

    #[test]
    fn interrupt_restores_checkpoint_messages() {
        let client: DynLlmClient = Arc::new(IdleClient);
        let root = PathBuf::from(std::env::temp_dir().join("omega-agent-session-test"));
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
}
