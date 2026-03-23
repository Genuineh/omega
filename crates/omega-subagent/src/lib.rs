use anyhow::{anyhow, Result};
use omega_client::{ChatRequest, ContentBlock, DynLlmClient, Message, ToolDefinition};

pub const DEFAULT_SUBAGENT_MAX_ITERATIONS: u32 = 30;
pub const DEFAULT_SUBAGENT_MAX_TOKENS: u32 = 8_000;

/// Fresh-context child agent that shares tools and filesystem access via the
/// caller-provided tool handler, but does not inherit the parent's transcript.
pub struct SubAgent {
    client: DynLlmClient,
    system: String,
    tool_definitions: Vec<ToolDefinition>,
    max_tokens: u32,
    max_iterations: u32,
}

impl SubAgent {
    pub fn new(
        client: DynLlmClient,
        system: impl Into<String>,
        tool_definitions: Vec<ToolDefinition>,
    ) -> Self {
        Self {
            client,
            system: system.into(),
            tool_definitions,
            max_tokens: DEFAULT_SUBAGENT_MAX_TOKENS,
            max_iterations: DEFAULT_SUBAGENT_MAX_ITERATIONS,
        }
    }

    pub fn set_max_tokens(&mut self, max_tokens: u32) {
        self.max_tokens = max_tokens.max(1);
    }

    pub fn set_max_iterations(&mut self, max_iterations: u32) {
        self.max_iterations = max_iterations.max(1);
    }

    pub async fn run<F>(&self, prompt: &str, handler: F) -> Result<String>
    where
        F: FnMut(&str, &serde_json::Value) -> Result<String>,
    {
        self.run_with(prompt, handler, |_, _, _, _| {}).await
    }

    pub async fn run_with<F, C>(
        &self,
        prompt: &str,
        mut handler: F,
        mut on_tool_call: C,
    ) -> Result<String>
    where
        F: FnMut(&str, &serde_json::Value) -> Result<String>,
        C: FnMut(&str, &str, &serde_json::Value, &str),
    {
        let mut messages = vec![Message::user(prompt)];

        for _ in 0..self.max_iterations {
            let response = self
                .client
                .chat(
                    ChatRequest::new(messages.clone())
                        .with_system(self.system.clone())
                        .with_tools(self.tool_definitions.clone())
                        .with_max_tokens(self.max_tokens),
                )
                .await
                .map_err(|error| anyhow!("{error}"))?;

            messages.push(Message::assistant(response.content.clone()));
            if !response.is_tool_use() {
                return Ok(response.text_content());
            }

            let mut results = Vec::new();
            for block in &response.content {
                if let ContentBlock::ToolUse { id, name, input } = block {
                    match handler(name, input) {
                        Ok(output) => {
                            on_tool_call(id, name, input, &output);
                            results.push(ContentBlock::tool_result(id, &output));
                        }
                        Err(error) => {
                            let error_message = error.to_string();
                            on_tool_call(id, name, input, &error_message);
                            results.push(ContentBlock::tool_result_error(id, &error_message));
                        }
                    }
                }
            }

            messages.push(Message::tool_results(results));
        }

        Err(anyhow!(
            "subagent loop exceeded {} iterations",
            self.max_iterations
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use omega_client::{
        ChatResponse, ClientError, LlmClient, Usage, STOP_REASON_END_TURN, STOP_REASON_TOOL_USE,
    };
    use serde_json::json;

    use super::*;

    struct MockLlmClient {
        responses: Mutex<Vec<ChatResponse>>,
        requests: Arc<Mutex<Vec<ChatRequest>>>,
    }

    impl MockLlmClient {
        fn new(responses: Vec<ChatResponse>, requests: Arc<Mutex<Vec<ChatRequest>>>) -> Self {
            Self {
                responses: Mutex::new(responses),
                requests,
            }
        }
    }

    #[async_trait]
    impl LlmClient for MockLlmClient {
        async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, ClientError> {
            self.requests.lock().unwrap().push(request);
            let mut responses = self.responses.lock().unwrap();
            Ok(responses.remove(0))
        }

        fn provider_name(&self) -> &'static str {
            "mock"
        }
    }

    fn text_response(text: &str) -> ChatResponse {
        ChatResponse {
            id: "msg-final".to_string(),
            model: Some("mock".to_string()),
            content: vec![ContentBlock::text(text)],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: Some(Usage {
                input_tokens: 10,
                output_tokens: 5,
            }),
        }
    }

    fn tool_use_response(id: &str, name: &str, input: serde_json::Value) -> ChatResponse {
        ChatResponse {
            id: "msg-tool".to_string(),
            model: Some("mock".to_string()),
            content: vec![ContentBlock::tool_use(id, name, input)],
            stop_reason: Some(STOP_REASON_TOOL_USE.to_string()),
            usage: Some(Usage {
                input_tokens: 10,
                output_tokens: 5,
            }),
        }
    }

    fn sample_tool_definition() -> ToolDefinition {
        ToolDefinition {
            name: "read_file".to_string(),
            description: "Read a file".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                },
                "required": ["path"]
            }),
        }
    }

    #[tokio::test]
    async fn run_returns_final_text_without_tool_calls() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let client: DynLlmClient = Arc::new(MockLlmClient::new(
            vec![text_response("subagent summary")],
            requests.clone(),
        ));
        let subagent = SubAgent::new(client, "subagent system", vec![sample_tool_definition()]);

        let result = subagent
            .run("inspect src", |_name, _input| Ok(String::new()))
            .await;

        assert_eq!(result.unwrap(), "subagent summary");
        let recorded = requests.lock().unwrap();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].messages, vec![Message::user("inspect src")]);
    }

    #[tokio::test]
    async fn run_executes_tool_calls_and_keeps_fresh_context() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let client: DynLlmClient = Arc::new(MockLlmClient::new(
            vec![
                tool_use_response("tool-1", "read_file", json!({"path": "src/lib.rs"})),
                text_response("done"),
            ],
            requests.clone(),
        ));
        let calls: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
        let calls_clone = calls.clone();
        let subagent = SubAgent::new(client, "subagent system", vec![sample_tool_definition()]);

        let result = subagent
            .run_with(
                "inspect src",
                |_name, input| Ok(format!("read {}", input["path"].as_str().unwrap())),
                move |tool_use_id, name, _input, output| {
                    calls_clone
                        .lock()
                        .unwrap()
                        .push((format!("{tool_use_id}:{name}"), output.to_string()));
                },
            )
            .await;

        assert_eq!(result.unwrap(), "done");
        assert_eq!(
            calls.lock().unwrap().as_slice(),
            &[(
                "tool-1:read_file".to_string(),
                "read src/lib.rs".to_string()
            )]
        );

        let recorded = requests.lock().unwrap();
        assert_eq!(recorded.len(), 2);
        assert_eq!(recorded[0].messages, vec![Message::user("inspect src")]);
        assert_eq!(recorded[1].messages.len(), 3);
        assert!(matches!(
            &recorded[1].messages[1].content,
            omega_client::MessageContent::Blocks(blocks)
                if matches!(blocks.as_slice(), [ContentBlock::ToolUse { name, .. }] if name == "read_file")
        ));
        assert!(matches!(
            &recorded[1].messages[2].content,
            omega_client::MessageContent::Blocks(blocks)
                if matches!(blocks.as_slice(), [ContentBlock::ToolResult { is_error, .. }] if is_error.is_none())
        ));
    }

    #[tokio::test]
    async fn tool_errors_roundtrip_as_tool_result_errors() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let client: DynLlmClient = Arc::new(MockLlmClient::new(
            vec![
                tool_use_response("tool-1", "read_file", json!({"path": "missing.rs"})),
                text_response("handled"),
            ],
            requests.clone(),
        ));
        let subagent = SubAgent::new(client, "subagent system", vec![sample_tool_definition()]);

        let result = subagent
            .run("inspect src", |_name, _input| Err(anyhow!("missing file")))
            .await;

        assert_eq!(result.unwrap(), "handled");

        let recorded = requests.lock().unwrap();
        assert!(matches!(
            &recorded[1].messages[2].content,
            omega_client::MessageContent::Blocks(blocks)
                if matches!(blocks.as_slice(), [ContentBlock::ToolResult { is_error, .. }] if is_error == &Some(true))
        ));
    }
}
