use std::io::{BufRead, Write};
use std::path::PathBuf;

use omega_core::{Agent, DynLlmClient};

pub fn should_exit_input(input: &str) -> bool {
    matches!(input.trim().to_ascii_lowercase().as_str(), "q" | "exit")
}

pub fn preview_output(output: &str, limit: usize) -> String {
    let mut chars = output.chars();
    let preview: String = chars.by_ref().take(limit).collect();
    if chars.next().is_some() {
        format!("{}...", preview)
    } else {
        output.to_string()
    }
}

pub fn format_tool_feedback(name: &str, input: &serde_json::Value, output: &str) -> Vec<String> {
    let mut lines = Vec::new();
    if name == "bash" {
        if let Some(command) = input.get("command").and_then(|value| value.as_str()) {
            lines.push(format!("$ {}", command));
        }
    }
    lines.push(preview_output(output, 200));
    lines
}

pub async fn run_repl<R, W>(
    reader: &mut R,
    writer: &mut W,
    client: DynLlmClient,
    cwd: PathBuf,
    system: String,
) -> anyhow::Result<()>
where
    R: BufRead,
    W: Write,
{
    let dispatcher = omega_core::create_default_tools(cwd.clone());
    let mut agent = Agent::new(client, system, dispatcher)?;

    loop {
        write!(writer, "omega >> ")?;
        writer.flush()?;

        let mut query = String::new();
        let bytes_read = reader.read_line(&mut query)?;
        if bytes_read == 0 {
            break;
        }

        let query = query.trim_end_matches(['\n', '\r']);
        if should_exit_input(query) {
            break;
        }
        if query.trim().is_empty() {
            continue;
        }

        agent.add_user_message(query);
        let response = agent
            .run_loop_with(|name, tool_input, output| {
                for line in format_tool_feedback(name, tool_input, output) {
                    let _ = writeln!(writer, "{}", line);
                }
            })
            .await?;

        if !response.is_empty() {
            writeln!(writer, "{}", response)?;
        }
        writeln!(writer)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use omega_client::{
        ChatRequest, ChatResponse, ClientError, ContentBlock, LlmClient, Usage,
        STOP_REASON_END_TURN, STOP_REASON_TOOL_USE,
    };

    struct MockLlmClient {
        responses: Mutex<Vec<ChatResponse>>,
    }

    impl MockLlmClient {
        fn new(responses: Vec<ChatResponse>) -> Self {
            Self {
                responses: Mutex::new(responses),
            }
        }
    }

    #[async_trait]
    impl LlmClient for MockLlmClient {
        async fn chat(&self, _request: ChatRequest) -> Result<ChatResponse, ClientError> {
            let mut responses = self.responses.lock().unwrap();
            assert!(!responses.is_empty(), "MockLlmClient: no more responses");
            Ok(responses.remove(0))
        }

        fn provider_name(&self) -> &'static str {
            "mock"
        }
    }

    fn text_response(text: &str) -> ChatResponse {
        ChatResponse {
            id: "msg_test".to_string(),
            model: Some("mock".to_string()),
            content: vec![ContentBlock::text(text)],
            stop_reason: Some(STOP_REASON_END_TURN.to_string()),
            usage: Some(Usage {
                input_tokens: 10,
                output_tokens: 5,
            }),
        }
    }

    fn tool_use_response(tool_id: &str, name: &str, input: serde_json::Value) -> ChatResponse {
        ChatResponse {
            id: "msg_test".to_string(),
            model: Some("mock".to_string()),
            content: vec![ContentBlock::tool_use(tool_id, name, input)],
            stop_reason: Some(STOP_REASON_TOOL_USE.to_string()),
            usage: Some(Usage {
                input_tokens: 10,
                output_tokens: 5,
            }),
        }
    }

    #[test]
    fn exit_commands_are_recognized() {
        assert!(should_exit_input("q"));
        assert!(should_exit_input(" EXIT "));
        assert!(!should_exit_input(""));
        assert!(!should_exit_input("continue"));
    }

    #[test]
    fn preview_output_is_utf8_safe() {
        assert_eq!(preview_output("你好世界", 3), "你好世...");
    }

    #[test]
    fn bash_feedback_includes_command_preview() {
        let lines = format_tool_feedback(
            "bash",
            &serde_json::json!({"command": "echo hello"}),
            "hello",
        );
        assert_eq!(lines, vec!["$ echo hello", "hello"]);
    }

    #[tokio::test]
    async fn repl_runs_query_and_exits_on_followup_command() {
        let client: DynLlmClient = Arc::new(MockLlmClient::new(vec![
            tool_use_response("t1", "bash", serde_json::json!({"command": "echo hello"})),
            text_response("Done!"),
        ]));
        let root = std::env::temp_dir().join("omega-repl-test");
        let _ = std::fs::create_dir_all(&root);

        let mut input = Cursor::new(b"run echo\nq\n".to_vec());
        let mut output = Vec::new();

        run_repl(
            &mut input,
            &mut output,
            client,
            root,
            "Test system prompt.".to_string(),
        )
        .await
        .unwrap();

        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("omega >> "));
        assert!(output.contains("$ echo hello"));
        assert!(output.contains("hello"));
        assert!(output.contains("Done!"));
    }

    #[tokio::test]
    async fn blank_line_does_not_exit_repl() {
        let client: DynLlmClient = Arc::new(MockLlmClient::new(vec![text_response("Handled")]));
        let root = std::env::temp_dir().join("omega-repl-blank-line-test");
        let _ = std::fs::create_dir_all(&root);

        let mut input = Cursor::new(b"\nhello\nq\n".to_vec());
        let mut output = Vec::new();

        run_repl(
            &mut input,
            &mut output,
            client,
            root,
            "Test system prompt.".to_string(),
        )
        .await
        .unwrap();

        let output = String::from_utf8(output).unwrap();
        assert!(output.matches("omega >> ").count() >= 3);
        assert!(output.contains("Handled"));
    }
}
