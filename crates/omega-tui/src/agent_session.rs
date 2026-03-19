use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

use omega_core::{Agent, DynLlmClient, Message};
use tokio::runtime::Handle;
use tracing::error;

pub enum LogUpdate {
    ToolLog { turn_id: u64, log: String },
    Output { turn_id: u64, text: String },
    Done { turn_id: u64 },
}

struct AgentSlot {
    turn_id: u64,
    agent: Option<Agent>,
}

pub struct AgentSession {
    agent_slot: Arc<Mutex<AgentSlot>>,
    turn_checkpoint: Arc<Mutex<Vec<Message>>>,
    client: DynLlmClient,
    system: String,
    cwd: PathBuf,
    runtime_handle: Handle,
}

impl AgentSession {
    pub fn new(
        client: DynLlmClient,
        system: String,
        cwd: PathBuf,
        runtime_handle: Handle,
    ) -> anyhow::Result<Self> {
        let agent = Agent::new(
            client.clone(),
            system.clone(),
            omega_core::create_default_tools(cwd.clone()),
        )?;
        let checkpoint = agent.messages().to_vec();

        Ok(Self {
            agent_slot: Arc::new(Mutex::new(AgentSlot {
                turn_id: 0,
                agent: Some(agent),
            })),
            turn_checkpoint: Arc::new(Mutex::new(checkpoint)),
            client,
            system,
            cwd,
            runtime_handle,
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

    pub fn set_turn_id(&self, turn_id: u64) {
        self.agent_slot.lock().unwrap().turn_id = turn_id;
    }

    pub fn interrupt(&self, turn_id: u64) -> anyhow::Result<()> {
        let checkpoint = self.turn_checkpoint.lock().unwrap().clone();
        let mut replacement = Agent::new(
            self.client.clone(),
            self.system.clone(),
            omega_core::create_default_tools(self.cwd.clone()),
        )?;
        replacement.set_messages(checkpoint);

        let mut slot = self.agent_slot.lock().unwrap();
        slot.turn_id = turn_id;
        slot.agent = Some(replacement);
        Ok(())
    }

    pub fn spawn_turn(
        &self,
        input: String,
        turn_id: u64,
        tx: mpsc::Sender<LogUpdate>,
    ) -> anyhow::Result<()> {
        self.set_turn_id(turn_id);

        let agent_slot = self.agent_slot.clone();
        let mut agent = match self.agent_slot.lock().unwrap().agent.take() {
            Some(agent) => agent,
            None => return Err(anyhow::anyhow!("agent turn already in progress")),
        };

        let tx_callback = tx.clone();
        let tx_result = tx;
        let handle = self.runtime_handle.clone();
        thread::spawn(move || {
            agent.add_user_message(&input);

            let result = handle.block_on(agent.run_loop_with(move |name, tool_input, output| {
                if name == "bash" {
                    if let Some(cmd) = tool_input["command"].as_str() {
                        let _ = tx_callback.send(LogUpdate::ToolLog {
                            turn_id,
                            log: format!("$ {}", cmd),
                        });
                    }
                }
                let _ = tx_callback.send(LogUpdate::ToolLog {
                    turn_id,
                    log: preview_text(output, 100),
                });
            }));

            match result {
                Ok(text) => {
                    if !text.is_empty() {
                        let _ = tx_result.send(LogUpdate::Output { turn_id, text });
                    }
                }
                Err(e) => {
                    error!(error = %e, "agent loop error");
                    let _ = tx_result.send(LogUpdate::Output {
                        turn_id,
                        text: format!("Error: {e}"),
                    });
                }
            }

            let mut slot = agent_slot.lock().unwrap();
            if slot.turn_id == turn_id {
                slot.agent = Some(agent);
            }

            let _ = tx_result.send(LogUpdate::Done { turn_id });
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
    use super::preview_text;

    use std::path::PathBuf;
    use std::sync::Arc;

    use async_trait::async_trait;
    use omega_client::{ChatRequest, ChatResponse, ClientError};
    use omega_core::{DynLlmClient, LlmClient};

    use super::AgentSession;

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
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let session =
            AgentSession::new(client, "system".to_string(), root, runtime.handle().clone())
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
