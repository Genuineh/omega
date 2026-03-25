use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use anyhow::Result;
#[cfg(unix)]
use libc::{kill, SIGKILL};
use omega_tools::{ToolErrorKind, ToolHandler, ToolResult};
use serde_json::{json, Value};
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use tempfile::NamedTempFile;
use tracing::{debug, error, info, warn};
use wait_timeout::ChildExt;

use crate::path_safety::{resolve_file_root, safe_path_within_root, safe_path_within_root_from};
use crate::shared::{
    build_tool_error, build_tool_result, truncate_output_chars, workspace_relative_path,
    CapturedOutput, MAX_OUTPUT_CHARS,
};

const DEFAULT_TIMEOUT_SECONDS: u64 = 120;
const DEFAULT_ALLOWED_COMMANDS: &[&str] = &[
    "cat", "echo", "false", "find", "grep", "head", "ls", "printf", "pwd", "rg", "sleep", "tail",
    "touch", "tr", "true", "wait", "wc", "yes",
];
const DISALLOWED_SHELL_TOKENS: &[char] = &['$', '`', '\n', '\r'];
const FILE_PATH_COMMANDS: &[&str] = &[
    "cat", "find", "grep", "head", "ls", "rg", "tail", "touch", "wc",
];
const FIND_BLOCKED_ACTIONS: &[&str] = &[
    "-delete", "-exec", "-execdir", "-fprint", "-fprint0", "-fprintf", "-fls", "-ok", "-okdir",
];

pub fn default_bash_allowed_commands() -> Vec<String> {
    DEFAULT_ALLOWED_COMMANDS
        .iter()
        .map(|command| (*command).to_string())
        .collect()
}

#[derive(Debug, Clone)]
struct BashWorkdir {
    path: PathBuf,
    label: String,
}

#[derive(Debug, Clone)]
struct BashRequest {
    command: String,
    description: Option<String>,
    timeout_seconds: u64,
    workdir: BashWorkdir,
}

#[derive(Debug, Clone)]
struct BashInputError {
    code: &'static str,
    message: String,
    error_kind: ToolErrorKind,
}

impl BashInputError {
    fn validation(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            error_kind: ToolErrorKind::Validation,
        }
    }

    fn policy(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            error_kind: ToolErrorKind::Policy,
        }
    }
}

#[derive(Debug, Clone)]
pub struct BashHandler {
    root: PathBuf,
    default_timeout_seconds: u64,
    allowed_commands: BTreeSet<String>,
}

impl BashHandler {
    pub fn new(root: PathBuf) -> Self {
        Self::with_allowed_commands(root, default_bash_allowed_commands())
    }

    pub fn with_allowed_commands(root: PathBuf, allowed_commands: Vec<String>) -> Self {
        Self {
            root: resolve_file_root(root),
            default_timeout_seconds: DEFAULT_TIMEOUT_SECONDS,
            allowed_commands: Self::normalize_allowed_commands(allowed_commands),
        }
    }

    pub fn with_timeout(root: PathBuf, default_timeout_seconds: u64) -> Self {
        Self::with_timeout_and_allowed_commands(
            root,
            default_timeout_seconds,
            default_bash_allowed_commands(),
        )
    }

    pub fn with_timeout_and_allowed_commands(
        root: PathBuf,
        default_timeout_seconds: u64,
        allowed_commands: Vec<String>,
    ) -> Self {
        Self {
            root: resolve_file_root(root),
            default_timeout_seconds,
            allowed_commands: Self::normalize_allowed_commands(allowed_commands),
        }
    }

    fn normalize_allowed_commands(allowed_commands: Vec<String>) -> BTreeSet<String> {
        allowed_commands
            .into_iter()
            .map(|command| command.trim().to_ascii_lowercase())
            .filter(|command| !command.is_empty())
            .collect()
    }

    fn normalize_command_for_safety(command: &str) -> String {
        let mut normalized = String::with_capacity(command.len());
        let mut last_was_whitespace = false;

        for character in command.chars() {
            match character {
                '\'' | '"' => {}
                character if character.is_whitespace() => {
                    if !last_was_whitespace {
                        normalized.push(' ');
                        last_was_whitespace = true;
                    }
                }
                character => {
                    normalized.push(character.to_ascii_lowercase());
                    last_was_whitespace = false;
                }
            }
        }

        normalized.trim().to_string()
    }

    fn validate_path_within_root(
        &self,
        base_dir: &Path,
        path_arg: &str,
    ) -> std::result::Result<(), BashInputError> {
        safe_path_within_root_from(&self.root, base_dir, path_arg)
            .map(|_| ())
            .map_err(|message| BashInputError::policy("path_outside_workspace", message))
    }

    fn validate_segment(
        &self,
        segment: &str,
        workdir: &Path,
    ) -> std::result::Result<(), BashInputError> {
        let argv = shlex::split(segment).ok_or_else(|| {
            BashInputError::validation("command_parse_failed", "Error: Command parsing failed")
        })?;
        let (program, args) = argv.split_first().ok_or_else(|| {
            BashInputError::validation("empty_command", "Error: Command cannot be empty")
        })?;

        if !self.allowed_commands.contains(program.as_str()) {
            return Err(BashInputError::policy(
                "command_not_allowed",
                format!("Error: Command '{program}' is blocked by safety policy"),
            ));
        }

        if program == "find" {
            self.validate_find_args(args)?;
        }

        if FILE_PATH_COMMANDS.contains(&program.as_str()) {
            for arg in args {
                if arg.starts_with('-') {
                    continue;
                }
                self.validate_path_within_root(workdir, arg)?;
            }
        }

        Ok(())
    }

    fn validate_find_args(&self, args: &[String]) -> std::result::Result<(), BashInputError> {
        if args
            .iter()
            .any(|arg| FIND_BLOCKED_ACTIONS.contains(&arg.as_str()))
        {
            return Err(BashInputError::policy(
                "find_action_blocked",
                "Error: Dangerous find action blocked",
            ));
        }

        Ok(())
    }

    fn command(&self, input: &Value) -> std::result::Result<String, BashInputError> {
        let command = input
            .get("command")
            .and_then(Value::as_str)
            .map(str::trim)
            .ok_or_else(|| {
                BashInputError::validation(
                    "missing_command",
                    "Error: Missing required field 'command'",
                )
            })?;

        if command.is_empty() {
            return Err(BashInputError::validation(
                "empty_command",
                "Error: Command cannot be empty",
            ));
        }

        Ok(command.to_string())
    }

    fn parse_description(
        &self,
        input: &Value,
    ) -> std::result::Result<Option<String>, BashInputError> {
        match input.get("description") {
            None => Ok(None),
            Some(Value::String(description)) => {
                let description = description.trim();
                if description.is_empty() {
                    return Err(BashInputError::validation(
                        "empty_description",
                        "Error: Field 'description' cannot be empty",
                    ));
                }
                Ok(Some(description.to_string()))
            }
            Some(_) => Err(BashInputError::validation(
                "invalid_description",
                "Error: Field 'description' must be a string",
            )),
        }
    }

    fn workdir(&self, input: &Value) -> std::result::Result<BashWorkdir, BashInputError> {
        match input.get("workdir") {
            None => Ok(BashWorkdir {
                path: self.root.clone(),
                label: ".".to_string(),
            }),
            Some(Value::String(workdir)) => {
                let workdir = workdir.trim();
                if workdir.is_empty() {
                    return Err(BashInputError::validation(
                        "empty_workdir",
                        "Error: Field 'workdir' cannot be empty",
                    ));
                }

                let resolved = safe_path_within_root(&self.root, workdir).map_err(|message| {
                    BashInputError::policy("workdir_outside_workspace", message)
                })?;

                if !resolved.exists() {
                    return Err(BashInputError::validation(
                        "workdir_not_found",
                        format!("Error: Working directory '{workdir}' does not exist"),
                    ));
                }

                if !resolved.is_dir() {
                    return Err(BashInputError::validation(
                        "workdir_not_directory",
                        format!("Error: Working directory '{workdir}' is not a directory"),
                    ));
                }

                let label = if resolved == self.root {
                    ".".to_string()
                } else {
                    workspace_relative_path(&self.root, &resolved)
                };

                Ok(BashWorkdir {
                    path: resolved,
                    label,
                })
            }
            Some(_) => Err(BashInputError::validation(
                "invalid_workdir",
                "Error: Field 'workdir' must be a string",
            )),
        }
    }

    fn validate_command(
        &self,
        command: &str,
        workdir: &Path,
    ) -> std::result::Result<(), BashInputError> {
        if command
            .chars()
            .any(|character| DISALLOWED_SHELL_TOKENS.contains(&character))
        {
            return Err(BashInputError::policy(
                "shell_expansion_not_allowed",
                "Error: Shell expansion is not allowed",
            ));
        }

        if command.contains('>') || command.contains('<') {
            return Err(BashInputError::policy(
                "shell_redirection_not_allowed",
                "Error: Shell redirection is not allowed",
            ));
        }

        let normalized_command = Self::normalize_command_for_safety(command);
        if normalized_command.contains("rm -rf") || normalized_command.contains("rm -fr") {
            return Err(BashInputError::policy(
                "dangerous_command_blocked",
                "Error: Dangerous command blocked",
            ));
        }

        for segment in normalized_command
            .split(['|', '&', ';', '(', ')'])
            .map(str::trim)
            .filter(|segment| !segment.is_empty())
        {
            self.validate_segment(segment, workdir)?;
        }

        Ok(())
    }

    fn timeout_seconds(&self, input: &Value) -> std::result::Result<u64, BashInputError> {
        match input.get("timeout") {
            None => Ok(self.default_timeout_seconds),
            Some(Value::Number(number)) => number
                .as_u64()
                .filter(|timeout| *timeout > 0)
                .ok_or_else(|| {
                    BashInputError::validation(
                        "invalid_timeout",
                        "Error: Timeout must be a positive integer",
                    )
                }),
            Some(_) => Err(BashInputError::validation(
                "invalid_timeout",
                "Error: Timeout must be a positive integer",
            )),
        }
    }

    fn parse_request(&self, input: &Value) -> std::result::Result<BashRequest, BashInputError> {
        let command = self.command(input)?;
        let description = self.parse_description(input)?;
        let workdir = self.workdir(input)?;
        self.validate_command(&command, &workdir.path)?;
        let timeout_seconds = self.timeout_seconds(input)?;

        Ok(BashRequest {
            command,
            description,
            timeout_seconds,
            workdir,
        })
    }

    fn bash_metadata(
        command: Option<&str>,
        workdir: Option<&str>,
        description: Option<&str>,
        timeout_seconds: Option<u64>,
        error_code: Option<&str>,
    ) -> Value {
        let mut metadata = serde_json::Map::new();
        if let Some(command) = command {
            metadata.insert("command".to_string(), json!(command));
        }
        if let Some(workdir) = workdir {
            metadata.insert("workdir".to_string(), json!(workdir));
        }
        if let Some(description) = description {
            metadata.insert("description".to_string(), json!(description));
        }
        if let Some(timeout_seconds) = timeout_seconds {
            metadata.insert("timeout_seconds".to_string(), json!(timeout_seconds));
        }
        if let Some(error_code) = error_code {
            metadata.insert("error_code".to_string(), json!(error_code));
        }
        Value::Object(metadata)
    }

    fn truncate_output(output: String) -> CapturedOutput {
        truncate_output_chars(output, MAX_OUTPUT_CHARS)
    }

    fn read_output_file(path: &Path) -> Vec<u8> {
        fs::read(path).unwrap_or_default()
    }

    pub(crate) fn format_output(stdout: &[u8], stderr: &[u8]) -> CapturedOutput {
        let mut output = String::from_utf8_lossy(stdout).into_owned();
        output.push_str(&String::from_utf8_lossy(stderr));
        let output = output.trim().to_string();

        if output.is_empty() {
            return CapturedOutput {
                output: "(no output)".to_string(),
                truncated: false,
            };
        }

        if output.chars().count() > MAX_OUTPUT_CHARS {
            return Self::truncate_output(output);
        }

        CapturedOutput {
            output,
            truncated: false,
        }
    }

    fn build_command(
        &self,
        command: &str,
        workdir: &Path,
        stdout_handle: std::fs::File,
        stderr_handle: std::fs::File,
    ) -> Command {
        let mut process = Command::new("sh");
        process
            .arg("-c")
            .arg(command)
            .current_dir(workdir)
            .stdout(Stdio::from(stdout_handle))
            .stderr(Stdio::from(stderr_handle));

        #[cfg(unix)]
        process.process_group(0);

        process
    }

    fn run_command(&self, request: &BashRequest) -> ToolResult {
        let metadata = Self::bash_metadata(
            Some(&request.command),
            Some(&request.workdir.label),
            request.description.as_deref(),
            Some(request.timeout_seconds),
            None,
        );
        let stdout_file = match NamedTempFile::new() {
            Ok(file) => file,
            Err(error) => {
                return build_tool_error(
                    format!("Error: Failed to create stdout capture file: {error}"),
                    metadata,
                    ToolErrorKind::Execution,
                );
            }
        };
        let stderr_file = match NamedTempFile::new() {
            Ok(file) => file,
            Err(error) => {
                return build_tool_error(
                    format!("Error: Failed to create stderr capture file: {error}"),
                    metadata,
                    ToolErrorKind::Execution,
                );
            }
        };

        let stdout_path = stdout_file.path().to_path_buf();
        let stderr_path = stderr_file.path().to_path_buf();

        let stdout_handle = match stdout_file.reopen() {
            Ok(file) => file,
            Err(error) => {
                return build_tool_error(
                    format!("Error: Failed to reopen stdout capture file: {error}"),
                    metadata,
                    ToolErrorKind::Execution,
                );
            }
        };
        let stderr_handle = match stderr_file.reopen() {
            Ok(file) => file,
            Err(error) => {
                return build_tool_error(
                    format!("Error: Failed to reopen stderr capture file: {error}"),
                    metadata,
                    ToolErrorKind::Execution,
                );
            }
        };

        let mut child = match self
            .build_command(
                &request.command,
                &request.workdir.path,
                stdout_handle,
                stderr_handle,
            )
            .spawn()
        {
            Ok(child) => child,
            Err(error) => {
                return build_tool_error(
                    format!("Error: Failed to start command: {error}"),
                    metadata,
                    ToolErrorKind::Execution,
                );
            }
        };

        let timeout = Duration::from_secs(request.timeout_seconds);
        match child.wait_timeout(timeout) {
            Ok(Some(_status)) => match child.wait() {
                Ok(_) => {
                    let stdout = Self::read_output_file(stdout_path.as_path());
                    let stderr = Self::read_output_file(stderr_path.as_path());
                    let captured = Self::format_output(&stdout, &stderr);
                    build_tool_result(captured.output, metadata, captured.truncated, None)
                }
                Err(error) => build_tool_error(
                    format!("Error: Failed to collect command output: {error}"),
                    metadata,
                    ToolErrorKind::Execution,
                ),
            },
            Ok(None) => {
                self.terminate_child(&mut child);
                let _ = child.wait();
                build_tool_error(
                    format!("Error: Timeout ({}s)", request.timeout_seconds),
                    metadata,
                    ToolErrorKind::Timeout,
                )
            }
            Err(error) => {
                self.terminate_child(&mut child);
                let _ = child.wait();
                build_tool_error(
                    format!("Error: Failed to wait for command: {error}"),
                    metadata,
                    ToolErrorKind::Execution,
                )
            }
        }
    }

    fn run(&self, input: Value) -> ToolResult {
        let command_for_log = input
            .get("command")
            .and_then(Value::as_str)
            .unwrap_or("(missing)");
        let workdir_for_log = input.get("workdir").and_then(Value::as_str).unwrap_or(".");
        let description_for_log = input.get("description").and_then(Value::as_str);
        info!(
            bash.command = %command_for_log,
            bash.workdir = %workdir_for_log,
            bash.description = ?description_for_log
        );

        let request = match self.parse_request(&input) {
            Ok(request) => request,
            Err(error) => {
                warn!(
                    bash.error_code = error.code,
                    bash.error_kind = error.error_kind.as_str(),
                    bash.blocked_reason = %error.message,
                    bash.command = %command_for_log,
                    bash.workdir = %workdir_for_log,
                    bash.description = ?description_for_log,
                );
                return build_tool_error(
                    error.message,
                    Self::bash_metadata(
                        input.get("command").and_then(Value::as_str),
                        input.get("workdir").and_then(Value::as_str),
                        input.get("description").and_then(Value::as_str),
                        input.get("timeout").and_then(Value::as_u64),
                        Some(error.code),
                    ),
                    error.error_kind,
                );
            }
        };

        let result = self.run_command(&request);

        if result.error_kind == Some(ToolErrorKind::Timeout) {
            error!(
                bash.timeout_seconds = request.timeout_seconds,
                bash.command = %request.command,
                bash.workdir = %request.workdir.label
            );
        }

        if result.truncated {
            debug!(
                bash.output_truncated = true,
                bash.max_chars = MAX_OUTPUT_CHARS
            );
        }

        result
    }

    #[cfg(unix)]
    fn terminate_child(&self, child: &mut Child) {
        let process_group_id = -(child.id() as i32);
        unsafe {
            let _ = kill(process_group_id, SIGKILL);
        }
        let _ = child.kill();
    }

    #[cfg(not(unix))]
    fn terminate_child(&self, child: &mut Child) {
        let _ = child.kill();
    }
}

impl ToolHandler for BashHandler {
    fn name(&self) -> &str {
        "bash"
    }

    fn description(&self) -> &str {
        "Run a shell command."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "Shell command to execute from the workspace root or the optional `workdir`. Pipes and compound shell forms are supported for an allowlisted command set, while shell expansion, redirection, non-allowlisted commands, dangerous find actions, and paths outside the workspace root are blocked for safety."
                },
                "workdir": {
                    "type": "string",
                    "description": "Optional working directory for command execution. Must resolve to an existing directory inside the workspace root. Relative paths are resolved from the workspace root."
                },
                "description": {
                    "type": "string",
                    "description": "Optional short explanation of what the command is doing. This is surfaced in runtime tool previews."
                },
                "timeout": {
                    "type": "integer",
                    "description": "Optional timeout in seconds",
                    "minimum": 1
                }
            },
            "required": ["command"]
        })
    }

    fn execute(&self, input: Value) -> Result<String> {
        Ok(self.run(input).output)
    }

    fn execute_v2(&self, input: Value) -> Result<ToolResult> {
        Ok(self.run(input))
    }
}
