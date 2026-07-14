use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliOutput {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl CliOutput {
    pub fn new(exit_code: i32, stdout: impl Into<String>, stderr: impl Into<String>) -> Self {
        Self {
            exit_code,
            stdout: stdout.into(),
            stderr: stderr.into(),
        }
    }
}

pub fn to_pretty_json<T>(value: &T) -> String
where
    T: Serialize,
{
    let mut rendered = serde_json::to_string_pretty(value).unwrap_or_else(|error| {
        serde_json::json!({
            "ok": false,
            "error": format!("failed to serialize output: {error}"),
        })
        .to_string()
    });
    rendered.push('\n');
    rendered
}
