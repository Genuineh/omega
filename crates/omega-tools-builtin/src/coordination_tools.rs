use anyhow::Result;
use omega_tools::{ToolHandler, ToolResult};
use serde_json::{json, Value};

#[derive(Debug, Default)]
pub struct AskUserQuestionHandler;

impl ToolHandler for AskUserQuestionHandler {
    fn name(&self) -> &str {
        "ask_user_question"
    }

    fn description(&self) -> &str {
        "Request explicit user input with structured question metadata."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "question": {
                    "type": "string",
                    "description": "The exact question to show the user."
                },
                "context": {
                    "type": "string",
                    "description": "Optional short context shown with the question."
                },
                "options": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional suggested answers."
                },
                "allow_freeform": {
                    "type": "boolean",
                    "description": "Whether the user may answer with freeform text.",
                    "default": true
                }
            },
            "required": ["question"]
        })
    }

    fn execute(&self, input: Value) -> Result<String> {
        Ok(self.execute_v2(input)?.output)
    }

    fn execute_v2(&self, input: Value) -> Result<ToolResult> {
        let question = required_string(&input, "question")?;
        let context = optional_string(&input, "context");
        let options = optional_string_array(&input, "options")?;
        let allow_freeform = input
            .get("allow_freeform")
            .and_then(Value::as_bool)
            .unwrap_or(true);

        let mut lines = vec![format!("Question requested: {question}")];
        if let Some(context) = context.as_deref() {
            lines.push(format!("Context: {context}"));
        }
        if !options.is_empty() {
            lines.push(format!("Options: {}", options.join(", ")));
        }
        lines.push(
            "Wait for the user's next chat response before continuing any branch that depends on this answer."
                .to_string(),
        );

        Ok(ToolResult::success(lines.join("\n"))
            .with_preview(format!("Question requested: {question}"))
            .with_metadata(json!({
                "interaction_kind": "ask_user_question",
                "question": question,
                "context": context,
                "options": options,
                "allow_freeform": allow_freeform,
            })))
    }
}

#[derive(Debug, Default)]
pub struct TaskHandler;

impl ToolHandler for TaskHandler {
    fn name(&self) -> &str {
        "task"
    }

    fn description(&self) -> &str {
        "Record a structured child-task request aligned with fresh-context subagent execution."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "description": {
                    "type": "string",
                    "description": "Short label for the delegated task."
                },
                "prompt": {
                    "type": "string",
                    "description": "Detailed child-task prompt or instructions."
                },
                "agent": {
                    "type": "string",
                    "description": "Optional target agent name or execution profile."
                },
                "expected_output": {
                    "type": "string",
                    "description": "Optional description of the expected child result."
                }
            },
            "required": ["description", "prompt"]
        })
    }

    fn execute(&self, input: Value) -> Result<String> {
        Ok(self.execute_v2(input)?.output)
    }

    fn execute_v2(&self, input: Value) -> Result<ToolResult> {
        let description = required_string(&input, "description")?;
        let prompt = required_string(&input, "prompt")?;
        let agent = optional_string(&input, "agent");
        let expected_output = optional_string(&input, "expected_output");

        Ok(ToolResult::success(
            "Task delegation request recorded only; child execution is not yet wired into this runtime.",
        )
        .with_preview(format!("Task request: {description}"))
        .with_metadata(json!({
            "interaction_kind": "task",
            "description": description,
            "prompt": prompt,
            "agent": agent,
            "expected_output": expected_output,
            "execution_state": "recorded_only",
        })))
    }
}

fn required_string(input: &Value, field: &str) -> Result<String> {
    let value = input
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("Missing required field '{field}'"))?;
    Ok(value.to_string())
}

fn optional_string(input: &Value, field: &str) -> Option<String> {
    input
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn optional_string_array(input: &Value, field: &str) -> Result<Vec<String>> {
    let Some(value) = input.get(field) else {
        return Ok(Vec::new());
    };

    let items = value
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("Field '{field}' must be an array"))?;
    let mut values = Vec::with_capacity(items.len());
    for item in items {
        let item = item
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow::anyhow!("Field '{field}' must contain non-empty strings"))?;
        values.push(item.to_string());
    }
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::{AskUserQuestionHandler, TaskHandler};
    use omega_tools::ToolHandler;
    use serde_json::json;

    #[test]
    fn ask_user_question_returns_structured_metadata() {
        let result = AskUserQuestionHandler
            .execute_v2(json!({
                "question": "Use the fast path?",
                "context": "The current branch has two valid options.",
                "options": ["yes", "no"],
                "allow_freeform": false,
            }))
            .unwrap();

        assert_eq!(
            result.metadata["interaction_kind"].as_str(),
            Some("ask_user_question")
        );
        assert_eq!(
            result.metadata["question"].as_str(),
            Some("Use the fast path?")
        );
        assert_eq!(result.metadata["allow_freeform"].as_bool(), Some(false));
    }

    #[test]
    fn task_handler_marks_recorded_only_execution_state() {
        let result = TaskHandler
            .execute_v2(json!({
                "description": "Inspect API surface",
                "prompt": "Review the request builder and summarize issues.",
            }))
            .unwrap();

        assert_eq!(result.metadata["interaction_kind"].as_str(), Some("task"));
        assert_eq!(
            result.metadata["execution_state"].as_str(),
            Some("recorded_only")
        );
        assert!(result.output.contains("recorded only"));
    }
}
